//! The recording editor's timeline card.
//!
//! The chrome (tool row, transport, ruler, lanes, playhead, zoom pill) mirrors
//! the source editor. The video lane holds the trimmed, filmstripped clip; the
//! zoom and cursor-hide lanes hold the effect pills the tool row drops at the
//! playhead. The playhead scrubs, playback advances the clocks, and the zoom
//! pill (or the wheel and scrollbar) scales and pans the whole board.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk4::cairo::{Context, FontSlant, FontWeight, LinearGradient};
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Align, ApplicationWindow, AspectFrame, Box as GtkBox, Button, DrawingArea,
    EventControllerMotion, EventControllerScroll, EventControllerScrollFlags, GestureClick,
    GestureDrag, Image, Label, Orientation, Overlay, Popover, PositionType, Scale, Scrollbar,
};

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::recording::editor::auto_zoom;
use crate::recording::editor::ffmpeg;

use super::history::EditHistory;

const PLACEHOLDER_SECONDS: f64 = 30.0;
const FIT_SCALE_RANGE: f64 = 7.0;
const MIN_CLIP_SECONDS: f64 = 0.1;
const DEFAULT_ZOOM_SCALE: f64 = 1.8;
const DEFAULT_EFFECT_SECONDS: f64 = 1.8;
const MIN_ZOOM_SCALE: f64 = 1.2;
const MAX_ZOOM_SCALE: f64 = 5.0;

const HANDLE_INSET: f64 = 6.0;
const HANDLE_WIDTH: f64 = 4.0;
const HANDLE_HIT: f64 = 6.0;
const PLAYHEAD_HIT: f64 = 6.0;
/// How far the extend band's white mover line sits inside the panel's edge.
const EXTEND_MOVER_INSET: f64 = 10.0;
/// Pixels within which a drop point snaps onto the playhead.
const PLAYHEAD_SNAP: f64 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectTrack {
    Zoom,
    Hide,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ZoomMode {
    /// The camera follows the recorded pointer, on the follow spring.
    Auto,
    /// The clip holds one focus point the user chose.
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ZoomClip {
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) scale: f64,
    pub(super) center: (f64, f64),
    pub(super) hidden: bool,
    /// Follows the pointer (Auto), or holds its own focus point (Manual).
    /// Detect places Auto clips; a clip the user places or drags is Manual.
    pub(super) mode: ZoomMode,
    /// Snap in and out instead of easing. The clip is at its framing on its
    /// first frame and back to the full frame on the frame after its last.
    pub(super) instant: bool,
    /// The recorded footage this clip frames, in source seconds.
    ///
    /// A clip's span is composition time, and trimming, cutting or moving a
    /// piece slides different footage under it. The anchor is the footage the
    /// clip was placed for, and `reproject_anchored_zooms` turns it back into a
    /// span whenever the composition changes. A clip the user moved or resized
    /// is unanchored: they said where it belongs.
    pub(super) anchor: Option<(f64, f64)>,
}

#[derive(Clone, Copy, PartialEq)]
struct HideClip {
    start: f64,
    end: f64,
    hidden: bool,
}

/// One cut or copied effect, waiting for Paste. The paste lands at the
/// playhead on the track it came from, as the source editor's clipboard does.
#[derive(Clone, Copy, PartialEq)]
struct ClipTemplate {
    track: EffectTrack,
    duration: f64,
    scale: f64,
    center: (f64, f64),
    hidden: bool,
    mode: ZoomMode,
    instant: bool,
}

#[derive(Default)]
struct Data {
    path: Option<PathBuf>,
    duration: f64,
    frame_size: Option<(u32, u32)>,
    trim_start: f64,
    trim_end: f64,
    playhead: f64,
    scale: f64,
    scroll: f64,
    playing: bool,
    selected: bool,
    hover: Option<f64>,
    frames: Vec<gtk4::gdk_pixbuf::Pixbuf>,
    zoom_clips: Vec<ZoomClip>,
    /// Camera-wide zoom settings: the follow spring and the early lead-in.
    zoom_camera: super::zoom::ZoomCameraSettings,
    hide_clips: Vec<HideClip>,
    selected_zoom: Option<usize>,
    /// Bumped every time a zoom clip is selected. The panel reveals itself on
    /// a new selection, and a counter is what tells a repeated click on the
    /// pill that is already selected apart from no click at all.
    zoom_selection_epoch: u64,
    selected_hide: Option<usize>,
    clipboard: Option<ClipTemplate>,
    /// Where the pointer sits on each pill lane, in composition time. Copy and
    /// Cut put a clip aside invisibly, so the lane draws that clip under the
    /// pointer as a ghost to show what a click would place.
    hover_zoom: Option<f64>,
    hover_hide: Option<f64>,
    /// True while a pill drag is in flight; the ghost is hidden then, because
    /// the pointer is moving a clip rather than placing one.
    dragging_effect: bool,
    /// Source-time cut points. The video lane draws one body per segment, so a
    /// split reads as two chips meeting at the cut.
    cuts: Vec<f64>,
    /// Composition start of each segment, one entry per cut piece. Splitting
    /// gives the new piece its own start; dragging a segment changes only that
    /// start, so pieces can be pulled apart and leave a gap.
    segment_starts: Vec<f64>,
    /// The segment under an in-flight drag. It paints last so it rides over
    /// the pieces it crosses.
    moving_segment: Option<usize>,
    /// Held seconds after the footage ends: the last frame frozen on screen.
    /// Only the final segment can carry it, and it only exists once the clip
    /// is extended past the source, exactly as the source editor's freeze
    /// tail does.
    freeze_tail: f64,
    /// Ctrl+Z and Ctrl+Shift+Z. Runtime-only: a reopened recording starts with
    /// nothing to take back.
    history: EditHistory<EditSnapshot>,
}

/// The document as one undo step restores it.
///
/// The media identity travels with the edits because Delete on the video clip
/// empties the whole composition: undoing it has to put the recording back,
/// not just drop the effects it carried. Everything here is either plain data
/// or a refcounted handle (the filmstrip's pixbufs), so a snapshot is cheap
/// enough to take on the first motion event of a drag.
#[derive(Clone, PartialEq)]
struct EditSnapshot {
    path: Option<PathBuf>,
    duration: f64,
    frame_size: Option<(u32, u32)>,
    frames: Vec<gtk4::gdk_pixbuf::Pixbuf>,
    trim_start: f64,
    trim_end: f64,
    cuts: Vec<f64>,
    segment_starts: Vec<f64>,
    freeze_tail: f64,
    zoom_clips: Vec<ZoomClip>,
    hide_clips: Vec<HideClip>,
    zoom_camera: super::zoom::ZoomCameraSettings,
    selected: bool,
    selected_zoom: Option<usize>,
    selected_hide: Option<usize>,
    clipboard: Option<ClipTemplate>,
}

impl ZoomClip {
    fn to_file(self) -> super::project::ZoomClipFile {
        super::project::ZoomClipFile {
            start: self.start,
            end: self.end,
            scale: self.scale,
            center: self.center,
            mode: match self.mode {
                ZoomMode::Auto => super::project::ZoomModeFile::Auto,
                ZoomMode::Manual => super::project::ZoomModeFile::Manual,
            },
            instant: self.instant,
            hidden: self.hidden,
            anchor: self.anchor.map(
                |(source_start, source_end)| super::project::ZoomAnchorFile {
                    source_start,
                    source_end,
                },
            ),
        }
    }

    /// Read one clip back, dropping anything that could not be drawn: a span
    /// shorter than a pill, a scale past the camera's range, a centre that is
    /// not a number.
    fn from_file(file: &super::project::ZoomClipFile) -> Option<Self> {
        let span_ok = file.start.is_finite()
            && file.end.is_finite()
            && file.end - file.start >= MIN_CLIP_SECONDS;
        let center_ok = file.center.0.is_finite() && file.center.1.is_finite();
        let anchor_ok = file
            .anchor
            .is_none_or(|anchor| anchor.source_start.is_finite() && anchor.source_end.is_finite());
        (span_ok && center_ok && anchor_ok).then(|| Self {
            start: file.start.max(0.0),
            end: file.end,
            scale: if file.scale.is_finite() {
                file.scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE)
            } else {
                DEFAULT_ZOOM_SCALE
            },
            center: file.center,
            hidden: file.hidden,
            mode: match file.mode {
                super::project::ZoomModeFile::Auto => ZoomMode::Auto,
                super::project::ZoomModeFile::Manual => ZoomMode::Manual,
            },
            instant: file.instant,
            anchor: file
                .anchor
                .map(|anchor| (anchor.source_start, anchor.source_end)),
        })
    }
}

impl Data {
    fn set_frame_size(&mut self, size: Option<(u32, u32)>) {
        self.frame_size = size.filter(|(width, height)| *width > 0 && *height > 0);
    }

    /// Composition-to-source windows for the follow camera, one per segment.
    fn camera_mapping(&self) -> super::zoom::TimeMapping {
        let boundaries = self.segment_boundaries();
        super::zoom::TimeMapping::new(
            (0..self.segment_count())
                .filter_map(|index| {
                    let (start, end) = self.segment_span(index)?;
                    let &(source_start, source_end) = boundaries.get(index)?;
                    Some((start, end, source_start, source_end))
                })
                .collect(),
        )
    }

    fn frame_center(&self) -> (f64, f64) {
        self.frame_size
            .map(|(width, height)| (f64::from(width) / 2.0, f64::from(height) / 2.0))
            .unwrap_or((0.0, 0.0))
    }

    fn has_media(&self) -> bool {
        self.path.is_some() && self.duration > 0.0
    }

    fn clip_duration(&self) -> f64 {
        (self.trim_end - self.trim_start).max(0.0)
    }

    /// Length of the drawn clip in composition time: the furthest segment end,
    /// plus any held tail. Effects, the playhead and the ruler all work against
    /// this, while the filmstrip is mapped against the footage alone.
    fn total_duration(&self) -> f64 {
        (0..self.segment_count())
            .filter_map(|index| self.segment_span(index))
            .map(|(_, end)| end)
            .fold(0.0, f64::max)
    }

    fn segment_count(&self) -> usize {
        self.segment_boundaries().len()
    }

    fn segment_start(&self, index: usize) -> f64 {
        self.segment_starts
            .get(index)
            .copied()
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Composition span of one segment, hold included on the last piece.
    fn segment_span(&self, index: usize) -> Option<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        let &(source_start, source_end) = boundaries.get(index)?;
        let start = self.segment_start(index);
        let mut end = start + (source_end - source_start).max(0.0);
        if index + 1 == boundaries.len() {
            end += self.freeze_tail;
        }
        Some((start, end))
    }

    /// Composition time to source time through the segment that owns it. A
    /// gap owns nothing, so it answers `None` there.
    fn source_time_at(&self, composition_time: f64) -> Option<f64> {
        let boundaries = self.segment_boundaries();
        for (index, &(source_start, source_end)) in boundaries.iter().enumerate() {
            let start = self.segment_start(index);
            let end = start + (source_end - source_start).max(0.0);
            if composition_time + 1e-9 >= start && composition_time <= end + 1e-9 {
                return Some(
                    (source_start + (composition_time - start)).clamp(source_start, source_end),
                );
            }
        }
        None
    }

    /// The source frame the playhead stands on. A gap between moved pieces
    /// shows the last frame before it, and a hold shows the footage's final
    /// frame, so the preview never flashes black just because the playhead is
    /// over composition time with no footage under it.
    fn playhead_source_time(&self) -> f64 {
        if let Some(source) = self.source_time_at(self.playhead) {
            return source;
        }
        let mut nearest = self.trim_start;
        for (index, &(source_start, source_end)) in self.segment_boundaries().iter().enumerate() {
            let start = self.segment_start(index);
            let end = start + (source_end - source_start).max(0.0);
            if end <= self.playhead + 1e-9 {
                nearest = source_end;
            } else if start > self.playhead {
                break;
            }
        }
        nearest
    }

    /// Move one segment. A lone clip stays at zero: it is the whole
    /// composition, and dead air at the head would only be skipped. Overlaps
    /// are allowed while the drag is in flight and settled on release.
    fn set_segment_start(&mut self, index: usize, start: f64) {
        if index >= self.segment_count() {
            return;
        }
        self.record_continuous_edit();
        let start = if self.segment_count() <= 1 || !start.is_finite() {
            0.0
        } else {
            start.max(0.0)
        };
        if self.segment_starts.len() <= index {
            self.segment_starts.resize(index + 1, 0.0);
        }
        self.segment_starts[index] = start;
        self.clamp_playhead();
        self.reproject_anchored_zooms();
        self.clamp_effects();
        self.clamp_scroll();
    }

    /// On release, push a moved segment to the nearest free spot instead of
    /// leaving it stacked on a neighbour.
    fn settle_segment_start(&mut self, index: usize) {
        let boundaries = self.segment_boundaries();
        let Some(&(source_start, source_end)) = boundaries.get(index) else {
            return;
        };
        let duration = (source_end - source_start).max(0.0);
        let mut start = self.segment_start(index);
        for _ in 0..boundaries.len() {
            let end = start + duration;
            let overlapping = boundaries
                .iter()
                .enumerate()
                .find_map(|(other, &(s0, s1))| {
                    if other == index {
                        return None;
                    }
                    let other_start = self.segment_start(other);
                    let other_end = other_start + (s1 - s0).max(0.0);
                    ranges_overlap(start, end, other_start, other_end)
                        .then_some((other_start, other_end))
                });
            let Some((other_start, other_end)) = overlapping else {
                break;
            };
            let left = (other_start - duration).max(0.0);
            let right = other_end;
            let prefer_right = start + duration * 0.5 >= (other_start + other_end) * 0.5;
            start = if prefer_right || left + duration > other_start + 1e-9 {
                right
            } else {
                left
            };
        }
        if self.segment_starts.len() <= index {
            self.segment_starts.resize(index + 1, 0.0);
        }
        self.segment_starts[index] = start.max(0.0);
        self.clamp_playhead();
        self.reproject_anchored_zooms();
        self.clamp_effects();
        self.clamp_scroll();
    }

    /// Keep one start per cut piece. Shorter lists are filled contiguously so
    /// a fresh split lands where the footage says it should.
    fn normalize_segment_starts(&mut self) {
        let boundaries = self.segment_boundaries();
        let count = boundaries.len();
        if self.segment_starts.len() > count {
            self.segment_starts.truncate(count);
        }
        while self.segment_starts.len() < count {
            let index = self.segment_starts.len();
            if index == 0 {
                self.segment_starts.push(0.0);
                continue;
            }
            let previous_start = self.segment_starts[index - 1];
            let previous_length = boundaries
                .get(index - 1)
                .map(|(start, end)| (end - start).max(0.0))
                .unwrap_or(0.0);
            self.segment_starts.push(previous_start + previous_length);
        }
        if let Some(first) = self.segment_starts.first_mut() {
            if count <= 1 {
                *first = 0.0;
            }
        }
    }

    fn visible_span(&self) -> f64 {
        if !self.has_media() {
            return PLACEHOLDER_SECONDS;
        }
        self.duration / (1.0 + (self.scale.clamp(0.0, 100.0) / 100.0) * FIT_SCALE_RANGE)
    }

    fn canvas_seconds(&self) -> f64 {
        if !self.has_media() {
            return PLACEHOLDER_SECONDS;
        }
        self.total_duration() + self.visible_span()
    }

    fn max_scroll(&self) -> f64 {
        (self.canvas_seconds() - self.visible_span()).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
    }

    fn pixels_per_second(&self, width: f64) -> f64 {
        width.max(1.0) / self.visible_span().max(0.001)
    }

    fn time_to_x(&self, time: f64, width: f64) -> f64 {
        (time - self.scroll) * self.pixels_per_second(width)
    }

    fn x_to_time(&self, x: f64, width: f64) -> f64 {
        (self.scroll + x / self.pixels_per_second(width)).max(0.0)
    }

    fn clamp_playhead(&mut self) {
        self.playhead = self.playhead.clamp(0.0, self.total_duration());
    }

    /// Move the in-point. Every piece after it slides earlier in composition
    /// time, so anchored zooms follow their footage instead of drifting off it.
    fn set_trim_start(&mut self, start: f64) {
        self.record_continuous_edit();
        self.trim_start = start.clamp(0.0, (self.trim_end - MIN_CLIP_SECONDS).max(0.0));
        self.clamp_playhead();
        self.clamp_cuts();
        self.reproject_anchored_zooms();
        self.clamp_effects();
    }

    /// Set the clip's drawn length in composition time, the way the source
    /// editor's `set_trim_end` works: footage grows back out to the source end
    /// first, and anything past that is held as a freeze tail. Dragging the
    /// extend band and the right-hand trim handle both land here.
    fn set_visible_length(&mut self, desired: f64) {
        if !self.has_media() {
            return;
        }
        self.record_continuous_edit();
        let duration = self.duration.max(0.0);
        let final_cut = self.cuts.last().copied().unwrap_or(self.trim_start);
        let floor = (self.trim_start + MIN_CLIP_SECONDS)
            .max(final_cut + MIN_CLIP_SECONDS)
            .min(duration);
        // The out-point belongs to the last kept piece, which may have been
        // moved away from where its footage would otherwise sit, so measure
        // the requested end from that piece rather than from time zero.
        let boundaries = self.segment_boundaries();
        let last = boundaries.len().saturating_sub(1);
        let (last_source_start, _) = boundaries
            .get(last)
            .copied()
            .unwrap_or((self.trim_start, self.trim_end));
        let last_start = self.segment_start(last);
        let value = last_source_start + (desired.max(0.0) - last_start).max(0.0);
        self.trim_end = value.clamp(floor, duration);
        self.freeze_tail = (value - duration).max(0.0);
        self.clamp_playhead();
        self.clamp_cuts();
        self.reproject_anchored_zooms();
        self.clamp_effects();
        self.clamp_scroll();
    }

    fn reset_media(&mut self) {
        self.path = None;
        self.duration = 0.0;
        self.frame_size = None;
        self.trim_start = 0.0;
        self.trim_end = 0.0;
        self.playhead = 0.0;
        self.scroll = 0.0;
        self.scale = 0.0;
        self.playing = false;
        self.selected = false;
        self.hover = None;
        self.frames.clear();
        self.zoom_clips.clear();
        self.hide_clips.clear();
        self.selected_zoom = None;
        self.selected_hide = None;
        self.zoom_selection_epoch = 0;
        self.clipboard = None;
        self.hover_zoom = None;
        self.hover_hide = None;
        self.dragging_effect = false;
        self.zoom_camera = super::zoom::ZoomCameraSettings::default();
        self.cuts.clear();
        self.segment_starts.clear();
        self.moving_segment = None;
        self.freeze_tail = 0.0;
    }

    /// The editable state, as one undo step keeps it.
    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            path: self.path.clone(),
            duration: self.duration,
            frame_size: self.frame_size,
            frames: self.frames.clone(),
            trim_start: self.trim_start,
            trim_end: self.trim_end,
            cuts: self.cuts.clone(),
            segment_starts: self.segment_starts.clone(),
            freeze_tail: self.freeze_tail,
            zoom_clips: self.zoom_clips.clone(),
            hide_clips: self.hide_clips.clone(),
            zoom_camera: self.zoom_camera,
            selected: self.selected,
            selected_zoom: self.selected_zoom,
            selected_hide: self.selected_hide,
            clipboard: self.clipboard,
        }
    }

    /// Take the step a continuous edit is about to change.
    ///
    /// One trim drag or one slider sweep arrives as a stream of updates, so
    /// the step is taken on the first of the burst and the rest leave it
    /// alone.
    fn record_continuous_edit(&mut self) {
        let before = self.snapshot();
        self.history.record_continuous(before);
    }

    /// Record a step for a command that has already been applied, so an edit
    /// the document refused leaves nothing to undo.
    fn record_edit(&mut self, before: EditSnapshot) {
        self.history.record_command(before);
    }

    /// Restore the document to how it stood before the last edit.
    ///
    /// Returns `false` when there is nothing left to take back, which leaves
    /// the document untouched.
    fn undo_edit(&mut self) -> bool {
        let current = self.snapshot();
        let Some(previous) = self.history.undo(current) else {
            return false;
        };
        self.restore(previous);
        true
    }

    /// Replay the edit the last undo took back.
    fn redo_edit(&mut self) -> bool {
        let current = self.snapshot();
        let Some(next) = self.history.redo(current) else {
            return false;
        };
        self.restore(next);
        true
    }

    /// Put a step back on the document. The values were clamped when the step
    /// was captured, but the recording under them may have changed since, so
    /// the same clamps the saved edit list runs on load run again here.
    fn restore(&mut self, snapshot: EditSnapshot) {
        // The filmstrip is a view of the recording, not an edit: keep the one
        // on screen unless the step is putting a different recording back.
        if self.path != snapshot.path {
            self.frames = snapshot.frames;
        }
        self.path = snapshot.path;
        self.duration = snapshot.duration;
        self.frame_size = snapshot.frame_size;
        self.trim_start = snapshot.trim_start;
        self.trim_end = snapshot.trim_end;
        self.cuts = snapshot.cuts;
        self.segment_starts = snapshot.segment_starts;
        self.freeze_tail = snapshot.freeze_tail;
        self.zoom_clips = snapshot.zoom_clips;
        self.hide_clips = snapshot.hide_clips;
        self.zoom_camera = snapshot.zoom_camera;
        self.selected = snapshot.selected;
        self.selected_zoom = snapshot.selected_zoom;
        self.selected_hide = snapshot.selected_hide;
        self.clipboard = snapshot.clipboard;
        self.playing = false;
        // A restored selection is a new one for the panel, which reveals
        // itself when the epoch changes.
        self.zoom_selection_epoch = self.zoom_selection_epoch.wrapping_add(1);
        self.clamp_cuts();
        self.clamp_effects();
        self.clamp_scroll();
        self.clamp_playhead();
    }

    /// The edits as they should be written for this recording, or `None` when
    /// the session still matches a freshly opened video — a file that records
    /// "nothing changed" would only put an empty card in History.
    fn project_file(&self) -> Option<super::project::EditorProjectFile> {
        let path = self.path.as_ref()?;
        let pristine = self.cuts.is_empty()
            && self.zoom_clips.is_empty()
            && self.hide_clips.is_empty()
            && self.freeze_tail <= f64::EPSILON
            && self.trim_start.abs() <= 0.001
            && (self.duration - self.trim_end).abs() <= 0.001
            && self.zoom_camera == super::zoom::ZoomCameraSettings::default();
        if pristine {
            return None;
        }
        let (source_size, source_mtime_secs) = super::project::fingerprint(path)?;
        Some(super::project::EditorProjectFile {
            version: super::project::PROJECT_VERSION,
            source_path: path.clone(),
            source_size,
            source_mtime_secs,
            trim_start_seconds: self.trim_start,
            trim_end_seconds: self.trim_end,
            cuts: self.cuts.clone(),
            segment_starts: self.segment_starts.clone(),
            freeze_tail: self.freeze_tail,
            zoom_clips: self.zoom_clips.iter().map(|clip| clip.to_file()).collect(),
            zoom_camera: super::project::ZoomCameraFile {
                speed: self.zoom_camera.speed,
                smoothness: self.zoom_camera.smoothness,
                start_early: self.zoom_camera.start_early,
            },
            cursor_hide_clips: self
                .hide_clips
                .iter()
                .map(|clip| super::project::HideClipFile {
                    start: clip.start,
                    end: clip.end,
                    hidden: clip.hidden,
                })
                .collect(),
            playhead_seconds: self.playhead,
            timeline_scale: self.scale,
            timeline_scroll_seconds: self.scroll,
        })
    }

    /// Restore a saved edit list onto freshly loaded media. Every value is
    /// clamped to the recording that is on disk, so a project written against
    /// different footage — or one hand-edited into nonsense — still lands
    /// inside the composition instead of dragging the board off it.
    fn apply_project(&mut self, file: &super::project::EditorProjectFile) {
        let duration = self.duration.max(0.0);
        self.trim_start = file.trim_start_seconds.clamp(0.0, duration);
        self.trim_end = file.trim_end_seconds.clamp(self.trim_start, duration);
        self.cuts = file
            .cuts
            .iter()
            .copied()
            .filter(|cut| {
                cut.is_finite()
                    && *cut > self.trim_start + MIN_CLIP_SECONDS
                    && *cut < self.trim_end - MIN_CLIP_SECONDS
            })
            .collect();
        self.cuts.sort_by(f64::total_cmp);
        self.segment_starts = file
            .segment_starts
            .iter()
            .copied()
            .filter(|start| start.is_finite() && *start >= 0.0)
            .collect();
        self.freeze_tail = if file.freeze_tail.is_finite() {
            file.freeze_tail.max(0.0)
        } else {
            0.0
        };
        self.zoom_clips = file
            .zoom_clips
            .iter()
            .filter_map(ZoomClip::from_file)
            .collect();
        self.hide_clips = file
            .cursor_hide_clips
            .iter()
            .filter_map(|clip| {
                let span_ok = clip.start.is_finite()
                    && clip.end.is_finite()
                    && clip.end - clip.start >= MIN_CLIP_SECONDS;
                span_ok.then(|| HideClip {
                    start: clip.start.max(0.0),
                    end: clip.end,
                    hidden: clip.hidden,
                })
            })
            .collect();
        self.zoom_camera = super::zoom::ZoomCameraSettings {
            speed: file.zoom_camera.speed,
            smoothness: file.zoom_camera.smoothness,
            start_early: file.zoom_camera.start_early,
        }
        .clamped();
        self.scale = if file.timeline_scale.is_finite() {
            file.timeline_scale.clamp(0.0, 100.0)
        } else {
            0.0
        };
        self.scroll = if file.timeline_scroll_seconds.is_finite() {
            file.timeline_scroll_seconds
        } else {
            0.0
        };
        self.playhead = if file.playhead_seconds.is_finite() {
            file.playhead_seconds
        } else {
            0.0
        };
        self.clamp_cuts();
        self.clamp_effects();
        self.clamp_scroll();
        self.clamp_playhead();
    }

    /// Source-time spans of the video lane's segments. With no cuts there is
    /// one span; every split adds another.
    fn segment_boundaries(&self) -> Vec<(f64, f64)> {
        if !self.has_media() || self.clip_duration() <= 0.0 {
            return Vec::new();
        }
        let mut boundaries = Vec::with_capacity(self.cuts.len() + 1);
        let mut previous = self.trim_start;
        for &cut in &self.cuts {
            if cut > previous && cut < self.trim_end {
                boundaries.push((previous, cut));
                previous = cut;
            }
        }
        boundaries.push((previous, self.trim_end));
        boundaries
    }

    /// Split the clip at `composition_time`. Refused within 0.1s of a source
    /// end, another cut, or in a gap between moved segments, exactly like the
    /// source editor's split.
    fn add_cut(&mut self, composition_time: f64) -> bool {
        if !self.has_media() {
            return false;
        }
        let Some(source) = self.source_time_at(composition_time) else {
            return false;
        };
        if source <= self.trim_start + 0.1 || source >= self.trim_end - 0.1 {
            return false;
        }
        if self.cuts.iter().any(|&cut| (cut - source).abs() < 0.1) {
            return false;
        }
        let before = self.snapshot();
        let split_index = self.cuts.partition_point(|&cut| cut < source);
        let boundaries = self.segment_boundaries();
        let left_start = self.segment_start(split_index);
        let left_source_start = boundaries
            .get(split_index)
            .map(|(start, _)| *start)
            .unwrap_or(self.trim_start);
        let right_start = left_start + (source - left_source_start).max(0.0);
        self.cuts.insert(split_index, source);
        if self.segment_starts.len() <= split_index {
            self.segment_starts.resize(split_index + 1, left_start);
        }
        self.segment_starts.insert(split_index + 1, right_start);
        self.normalize_segment_starts();
        self.reproject_anchored_zooms();
        self.record_edit(before);
        true
    }

    /// Trimming can leave cuts outside the kept footage; those segments no
    /// longer exist, and their starts go with them.
    fn clamp_cuts(&mut self) {
        let before = self.segment_boundaries();
        let (start, end) = (self.trim_start, self.trim_end);
        self.cuts
            .retain(|cut| *cut > start + 0.1 && *cut < end - 0.1);
        let after = self.segment_boundaries();
        let mut starts = Vec::with_capacity(after.len());
        for &(source_start, _) in &after {
            let inherited = before
                .iter()
                .position(|&(old_start, _)| (old_start - source_start).abs() < 1e-9)
                .map(|index| self.segment_start(index))
                .unwrap_or(0.0);
            starts.push(inherited);
        }
        self.segment_starts = starts;
        self.normalize_segment_starts();
    }

    fn effect_count(&self, track: EffectTrack) -> usize {
        match track {
            EffectTrack::Zoom => self.zoom_clips.len(),
            EffectTrack::Hide => self.hide_clips.len(),
        }
    }

    fn effect_span(&self, track: EffectTrack, index: usize) -> Option<(f64, f64)> {
        match track {
            EffectTrack::Zoom => self
                .zoom_clips
                .get(index)
                .map(|clip| (clip.start, clip.end)),
            EffectTrack::Hide => self
                .hide_clips
                .get(index)
                .map(|clip| (clip.start, clip.end)),
        }
    }

    fn effect_spans(&self, track: EffectTrack) -> Vec<(usize, f64, f64)> {
        (0..self.effect_count(track))
            .filter_map(|index| {
                self.effect_span(track, index)
                    .map(|(start, end)| (index, start, end))
            })
            .collect()
    }

    fn zoom_scale_at(&self, index: usize) -> f64 {
        self.zoom_clips
            .get(index)
            .map(|clip| clip.scale)
            .unwrap_or(DEFAULT_ZOOM_SCALE)
    }

    fn effect_hidden(&self, track: EffectTrack, index: usize) -> bool {
        match track {
            EffectTrack::Zoom => self.zoom_clips.get(index).is_some_and(|clip| clip.hidden),
            EffectTrack::Hide => self.hide_clips.get(index).is_some_and(|clip| clip.hidden),
        }
    }

    fn set_effect_hidden(&mut self, track: EffectTrack, index: usize, hidden: bool) {
        if self.effect_span(track, index).is_none() {
            return;
        }
        let before = self.snapshot();
        match track {
            EffectTrack::Zoom => {
                if let Some(clip) = self.zoom_clips.get_mut(index) {
                    clip.hidden = hidden;
                }
            }
            EffectTrack::Hide => {
                if let Some(clip) = self.hide_clips.get_mut(index) {
                    clip.hidden = hidden;
                }
            }
        }
        self.record_edit(before);
    }

    fn selected_effect(&self, track: EffectTrack) -> Option<usize> {
        match track {
            EffectTrack::Zoom => self.selected_zoom,
            EffectTrack::Hide => self.selected_hide,
        }
    }

    fn select_effect(&mut self, track: EffectTrack, index: Option<usize>) {
        match track {
            EffectTrack::Zoom => self.selected_zoom = index,
            EffectTrack::Hide => self.selected_hide = index,
        }
        if index.is_some() {
            self.selected = false;
        }
        if track == EffectTrack::Zoom && index.is_some() {
            self.zoom_selection_epoch = self.zoom_selection_epoch.wrapping_add(1);
        }
    }

    /// Drop the toolbar's default-length effect at `start`, in composition
    /// time. Refused when there is no room left or it would overlap a sibling,
    /// which is what the source editor does instead of stacking pills.
    fn add_effect(&mut self, track: EffectTrack, start: f64) -> Option<usize> {
        if !self.has_media() {
            return None;
        }
        let limit = self.total_duration();
        if limit <= MIN_CLIP_SECONDS {
            return None;
        }
        let start = start.clamp(0.0, (limit - MIN_CLIP_SECONDS).max(0.0));
        let end = (start + DEFAULT_EFFECT_SECONDS).min(limit);
        let before = self.snapshot();
        let index = self.insert_effect(track, start, end, DEFAULT_ZOOM_SCALE, false)?;
        self.select_effect(track, Some(index));
        self.record_edit(before);
        Some(index)
    }

    /// Insert one effect somewhere in the lane, keeping the lane sorted and
    /// refusing spans that leave the composition or touch a neighbour.
    fn insert_effect(
        &mut self,
        track: EffectTrack,
        start: f64,
        end: f64,
        scale: f64,
        hidden: bool,
    ) -> Option<usize> {
        if !self.has_media() {
            return None;
        }
        let limit = self.total_duration();
        if start < -1e-6 || end > limit + 1e-6 || end - start < MIN_CLIP_SECONDS {
            return None;
        }
        if self
            .effect_spans(track)
            .iter()
            .any(|&(_, other_start, other_end)| ranges_overlap(start, end, other_start, other_end))
        {
            return None;
        }
        let index = self
            .effect_spans(track)
            .iter()
            .position(|&(_, other_start, _)| other_start > start)
            .unwrap_or_else(|| self.effect_count(track));
        match track {
            EffectTrack::Zoom => self.zoom_clips.insert(
                index,
                ZoomClip {
                    start,
                    end,
                    scale,
                    center: self.frame_center(),
                    hidden,
                    mode: ZoomMode::Manual,
                    instant: false,
                    // The user dropped this one where they wanted it.
                    anchor: None,
                },
            ),
            EffectTrack::Hide => self
                .hide_clips
                .insert(index, HideClip { start, end, hidden }),
        }
        Some(index)
    }

    /// Copy one clip onto the menu clipboard without removing it.
    fn copy_effect(&mut self, track: EffectTrack, index: usize) -> bool {
        let Some((start, end)) = self.effect_span(track, index) else {
            return false;
        };
        let clip = self
            .zoom_clips
            .get(index)
            .filter(|_| track == EffectTrack::Zoom);
        self.clipboard = Some(ClipTemplate {
            track,
            duration: (end - start).max(0.0),
            scale: self.zoom_scale_at(index),
            center: clip
                .map(|clip| clip.center)
                .unwrap_or_else(|| self.frame_center()),
            hidden: self.effect_hidden(track, index),
            mode: clip.map(|clip| clip.mode).unwrap_or(ZoomMode::Manual),
            instant: clip.is_some_and(|clip| clip.instant),
        });
        true
    }

    /// Cut is copy-then-remove: the clipboard keeps the clip for Paste.
    fn cut_effect(&mut self, track: EffectTrack, index: usize) -> bool {
        if self.effect_span(track, index).is_none() {
            return false;
        }
        // The clipboard travels with the step, so undoing the cut drops the
        // paste it was waiting for instead of leaving the lane dimmed for an
        // action the user just took back.
        let before = self.snapshot();
        if !self.copy_effect(track, index) {
            return false;
        }
        self.remove_effect_at(track, index);
        self.record_edit(before);
        true
    }

    fn can_paste(&self) -> bool {
        let Some(template) = self.clipboard else {
            return false;
        };
        self.paste_spot_free(template.track, self.playhead, template.duration)
    }

    /// Whether a clip of `duration` starting at `start` would fit on `track`.
    /// The painter uses this to show a placeable ghost differently from one
    /// that would collide, so it has to answer exactly what a paste would do.
    fn paste_spot_free(&self, track: EffectTrack, start: f64, duration: f64) -> bool {
        if !self.has_media() {
            return false;
        }
        let start = start.max(0.0);
        let end = start + duration;
        end <= self.total_duration() + 1e-6
            && end - start >= MIN_CLIP_SECONDS
            && !self
                .effect_spans(track)
                .iter()
                .any(|&(_, other_start, other_end)| {
                    ranges_overlap(start, end, other_start, other_end)
                })
    }

    /// Paste the clipboard at the playhead, on the track it came from.
    fn paste_effect(&mut self) -> Option<usize> {
        let template = self.clipboard?;
        self.paste_effect_at(template.track, self.playhead)
    }

    /// Place the copied clip at `start` on `track`. A mismatch, a collision or
    /// an overrun refuses the placement. Landing it finishes the copy, so the
    /// ghost leaves the lane and the next click adds a clip again.
    fn paste_effect_at(&mut self, track: EffectTrack, start: f64) -> Option<usize> {
        let template = self.clipboard?;
        if template.track != track {
            return None;
        }
        let before = self.snapshot();
        let index = self.insert_effect(
            track,
            start,
            start + template.duration,
            template.scale,
            template.hidden,
        )?;
        if track == EffectTrack::Zoom {
            self.zoom_clips[index].center = template.center;
            self.zoom_clips[index].mode = template.mode;
            self.zoom_clips[index].instant = template.instant;
            self.zoom_clips[index].anchor = None;
        }
        self.clipboard = None;
        self.select_effect(track, Some(index));
        self.record_edit(before);
        Some(index)
    }

    /// Length of the clip waiting on the clipboard, but only for the track the
    /// caller is drawing. `None` means this lane has nothing to place.
    fn clipboard_span(&self, track: EffectTrack) -> Option<f64> {
        self.clipboard
            .filter(|template| template.track == track)
            .map(|template| template.duration)
    }

    fn hover_time(&self, track: EffectTrack) -> Option<f64> {
        match track {
            EffectTrack::Zoom => self.hover_zoom,
            EffectTrack::Hide => self.hover_hide,
        }
    }

    fn set_hover_time(&mut self, track: EffectTrack, time: Option<f64>) {
        match track {
            EffectTrack::Zoom => self.hover_zoom = time,
            EffectTrack::Hide => self.hover_hide = time,
        }
    }

    /// A duplicate lands directly after the clip it copies, the way the
    /// source editor's pair reads as one run.
    fn duplicate_effect(&mut self, track: EffectTrack, index: usize) -> Option<usize> {
        let (start, end) = self.effect_span(track, index)?;
        let duration = (end - start).max(0.0);
        let source = self
            .zoom_clips
            .get(index)
            .filter(|_| track == EffectTrack::Zoom)
            .copied();
        let before = self.snapshot();
        let new_index = self.insert_effect(
            track,
            end,
            end + duration,
            self.zoom_scale_at(index),
            self.effect_hidden(track, index),
        )?;
        if track == EffectTrack::Zoom {
            self.zoom_clips[new_index].center = source
                .map(|clip| clip.center)
                .unwrap_or_else(|| self.frame_center());
            self.zoom_clips[new_index].mode =
                source.map(|clip| clip.mode).unwrap_or(ZoomMode::Manual);
            self.zoom_clips[new_index].instant = source.is_some_and(|clip| clip.instant);
            // The copy sits where the user dropped it, not on the original's
            // footage.
            self.zoom_clips[new_index].anchor = None;
        }
        self.select_effect(track, Some(new_index));
        self.record_edit(before);
        Some(new_index)
    }

    fn remove_effect(&mut self, track: EffectTrack, index: usize) {
        if self.effect_span(track, index).is_none() {
            return;
        }
        let before = self.snapshot();
        self.remove_effect_at(track, index);
        self.record_edit(before);
    }

    /// Remove one clip without taking a step; the caller owns the step.
    fn remove_effect_at(&mut self, track: EffectTrack, index: usize) {
        match track {
            EffectTrack::Zoom => {
                if index >= self.zoom_clips.len() {
                    return;
                }
                self.zoom_clips.remove(index);
                self.selected_zoom = shift_selection(self.selected_zoom, index);
            }
            EffectTrack::Hide => {
                if index >= self.hide_clips.len() {
                    return;
                }
                self.hide_clips.remove(index);
                self.selected_hide = shift_selection(self.selected_hide, index);
            }
        }
    }

    /// The outline the preview draws while a Manual zoom is being placed: the
    /// clip's scale and focus point, shown over an unzoomed preview. Playing
    /// puts the outline away, because the picture is moving.
    fn placement(&self) -> Option<(f64, (f64, f64))> {
        if self.playing {
            return None;
        }
        let clip = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get(index))?;
        (clip.mode == ZoomMode::Manual).then_some((clip.scale, clip.center))
    }

    /// Returns whether the focus point actually moved, so the caller can skip a
    /// repaint when a drag event repeats the same pixel.
    fn set_selected_zoom_center(&mut self, center: (f64, f64)) -> bool {
        let Some(index) = self.selected_zoom else {
            return false;
        };
        let clamp_to_frame = |value: f64, extent: u32| {
            if value.is_finite() {
                value.clamp(0.0, f64::from(extent))
            } else {
                f64::from(extent) / 2.0
            }
        };
        let Some(clip) = self.zoom_clips.get(index) else {
            return false;
        };
        if clip.mode != ZoomMode::Manual {
            return false;
        }
        let center = match self.frame_size {
            Some((width, height)) => (
                clamp_to_frame(center.0, width),
                clamp_to_frame(center.1, height),
            ),
            None => return false,
        };
        if clip.center == center {
            return false;
        }
        self.record_continuous_edit();
        self.zoom_clips[index].center = center;
        true
    }

    fn set_selected_zoom_mode(&mut self, mode: ZoomMode, visible_center: Option<(f64, f64)>) {
        let Some(index) = self.selected_zoom else {
            return;
        };
        let Some(clip) = self.zoom_clips.get(index) else {
            return;
        };
        if clip.mode == mode {
            return;
        }
        let before = self.snapshot();
        if let Some(center) = visible_center {
            self.zoom_clips[index].center = center;
        }
        self.zoom_clips[index].mode = mode;
        self.record_edit(before);
    }

    fn set_selected_zoom_scale(&mut self, scale: f64) {
        let Some(index) = self.selected_zoom else {
            return;
        };
        if self.zoom_clips.get(index).is_none() {
            return;
        }
        self.record_continuous_edit();
        self.zoom_clips[index].scale = scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
    }

    fn set_selected_zoom_instant(&mut self, instant: bool) {
        let Some(index) = self.selected_zoom else {
            return;
        };
        let Some(clip) = self.zoom_clips.get(index) else {
            return;
        };
        if clip.instant == instant {
            return;
        }
        let before = self.snapshot();
        self.zoom_clips[index].instant = instant;
        self.record_edit(before);
    }

    fn reset_zoom_animation(&mut self) {
        let before = self.snapshot();
        self.zoom_camera = super::zoom::ZoomCameraSettings::default();
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.instant = false;
        }
        self.record_edit(before);
    }

    fn update_zoom_camera(&mut self, update: impl FnOnce(&mut super::zoom::ZoomCameraSettings)) {
        let mut settings = self.zoom_camera;
        update(&mut settings);
        let settings = settings.clamped();
        if settings == self.zoom_camera {
            return;
        }
        self.record_continuous_edit();
        self.zoom_camera = settings;
    }

    /// Delete on the video clip empties the composition. The window still has
    /// the document open, so reloading the recording brings the clip back;
    /// undo also brings it back, which is why the media travels in the step.
    fn remove_video_clip(&mut self) {
        if !self.has_media() {
            return;
        }
        let before = self.snapshot();
        self.reset_media();
        self.record_edit(before);
    }

    /// Where a source interval sits in composition time, for placing a detected
    /// zoom where its click actually is: trimming and cutting move one off the
    /// other. The interval is clamped to the piece that owns its start, and
    /// footage the edits removed yields `None`.
    fn composition_span_for_source(&self, start: f64, end: f64) -> Option<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        // At an exact cut both pieces contain the timestamp; the later source
        // start owns it, as everywhere else.
        let index = boundaries
            .iter()
            .enumerate()
            .filter(|(_, &(source_start, source_end))| {
                start + 1e-9 >= source_start && start <= source_end + 1e-9
            })
            .max_by(|(_, left), (_, right)| left.0.total_cmp(&right.0))
            .map(|(index, _)| index)?;
        let (source_start, source_end) = boundaries[index];
        let end = end.min(source_end);
        if end - start <= 1e-9 {
            return None;
        }
        let composition_start = self.segment_start(index);
        Some((
            composition_start + (start - source_start),
            composition_start + (end - source_start),
        ))
    }

    /// The footage a composition span plays, for anchoring a detected zoom to
    /// it. The span is clamped to the piece that owns its start: a click near a
    /// cut cannot claim footage the cut removed.
    fn source_anchor(&self, start: f64, end: f64) -> Option<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        for (index, &(source_start, source_end)) in boundaries.iter().enumerate() {
            let composition_start = self.segment_start(index);
            let composition_end = composition_start + (source_end - source_start).max(0.0);
            let overlap_start = start.max(composition_start);
            let overlap_end = end.min(composition_end);
            if overlap_end - overlap_start <= 1e-9 {
                continue;
            }
            return Some((
                source_start + (overlap_start - composition_start),
                source_start + (overlap_end - composition_start),
            ));
        }
        None
    }

    /// Put every anchored clip back on the footage it was placed for.
    ///
    /// Trimming the head, cutting, or moving a piece all change which footage
    /// sits under a given composition time. Anchored clips follow their
    /// footage; a clip that lost all of it — the piece it framed was cut away —
    /// is dropped. Clips the user placed or dragged carry no anchor and never
    /// move. Overlapping zooms have no defined blending, so a moved clip gives
    /// way to one the user placed.
    fn reproject_anchored_zooms(&mut self) {
        if self.zoom_clips.iter().all(|clip| clip.anchor.is_none()) {
            return;
        }
        let selected = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get(index))
            .map(|clip| (clip.anchor, clip.start));
        let boundaries = self.segment_boundaries();
        let starts: Vec<f64> = (0..boundaries.len())
            .map(|index| self.segment_start(index))
            .collect();
        let mut reprojected: Vec<ZoomClip> = Vec::with_capacity(self.zoom_clips.len());
        for clip in self.zoom_clips.drain(..) {
            let Some((anchor_start, anchor_end)) = clip.anchor else {
                reprojected.push(clip);
                continue;
            };
            // At an exact cut both neighbouring pieces contain the timestamp;
            // the later source start owns it, as placement does.
            let owner = boundaries
                .iter()
                .enumerate()
                .filter(|(_, &(source_start, source_end))| {
                    anchor_start + 1e-9 >= source_start && anchor_start <= source_end + 1e-9
                })
                .max_by(|(_, left), (_, right)| left.0.total_cmp(&right.0))
                .map(|(index, _)| index);
            let Some(index) = owner else {
                continue;
            };
            let (source_start, source_end) = boundaries[index];
            let start = anchor_start.max(source_start);
            let end = anchor_end.min(source_end);
            if end - start <= f64::EPSILON {
                continue;
            }
            let composition_start = starts.get(index).copied().unwrap_or(0.0);
            reprojected.push(ZoomClip {
                start: composition_start + (start - source_start),
                end: composition_start + (end - source_start),
                ..clip
            });
        }
        reprojected.sort_by(|left, right| left.start.total_cmp(&right.start));

        let mut resolved: Vec<ZoomClip> = Vec::with_capacity(reprojected.len());
        for clip in reprojected {
            if let Some(previous) = resolved.last() {
                if ranges_overlap(clip.start, clip.end, previous.start, previous.end) {
                    if clip.anchor.is_some() {
                        continue;
                    }
                    if previous.anchor.is_some() {
                        resolved.pop();
                    }
                }
            }
            resolved.push(clip);
        }
        self.zoom_clips = resolved;
        self.selected_zoom = match selected {
            Some((anchor, start)) => anchor
                .and_then(|anchor| {
                    self.zoom_clips
                        .iter()
                        .position(|clip| clip.anchor == Some(anchor))
                })
                .or_else(|| {
                    self.zoom_clips
                        .iter()
                        .position(|clip| (clip.start - start).abs() < 1e-6)
                }),
            None => self.selected_zoom,
        }
        .filter(|index| *index < self.zoom_clips.len());
    }

    /// Resize one pill, clamped between its neighbours so the lanes stay
    /// sorted and no two pills overlap.
    ///
    /// Resizing a zoom pill by hand takes it off its anchor: the user said
    /// where it belongs.
    fn set_effect_range(&mut self, track: EffectTrack, index: usize, start: f64, end: f64) {
        if self.effect_span(track, index).is_none() {
            return;
        }
        self.record_continuous_edit();
        let limit = self.total_duration();
        let lower = if index > 0 {
            self.effect_span(track, index - 1)
                .map(|(_, previous_end)| previous_end)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let upper = self
            .effect_span(track, index + 1)
            .map(|(next_start, _)| next_start)
            .unwrap_or(limit)
            .min(limit);
        let start = start.clamp(lower, (upper - MIN_CLIP_SECONDS).max(lower));
        let end = end.clamp((start + MIN_CLIP_SECONDS).min(upper), upper);
        match track {
            EffectTrack::Zoom => {
                if let Some(clip) = self.zoom_clips.get_mut(index) {
                    clip.start = start;
                    clip.end = end;
                    // The user said where this one belongs; no later
                    // composition edit may pull it back to the footage.
                    clip.anchor = None;
                }
            }
            EffectTrack::Hide => {
                if let Some(clip) = self.hide_clips.get_mut(index) {
                    clip.start = start;
                    clip.end = end;
                }
            }
        }
    }

    /// Slide one pill to `start`, keeping its length and its neighbours' space.
    fn move_effect(&mut self, track: EffectTrack, index: usize, start: f64) {
        let Some((clip_start, clip_end)) = self.effect_span(track, index) else {
            return;
        };
        let duration = (clip_end - clip_start).max(0.0);
        let limit = self.total_duration();
        let lower = if index > 0 {
            self.effect_span(track, index - 1)
                .map(|(_, previous_end)| previous_end)
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let upper = self
            .effect_span(track, index + 1)
            .map(|(next_start, _)| next_start)
            .unwrap_or(limit)
            .min(limit);
        let start = start.clamp(lower, (upper - duration).max(lower));
        self.set_effect_range(track, index, start, start + duration);
    }

    /// Video trims shorten the composition, so effects past the new end are
    /// pulled back to it rather than left hanging off the lane.
    fn clamp_effects(&mut self) {
        let limit = self.total_duration();
        self.zoom_clips.retain_mut(|clip| {
            clip.start = clip.start.min(limit);
            clip.end = clip.end.min(limit);
            clip.end - clip.start >= MIN_CLIP_SECONDS
        });
        self.hide_clips.retain_mut(|clip| {
            clip.start = clip.start.min(limit);
            clip.end = clip.end.min(limit);
            clip.end - clip.start >= MIN_CLIP_SECONDS
        });
        if self
            .selected_zoom
            .is_some_and(|index| index >= self.zoom_clips.len())
        {
            self.selected_zoom = None;
        }
        if self
            .selected_hide
            .is_some_and(|index| index >= self.hide_clips.len())
        {
            self.selected_hide = None;
        }
    }

    fn filmstrip_tiles(&self) -> Vec<(f64, f64)> {
        if self.frames.is_empty() || self.duration <= 0.0 {
            return Vec::new();
        }
        let count = self.frames.len();
        let step = self.duration / count as f64;
        (0..count)
            .map(|index| {
                let start = thumbnail_timestamp(self.duration, index, count);
                let end = if index + 1 < count {
                    thumbnail_timestamp(self.duration, index + 1, count).max(start)
                } else {
                    (start + step).min(self.duration).max(start)
                };
                (start, end)
            })
            .collect()
    }
}

/// Sample time for a filmstrip frame. The last one stops just before the
/// reported end so ffmpeg still decodes a real frame.
fn thumbnail_timestamp(duration: f64, index: usize, count: usize) -> f64 {
    if count <= 1 || !duration.is_finite() || duration <= 0.0 {
        return 0.0;
    }
    let epsilon = (duration * 0.02).clamp(0.04, 0.15);
    let usable_end = (duration - epsilon).max(0.0);
    usable_end * (index as f64 / (count - 1) as f64)
}

/// One tick of preview state: what the stage shows and what it outlines.
#[derive(Clone, Copy)]
pub(super) struct PreviewFrame {
    pub source_seconds: f64,
    /// The composition's length, for the export size estimate.
    pub composition_seconds: f64,
    pub playing: bool,
    pub camera: super::zoom::Camera,
    /// Set while a Manual zoom's focus point is being placed: the clip's scale
    /// and its focus point, outlined over an unzoomed preview.
    pub placement: Option<(f64, (f64, f64))>,
}

/// Everything the Zoom panel shows: which clip is selected, how it frames,
/// the camera-wide settings, and whether this recording can follow a pointer
/// at all.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ZoomPanelState {
    pub selected: Option<usize>,
    /// Changes on every new selection, including a second click on the pill
    /// that was already selected.
    pub selection_epoch: u64,
    pub clip: Option<ZoomClip>,
    pub camera: super::zoom::ZoomCameraSettings,
    pub auto_available: bool,
}

/// The composition an export reads.
pub(super) struct ExportFootage {
    pub source: PathBuf,
    pub trim_start: f64,
    pub trim_end: f64,
    pub cuts: Vec<f64>,
    pub zooms: Vec<ZoomClip>,
    pub zoom_camera: super::zoom::ZoomCameraSettings,
}

#[derive(Clone)]
pub(super) struct Timeline {
    pub root: GtkBox,
    data: Rc<RefCell<Data>>,
    cursor: super::cursor::CursorModel,
    camera_cache: Rc<RefCell<super::zoom::ZoomCameraCache>>,
    generation: Rc<Cell<u64>>,
    zoom_scale: Scale,
    repaint: Rc<dyn Fn()>,
    frame_sink: Rc<RefCell<Option<Rc<dyn Fn(PreviewFrame)>>>>,
    zoom_listener: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    /// The saved edit list: what was last written, and whether the editor has
    /// repainted since, which is when it may need writing again.
    project: super::project::ProjectSync,
}

impl Timeline {
    /// Send the source time under the playhead, the composition's length, the
    /// transport state, and the camera to the rest of the editor. The first
    /// call carries the state already on screen.
    pub fn set_frame_sink(&self, sink: Rc<dyn Fn(PreviewFrame)>) {
        *self.frame_sink.borrow_mut() = Some(sink.clone());
        sink(self.current_frame());
    }

    /// What the stage should show right now. The 50 ms tick reads it every
    /// step, and an edit calls [`Timeline::publish_frame`] so a drag or a
    /// slider is on screen in the same event instead of at the next tick.
    fn current_frame(&self) -> PreviewFrame {
        let data = self.data.borrow();
        let placement = data.placement();
        let camera = match placement {
            Some(_) => (1.0, data.frame_center()),
            None => {
                let frame = data
                    .frame_size
                    .map(|(width, height)| (f64::from(width), f64::from(height)))
                    .unwrap_or((0.0, 0.0));
                self.camera_cache.borrow_mut().evaluate(
                    &data.zoom_clips,
                    frame,
                    &data.camera_mapping(),
                    data.zoom_camera,
                    data.playhead,
                    || self.cursor.sidecar(),
                )
            }
        };
        PreviewFrame {
            source_seconds: data.playhead_source_time(),
            composition_seconds: data.total_duration(),
            playing: data.playing,
            camera,
            placement,
        }
    }

    fn publish_frame(&self) {
        let Some(sink) = self.frame_sink.borrow().clone() else {
            return;
        };
        sink(self.current_frame());
    }

    /// Length of the recording's footage, before any trimming.
    pub fn source_duration(&self) -> f64 {
        self.data.borrow().duration
    }

    /// Length of the composition as it stands on the timeline.
    pub fn composition_duration(&self) -> f64 {
        self.data.borrow().total_duration()
    }

    /// What an export of this composition reads: the recording, its kept
    /// footage, the cuts that split it, and the zoom clips that shape it.
    pub fn export_footage(&self) -> Option<ExportFootage> {
        let data = self.data.borrow();
        let path = data.path.clone()?;
        Some(ExportFootage {
            source: path,
            trim_start: data.trim_start,
            trim_end: data.trim_end,
            cuts: data.cuts.clone(),
            zooms: data.zoom_clips.clone(),
            zoom_camera: data.zoom_camera,
        })
    }

    /// What the Zoom panel shows, and whether Auto is possible for this
    /// recording: following the pointer needs the recorded pointer track.
    pub fn zoom_panel_state(&self) -> ZoomPanelState {
        let auto_available = self
            .cursor
            .sidecar()
            .is_some_and(|sidecar| !sidecar.pointer.is_empty());
        let data = self.data.borrow();
        ZoomPanelState {
            selected: data.selected_zoom,
            selection_epoch: data.zoom_selection_epoch,
            clip: data
                .selected_zoom
                .and_then(|index| data.zoom_clips.get(index))
                .copied(),
            camera: data.zoom_camera,
            auto_available,
        }
    }

    /// Ask to be told whenever the zoom track, its selection, or the camera
    /// settings change. The panel refreshes from it, and this first call
    /// delivers the state already on screen.
    pub fn set_zoom_listener(&self, listener: Rc<dyn Fn()>) {
        *self.zoom_listener.borrow_mut() = Some(listener.clone());
        listener();
    }

    /// Follow the pointer, or hold the focus point the clip carries.
    ///
    /// Switching to Manual keeps what the preview currently shows: the camera
    /// may be chasing a pointer position that is nowhere near the clip's
    /// stored centre, and the user expects the framing they can see to stay.
    pub fn set_zoom_mode(&self, mode: ZoomMode) {
        let visible_center = if mode == ZoomMode::Manual {
            self.camera_at_playhead().map(|camera| camera.1)
        } else {
            None
        };
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.set_selected_zoom_mode(mode, visible_center);
        }
        (self.repaint)();
        self.publish_frame();
    }

    pub fn set_zoom_scale(&self, scale: f64) {
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.set_selected_zoom_scale(scale);
        }
        (self.repaint)();
        self.publish_frame();
    }

    pub fn set_zoom_instant(&self, instant: bool) {
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.set_selected_zoom_instant(instant);
        }
        (self.repaint)();
        self.publish_frame();
    }

    /// Put the selected Manual clip's focus point where the pointer dropped it.
    pub fn set_selected_zoom_center(&self, center: (f64, f64)) {
        let moved = self.data.borrow_mut().set_selected_zoom_center(center);
        if moved {
            self.publish_frame();
        }
    }

    /// Back to the animated defaults: the shared spring and an eased clip.
    pub fn reset_zoom_animation(&self) {
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.reset_zoom_animation();
        }
        (self.repaint)();
        self.publish_frame();
    }

    /// Edit the camera-wide settings. They are clamped as they are stored, so
    /// the panel and the camera always agree on what is in force.
    pub fn update_zoom_camera(&self, update: impl FnOnce(&mut super::zoom::ZoomCameraSettings)) {
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.update_zoom_camera(update);
        }
        (self.repaint)();
        self.publish_frame();
    }

    /// The camera as the preview shows it right now.
    fn camera_at_playhead(&self) -> Option<super::zoom::Camera> {
        let data = self.data.borrow();
        let (width, height) = data.frame_size?;
        let frame = (f64::from(width), f64::from(height));
        Some(self.camera_cache.borrow_mut().evaluate(
            &data.zoom_clips,
            frame,
            &data.camera_mapping(),
            data.zoom_camera,
            data.playhead,
            || self.cursor.sidecar(),
        ))
    }

    /// Write this recording's edits now. Called when another recording replaces
    /// this one and when the window closes, both of which land before the
    /// repaint-driven autosave's next tick.
    pub fn flush_project(&self) {
        let snapshot = self.data.borrow().project_file();
        self.project.sync(snapshot, true);
    }

    /// Take the document back to how it stood before the last edit.
    ///
    /// Returns `false` when there is nothing left to take back, so the key
    /// that asked can travel on to whoever else might handle it.
    pub fn undo(&self) -> bool {
        let changed = self.data.borrow_mut().undo_edit();
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// Replay the edit the last undo took back.
    pub fn redo(&self) -> bool {
        let changed = self.data.borrow_mut().redo_edit();
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// Swap the recording the timeline describes. `None` returns it to the
    /// empty placeholder scale.
    ///
    /// The outgoing recording's edits are written first, then the incoming
    /// recording's saved edit list is restored on top of its probed lengths.
    pub fn set_document(&self, path: Option<&Path>) {
        self.flush_project();
        self.camera_cache.borrow_mut().clear();
        let source = path.filter(|path| path.is_file()).map(Path::to_path_buf);
        self.generation.set(self.generation.get() + 1);

        let mut data = self.data.borrow_mut();
        data.reset_media();
        // Steps belong to the recording they were taken on; the incoming one
        // starts with nothing to take back.
        data.history.clear();
        let mut duration = 0.0;
        let mut restored = None;
        if let Some(source) = &source {
            duration = ffmpeg::probe_duration(source).unwrap_or(0.0);
            data.path = Some(source.clone());
            data.duration = duration;
            data.trim_end = duration;
            if duration > 0.0 {
                restored = super::project::load_project(source);
                if let Some(file) = &restored {
                    data.apply_project(file);
                }
            }
        }
        data.clamp_cuts();
        data.clamp_scroll();
        data.clamp_playhead();
        let scale = data.scale;
        drop(data);

        self.project.adopt(restored);
        self.zoom_scale.set_value(scale);
        (self.repaint)();
        if let Some(source) = source {
            if duration > 0.0 {
                self.spawn_filmstrip(source, duration);
            }
        }
    }

    fn spawn_filmstrip(&self, source: PathBuf, duration: f64) {
        let directory = ffmpeg::thumbnail_cache_dir(&source).join("filmstrip");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(ffmpeg::generate_filmstrip(&source, &directory, duration));
        });

        let data = self.data.clone();
        let repaint = self.repaint.clone();
        let generation = self.generation.clone();
        let started_at = generation.get();
        gtk4::glib::timeout_add_local(Duration::from_millis(80), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    if generation.get() == started_at {
                        if let Ok(paths) = result {
                            let frames = paths
                                .iter()
                                .filter_map(|path| gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok())
                                .collect();
                            data.borrow_mut().frames = frames;
                        }
                        repaint();
                    }
                    gtk4::glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => gtk4::glib::ControlFlow::Break,
            }
        });
    }
}

pub(super) fn build(
    window: &ApplicationWindow,
    canvas: &AspectFrame,
    scale: f64,
    prefers_dark: bool,
    cursor: super::cursor::CursorModel,
    source_size: Rc<Cell<Option<(u32, u32)>>>,
) -> Timeline {
    let data = Rc::new(RefCell::new(Data::default()));
    let camera_cache = Rc::new(RefCell::new(super::zoom::ZoomCameraCache::default()));
    let project = super::project::ProjectSync::default();

    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.set_valign(Align::End);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let playhead_clock = Label::new(Some(&format_clock(0.0)));
    playhead_clock.add_css_class("recording-editor-timeline-clock");
    let duration_clock = Label::new(Some(&format_clock(0.0)));
    duration_clock.add_css_class("recording-editor-timeline-clock");

    let (play, play_icon) = transport_button("media-playback-start-symbolic", &t("Play"));
    play.add_css_class("recording-editor-timeline-play");
    let (skip_back, _) = transport_button("media-skip-backward-symbolic", &t("Skip back 1s"));
    let (skip_forward, _) = transport_button("media-skip-forward-symbolic", &t("Skip forward 1s"));

    let zoom = tool_button(
        "zoom-fit-best-symbolic",
        &t("Zoom"),
        &t("Add zoom at playhead"),
    );
    let hide = tool_button(
        "view-conceal-symbolic",
        &t("Hide"),
        &t("Hide cursor at playhead"),
    );
    let split = tool_button(
        icon_names::custom::SQUARE_SPLIT_HORIZONTAL_SYMBOLIC,
        &t("Split"),
        &t("Split at playhead"),
    );
    let detect = tool_button(
        icon_names::custom::WAND_SPARKLES_SYMBOLIC,
        &t("Detect"),
        &t("Detect automatic zooms from recorded clicks"),
    );

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_hexpand(true);

    let left = GtkBox::new(Orientation::Horizontal, 4);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.append(&zoom);
    left.append(&hide);
    left.append(&split);
    left.append(&detect);

    let center = GtkBox::new(Orientation::Horizontal, 8);
    center.add_css_class("recording-editor-timeline-transport");
    center.set_halign(Align::Center);
    center.set_hexpand(false);
    center.append(&playhead_clock);
    center.append(&skip_back);
    center.append(&play);
    center.append(&skip_forward);
    center.append(&duration_clock);

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.add_css_class("recording-editor-timeline-zoom-row");
    right.set_halign(Align::End);
    right.set_hexpand(true);
    let balance = GtkBox::new(Orientation::Horizontal, 0);
    balance.set_size_request(0, -1);
    right.append(&balance);

    let (zoom_pill, zoom_scale, zoom_track, zoom_thumb) = build_zoom_pill(scale);
    right.append(&zoom_pill);

    toolbar.append(&left);
    toolbar.append(&center);
    toolbar.append(&right);
    centre_transport_over_canvas(window, canvas, &center, &balance);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-card-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, (36.0 * scale).round() as i32);
    ruler.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_ruler(context, width, height, &data.borrow(), prefers_dark, scale);
        }
    });

    let video_track = DrawingArea::new();
    video_track.add_css_class("recording-editor-card-video-track");
    video_track.set_hexpand(true);
    video_track.set_size_request(-1, (56.0 * scale).round() as i32);
    video_track.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_video_clip(context, width, height, &data.borrow(), prefers_dark, scale);
        }
    });

    let zoom_track_lane = DrawingArea::new();
    zoom_track_lane.add_css_class("recording-editor-card-zoom-track");
    zoom_track_lane.set_hexpand(true);
    zoom_track_lane.set_size_request(-1, (56.0 * scale).round() as i32);
    zoom_track_lane.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_effect_clips(
                context,
                width,
                height,
                &data.borrow(),
                EffectTrack::Zoom,
                prefers_dark,
                scale,
            );
        }
    });

    let hide_track = DrawingArea::new();
    hide_track.add_css_class("recording-editor-card-hide-track");
    hide_track.set_hexpand(true);
    hide_track.set_size_request(-1, (56.0 * scale).round() as i32);
    hide_track.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_effect_clips(
                context,
                width,
                height,
                &data.borrow(),
                EffectTrack::Hide,
                prefers_dark,
                scale,
            );
        }
    });

    let lanes = GtkBox::new(Orientation::Vertical, (10.0 * scale).round() as i32);
    lanes.add_css_class("recording-editor-card-tracks");
    lanes.set_hexpand(true);
    lanes.append(&video_track);
    lanes.append(&zoom_track_lane);
    lanes.append(&hide_track);

    // The frosted extend area is one layer over the three lanes, so it spans
    // the video, zoom and hide rows (and the gaps between them) instead of
    // stopping at the video lane's seam. It sits above the lanes but lets
    // input through, because the lanes own the pointer.
    let band_hover = Rc::new(Cell::new(false));
    let extend = DrawingArea::new();
    extend.add_css_class("recording-editor-card-extend");
    extend.set_hexpand(true);
    extend.set_vexpand(true);
    extend.set_can_target(false);
    extend.set_draw_func({
        let data = data.clone();
        let band_hover = band_hover.clone();
        move |_, context, width, height| {
            draw_extend_band(
                context,
                width,
                height,
                &data.borrow(),
                prefers_dark,
                band_hover.get(),
                scale,
            );
        }
    });

    let lanes_overlay = Overlay::new();
    lanes_overlay.set_hexpand(true);
    lanes_overlay.set_child(Some(&lanes));
    lanes_overlay.add_overlay(&extend);

    let tracks = GtkBox::new(Orientation::Vertical, (10.0 * scale).round() as i32);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&lanes_overlay);

    let board = Overlay::new();
    board.add_css_class("recording-editor-card-board");
    board.set_hexpand(true);
    board.set_child(Some(&tracks));

    let playhead = DrawingArea::new();
    playhead.add_css_class("recording-editor-card-playhead");
    playhead.set_hexpand(true);
    playhead.set_vexpand(true);
    playhead.set_can_target(false);
    playhead.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_playhead(context, width, height, &data.borrow(), prefers_dark, scale);
        }
    });
    board.add_overlay(&playhead);

    let well = GtkBox::new(Orientation::Vertical, 0);
    well.add_css_class("recording-editor-timeline-well");
    well.set_hexpand(true);
    let adjustment = Adjustment::new(0.0, 0.0, PLACEHOLDER_SECONDS, 0.1, 1.0, 1.0);
    let scrollbar = Scrollbar::new(Orientation::Horizontal, Some(&adjustment));
    scrollbar.add_css_class("recording-editor-timeline-scroll");
    scrollbar.set_hexpand(true);
    well.append(&scrollbar);

    card.append(&toolbar);
    card.append(&board);
    card.append(&well);
    dock.append(&card);

    let scroll_syncing = Rc::new(Cell::new(false));
    // The Zoom panel follows every change to the zoom track or its selection.
    // Repainting is the one place every mutation already passes through.
    let zoom_listener: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let repaint: Rc<dyn Fn()> = {
        let data = data.clone();
        let ruler = ruler.clone();
        let video_track = video_track.clone();
        let zoom_track_lane = zoom_track_lane.clone();
        let hide_track = hide_track.clone();
        let playhead = playhead.clone();
        let extend = extend.clone();
        let playhead_clock = playhead_clock.clone();
        let duration_clock = duration_clock.clone();
        let adjustment = adjustment.clone();
        let scroll_syncing = scroll_syncing.clone();
        let zoom_listener = zoom_listener.clone();
        let project = project.clone();
        Rc::new(move || {
            {
                let data = data.borrow();
                playhead_clock.set_text(&format_clock(data.playhead));
                duration_clock.set_text(&format_clock(data.total_duration()));
                scroll_syncing.set(true);
                adjustment.set_upper(data.canvas_seconds().max(data.visible_span()).max(0.001));
                adjustment.set_page_size(data.visible_span().max(0.001));
                adjustment.set_value(data.scroll);
                scroll_syncing.set(false);
            }
            ruler.queue_draw();
            video_track.queue_draw();
            zoom_track_lane.queue_draw();
            hide_track.queue_draw();
            extend.queue_draw();
            playhead.queue_draw();
            if let Some(listener) = zoom_listener.borrow().as_ref() {
                listener();
            }
            // Every mutation repaints, so this is the one place that can tell
            // the autosave a change may be waiting to be written.
            project.mark_changed();
        })
    };

    // Autosave. The repaint above only notes that something may have changed;
    // this tick decides whether it is worth a write by comparing against the
    // file on disk, so a hover, a scroll or a playhead tick never rewrites it.
    {
        let data = data.clone();
        let project = project.clone();
        gtk4::glib::timeout_add_local(Duration::from_secs(2), move || {
            if project.take_changed() {
                let snapshot = data.borrow().project_file();
                project.sync(snapshot, false);
            }
            gtk4::glib::ControlFlow::Continue
        });
    }

    let sync_play_icon = {
        let data = data.clone();
        let play_icon = play_icon.clone();
        Rc::new(move || {
            let playing = data.borrow().playing;
            set_play_icon(&play_icon, playing);
        })
    };

    bind_playhead(
        &ruler,
        data.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
    );
    bind_clip(
        &video_track,
        data.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
        scale,
    );
    bind_board_hover(
        &tracks,
        data.clone(),
        playhead.clone(),
        extend.clone(),
        band_hover.clone(),
        scale,
    );
    bind_clip_cursor(&video_track, data.clone(), scale);
    bind_effect_lane(
        &zoom_track_lane,
        EffectTrack::Zoom,
        data.clone(),
        source_size.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
        scale,
    );
    bind_effect_lane(
        &hide_track,
        EffectTrack::Hide,
        data.clone(),
        source_size.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
        scale,
    );

    for (button, track) in [(&zoom, EffectTrack::Zoom), (&hide, EffectTrack::Hide)] {
        button.connect_clicked({
            let data = data.clone();
            let source_size = source_size.clone();
            let repaint = repaint.clone();
            move |_| {
                {
                    let mut data = data.borrow_mut();
                    data.set_frame_size(source_size.get());
                    let at = data.playhead;
                    data.add_effect(track, at);
                }
                repaint();
            }
        });
    }

    detect.connect_clicked({
        let data = data.clone();
        let cursor = cursor.clone();
        let source_size = source_size.clone();
        let repaint = repaint.clone();
        move |_| {
            let Some(sidecar) = cursor.sidecar() else {
                return;
            };
            let Some((width, height)) = source_size.get() else {
                return;
            };
            let duration = {
                let data = data.borrow();
                if !data.has_media() {
                    return;
                }
                data.duration
            };
            if sidecar.clicks.is_empty() && sidecar.pointer.is_empty() {
                return;
            }

            let width_f64 = f64::from(width);
            let height_f64 = f64::from(height);
            let clicks = auto_zoom::clicks_from_sidecar(&sidecar, width_f64, height_f64);
            let pointer = auto_zoom::pointer_from_sidecar(&sidecar, width_f64, height_f64);
            let detections = auto_zoom::automatic_zooms(&clicks, &pointer, duration);
            if detections.is_empty() {
                return;
            }

            let inserted = {
                let mut data = data.borrow_mut();
                data.set_frame_size(Some((width, height)));
                // Detections carry source seconds; the timeline places in
                // composition seconds, and a trim or a cut moves the two apart.
                let detections: Vec<auto_zoom::AutoZoom> = detections
                    .into_iter()
                    .filter_map(|detection| {
                        let (start, end) =
                            data.composition_span_for_source(detection.start, detection.end)?;
                        Some(auto_zoom::AutoZoom {
                            start,
                            end,
                            center_time: detection.center_time,
                            center: detection.center,
                            scale: detection.scale,
                        })
                    })
                    .collect();
                if detections.is_empty() {
                    return;
                }
                let placements = place_auto_zooms(&data.zoom_clips, &detections, (width, height));
                // One Detect is one step: undoing has to put the track back
                // the way a single click found it, not clip by clip.
                let before = data.snapshot();
                let mut inserted = 0;
                for placement in placements {
                    let anchor = data.source_anchor(placement.start, placement.end);
                    if let Some(index) = data.insert_effect(
                        EffectTrack::Zoom,
                        placement.start,
                        placement.end,
                        placement.scale,
                        placement.hidden,
                    ) {
                        data.zoom_clips[index].center = placement.center;
                        data.zoom_clips[index].mode = placement.mode;
                        data.zoom_clips[index].instant = placement.instant;
                        data.zoom_clips[index].anchor = anchor;
                        inserted += 1;
                    }
                }
                if inserted > 0 {
                    data.record_edit(before);
                }
                inserted
            };
            if inserted > 0 {
                repaint();
            }
        }
    });

    split.connect_clicked({
        let data = data.clone();
        let repaint = repaint.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                let at = data.playhead;
                data.add_cut(at);
            }
            repaint();
        }
    });

    play.connect_clicked({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                if !data.has_media() {
                    return;
                }
                if data.playing {
                    data.playing = false;
                } else {
                    if data.playhead >= data.total_duration() - 1e-3 {
                        data.playhead = 0.0;
                    }
                    data.playing = true;
                }
            }
            sync_play_icon();
            repaint();
        }
    });

    skip_back.connect_clicked({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                data.playing = false;
                data.playhead = (data.playhead - 1.0).max(0.0);
            }
            sync_play_icon();
            repaint();
        }
    });

    skip_forward.connect_clicked({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                data.playing = false;
                data.playhead = (data.playhead + 1.0).min(data.total_duration());
            }
            sync_play_icon();
            repaint();
        }
    });

    zoom_scale.connect_value_changed({
        let data = data.clone();
        let repaint = repaint.clone();
        move |scale| {
            {
                let mut data = data.borrow_mut();
                data.scale = scale.value().clamp(0.0, 100.0);
                data.clamp_scroll();
            }
            repaint();
        }
    });

    adjustment.connect_value_changed({
        let data = data.clone();
        let repaint = repaint.clone();
        let scroll_syncing = scroll_syncing.clone();
        move |adjustment| {
            if scroll_syncing.get() {
                return;
            }
            {
                let mut data = data.borrow_mut();
                data.scroll = adjustment.value();
                data.clamp_scroll();
            }
            repaint();
        }
    });

    let wheel = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::HORIZONTAL,
    );
    wheel.connect_scroll({
        let data = data.clone();
        let repaint = repaint.clone();
        move |_, dx, dy| {
            let delta = if dx.abs() > f64::EPSILON { dx } else { dy };
            if delta.abs() < f64::EPSILON {
                return gtk4::glib::Propagation::Proceed;
            }
            {
                let mut data = data.borrow_mut();
                data.scroll += data.visible_span() * 0.08 * delta;
                data.clamp_scroll();
            }
            repaint();
            gtk4::glib::Propagation::Stop
        }
    });
    board.add_controller(wheel);

    // Playback, and the zoom thumb's placement, which follows the track on a
    // resize just as the source editor's 50 ms tick does. The same tick feeds
    // the preview the source frame under the playhead, and the stage the
    // composition's current length for its export estimate.
    let frame_sink: Rc<RefCell<Option<Rc<dyn Fn(PreviewFrame)>>>> = Rc::new(RefCell::new(None));
    gtk4::glib::timeout_add_local(Duration::from_millis(50), {
        let data = data.clone();
        let cursor = cursor.clone();
        let camera_cache = camera_cache.clone();
        let source_size = source_size.clone();
        let repaint = repaint.clone();
        let play_icon = play_icon.clone();
        let zoom_track = zoom_track.clone();
        let zoom_thumb = zoom_thumb.clone();
        let zoom_scale = zoom_scale.clone();
        let frame_sink = frame_sink.clone();
        let last_playing = Rc::new(Cell::new(false));
        move || {
            let (playing, changed, preview) = {
                let mut data = data.borrow_mut();
                data.set_frame_size(source_size.get());
                let was_playing = data.playing;
                if was_playing {
                    data.playhead += 0.05;
                    if data.playhead >= data.total_duration() {
                        data.playhead = 0.0;
                        data.playing = false;
                    }
                }
                let composition_time = data.playhead;
                let placement = data.placement();
                let camera = match placement {
                    Some(_) => (1.0, data.frame_center()),
                    None => {
                        let frame = data
                            .frame_size
                            .map(|(width, height)| (f64::from(width), f64::from(height)))
                            .unwrap_or((0.0, 0.0));
                        camera_cache.borrow_mut().evaluate(
                            &data.zoom_clips,
                            frame,
                            &data.camera_mapping(),
                            data.zoom_camera,
                            composition_time,
                            || cursor.sidecar(),
                        )
                    }
                };
                (
                    data.playing,
                    was_playing || data.playing,
                    PreviewFrame {
                        source_seconds: data.playhead_source_time(),
                        composition_seconds: data.total_duration(),
                        playing: data.playing,
                        camera,
                        placement,
                    },
                )
            };
            if last_playing.get() != playing {
                set_play_icon(&play_icon, playing);
                last_playing.set(playing);
            }
            place_zoom_thumb(&zoom_track, &zoom_thumb, zoom_scale.value() / 100.0);
            if changed {
                repaint();
            }
            if let Some(sink) = frame_sink.borrow().as_ref() {
                sink(preview);
            }
            gtk4::glib::ControlFlow::Continue
        }
    });

    (repaint)();

    Timeline {
        root: dock,
        data,
        cursor,
        camera_cache,
        generation: Rc::new(Cell::new(0)),
        zoom_scale,
        repaint,
        frame_sink,
        zoom_listener,
        project,
    }
}

fn transport_button(icon_name: &str, tooltip: &str) -> (Button, Image) {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    (button, icon)
}

fn tool_button(icon_name: &str, label: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-tool");
    button.set_tooltip_text(Some(tooltip));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let text = Label::new(Some(label));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    row.append(&icon);
    row.append(&text);
    button.set_child(Some(&row));
    button
}

fn set_play_icon(icon: &Image, playing: bool) {
    icon.set_icon_name(Some(if playing {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    }));
}

fn build_zoom_pill(scale: f64) -> (GtkBox, Scale, GtkBox, GtkBox) {
    let glyph = Image::from_icon_name("zoom-in-symbolic");
    glyph.set_pixel_size((14.0 * scale).round() as i32);
    glyph.set_can_target(false);
    let glyph_slot = GtkBox::new(Orientation::Horizontal, 0);
    glyph_slot.add_css_class("recording-editor-timeline-zoom-glyph");
    glyph_slot.set_halign(Align::Start);
    glyph_slot.set_valign(Align::Center);
    glyph_slot.append(&glyph);

    let thumb = GtkBox::new(Orientation::Horizontal, 0);
    thumb.add_css_class("recording-editor-timeline-zoom-bar");
    thumb.set_valign(Align::Center);
    let track = GtkBox::new(Orientation::Horizontal, 0);
    track.set_hexpand(true);
    track.append(&thumb);

    let zoom = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 10.0);
    zoom.add_css_class("recording-editor-timeline-zoom");
    zoom.set_draw_value(false);
    zoom.set_value(0.0);
    zoom.set_valign(Align::Center);
    zoom.set_tooltip_text(Some(&t("Zoom")));

    let area = Overlay::new();
    area.set_hexpand(true);
    area.set_child(Some(&glyph_slot));
    area.add_overlay(&track);
    area.add_overlay(&zoom);

    let pill = GtkBox::new(Orientation::Horizontal, 0);
    pill.add_css_class("recording-editor-timeline-zoom-pill");
    pill.append(&area);
    (pill, zoom, track, thumb)
}

/// Put the zoom thumb at `fraction` (0..1) along the pill's track.
fn place_zoom_thumb(track: &GtkBox, thumb: &GtkBox, fraction: f64) {
    let width = track.allocated_width();
    let thumb_width = thumb.allocated_width();
    if width <= 0 || thumb_width <= 0 || width <= thumb_width {
        return;
    }
    let travel = (width - thumb_width) as f64 * fraction.clamp(0.0, 1.0);
    let offset = travel.round() as i32;
    if thumb.margin_start() != offset {
        thumb.set_margin_start(offset);
    }
}

/// Line the transport up with the canvas, not with the dock it sits in, so the
/// play button lands under the picture rather than half a sidebar to its right.
fn centre_transport_over_canvas(
    window: &ApplicationWindow,
    canvas: &AspectFrame,
    transport: &GtkBox,
    balance: &GtkBox,
) {
    let window = window.clone();
    let canvas = canvas.clone();
    let transport = transport.clone();
    let balance = balance.clone();
    transport.clone().add_tick_callback(move |_, _| {
        if canvas.allocated_width() <= 1 || transport.allocated_width() <= 1 {
            return gtk4::glib::ControlFlow::Continue;
        }
        let (Some(canvas_origin), Some(transport_origin)) = (
            canvas.compute_point(&window, &gtk4::graphene::Point::new(0.0, 0.0)),
            transport.compute_point(&window, &gtk4::graphene::Point::new(0.0, 0.0)),
        ) else {
            return gtk4::glib::ControlFlow::Continue;
        };
        let canvas_centre = canvas_origin.x() as f64 + canvas.allocated_width() as f64 / 2.0;
        let transport_centre =
            transport_origin.x() as f64 + transport.allocated_width() as f64 / 2.0;
        let drift = transport_centre - canvas_centre;
        if drift > 0.5 {
            balance.set_size_request((drift * 2.0).round() as i32, -1);
        }
        gtk4::glib::ControlFlow::Break
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragKind {
    None,
    Seek,
    TrimStart,
    TrimEnd,
    /// Drag anywhere past the clip's edge to grow it, with any part past the
    /// source becoming a held last frame.
    Extend,
    /// Drag a split piece by its body. A lone clip is the whole composition,
    /// so only a cut arrangement can move.
    MoveSegment(usize),
}

fn seek_to_x(data: &mut Data, width: f64, x: f64) {
    data.playing = false;
    let time = data.x_to_time(x.clamp(0.0, width), width);
    data.playhead = time.clamp(0.0, data.total_duration());
}

fn near_playhead(data: &Data, width: f64, x: f64, scale: f64) -> bool {
    (x - data.time_to_x(data.playhead, width)).abs() <= PLAYHEAD_HIT * scale
}

/// Snap a drop point onto the playhead when the pointer is close enough, so a
/// click near the read-out line lands exactly on it.
fn snap_time_to_playhead(data: &Data, width: f64, time: f64, scale: f64) -> f64 {
    let threshold = PLAYHEAD_SNAP * scale / data.pixels_per_second(width).max(1e-6);
    let snapped = if (time - data.playhead).abs() <= threshold {
        data.playhead
    } else {
        time
    };
    snapped.max(0.0)
}

/// Snap a moving span onto the playhead by either end, preferring whichever
/// end is closer. A moved piece therefore clicks into the read-out line as it
/// passes, the same as a pill does.
fn snap_range_start_to_playhead(
    data: &Data,
    width: f64,
    start: f64,
    duration: f64,
    scale: f64,
) -> f64 {
    let threshold = PLAYHEAD_SNAP * scale / data.pixels_per_second(width).max(1e-6);
    let end = start + duration;
    let to_start = (data.playhead - start).abs();
    let to_end = (data.playhead - end).abs();
    let can_snap_start = to_start <= threshold;
    let aligned_start = (data.playhead - duration).max(0.0);
    let can_snap_end = to_end <= threshold && data.playhead - duration >= -1e-9;
    let snapped = if can_snap_start && (!can_snap_end || to_start <= to_end) {
        data.playhead.max(0.0)
    } else if can_snap_end {
        aligned_start
    } else {
        start
    };
    snapped.max(0.0)
}

/// Right edge of the drawn clip, including any freeze hold. Everything from
/// here to the lane's edge is the extend band.
fn clip_end_x(data: &Data, width: f64, _scale: f64) -> f64 {
    if !data.has_media() {
        return 0.0;
    }
    data.time_to_x(data.total_duration(), width).max(0.0)
}

/// True when `x` is in the extend band: the mover at the clip's edge plus the
/// frosted affordance past it. The band spans every lane, so a drag here
/// extends the clip whichever row the pointer is over.
fn in_extend_band(data: &Data, width: f64, x: f64, scale: f64) -> bool {
    data.has_media() && x >= clip_end_x(data, width, scale) - HANDLE_HIT * scale
}

/// The cut piece whose drawn body contains `x`.
fn segment_at_x(data: &Data, width: f64, x: f64, scale: f64) -> Option<usize> {
    for index in 0..data.segment_count() {
        let Some((start, end)) = data.segment_span(index) else {
            continue;
        };
        let x0 = data.time_to_x(start, width);
        let x1 = data.time_to_x(end, width).max(x0 + 8.0 * scale);
        if x >= x0 && x <= x1 {
            return Some(index);
        }
    }
    None
}

/// The clip's own trim handles: the in-point on the first segment and the
/// out-point on the last. Every other edge belongs to a cut, and dragging a
/// piece by its body is how those move.
fn clip_edge_at(data: &Data, width: f64, x: f64, scale: f64) -> Option<DragKind> {
    if !data.has_media() {
        return None;
    }
    let (first_start, _) = data.segment_span(0)?;
    let (_, last_end) = data.segment_span(data.segment_count().saturating_sub(1))?;
    let left = data.time_to_x(first_start, width) + HANDLE_INSET * scale;
    let right = data.time_to_x(last_end, width) - HANDLE_INSET * scale - HANDLE_WIDTH * scale;
    let hit = HANDLE_HIT * scale;
    if x >= left - hit && x <= left + HANDLE_WIDTH * scale + hit {
        Some(DragKind::TrimStart)
    } else if x >= right - hit && x <= right + HANDLE_WIDTH * scale + hit {
        Some(DragKind::TrimEnd)
    } else {
        None
    }
}

fn inside_clip(data: &Data, width: f64, x: f64, scale: f64) -> bool {
    segment_at_x(data, width, x, scale).is_some()
}

fn bind_playhead(
    ruler: &DrawingArea,
    data: Rc<RefCell<Data>>,
    repaint: Rc<dyn Fn()>,
    sync_play_icon: Rc<dyn Fn()>,
) {
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            seek_to_x(&mut data.borrow_mut(), width, x);
            sync_play_icon();
            repaint();
        }
    });
    ruler.add_controller(click);

    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            seek_to_x(&mut data.borrow_mut(), width, x);
            sync_play_icon();
            repaint();
        }
    });
    drag.connect_drag_update({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            seek_to_x(&mut data.borrow_mut(), width, start_x + offset_x);
            sync_play_icon();
            repaint();
        }
    });
    ruler.add_controller(drag);
}

fn bind_clip(
    lane: &DrawingArea,
    data: Rc<RefCell<Data>>,
    repaint: Rc<dyn Fn()>,
    sync_play_icon: Rc<dyn Fn()>,
    scale: f64,
) {
    // Selection and single clicks. The playhead wins over the clip body, but
    // never over a trim handle.
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let data = data.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            {
                let mut data = data.borrow_mut();
                let edge = clip_edge_at(&data, width, x, scale);
                if in_extend_band(&data, width, x, scale) {
                    // The band is the clip's extend affordance, so pressing it
                    // selects the clip rather than seeking.
                    data.selected = true;
                    data.selected_zoom = None;
                    data.selected_hide = None;
                } else if near_playhead(&data, width, x, scale) && edge.is_none() {
                    seek_to_x(&mut data, width, x);
                } else if edge.is_some() || inside_clip(&data, width, x, scale) {
                    data.selected = true;
                    data.selected_zoom = None;
                    data.selected_hide = None;
                } else {
                    data.selected = false;
                }
            }
            sync_play_icon();
            repaint();
        }
    });
    lane.add_controller(click);

    let menu = GestureClick::new();
    menu.set_button(3);
    menu.connect_pressed({
        let data = data.clone();
        let repaint = repaint.clone();
        let lane = lane.clone();
        move |_, _, x, y| {
            let width = lane.allocated_width().max(1) as f64;
            {
                let mut data = data.borrow_mut();
                if !inside_clip(&data, width, x, scale) {
                    return;
                }
                data.selected = true;
                data.selected_zoom = None;
                data.selected_hide = None;
            }
            repaint();
            show_clip_menu(
                &lane,
                MenuTarget::Video,
                x,
                y,
                data.clone(),
                repaint.clone(),
            );
        }
    });
    lane.add_controller(menu);

    let drag_kind = Rc::new(Cell::new(DragKind::None));
    let anchor = Rc::new(Cell::new(0.0));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let anchor = anchor.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut data = data.borrow_mut();
            let edge = clip_edge_at(&data, width, x, scale);
            let segment = segment_at_x(&data, width, x, scale);
            let kind = if in_extend_band(&data, width, x, scale) {
                DragKind::Extend
            } else if near_playhead(&data, width, x, scale) && edge.is_none() {
                DragKind::Seek
            } else {
                match edge {
                    Some(kind) => kind,
                    None if data.segment_count() > 1 => match segment {
                        Some(index) => DragKind::MoveSegment(index),
                        None => DragKind::None,
                    },
                    None if segment.is_some() => {
                        data.selected = true;
                        data.selected_zoom = None;
                        data.selected_hide = None;
                        DragKind::None
                    }
                    None => DragKind::None,
                }
            };
            match kind {
                DragKind::TrimStart | DragKind::TrimEnd | DragKind::Extend => {
                    data.selected = true;
                    data.selected_zoom = None;
                    data.selected_hide = None;
                    data.playing = false;
                    anchor.set(match kind {
                        DragKind::TrimStart => data.trim_start,
                        _ => data.total_duration(),
                    });
                }
                DragKind::MoveSegment(index) => {
                    data.selected = true;
                    data.selected_zoom = None;
                    data.selected_hide = None;
                    data.playing = false;
                    data.moving_segment = Some(index);
                    anchor.set(data.segment_start(index));
                }
                _ => {}
            }
            drag_kind.set(kind);
            drop(data);
            sync_play_icon();
            repaint();
        }
    });
    drag.connect_drag_update({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let anchor = anchor.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            {
                let mut data = data.borrow_mut();
                match drag_kind.get() {
                    DragKind::Seek => seek_to_x(&mut data, width, start_x + offset_x),
                    DragKind::TrimStart => {
                        let delta = offset_x / data.pixels_per_second(width).max(1e-6);
                        data.set_trim_start(anchor.get() + delta);
                    }
                    DragKind::TrimEnd | DragKind::Extend => {
                        let delta = offset_x / data.pixels_per_second(width).max(1e-6);
                        data.set_visible_length(anchor.get() + delta);
                    }
                    DragKind::MoveSegment(index) => {
                        let delta = offset_x / data.pixels_per_second(width).max(1e-6);
                        let duration = data
                            .segment_span(index)
                            .map(|(start, end)| (end - start).max(0.0))
                            .unwrap_or(0.0);
                        let start = snap_range_start_to_playhead(
                            &data,
                            width,
                            anchor.get() + delta,
                            duration,
                            scale,
                        );
                        data.set_segment_start(index, start);
                    }
                    DragKind::None => {}
                }
            }
            sync_play_icon();
            repaint();
        }
    });
    drag.connect_drag_end({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let repaint = repaint.clone();
        move |_, _, _| {
            let kind = drag_kind.get();
            {
                let mut data = data.borrow_mut();
                if let DragKind::MoveSegment(index) = kind {
                    data.settle_segment_start(index);
                }
                data.moving_segment = None;
            }
            drag_kind.set(DragKind::None);
            repaint();
        }
    });
    lane.add_controller(drag);
}

/// The red read-out line belongs to the board, not to one lane: motion over
/// the ruler, a gap, a lane or a pill all report the same time, so the line
/// stays put as the pointer crosses between them instead of blinking out.
fn bind_board_hover(
    tracks: &GtkBox,
    data: Rc<RefCell<Data>>,
    playhead: DrawingArea,
    extend: DrawingArea,
    band_hover: Rc<Cell<bool>>,
    scale: f64,
) {
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let data = data.clone();
        let tracks = tracks.clone();
        let playhead = playhead.clone();
        let extend = extend.clone();
        let band_hover = band_hover.clone();
        move |_, x, _| {
            let width = tracks.allocated_width().max(1) as f64;
            let band = {
                let mut data = data.borrow_mut();
                data.hover = Some(data.x_to_time(x, width));
                in_extend_band(&data, width, x, scale)
            };
            if band_hover.get() != band {
                band_hover.set(band);
                extend.queue_draw();
            }
            playhead.queue_draw();
        }
    });
    motion.connect_leave({
        let data = data.clone();
        let playhead = playhead.clone();
        let extend = extend.clone();
        let band_hover = band_hover.clone();
        move |_| {
            data.borrow_mut().hover = None;
            if band_hover.get() {
                band_hover.set(false);
                extend.queue_draw();
            }
            playhead.queue_draw();
        }
    });
    tracks.add_controller(motion);
}

fn bind_clip_cursor(lane: &DrawingArea, data: Rc<RefCell<Data>>, scale: f64) {
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let data = data.clone();
        let lane = lane.clone();
        move |_, x, _| {
            let data = data.borrow();
            let width = lane.allocated_width().max(1) as f64;
            let resize = clip_edge_at(&data, width, x, scale).is_some()
                || in_extend_band(&data, width, x, scale);
            let grab = !resize
                && data.segment_count() > 1
                && segment_at_x(&data, width, x, scale).is_some();
            lane.set_cursor_from_name(Some(if resize {
                "ew-resize"
            } else if grab {
                "grab"
            } else {
                "default"
            }));
        }
    });
    motion.connect_leave({
        let lane = lane.clone();
        move |_| lane.set_cursor_from_name(None)
    });
    lane.add_controller(motion);
}

fn ranges_overlap(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> bool {
    a_start < b_end && a_end > b_start
}

fn place_auto_zooms(
    existing: &[ZoomClip],
    detections: &[auto_zoom::AutoZoom],
    frame_size: (u32, u32),
) -> Vec<ZoomClip> {
    let (width, height) = frame_size;
    if width < 2 || height < 2 {
        return Vec::new();
    }

    let mut ordered: Vec<auto_zoom::AutoZoom> = detections.to_vec();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut spans: Vec<(f64, f64)> = existing.iter().map(|clip| (clip.start, clip.end)).collect();
    let mut clips = Vec::new();
    for detection in ordered {
        if !detection.start.is_finite()
            || !detection.end.is_finite()
            || detection.start < 0.0
            || detection.end - detection.start < auto_zoom::MIN_WINDOW_SECONDS
            || !detection.center.0.is_finite()
            || !detection.center.1.is_finite()
        {
            continue;
        }
        if spans
            .iter()
            .any(|&(start, end)| ranges_overlap(detection.start, detection.end, start, end))
        {
            continue;
        }

        let scale = if detection.scale.is_finite() {
            detection.scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE)
        } else {
            auto_zoom::DEFAULT_SCALE.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE)
        };
        let center = clamp_frame_center(detection.center, scale, frame_size);
        spans.push((detection.start, detection.end));
        clips.push(ZoomClip {
            start: detection.start,
            end: detection.end,
            scale,
            center,
            hidden: false,
            mode: ZoomMode::Auto,
            instant: false,
            // Detect anchors the clip in the caller, where the timeline knows
            // which footage it read the click from.
            anchor: None,
        });
    }
    clips
}

fn clamp_frame_center(center: (f64, f64), scale: f64, frame_size: (u32, u32)) -> (f64, f64) {
    let (width, height) = frame_size;
    let crop_width = even_crop_dimension(
        ((f64::from(width) / scale).round() as u32)
            .max(2)
            .min(width),
    );
    let crop_height = even_crop_dimension(
        ((f64::from(height) / scale).round() as u32)
            .max(2)
            .min(height),
    );
    let half_width = f64::from(crop_width) / 2.0;
    let half_height = f64::from(crop_height) / 2.0;
    (
        center
            .0
            .clamp(half_width, (f64::from(width) - half_width).max(half_width)),
        center.1.clamp(
            half_height,
            (f64::from(height) - half_height).max(half_height),
        ),
    )
}

fn even_crop_dimension(value: u32) -> u32 {
    let value = value.max(2);
    if value.is_multiple_of(2) {
        value
    } else {
        value - 1
    }
}

fn shift_selection(selected: Option<usize>, removed: usize) -> Option<usize> {
    match selected {
        Some(index) if index == removed => None,
        Some(index) if index > removed => Some(index - 1),
        other => other,
    }
}

fn effect_edge_at(
    data: &Data,
    track: EffectTrack,
    width: f64,
    x: f64,
    scale: f64,
) -> Option<(usize, bool)> {
    for (index, start, end) in data.effect_spans(track) {
        let x0 = data.time_to_x(start, width);
        let x1 = data.time_to_x(end, width).max(x0 + 22.0 * scale);
        let left = x0 + HANDLE_INSET * scale;
        let right = x1 - HANDLE_INSET * scale - HANDLE_WIDTH * scale;
        let hit = HANDLE_HIT * scale;
        if x >= left - hit && x <= left + HANDLE_WIDTH * scale + hit {
            return Some((index, true));
        }
        if x >= right - hit && x <= right + HANDLE_WIDTH * scale + hit {
            return Some((index, false));
        }
    }
    None
}

fn effect_at(data: &Data, track: EffectTrack, width: f64, x: f64, scale: f64) -> Option<usize> {
    data.effect_spans(track)
        .into_iter()
        .find(|&(_, start, end)| {
            let x0 = data.time_to_x(start, width);
            let x1 = data.time_to_x(end, width).max(x0 + 22.0 * scale);
            x >= x0 && x <= x1
        })
        .map(|(index, _, _)| index)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectDrag {
    None,
    Seek,
    TrimStart(usize),
    TrimEnd(usize),
    Move(usize),
    /// The frosted band past the clip, shared by every lane.
    Extend,
}

fn bind_effect_lane(
    lane: &DrawingArea,
    track: EffectTrack,
    data: Rc<RefCell<Data>>,
    source_size: Rc<Cell<Option<(u32, u32)>>>,
    repaint: Rc<dyn Fn()>,
    sync_play_icon: Rc<dyn Fn()>,
    scale: f64,
) {
    // Press on a pill selects it; press on empty lane space drops the same
    // default-length clip the tool row would, at the pointer instead of the
    // playhead — the source editor's lane click does the same.
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let data = data.clone();
        let source_size = source_size.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            {
                let mut data = data.borrow_mut();
                data.set_frame_size(source_size.get());
                if in_extend_band(&data, width, x, scale) {
                    // The band is the video's extend affordance, not the pill
                    // lane: a click there must not add a clip.
                } else if near_playhead(&data, width, x, scale) {
                    seek_to_x(&mut data, width, x);
                } else if let Some((index, _)) = effect_edge_at(&data, track, width, x, scale) {
                    data.select_effect(track, Some(index));
                } else if let Some(index) = effect_at(&data, track, width, x, scale) {
                    data.select_effect(track, Some(index));
                } else {
                    // A copied clip takes the click; otherwise the click adds
                    // a clip the way it always has. Placing first means a click
                    // that lands the ghost never leaves an extra clip behind.
                    let at = snap_time_to_playhead(&data, width, data.x_to_time(x, width), scale);
                    if data.paste_effect_at(track, at).is_none()
                        && data.add_effect(track, at).is_none()
                    {
                        data.select_effect(track, None);
                    }
                }
            }
            sync_play_icon();
            repaint();
        }
    });
    lane.add_controller(click);

    // Right-click acts on the pill in place; a miss opens nothing rather than
    // acting on whatever happened to be selected.
    let menu = GestureClick::new();
    menu.set_button(3);
    menu.connect_pressed({
        let data = data.clone();
        let repaint = repaint.clone();
        let lane = lane.clone();
        move |_, _, x, y| {
            let width = lane.allocated_width().max(1) as f64;
            let Some(index) = effect_at(&data.borrow(), track, width, x, scale) else {
                return;
            };
            data.borrow_mut().select_effect(track, Some(index));
            repaint();
            let target = match track {
                EffectTrack::Zoom => MenuTarget::Zoom(index),
                EffectTrack::Hide => MenuTarget::Hide(index),
            };
            show_clip_menu(&lane, target, x, y, data.clone(), repaint.clone());
        }
    });
    lane.add_controller(menu);

    let drag_kind = Rc::new(Cell::new(EffectDrag::None));
    let origin = Rc::new(Cell::new(0.0));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let origin = origin.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut data = data.borrow_mut();
            let kind = if in_extend_band(&data, width, x, scale) {
                data.playing = false;
                origin.set(data.total_duration());
                EffectDrag::Extend
            } else if near_playhead(&data, width, x, scale) {
                seek_to_x(&mut data, width, x);
                EffectDrag::Seek
            } else if let Some((index, is_start)) = effect_edge_at(&data, track, width, x, scale) {
                data.select_effect(track, Some(index));
                data.playing = false;
                if let Some((start, _)) = data.effect_span(track, index) {
                    origin.set(start);
                }
                if is_start {
                    EffectDrag::TrimStart(index)
                } else {
                    EffectDrag::TrimEnd(index)
                }
            } else if let Some(index) = effect_at(&data, track, width, x, scale) {
                data.select_effect(track, Some(index));
                data.playing = false;
                if let Some((start, _)) = data.effect_span(track, index) {
                    origin.set(start);
                }
                EffectDrag::Move(index)
            } else {
                EffectDrag::None
            };
            drag_kind.set(kind);
            // Dragging is not placing: the ghost would sit under the pointer
            // that is moving a clip, so hide it for the length of the drag.
            data.dragging_effect = true;
            data.set_hover_time(track, None);
            drop(data);
            sync_play_icon();
            repaint();
        }
    });
    drag.connect_drag_update({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let origin = origin.clone();
        let repaint = repaint.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            {
                let mut data = data.borrow_mut();
                let time = data.x_to_time(start_x + offset_x, width);
                match drag_kind.get() {
                    EffectDrag::Seek => seek_to_x(&mut data, width, start_x + offset_x),
                    EffectDrag::TrimStart(index) => {
                        if let Some((_, end)) = data.effect_span(track, index) {
                            data.set_effect_range(track, index, time, end);
                        }
                    }
                    EffectDrag::TrimEnd(index) => {
                        if let Some((start, _)) = data.effect_span(track, index) {
                            data.set_effect_range(track, index, start, time);
                        }
                    }
                    EffectDrag::Move(index) => {
                        let delta = offset_x / data.pixels_per_second(width).max(1e-6);
                        data.move_effect(track, index, origin.get() + delta);
                    }
                    EffectDrag::Extend => {
                        let delta = offset_x / data.pixels_per_second(width).max(1e-6);
                        data.set_visible_length(origin.get() + delta);
                    }
                    EffectDrag::None => {}
                }
            }
            sync_play_icon();
            repaint();
        }
    });
    drag.connect_drag_end({
        let data = data.clone();
        let drag_kind = drag_kind.clone();
        let repaint = repaint.clone();
        move |_, _, _| {
            drag_kind.set(EffectDrag::None);
            data.borrow_mut().dragging_effect = false;
            repaint();
        }
    });
    lane.add_controller(drag);

    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let data = data.clone();
        let lane = lane.clone();
        move |_, x, _| {
            let width = lane.allocated_width().max(1) as f64;
            let mut data = data.borrow_mut();
            let edge = effect_edge_at(&data, track, width, x, scale).is_some();
            let over_pill = edge || effect_at(&data, track, width, x, scale).is_some();
            let near = near_playhead(&data, width, x, scale);
            let band = in_extend_band(&data, width, x, scale);
            // The ghost only shows over empty lane space, where a click would
            // place the copied clip.
            let hover = if near || over_pill || band {
                None
            } else {
                Some(data.x_to_time(x, width))
            };
            data.set_hover_time(track, hover);
            drop(data);
            lane.set_cursor_from_name(Some(if edge || band { "ew-resize" } else { "default" }));
            lane.queue_draw();
        }
    });
    motion.connect_leave({
        let data = data.clone();
        let lane = lane.clone();
        move |_| {
            data.borrow_mut().set_hover_time(track, None);
            lane.set_cursor_from_name(None);
            lane.queue_draw();
        }
    });
    lane.add_controller(motion);
}

/// What a right-click landed on. The menu is built per target, the way the
/// source editor's single clip menu covers all three tracks.
#[derive(Clone, Copy)]
enum MenuTarget {
    Video,
    Zoom(usize),
    Hide(usize),
}

struct MenuItem {
    icon: &'static str,
    label: String,
    danger: bool,
    action: Rc<dyn Fn()>,
}

/// Pop up the clip menu at `(x, y)` in `anchor`'s space. It opens upward: the
/// tracks sit at the bottom of the editor, so a downward menu would fall off
/// the window.
fn show_clip_menu(
    anchor: &impl IsA<gtk4::Widget>,
    target: MenuTarget,
    x: f64,
    y: f64,
    data: Rc<RefCell<Data>>,
    repaint: Rc<dyn Fn()>,
) {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-clip-menu-popover");
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-clip-menu");

    let mut items = Vec::new();
    let mut offer_paste = false;
    match target {
        MenuTarget::Video => items.push(MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete"),
            danger: true,
            action: {
                let data = data.clone();
                let repaint = repaint.clone();
                Rc::new(move || {
                    data.borrow_mut().remove_video_clip();
                    repaint();
                })
            },
        }),
        MenuTarget::Zoom(index) => {
            offer_paste = effect_menu_items(EffectTrack::Zoom, index, &data, &repaint, &mut items);
        }
        MenuTarget::Hide(index) => {
            offer_paste = effect_menu_items(EffectTrack::Hide, index, &data, &repaint, &mut items);
        }
    }

    // Paste is offered only when it would land, and sits before Delete so the
    // destructive row stays last.
    if offer_paste && !matches!(target, MenuTarget::Video) {
        items.insert(
            items.len() - 1,
            MenuItem {
                icon: icon_names::shipped::CLIPBOARD_PASTE_REGULAR,
                label: t("Paste"),
                danger: false,
                action: {
                    let data = data.clone();
                    let repaint = repaint.clone();
                    Rc::new(move || {
                        data.borrow_mut().paste_effect();
                        repaint();
                    })
                },
            },
        );
    }

    for (position, item) in items.into_iter().enumerate() {
        if item.danger && position > 0 {
            card.append(&menu_separator());
        }
        card.append(&clip_menu_row(item, &popover));
    }

    popover.set_child(Some(&card));
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // `set_parent` hands the popover to the anchor for its lifetime, so it has
    // to be handed back when the menu closes.
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

/// Build the Zoom/Hide rows. Returns whether Paste would land for this
/// clipboard; the caller inserts that row itself.
fn effect_menu_items(
    track: EffectTrack,
    index: usize,
    data: &Rc<RefCell<Data>>,
    repaint: &Rc<dyn Fn()>,
    items: &mut Vec<MenuItem>,
) -> bool {
    let (hidden_now, can_paste) = {
        let data = data.borrow();
        (data.effect_hidden(track, index), data.can_paste())
    };

    items.push(MenuItem {
        icon: icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
        label: t("Duplicate"),
        danger: false,
        action: {
            let data = data.clone();
            let repaint = repaint.clone();
            Rc::new(move || {
                data.borrow_mut().duplicate_effect(track, index);
                repaint();
            })
        },
    });
    items.push(MenuItem {
        icon: icon_names::shipped::COPY_REGULAR,
        label: t("Copy"),
        danger: false,
        action: {
            let data = data.clone();
            let repaint = repaint.clone();
            Rc::new(move || {
                data.borrow_mut().copy_effect(track, index);
                repaint();
            })
        },
    });
    items.push(MenuItem {
        icon: icon_names::shipped::CUT_REGULAR,
        label: t("Cut"),
        danger: false,
        action: {
            let data = data.clone();
            let repaint = repaint.clone();
            Rc::new(move || {
                data.borrow_mut().cut_effect(track, index);
                repaint();
            })
        },
    });
    items.push(MenuItem {
        icon: if hidden_now {
            icon_names::shipped::EYE_REGULAR
        } else {
            icon_names::shipped::EYE_OFF_REGULAR
        },
        label: if hidden_now { t("Show") } else { t("Hide") },
        danger: false,
        action: {
            let data = data.clone();
            let repaint = repaint.clone();
            Rc::new(move || {
                data.borrow_mut()
                    .set_effect_hidden(track, index, !hidden_now);
                repaint();
            })
        },
    });
    items.push(MenuItem {
        icon: icon_names::custom::USER_TRASH_SYMBOLIC,
        label: t("Delete"),
        danger: true,
        action: {
            let data = data.clone();
            let repaint = repaint.clone();
            Rc::new(move || {
                data.borrow_mut().remove_effect(track, index);
                repaint();
            })
        },
    });

    can_paste
}

fn menu_separator() -> GtkBox {
    let separator = GtkBox::new(Orientation::Vertical, 0);
    separator.add_css_class("recording-editor-clip-menu-separator");
    separator.set_size_request(-1, 1);
    separator
}

fn clip_menu_row(item: MenuItem, popover: &Popover) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-clip-menu-row");
    if item.danger {
        button.add_css_class("recording-editor-clip-menu-row-danger");
    }
    button.set_has_frame(false);

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("recording-editor-clip-menu-row-inner");
    let icon = Image::from_icon_name(item.icon);
    icon.set_pixel_size(15);
    let label = Label::new(Some(&item.label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&icon);
    row.append(&label);
    button.set_child(Some(&row));

    let popover = popover.clone();
    button.connect_clicked(move |_| {
        (item.action)();
        popover.popdown();
    });
    button
}

fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn ruler_major_step(visible: f64) -> f64 {
    const STEPS: [f64; 10] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0];
    *STEPS
        .iter()
        .find(|step| visible / *step <= 10.0)
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

fn near_step(value: f64, step: f64) -> bool {
    let scaled = value / step;
    (scaled - scaled.round()).abs() < 0.02
}

fn format_ruler_label(seconds: f64, major: f64) -> String {
    let total = seconds.max(0.0);
    if major < 1.0 {
        let minutes = (total / 60.0).floor() as u64;
        let secs = total - minutes as f64 * 60.0;
        return format!("{minutes}:{secs:04.1}");
    }
    let whole = total.round().max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

fn draw_ruler(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    prefers_dark: bool,
    scale: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let visible = data.visible_span().max(0.001);
    let major = ruler_major_step(visible);
    let minor = (major / 5.0).max(0.05);
    let view_start = data.x_to_time(0.0, width).max(0.0);
    let view_end = data.x_to_time(width, width).max(view_start);
    let canvas_end = if data.has_media() {
        data.canvas_seconds()
    } else {
        PLACEHOLDER_SECONDS
    };
    let (tick_r, tick_g, tick_b) = if prefers_dark {
        (1.0, 1.0, 1.0)
    } else {
        (0.11, 0.13, 0.16)
    };

    context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
    context.set_font_size((10.0 * scale).round());
    context.set_line_width(1.0);

    let mut time = (view_start / minor).floor() * minor;
    if time < 0.0 {
        time = 0.0;
    }
    while time <= view_end.min(canvas_end) + 0.0001 {
        let x = data.time_to_x(time, width).floor() + 0.5;
        if x >= -8.0 && x <= width + 8.0 {
            let on_major = near_step(time, major);
            context.set_source_rgba(
                tick_r,
                tick_g,
                tick_b,
                if prefers_dark {
                    if on_major {
                        0.28
                    } else {
                        0.10
                    }
                } else if on_major {
                    0.42
                } else {
                    0.16
                },
            );
            context.move_to(
                x,
                if on_major {
                    height - (11.0 * scale)
                } else {
                    height - (5.0 * scale)
                },
            );
            context.line_to(x, height);
            let _ = context.stroke();
            if on_major {
                let label = format_ruler_label(time, major);
                let near_playhead = (time - data.playhead).abs() < major * 0.08;
                context.set_source_rgba(
                    tick_r,
                    tick_g,
                    tick_b,
                    if prefers_dark {
                        if near_playhead {
                            0.92
                        } else {
                            0.52
                        }
                    } else if near_playhead {
                        0.88
                    } else {
                        0.50
                    },
                );
                if let Ok(extents) = context.text_extents(&label) {
                    let label_x = if time <= 0.001 {
                        0.0
                    } else {
                        (x - extents.width() / 2.0).clamp(0.0, width - extents.width())
                    };
                    context.move_to(label_x, (12.0 * scale).round());
                    let _ = context.show_text(&label);
                }
            }
        }
        time += minor;
    }
}

fn draw_effect_clips(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    track: EffectTrack,
    prefers_dark: bool,
    scale: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let selected = data.selected_effect(track);
    for (index, start, end) in data.effect_spans(track) {
        let x0 = data.time_to_x(start, width);
        let x1 = data.time_to_x(end, width).max(x0 + 22.0 * scale);
        let pill_w = x1 - x0;
        let y = 7.0 * scale;
        let pill_h = (height - 14.0 * scale).max(10.0);
        let tone = effect_tone(track, prefers_dark, data.effect_hidden(track, index));

        rounded_rect(context, x0, y, pill_w, pill_h, 5.0 * scale);
        context.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
        let _ = context.fill();

        if selected == Some(index) {
            let inset = HANDLE_INSET * scale;
            let handle_w = HANDLE_WIDTH * scale;
            let handle_h = (pill_h - inset * 2.0).max(10.0);
            let handle_y = y + (pill_h - handle_h) / 2.0;
            draw_edge_handle(context, x0 + inset, handle_y, handle_w, handle_h, tone);
            draw_edge_handle(
                context,
                x0 + pill_w - handle_w - inset,
                handle_y,
                handle_w,
                handle_h,
                tone,
            );

            rounded_rect(
                context,
                x0 + 0.5,
                y + 0.5,
                (pill_w - 1.0).max(0.0),
                (pill_h - 1.0).max(0.0),
                4.5 * scale,
            );
            context.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
            context.set_line_width(1.5);
            let _ = context.stroke();
        }

        if pill_w > 40.0 * scale {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.92);
            context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
            context.set_font_size((11.0 * scale).round());
            let label = match track {
                EffectTrack::Zoom if pill_w > 92.0 * scale => format!(
                    "{}  {}",
                    if data.zoom_clips[index].mode == ZoomMode::Auto {
                        t("Auto")
                    } else {
                        t("Manual")
                    },
                    format_zoom_scale(data.zoom_scale_at(index))
                ),
                EffectTrack::Zoom => t("Zoom"),
                EffectTrack::Hide => t("Hide"),
            };
            context.move_to(x0 + 14.0 * scale, y + pill_h * 0.62);
            let _ = context.show_text(&label);
        }
    }

    // The copied clip rides the lane under the pointer: Copy and Cut leave no
    // other trace, so the ghost shows both that something is waiting and where
    // a click would drop it.
    if !data.dragging_effect {
        if let (Some(start), Some(duration)) = (data.hover_time(track), data.clipboard_span(track))
        {
            draw_effect_ghost(
                context,
                width,
                height,
                data,
                track,
                start,
                duration,
                prefers_dark,
                scale,
            );
        }
    }
}

/// The clip held on the clipboard, drawn where a click would drop it.
///
/// A placeable ghost reads as the real pill, so what the click will produce is
/// obvious. One that would collide keeps the outline and drops the fill,
/// matching the hollow treatment a disabled clip already uses.
fn draw_effect_ghost(
    context: &Context,
    width: f64,
    height: f64,
    data: &Data,
    track: EffectTrack,
    start: f64,
    duration: f64,
    prefers_dark: bool,
    scale: f64,
) {
    let placed = start.max(0.0);
    let x0 = data.time_to_x(placed, width);
    let x1 = data.time_to_x(placed + duration, width);
    let pill_w = (x1 - x0).max(22.0 * scale);
    let y = 7.0 * scale;
    let pill_h = (height - 14.0 * scale).max(10.0);
    let free = data.paste_spot_free(track, placed, duration);
    let (base_r, base_g, base_b) = match track {
        EffectTrack::Zoom => (0.0, 0.0, 1.0),
        EffectTrack::Hide => (0.4, 0.0, 0.2),
    };
    let fill_alpha = if free { 0.45 } else { 0.10 };

    rounded_rect(context, x0, y, pill_w, pill_h, 5.0 * scale);
    context.set_source_rgba(
        base_r,
        base_g,
        base_b,
        if prefers_dark {
            fill_alpha + 0.15
        } else {
            fill_alpha
        },
    );
    let _ = context.fill_preserve();
    if prefers_dark {
        context.set_source_rgba(1.0, 1.0, 1.0, 0.85);
    } else {
        let (ring_r, ring_g, ring_b) = match track {
            EffectTrack::Zoom => (0.0, 0.0, 0.45),
            EffectTrack::Hide => (0.20, 0.0, 0.10),
        };
        context.set_source_rgba(ring_r, ring_g, ring_b, 0.85);
    }
    context.set_line_width(1.0);
    context.set_dash(&[4.0 * scale, 3.0 * scale], 0.0);
    let _ = context.stroke();
    context.set_dash(&[], 0.0);

    if pill_w > 40.0 * scale {
        context.set_source_rgba(1.0, 1.0, 1.0, if free { 0.92 } else { 0.55 });
        context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
        context.set_font_size((11.0 * scale).round());
        context.move_to(x0 + 14.0 * scale, y + pill_h * 0.62);
        let label = if free { t("Paste") } else { t("Occupied") };
        let _ = context.show_text(&label);
    }
}

fn effect_tone(track: EffectTrack, prefers_dark: bool, hidden: bool) -> ClipTone {
    let mut tone = match (track, prefers_dark) {
        (EffectTrack::Zoom, true) => ClipTone {
            fill: (0.0, 0.0, 1.0, 1.0),
            edge: (1.0, 1.0, 1.0, 0.95),
            handle: (1.0, 1.0, 1.0, 0.95),
        },
        (EffectTrack::Zoom, false) => ClipTone {
            fill: (0.0, 0.0, 1.0, 1.0),
            edge: (0.0, 0.0, 0.45, 0.95),
            handle: (0.0, 0.0, 0.45, 0.95),
        },
        (EffectTrack::Hide, true) => ClipTone {
            fill: (0.4, 0.0, 0.2, 1.0),
            edge: (1.0, 0.8, 0.9, 0.95),
            handle: (1.0, 0.8, 0.9, 0.95),
        },
        (EffectTrack::Hide, false) => ClipTone {
            fill: (0.4, 0.0, 0.2, 1.0),
            edge: (0.2, 0.0, 0.1, 0.95),
            handle: (0.2, 0.0, 0.1, 0.95),
        },
    };
    if hidden {
        // A disabled clip keeps its span, ring and handles; only the fill
        // drops away, so "off" reads differently from "not selected".
        tone.fill = match track {
            EffectTrack::Zoom => (0.0, 0.0, 1.0, 0.12),
            EffectTrack::Hide => (0.4, 0.0, 0.2, 0.12),
        };
    }
    tone
}

/// The scale presets the Zoom panel offers, on the same stops as the shared
/// editor. Anything between two stops keeps its own scale; the panel then
/// lights no chip rather than rounding the clip to a value nobody picked.
pub(super) const ZOOM_SCALE_PRESETS: [(&str, f64); 6] = [
    ("1.25×", 1.25),
    ("1.5×", 1.5),
    ("1.8×", 1.8),
    ("2.2×", 2.2),
    ("3.5×", 3.5),
    ("5×", 5.0),
];

fn format_zoom_scale(scale: f64) -> String {
    for (label, preset) in ZOOM_SCALE_PRESETS {
        if (scale - preset).abs() < 0.04 {
            return label.to_string();
        }
    }
    if (scale - scale.round()).abs() < 0.05 {
        format!("{:.0}×", scale.round())
    } else {
        format!("{scale:.1}×")
    }
}

/// The preset closest to `scale`, which is the chip the panel marks active.
pub(super) fn nearest_zoom_preset(scale: f64) -> f64 {
    ZOOM_SCALE_PRESETS
        .iter()
        .min_by(|(_, left), (_, right)| (left - scale).abs().total_cmp(&(right - scale).abs()))
        .map(|(_, value)| *value)
        .unwrap_or(DEFAULT_ZOOM_SCALE)
}

/// The "extend duration" band: everything to the right of the clip, across the
/// whole lane stack.
///
/// The frosted panel starts at the clip's edge and is outlined in black. The
/// mover line sits inside it, just in from that edge: that is the grab point
/// for growing the clip. While the pointer is over the band it carries the
/// hint, because the band itself is the affordance.
fn draw_extend_band(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    prefers_dark: bool,
    show_hint: bool,
    scale: f64,
) {
    if !data.has_media() {
        // Before a video is loaded there is no clip to extend past, so the
        // lanes stay unfrosted rather than becoming one full-width band.
        return;
    }
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let end_x = clip_end_x(data, width, scale);
    let band = width - end_x;
    if band < 1.0 {
        return;
    }
    // The panel dims what it covers: dark on the dark theme, a light frost on
    // the light one.
    let (r, g, b, a) = if prefers_dark {
        (0.03, 0.04, 0.06, 0.55)
    } else {
        (0.95, 0.96, 0.99, 0.55)
    };
    let _ = context.save();
    rounded_rect(context, end_x, 0.0, band, height, 4.0 * scale);
    context.clip();
    context.set_source_rgba(r, g, b, a);
    let _ = context.paint();
    // Fade the scrim in from the panel's edge, so it reads as frosted glass
    // rather than a slab butted against the clip.
    let fade = band.min(96.0 * scale);
    let gradient = LinearGradient::new(end_x, 0.0, end_x + fade, 0.0);
    gradient.add_color_stop_rgba(0.0, r, g, b, a * 0.35);
    gradient.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let _ = context.set_source(&gradient);
    let _ = context.paint();
    let _ = context.restore();

    // The panel's border: a thin black outline.
    rounded_rect(
        context,
        end_x + 0.5,
        0.5,
        (band - 1.0).max(0.0),
        (height - 1.0).max(0.0),
        4.0 * scale,
    );
    context.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    context.set_line_width(1.0);
    let _ = context.stroke();

    // The mover: a short line inside the panel, just in from its left edge.
    let (mr, mg, mb) = if prefers_dark {
        (1.0, 1.0, 1.0)
    } else {
        (0.10, 0.11, 0.13)
    };
    context.set_source_rgba(mr, mg, mb, 0.9);
    let mover_y = (height * 0.30).max(2.0);
    let mover_h = (height * 0.40).max(6.0);
    rounded_rect(
        context,
        end_x + EXTEND_MOVER_INSET * scale - 1.0,
        mover_y,
        2.0,
        mover_h,
        1.0,
    );
    let _ = context.fill();

    if !show_hint {
        return;
    }
    let label = t("Drag to adjust duration");
    context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
    context.set_font_size((13.0 * scale).round());
    if let Ok(extents) = context.text_extents(&label) {
        // Only spell it out when the band is wide enough for the words to sit
        // clear of the divider instead of crowding it.
        if band > extents.width() + 36.0 * scale {
            let (tr, tg, tb, ta) = if prefers_dark {
                (1.0, 1.0, 1.0, 0.45)
            } else {
                (0.10, 0.12, 0.15, 0.60)
            };
            context.set_source_rgba(tr, tg, tb, ta);
            context.move_to(
                end_x + EXTEND_MOVER_INSET * scale + 14.0 * scale,
                (height + extents.height()) / 2.0 - 1.0,
            );
            let _ = context.show_text(&label);
        }
    }
}

fn draw_video_clip(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    prefers_dark: bool,
    scale: f64,
) {
    if !data.has_media() {
        // Before a recording is loaded there is nothing to clip: an empty
        // lane, never a zero-length slab.
        return;
    }
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let boundaries = data.segment_boundaries();
    if boundaries.is_empty() {
        return;
    }
    let y = 6.0 * scale;
    let clip_height = (height - 12.0 * scale).max(6.0);
    let radius = 5.0 * scale;
    // Half of the two-pixel seam a split leaves between its segments.
    let seam = (1.0 * scale).max(0.5);
    let show_handles = data.selected
        || data
            .hover
            .is_some_and(|hover| hover >= 0.0 && hover <= data.total_duration());
    let tone = clip_tone(data.selected, !data.frames.is_empty(), prefers_dark);
    let tiles = data.filmstrip_tiles();
    let last_index = boundaries.len() - 1;
    // The piece being dragged is painted last so it floats over the ones it
    // overlaps while the pointer moves it.
    let mut order: Vec<usize> = (0..boundaries.len()).collect();
    if let Some(moving) = data.moving_segment {
        if let Some(position) = order.iter().position(|&index| index == moving) {
            let moved = order.remove(position);
            order.push(moved);
        }
    }

    for &index in &order {
        let (source_start, source_end) = boundaries[index];
        let segment_start = data.segment_start(index);
        let footage = (source_end - source_start).max(0.0);
        let hold = if index == last_index {
            data.freeze_tail.max(0.0)
        } else {
            0.0
        };
        let body_start = data.time_to_x(segment_start, width);
        let body_end = data.time_to_x(segment_start + footage + hold, width);
        let left = if index == 0 {
            body_start
        } else {
            body_start + seam
        };
        let right = if index == last_index {
            body_end
        } else {
            (body_end - seam).max(left + 2.0)
        };
        let body_width = (right - left).max(4.0);

        rounded_rect(context, left, y, body_width, clip_height, radius);
        context.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
        let _ = context.fill();

        if !data.frames.is_empty() && !tiles.is_empty() {
            let _ = context.save();
            rounded_rect(context, left, y, body_width, clip_height, radius);
            context.clip();
            for (tile_index, frame) in data.frames.iter().enumerate() {
                let Some(&(start, end)) = tiles.get(tile_index) else {
                    continue;
                };
                // A tile belongs to the piece that owns its source span, so it
                // travels with that piece instead of the composition's origin.
                let from = start.max(source_start);
                let to = end.min(source_end);
                if to <= from {
                    continue;
                }
                let tile_x = data.time_to_x(segment_start + (from - source_start), width);
                let tile_right = data.time_to_x(segment_start + (to - source_start), width);
                let tile_w = (tile_right - tile_x).max(1.0);
                paint_cover(context, frame, tile_x, y, tile_w, clip_height);
            }
            let _ = context.restore();
        }

        // Past the last frame there is nothing to sample, so a held tail
        // repeats the final frame: that is what a freeze is, and it keeps the
        // clip reading as footage instead of a black slab.
        if index == last_index && hold > 0.0 {
            let hold_x0 = data
                .time_to_x(segment_start + footage, width)
                .clamp(left, right);
            if let Some(frame) = data.frames.last() {
                let _ = context.save();
                rounded_rect(context, left, y, body_width, clip_height, radius);
                context.clip();
                let source_w = f64::from(frame.width()).max(1.0);
                let source_h = f64::from(frame.height()).max(1.0);
                let tile_w = (clip_height * source_w / source_h).clamp(8.0 * scale, 240.0 * scale);
                let mut tile_x = hold_x0;
                while tile_x < right {
                    let draw_w = (right - tile_x).min(tile_w);
                    paint_cover(context, frame, tile_x, y, draw_w, clip_height);
                    tile_x += tile_w;
                }
                let _ = context.restore();
            }
            // Mark where the footage stops and the hold begins, even while
            // the filmstrip is still decoding.
            let mark = hold_x0.floor() + 0.5;
            if mark > left + 1.0 && mark < right - 1.0 {
                context.set_source_rgba(1.0, 1.0, 1.0, 0.35);
                context.set_line_width(1.0);
                context.set_dash(&[3.0, 3.0], 0.0);
                context.move_to(mark, y + 3.0 * scale);
                context.line_to(mark, y + clip_height - 3.0 * scale);
                let _ = context.stroke();
                context.set_dash(&[], 0.0);
            }
        }

        if data.selected {
            rounded_rect(
                context,
                left + 0.5,
                y + 0.5,
                (body_width - 1.0).max(0.0),
                (clip_height - 1.0).max(0.0),
                (radius - 0.5).max(0.0),
            );
            context.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
            context.set_line_width(1.5);
            let _ = context.stroke();
        }

        if show_handles {
            // Every segment carries its own pair of edge handles, so after a
            // split each chip reads as a clip you can trim on both sides.
            let inset = (HANDLE_INSET * scale).min(body_width / 4.0);
            let handle_w = (HANDLE_WIDTH * scale).min(body_width / 3.0).max(1.0);
            let handle_h = (clip_height - inset * 2.0).max(10.0);
            let handle_y = y + (clip_height - handle_h) / 2.0;
            draw_edge_handle(context, left + inset, handle_y, handle_w, handle_h, tone);
            draw_edge_handle(
                context,
                left + body_width - handle_w - inset,
                handle_y,
                handle_w,
                handle_h,
                tone,
            );
        }
    }
}

fn paint_cover(
    context: &Context,
    pixbuf: &gtk4::gdk_pixbuf::Pixbuf,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let source_w = f64::from(pixbuf.width()).max(1.0);
    let source_h = f64::from(pixbuf.height()).max(1.0);
    let scale = (width / source_w).max(height / source_h);
    let painted_w = source_w * scale;
    let painted_h = source_h * scale;
    let _ = context.save();
    context.rectangle(x, y, width, height);
    context.clip();
    context.translate(
        x + (width - painted_w) / 2.0,
        y + (height - painted_h) / 2.0,
    );
    context.scale(scale, scale);
    context.set_source_pixbuf(pixbuf, 0.0, 0.0);
    let _ = context.paint();
    let _ = context.restore();
}

#[derive(Clone, Copy)]
struct ClipTone {
    fill: (f64, f64, f64, f64),
    edge: (f64, f64, f64, f64),
    handle: (f64, f64, f64, f64),
}

fn clip_tone(selected: bool, has_frames: bool, prefers_dark: bool) -> ClipTone {
    let mut tone = if prefers_dark {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.28),
            edge: (0.98, 0.86, 0.42, 0.88),
            handle: (0.98, 0.86, 0.42, 0.96),
        }
    } else {
        ClipTone {
            fill: (0.78, 0.52, 0.14, 0.38),
            edge: (0.45, 0.25, 0.02, 0.92),
            handle: (0.48, 0.28, 0.03, 0.88),
        }
    };
    if selected {
        tone.fill.3 = if prefers_dark { 0.46 } else { 0.54 };
        tone.handle.3 = 1.0;
    }
    if !has_frames {
        // Frames are still decoding: a neutral media lane, not an orange slab.
        tone.fill = (0.10, 0.11, 0.13, 0.9);
    }
    tone
}

fn draw_edge_handle(context: &Context, x: f64, y: f64, width: f64, height: f64, tone: ClipTone) {
    context.set_source_rgba(tone.handle.0, tone.handle.1, tone.handle.2, tone.handle.3);
    rounded_rect(context, x, y, width, height, 2.0);
    let _ = context.fill();
}

fn draw_playhead(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    prefers_dark: bool,
    scale: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    if let Some(hover) = data.hover {
        let hover_x = data.time_to_x(hover, width);
        if (hover_x - data.time_to_x(data.playhead, width)).abs() > 1.0 {
            let x = hover_x.floor() + 0.5;
            context.set_source_rgba(0.80, 0.22, 0.20, 0.95);
            context.set_line_width(2.0);
            context.move_to(x, 0.0);
            context.line_to(x, height);
            let _ = context.stroke();
        }
    }
    let x = data.time_to_x(data.playhead, width).floor() + 0.5;
    let (stem_r, stem_g, stem_b) = if prefers_dark {
        (0.86, 0.90, 0.98)
    } else {
        (0.07, 0.08, 0.09)
    };
    let handle_w = (12.0 * scale).round();
    let handle_h = (26.0 * scale).round();
    let handle_top = (2.0 * scale).round();

    context.set_source_rgba(stem_r, stem_g, stem_b, 0.9);
    context.set_line_width(1.5);
    context.move_to(x, handle_top + handle_h);
    context.line_to(x, height);
    let _ = context.stroke();

    rounded_rect(
        context,
        x - handle_w / 2.0,
        handle_top,
        handle_w,
        handle_h,
        handle_h / 2.0,
    );
    if prefers_dark {
        context.set_source_rgba(0.04, 0.05, 0.07, 1.0);
    } else {
        context.set_source_rgba(0.94, 0.96, 1.0, 1.0);
    }
    let _ = context.fill_preserve();
    context.set_source_rgba(stem_r, stem_g, stem_b, 1.0);
    context.set_line_width(1.5);
    let _ = context.stroke();
}

fn rounded_rect(context: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    let right = x + width;
    let bottom = y + height;
    context.new_sub_path();
    context.arc(
        right - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
}

#[cfg(test)]
mod tests {
    use super::super::project;
    use super::*;
    use gtk4::cairo::{Format, ImageSurface};

    fn loaded() -> Data {
        let mut data = Data::default();
        data.path = Some(PathBuf::from("clip.mp4"));
        data.duration = 10.0;
        data.frame_size = Some((1920, 1080));
        data.trim_end = 10.0;
        data
    }

    /// A recording on disk, which is what a project file is keyed to.
    fn recording(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-timeline-project-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let video = directory.join("clip.mp4");
        std::fs::write(&video, b"footage").unwrap();
        video
    }

    #[test]
    fn a_fresh_session_has_nothing_to_save_and_an_edit_does() {
        let video = recording("fresh");
        let mut data = loaded();
        data.path = Some(video.clone());

        assert!(data.project_file().is_none());

        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        let file = data.project_file().expect("an edited session is saved");
        assert_eq!(file.zoom_clips.len(), 1);
        assert_eq!(file.source_path, video);
    }

    #[test]
    fn a_saved_session_restores_onto_the_recording_that_is_open() {
        let mut data = loaded();
        data.apply_project(&project::EditorProjectFile {
            version: project::PROJECT_VERSION,
            source_path: PathBuf::from("clip.mp4"),
            source_size: 0,
            source_mtime_secs: 0,
            trim_start_seconds: 2.0,
            trim_end_seconds: 40.0,
            cuts: vec![1.0, 5.0, 50.0],
            segment_starts: vec![0.0, 2.5],
            freeze_tail: 0.5,
            zoom_clips: vec![
                project::ZoomClipFile {
                    start: 0.5,
                    end: 2.0,
                    scale: 2.0,
                    center: (640.0, 360.0),
                    mode: project::ZoomModeFile::Auto,
                    instant: true,
                    hidden: false,
                    anchor: Some(project::ZoomAnchorFile {
                        source_start: 2.5,
                        source_end: 4.0,
                    }),
                },
                // Shorter than a pill: nothing could be drawn for it.
                project::ZoomClipFile {
                    start: 3.0,
                    end: 3.01,
                    scale: 2.0,
                    center: (0.0, 0.0),
                    mode: project::ZoomModeFile::Manual,
                    instant: false,
                    hidden: false,
                    anchor: None,
                },
            ],
            zoom_camera: project::ZoomCameraFile {
                speed: 9.0,
                smoothness: -1.0,
                start_early: true,
            },
            cursor_hide_clips: vec![project::HideClipFile {
                start: 1.0,
                end: 4.0,
                hidden: false,
            }],
            playhead_seconds: 99.0,
            timeline_scale: 400.0,
            timeline_scroll_seconds: 12.0,
        });

        // The recording is 10s: the project's tail past it is clamped, and a
        // cut inside the head margin or past the tail goes with it.
        assert!((data.trim_start - 2.0).abs() < 1e-9);
        assert!((data.trim_end - 10.0).abs() < 1e-9);
        assert_eq!(data.cuts, vec![5.0]);
        assert_eq!(data.segment_starts, vec![0.0, 2.5]);
        assert!((data.freeze_tail - 0.5).abs() < 1e-9);
        assert_eq!(data.zoom_clips.len(), 1);
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Auto);
        assert!(data.zoom_clips[0].instant);
        assert_eq!(data.zoom_clips[0].anchor, Some((2.5, 4.0)));
        assert_eq!(data.hide_clips.len(), 1);
        assert!((data.zoom_camera.speed - 3.0).abs() < 1e-9);
        assert!(data.zoom_camera.smoothness.abs() < 1e-9);
        assert!(data.zoom_camera.start_early);
        assert!((data.scale - 100.0).abs() < 1e-9);
        assert!(data.playhead <= data.total_duration());
        assert!(data.scroll <= data.max_scroll());
    }

    #[test]
    fn a_zoom_keeps_everything_it_carries_through_the_file() {
        let clip = ZoomClip {
            start: 1.0,
            end: 3.0,
            scale: 2.5,
            center: (400.0, 300.0),
            hidden: true,
            mode: ZoomMode::Auto,
            instant: true,
            anchor: Some((2.0, 4.0)),
        };

        assert_eq!(ZoomClip::from_file(&clip.to_file()), Some(clip));
    }

    #[test]
    fn zoom_and_hide_land_at_the_playhead_at_their_default_length() {
        let mut data = loaded();
        assert_eq!(data.add_effect(EffectTrack::Zoom, 2.0), Some(0));
        assert_eq!(data.add_effect(EffectTrack::Hide, 2.0), Some(0));
        let zoom = data.zoom_clips[0];
        assert!((zoom.start - 2.0).abs() < 1e-9);
        assert!((zoom.end - 3.8).abs() < 1e-9);
        assert!((zoom.scale - DEFAULT_ZOOM_SCALE).abs() < 1e-9);
        assert_eq!(zoom.center, (960.0, 540.0));
        assert_eq!(data.selected_zoom, Some(0));
        assert_eq!(data.selected_hide, Some(0));
        assert!(!data.selected);
    }

    #[test]
    fn effects_do_not_stack_on_each_other() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_none());
        assert!(data.add_effect(EffectTrack::Zoom, 4.0).is_some());
    }

    #[test]
    fn detected_zooms_skip_overlaps_clamp_focus_and_are_idempotent() {
        let existing = [ZoomClip {
            start: 1.0,
            end: 3.5,
            scale: DEFAULT_ZOOM_SCALE,
            center: (320.0, 240.0),
            hidden: false,
            mode: ZoomMode::Auto,
            instant: false,
            anchor: None,
        }];
        let detections = [
            auto_zoom::AutoZoom {
                start: 2.0,
                end: 4.0,
                center_time: 3.0,
                center: (320.0, 240.0),
                scale: 2.0,
            },
            auto_zoom::AutoZoom {
                start: 4.0,
                end: 5.0,
                center_time: 4.5,
                center: (-100.0, 900.0),
                scale: 20.0,
            },
            auto_zoom::AutoZoom {
                start: 6.0,
                end: 6.05,
                center_time: 6.0,
                center: (320.0, 240.0),
                scale: 2.0,
            },
        ];

        let placed = place_auto_zooms(&existing, &detections, (640, 480));
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].start, 4.0);
        assert_eq!(placed[0].scale, MAX_ZOOM_SCALE);
        assert_eq!(placed[0].center, (64.0, 432.0));
        assert!(place_auto_zooms(&[existing[0], placed[0]], &detections, (640, 480)).is_empty());
    }

    #[test]
    fn moving_a_pill_keeps_its_length_and_its_neighbours_space() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        assert!(data.add_effect(EffectTrack::Zoom, 5.0).is_some());
        data.move_effect(EffectTrack::Zoom, 1, 3.0);
        let first = data.zoom_clips[0];
        let second = data.zoom_clips[1];
        assert!((second.start - 3.0).abs() < 1e-9);
        assert!((second.end - 4.8).abs() < 1e-9);
        assert!(second.start >= first.end);
    }

    #[test]
    fn trimming_a_pill_cannot_swallow_its_neighbour() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Hide, 0.0).is_some());
        assert!(data.add_effect(EffectTrack::Hide, 5.0).is_some());
        data.set_effect_range(EffectTrack::Hide, 0, 0.0, 8.0);
        let first = data.hide_clips[0];
        assert!((first.end - 5.0).abs() < 1e-9);
        assert!(first.end - first.start >= MIN_CLIP_SECONDS);
    }

    fn pill_pixel(track: EffectTrack) -> [u8; 4] {
        let mut data = loaded();
        match track {
            EffectTrack::Zoom => data.zoom_clips.push(ZoomClip {
                start: 1.0,
                end: 4.0,
                scale: 1.5,
                center: (320.0, 240.0),
                hidden: false,
                mode: ZoomMode::Manual,
                instant: false,
                anchor: None,
            }),
            EffectTrack::Hide => data.hide_clips.push(HideClip {
                start: 1.0,
                end: 4.0,
                hidden: false,
            }),
        }
        data.select_effect(track, Some(0));

        let mut surface = ImageSurface::create(Format::ARgb32, 400, 56).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            draw_effect_clips(&context, 400, 56, &data, track, true, 1.0);
        }
        surface.flush();

        let stride = surface.stride() as usize;
        let width = surface.width() as usize;
        let x = data.time_to_x(3.5, 400.0).round() as usize;
        let contents = surface.data().unwrap();
        let offset = 28 * stride + x * 4;
        [
            contents[offset],
            contents[offset + 1],
            contents[offset + 2],
            contents[offset + 3],
        ]
    }

    #[test]
    fn zoom_pills_are_solid_blue() {
        let [b, g, r, a] = pill_pixel(EffectTrack::Zoom);
        assert_eq!(a, 255);
        assert_eq!((r, g, b), (0, 0, 255));
    }

    #[test]
    fn hide_pills_are_solid_maroon() {
        let [b, g, r, a] = pill_pixel(EffectTrack::Hide);
        assert_eq!(a, 255);
        assert_eq!((r, g, b), (102, 0, 51));
    }

    #[test]
    fn copy_then_paste_lands_at_the_playhead() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.playhead = 6.0;
        assert!(!data.can_paste());
        assert!(data.copy_effect(EffectTrack::Zoom, 0));
        assert!(data.can_paste());
        let pasted = data.paste_effect().expect("the paste should land");
        let clip = data.zoom_clips[pasted];
        assert!((clip.start - 6.0).abs() < 1e-9);
        assert!((clip.end - 7.8).abs() < 1e-9);
        assert!((clip.scale - DEFAULT_ZOOM_SCALE).abs() < 1e-9);
        assert_eq!(data.selected_zoom, Some(pasted));
    }

    #[test]
    fn a_paste_that_would_leave_the_clip_is_not_offered() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Hide, 1.0).is_some());
        assert!(data.copy_effect(EffectTrack::Hide, 0));
        data.playhead = 9.0;
        assert!(!data.can_paste());
        assert!(data.paste_effect().is_none());
    }

    #[test]
    fn cut_takes_the_clip_off_the_lane_and_keeps_it_for_paste() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        assert!(data.cut_effect(EffectTrack::Zoom, 0));
        assert!(data.zoom_clips.is_empty());
        assert_eq!(data.selected_zoom, None);
        data.playhead = 5.0;
        assert!(data.can_paste());
        assert!(data.paste_effect().is_some());
        assert_eq!(data.zoom_clips.len(), 1);
    }

    #[test]
    fn duplicate_lands_after_the_clip_and_refuses_a_crowded_spot() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        assert!(data.duplicate_effect(EffectTrack::Zoom, 0).is_some());
        assert_eq!(data.zoom_clips.len(), 2);
        assert!((data.zoom_clips[1].start - 1.8).abs() < 1e-9);
        assert_eq!(data.selected_zoom, Some(1));

        // A span that runs past the ten-second composition is refused, as is
        // one that would overlap a neighbour.
        assert!(data
            .insert_effect(EffectTrack::Zoom, 8.5, 10.3, DEFAULT_ZOOM_SCALE, false)
            .is_none());
        assert!(data
            .insert_effect(EffectTrack::Zoom, 1.0, 2.0, DEFAULT_ZOOM_SCALE, false)
            .is_none());
    }

    #[test]
    fn hide_toggles_without_losing_the_span() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Hide, 2.0).is_some());
        let before = data.hide_clips[0];
        data.set_effect_hidden(EffectTrack::Hide, 0, true);
        assert!(data.effect_hidden(EffectTrack::Hide, 0));
        let after = data.hide_clips[0];
        assert_eq!((after.start, after.end), (before.start, before.end));
    }

    #[test]
    fn deleting_the_video_clip_empties_the_timeline() {
        let mut data = loaded();
        assert!(data.has_media());
        data.remove_video_clip();
        assert!(!data.has_media());
        assert_eq!(data.clip_duration(), 0.0);
        assert!(data.zoom_clips.is_empty());
    }

    #[test]
    fn split_adds_one_cut_and_refuses_edges_and_duplicates() {
        let mut data = loaded();
        assert_eq!(data.segment_boundaries(), vec![(0.0, 10.0)]);
        assert!(data.add_cut(4.0));
        assert_eq!(data.segment_boundaries(), vec![(0.0, 4.0), (4.0, 10.0)]);
        // A second split within a tenth of a second of the first is refused,
        // and so is one that would leave a sliver at either end.
        assert!(!data.add_cut(4.05));
        assert!(!data.add_cut(0.05));
        assert!(!data.add_cut(9.95));
        assert_eq!(data.cuts.len(), 1);
    }

    #[test]
    fn split_pieces_sit_where_their_footage_does() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        assert_eq!(data.segment_starts, vec![0.0, 4.0]);
        assert!((data.total_duration() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn moving_a_split_piece_opens_a_gap() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.set_segment_start(1, 6.0);
        assert!((data.segment_start(1) - 6.0).abs() < 1e-9);
        // The second piece now ends at 12; nothing owns the gap at 5.
        assert!((data.total_duration() - 12.0).abs() < 1e-9);
        assert_eq!(data.source_time_at(5.0), None);
        assert!((data.source_time_at(6.5).unwrap_or_default() - 4.5).abs() < 1e-9);
    }

    #[test]
    fn a_lone_clip_cannot_be_moved_off_zero() {
        let mut data = loaded();
        data.set_segment_start(0, 3.0);
        assert_eq!(data.segment_start(0), 0.0);
        assert!((data.total_duration() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn releasing_a_piece_pushes_it_out_of_its_neighbour() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        // Dropped over the first piece, the moved piece's midpoint sits to the
        // right of its neighbour's, so it settles against that neighbour's end.
        data.set_segment_start(1, 2.0);
        data.settle_segment_start(1);
        assert!((data.segment_start(1) - 4.0).abs() < 1e-9);

        // With the small piece last, dropping the big one over it settles back
        // at the head, the side that still has room.
        let mut data = loaded();
        assert!(data.add_cut(8.0));
        data.set_segment_start(0, 4.0);
        data.settle_segment_start(0);
        assert!((data.segment_start(0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn extending_measures_from_a_moved_last_piece() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.set_segment_start(1, 6.0);
        data.set_visible_length(14.0);
        assert!((data.trim_end - 10.0).abs() < 1e-9);
        assert!((data.freeze_tail - 2.0).abs() < 1e-9);
        assert!((data.total_duration() - 14.0).abs() < 1e-9);
    }

    #[test]
    fn splitting_a_moved_piece_keeps_the_gap() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.set_segment_start(1, 6.0);
        // The gap owns nobody's footage, so nothing can be split there.
        assert!(!data.add_cut(5.0));
        // Inside the moved piece the cut lands at that piece's own offset.
        assert!(data.add_cut(8.0));
        assert_eq!(data.cuts, vec![4.0, 6.0]);
        assert!((data.segment_start(1) - 6.0).abs() < 1e-9);
        assert!((data.segment_start(2) - 8.0).abs() < 1e-9);
        assert!((data.total_duration() - 12.0).abs() < 1e-9);
    }

    #[test]
    fn trimming_drops_cuts_that_fall_outside_the_kept_footage() {
        let mut data = loaded();
        assert!(data.add_cut(2.0));
        data.trim_start = 3.0;
        data.clamp_cuts();
        assert!(data.cuts.is_empty());
        assert_eq!(data.segment_boundaries(), vec![(3.0, 10.0)]);
    }

    #[test]
    fn extending_past_the_source_holds_the_last_frame() {
        let mut data = loaded();
        data.set_visible_length(12.0);
        assert!((data.trim_end - 10.0).abs() < 1e-9);
        assert!((data.freeze_tail - 2.0).abs() < 1e-9);
        assert!((data.total_duration() - 12.0).abs() < 1e-9);
        // The held tail is composition, so the canvas grows with it.
        assert!(data.canvas_seconds() > data.total_duration());
    }

    #[test]
    fn extending_inside_the_source_grows_the_footage() {
        let mut data = loaded();
        data.trim_end = 6.0;
        data.set_visible_length(8.0);
        assert!((data.trim_end - 8.0).abs() < 1e-9);
        assert_eq!(data.freeze_tail, 0.0);
        assert!((data.total_duration() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn shrinking_the_clip_drops_its_hold() {
        let mut data = loaded();
        data.set_visible_length(12.0);
        data.set_visible_length(5.0);
        assert!((data.trim_end - 5.0).abs() < 1e-9);
        assert_eq!(data.freeze_tail, 0.0);
        assert!((data.total_duration() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn effects_can_sit_on_the_held_tail_and_survive_a_shorten() {
        let mut data = loaded();
        data.set_visible_length(12.0);
        data.playhead = 10.5;
        assert!(data.add_effect(EffectTrack::Hide, data.playhead).is_some());
        let clip = data.hide_clips[0];
        assert!((clip.start - 10.5).abs() < 1e-9);
        assert!((clip.end - 12.0).abs() < 1e-9);

        // Pulling the clip back inside the source drops the tail and with it
        // the piece of the pill that sat on it.
        data.set_visible_length(6.0);
        assert!(data.hide_clips.is_empty());
    }

    #[test]
    fn a_click_places_the_copied_clip_where_the_ghost_sat() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        assert!(data.copy_effect(EffectTrack::Zoom, 0));
        // The ghost only offers a spot the paste would take.
        assert!(data.paste_spot_free(EffectTrack::Zoom, 4.0, 1.8));
        assert!(!data.paste_spot_free(EffectTrack::Zoom, 0.5, 1.8));

        let placed = data
            .paste_effect_at(EffectTrack::Zoom, 4.0)
            .expect("the ghost's spot should place");
        assert!((data.zoom_clips[placed].start - 4.0).abs() < 1e-9);
        // Landing the clip finishes the copy, so the ghost leaves the lane.
        assert!(data.clipboard.is_none());
        assert_eq!(data.clipboard_span(EffectTrack::Zoom), None);
    }

    #[test]
    fn a_detected_zoom_keeps_its_origin_through_copy_paste_and_duplicate() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        assert_eq!(
            data.zoom_clips[0].mode,
            ZoomMode::Manual,
            "a placed zoom is the user's"
        );
        data.zoom_clips[0].mode = ZoomMode::Auto;

        assert!(data.copy_effect(EffectTrack::Zoom, 0));
        let pasted = data
            .paste_effect_at(EffectTrack::Zoom, 4.0)
            .expect("copy should place");
        assert_eq!(
            data.zoom_clips[pasted].mode,
            ZoomMode::Auto,
            "paste preserves how the clip frames"
        );

        let duplicate = data
            .duplicate_effect(EffectTrack::Zoom, pasted)
            .expect("the clip after the pasted one is free");
        assert_eq!(
            data.zoom_clips[duplicate].mode,
            ZoomMode::Auto,
            "duplicate preserves how the clip frames"
        );
    }

    #[test]
    fn the_zoom_panel_scale_edits_the_selected_clip_within_range() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_scale(2.2);
        assert!((data.zoom_clips[0].scale - 2.2).abs() < 1e-9);
        data.set_selected_zoom_scale(99.0);
        assert!((data.zoom_clips[0].scale - MAX_ZOOM_SCALE).abs() < 1e-9);
        data.set_selected_zoom_scale(0.1);
        assert!((data.zoom_clips[0].scale - MIN_ZOOM_SCALE).abs() < 1e-9);
    }

    #[test]
    fn switching_to_manual_keeps_the_framing_the_preview_shows() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_mode(ZoomMode::Auto, None);
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Auto);
        data.set_selected_zoom_mode(ZoomMode::Manual, Some((1420.0, 200.0)));
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Manual);
        assert_eq!(data.zoom_clips[0].center, (1420.0, 200.0));
        // Switching back to Auto leaves the focus point where it was: the
        // follow camera only uses it when the footage has no pointer samples.
        data.set_selected_zoom_mode(ZoomMode::Auto, None);
        assert_eq!(data.zoom_clips[0].center, (1420.0, 200.0));
    }

    #[test]
    fn instant_and_reset_move_the_clip_between_snap_and_glide() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_instant(true);
        assert!(data.zoom_clips[0].instant);
        data.update_zoom_camera(|settings| {
            settings.speed = 2.0;
            settings.start_early = true;
        });
        assert_eq!(data.zoom_camera.speed, 2.0);
        assert!(data.zoom_camera.start_early);
        data.reset_zoom_animation();
        assert!(!data.zoom_clips[0].instant);
        assert_eq!(
            data.zoom_camera,
            crate::video_editor::zoom::ZoomCameraSettings::default()
        );
    }

    #[test]
    fn a_manual_zoom_shows_its_placement_outline_while_paused() {
        let mut data = loaded();
        assert_eq!(data.placement(), None);
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        assert_eq!(data.placement(), Some((DEFAULT_ZOOM_SCALE, (960.0, 540.0))));
        data.playing = true;
        assert_eq!(data.placement(), None);
        data.playing = false;
        data.set_selected_zoom_mode(ZoomMode::Auto, None);
        assert_eq!(data.placement(), None);
        data.set_selected_zoom_mode(ZoomMode::Manual, None);
        assert!(data.placement().is_some());
        data.select_effect(EffectTrack::Zoom, None);
        assert_eq!(data.placement(), None);
    }

    #[test]
    fn placing_a_focus_point_moves_only_a_manual_zoom() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_center((400.0, 300.0));
        assert_eq!(data.zoom_clips[0].center, (400.0, 300.0));
        // A point outside the frame clamps to its edge.
        data.set_selected_zoom_center((-50.0, 5000.0));
        assert_eq!(data.zoom_clips[0].center, (0.0, 1080.0));
        // An Auto clip keeps the focus point it was detected with.
        data.set_selected_zoom_mode(ZoomMode::Auto, None);
        data.set_selected_zoom_center((10.0, 10.0));
        assert_eq!(data.zoom_clips[0].center, (0.0, 1080.0));
    }

    /// A zoom the way Detect leaves it: framing its footage and anchored to it.
    fn anchored_zoom(data: &mut Data, start: f64, end: f64) {
        let anchor = data.source_anchor(start, end);
        assert!(data
            .insert_effect(EffectTrack::Zoom, start, end, 2.0, false)
            .is_some());
        let index = data.zoom_clips.len() - 1;
        data.zoom_clips[index].anchor = anchor;
        data.select_effect(EffectTrack::Zoom, Some(index));
    }

    #[test]
    fn a_head_trim_keeps_a_detected_zoom_on_its_footage() {
        let mut data = loaded();
        anchored_zoom(&mut data, 6.0, 8.0);
        data.set_trim_start(2.0);
        let clip = data.zoom_clips[0];
        assert!((clip.start - 4.0).abs() < 1e-9, "start={}", clip.start);
        assert!((clip.end - 6.0).abs() < 1e-9, "end={}", clip.end);
        // The footage under the clip's middle is the source time it was
        // placed for, not the later time a composition-only span would show.
        let middle = (clip.start + clip.end) / 2.0;
        assert!((data.source_time_at(middle).unwrap() - 7.0).abs() < 1e-9);
    }

    #[test]
    fn a_zoom_shrinks_to_the_piece_its_footage_ended_up_in() {
        let mut data = loaded();
        anchored_zoom(&mut data, 2.0, 5.0);
        assert!(data.add_cut(3.0));
        // The clip's footage starts in the first piece, which now ends at 3 s.
        let clip = data.zoom_clips[0];
        assert!((clip.start - 2.0).abs() < 1e-9, "start={}", clip.start);
        assert!((clip.end - 3.0).abs() < 1e-9, "end={}", clip.end);
    }

    #[test]
    fn moving_a_piece_takes_its_zooms_with_it() {
        let mut data = loaded();
        assert!(data.add_cut(3.0));
        anchored_zoom(&mut data, 3.5, 5.0);
        data.set_segment_start(1, 5.0);
        let clip = data.zoom_clips[0];
        assert!((clip.start - 5.5).abs() < 1e-9, "start={}", clip.start);
        assert!((clip.end - 7.0).abs() < 1e-9, "end={}", clip.end);
    }

    #[test]
    fn a_dragged_zoom_stops_following_its_footage() {
        let mut data = loaded();
        anchored_zoom(&mut data, 2.0, 4.0);
        data.set_effect_range(EffectTrack::Zoom, 0, 2.5, 4.5);
        assert!(data.zoom_clips[0].anchor.is_none());
        data.set_trim_start(1.0);
        let clip = data.zoom_clips[0];
        assert!((clip.start - 2.5).abs() < 1e-9, "start={}", clip.start);
        assert!((clip.end - 4.5).abs() < 1e-9, "end={}", clip.end);
    }

    #[test]
    fn a_zoom_whose_footage_is_trimmed_away_is_dropped() {
        let mut data = loaded();
        anchored_zoom(&mut data, 2.0, 4.0);
        data.set_trim_start(5.0);
        assert!(data.zoom_clips.is_empty());
        assert_eq!(data.selected_zoom, None);
    }

    #[test]
    fn a_detected_click_is_placed_where_its_footage_now_sits() {
        let mut data = loaded();
        data.set_trim_start(2.0);
        // A click at source 8 s plays at composition 6 s after the trim.
        assert_eq!(data.composition_span_for_source(7.0, 9.0), Some((5.0, 7.0)));
        // Round-tripping the placed span gives the footage back.
        assert_eq!(data.source_anchor(5.0, 7.0), Some((7.0, 9.0)));
        // Footage the trim removed has no place on the timeline.
        assert_eq!(data.composition_span_for_source(0.5, 1.5), None);
    }

    #[test]
    fn a_detection_straddling_a_cut_stops_at_the_cut() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        assert_eq!(data.composition_span_for_source(3.0, 5.0), Some((3.0, 4.0)));
    }

    #[test]
    fn the_source_anchor_reads_the_piece_under_a_composition_span() {
        let mut data = loaded();
        data.set_trim_start(3.0);
        assert_eq!(data.source_anchor(1.0, 2.0), Some((4.0, 5.0)));
        assert_eq!(data.source_anchor(-2.0, 0.5), Some((3.0, 3.5)));
        assert_eq!(data.source_anchor(20.0, 22.0), None);
    }

    #[test]
    fn the_camera_settings_are_clamped_as_they_are_stored() {
        let mut data = loaded();
        data.update_zoom_camera(|settings| {
            settings.speed = 99.0;
            settings.smoothness = -1.0;
        });
        assert_eq!(data.zoom_camera.speed, 3.0);
        assert_eq!(data.zoom_camera.smoothness, 0.0);
        data.update_zoom_camera(|settings| settings.speed = f64::NAN);
        assert_eq!(data.zoom_camera.speed, 1.0);
    }

    #[test]
    fn a_paste_never_crosses_to_the_other_lane() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        assert!(data.copy_effect(EffectTrack::Zoom, 0));
        assert_eq!(data.clipboard_span(EffectTrack::Hide), None);
        assert!(data.paste_effect_at(EffectTrack::Hide, 4.0).is_none());
        // The copy is still waiting for its own lane.
        assert!(data.clipboard.is_some());
    }

    #[test]
    fn undoing_a_trim_brings_back_the_zoom_it_removed() {
        let mut data = loaded();
        anchored_zoom(&mut data, 6.0, 8.0);

        // Trim the head past the footage the zoom was placed for: the clip has
        // nothing left to follow and the composition drops it.
        data.set_trim_start(9.0);
        assert!(data.zoom_clips.is_empty());

        assert!(data.undo_edit());
        assert!(data.trim_start.abs() < 1e-9);
        assert_eq!(data.zoom_clips.len(), 1);
        assert_eq!(data.zoom_clips[0].anchor, Some((6.0, 8.0)));
        assert!((data.zoom_clips[0].start - 6.0).abs() < 1e-9);

        assert!(data.redo_edit());
        assert!(data.zoom_clips.is_empty());
        assert!((data.trim_start - 9.0).abs() < 1e-9);
    }

    #[test]
    fn a_trim_drag_is_one_undo_step() {
        let mut data = loaded();
        // A drag arrives as a stream of positions; one drag is one step, not
        // one per pointer event.
        for step in 1..=5 {
            data.set_trim_start(step as f64 * 0.2);
        }
        assert!((data.trim_start - 1.0).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!(data.trim_start.abs() < 1e-9);
        // The drag is the only step, so there is nothing older to take back.
        assert!(!data.undo_edit());
    }

    #[test]
    fn undoing_a_delete_brings_the_recording_and_its_edits_back() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        data.remove_video_clip();
        assert!(data.path.is_none());
        assert_eq!(data.duration, 0.0);

        assert!(data.undo_edit());
        assert_eq!(data.path.as_deref(), Some(Path::new("clip.mp4")));
        assert!((data.duration - 10.0).abs() < 1e-9);
        assert!((data.trim_end - 10.0).abs() < 1e-9);
        assert_eq!(data.zoom_clips.len(), 1);
    }

    #[test]
    fn a_refused_edit_leaves_nothing_to_undo() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        // The lane has no room here; the refused drop must not bury the add
        // under a step that changed nothing.
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_none());

        assert!(data.undo_edit());
        assert!(data.zoom_clips.is_empty());
        assert!(!data.undo_edit());
    }

    #[test]
    fn undoing_a_cut_drops_the_pending_paste() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        assert!(data.cut_effect(EffectTrack::Zoom, 0));
        assert!(data.zoom_clips.is_empty());
        assert!(data.can_paste());

        assert!(data.undo_edit());
        assert_eq!(data.zoom_clips.len(), 1);
        // A clip back on the track with a paste still pending would leave the
        // lane dimmed for an action the user just took back.
        assert!(!data.can_paste());
    }

    #[test]
    fn a_zoom_slider_sweep_is_one_undo_step() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        data.selected_zoom = Some(0);
        let original = data.zoom_clips[0].scale;
        for step in 1..=5 {
            data.set_selected_zoom_scale(original + step as f64 * 0.1);
        }
        assert!((data.zoom_clips[0].scale - (original + 0.5)).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!((data.zoom_clips[0].scale - original).abs() < 1e-9);
        // The add is the step below the sweep, and nothing is left after it.
        assert!(data.undo_edit());
        assert!(data.zoom_clips.is_empty());
        assert!(!data.undo_edit());
    }
}
