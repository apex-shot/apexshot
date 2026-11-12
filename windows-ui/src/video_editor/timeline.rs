//! The recording editor's timeline card.
//!
//! The chrome (tool row, transport, ruler, lanes, playhead, zoom pill) mirrors
//! the source editor. Every lane is a flat, full-width row, so time zero lands
//! on one x for the ruler, the filmstrip, the effect pills and the camera lane.
//! The video lane holds
//! the trimmed, filmstripped clip; the zoom and cursor-hide lanes always stay
//! open so clips can be dropped into them, and a recording with a camera track
//! gets a compact camera-layout lane beneath them whose chips select the
//! layout changes the Webcam panel edits. The playhead scrubs, playback
//! advances the clocks, and the zoom pill (or the wheel and scrollbar) scales
//! and pans the whole board.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gtk4::cairo::{Context, FontSlant, FontWeight, LinearGradient};
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Align, ApplicationWindow, AspectFrame, Box as GtkBox, Button, CssProvider,
    DrawingArea, EventControllerMotion, EventControllerScroll, EventControllerScrollFlags,
    GestureClick, GestureDrag, Image, Label, Orientation, Overlay, Popover, PositionType, Scale,
    Scrollbar,
};

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::recording::editor::auto_zoom;
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::CursorSettings;

use super::history::EditHistory;

const PLACEHOLDER_SECONDS: f64 = 30.0;
const FIT_SCALE_RANGE: f64 = 7.0;
const MIN_CLIP_SECONDS: f64 = 0.1;
/// Slowest and fastest segment speed a saved project may carry. The panel
/// offers a narrower range, but a file written by a build that allowed more
/// still opens at a speed this one can play.
const MIN_SEGMENT_SPEED: f64 = 0.125;
const MAX_SEGMENT_SPEED: f64 = 8.0;
/// The speed a typing suggestion applies at until the user changes it.
pub(super) const DEFAULT_TYPING_SPEED: f64 = 3.0;
/// The speed range the Cursor panel offers for typing suggestions.
pub(super) const MIN_TYPING_SPEED: f64 = 1.5;
pub(super) const MAX_TYPING_SPEED: f64 = 8.0;
const DEFAULT_ZOOM_SCALE: f64 = 2.0;
const DEFAULT_ZOOM_SECONDS: f64 = 3.0;
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
/// Layout change times closer than this are the same cue, so adding one twice
/// at a resting playhead selects the first instead of stacking a second.
const LAYOUT_CUE_EPSILON: f64 = 0.01;
/// Height of the ruler row, in unscaled pixels.
const RULER_HEIGHT: f64 = 28.0;
/// Height of the filmstrip lane, in unscaled pixels.
const VIDEO_LANE_HEIGHT: f64 = 42.0;
/// Height of one effect lane, in unscaled pixels.
const EFFECT_LANE_HEIGHT: f64 = 42.0;
/// Height of a collapsed lane, in unscaled pixels.
const THIN_LANE_HEIGHT: f64 = 18.0;
/// Gap between rows, in unscaled pixels.
const LANE_GAP: f64 = 8.0;
/// Horizontal room a chip gives its label, in unscaled pixels.
const LAYOUT_CHIP_LABEL_PADDING: f64 = 22.0;
/// The seam a chip leaves before the next cue, in unscaled pixels.
const LAYOUT_CHIP_GAP: f64 = 4.0;
/// How close to a chip's trailing edge a press grabs the resize instead of the
/// body, in unscaled pixels.
const LAYOUT_EDGE_GRAB: f64 = 8.0;
/// Top and bottom inset of a layout chip, matching the effect pills.
const LAYOUT_CHIP_INSET: f64 = 7.0;
/// Corner radius of a layout chip, matching the effect pills.
const LAYOUT_CHIP_RADIUS: f64 = 5.0;
/// Clickable radius around a chip's cue edge.
const LAYOUT_CHIP_HIT: f64 = 9.0;
/// A layout slider reports as it is dragged. Sweeps closer together than this
/// share a step, matching the history's own burst window.
const LAYOUT_EDIT_COALESCE: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectTrack {
    Zoom,
    Hide,
}

/// The secondary lane that is drawn at full height. The others rest as thin
/// strips, so the timeline keeps one height whichever lane is in use.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum TimelineLane {
    #[default]
    Zoom,
    Hide,
    Layout,
}

impl TimelineLane {
    const ALL: [Self; 3] = [Self::Zoom, Self::Hide, Self::Layout];

    fn of_effect(track: EffectTrack) -> Self {
        match track {
            EffectTrack::Zoom => Self::Zoom,
            EffectTrack::Hide => Self::Hide,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Zoom => 0,
            Self::Hide => 1,
            Self::Layout => 2,
        }
    }
}

/// What the lane stack shows: which rows are present. Playback never changes
/// it, so the layout cannot breathe in and out while the playhead moves.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct LaneState {
    camera: bool,
    zoom: bool,
    hide: bool,
    expanded: TimelineLane,
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

struct Data {
    path: Option<PathBuf>,
    /// Whether this recording has a project of its own yet. A fresh import does,
    /// so an unedited stage is written and kept; a recording opened without one
    /// has nothing worth saving until its first edit.
    has_project: bool,
    duration: f64,
    frame_size: Option<(u32, u32)>,
    fps: f64,
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
    /// The click sound and the keyboard captions the edit list carries.
    effects: super::project::EffectsSettings,
    hide_clips: Vec<HideClip>,
    selected_zoom: Option<usize>,
    /// Bumped every time a zoom clip is selected. The panel reveals itself on
    /// a new selection, and a counter is what tells a repeated click on the
    /// pill that is already selected apart from no click at all.
    zoom_selection_epoch: u64,
    selected_hide: Option<usize>,
    clipboard: Option<ClipTemplate>,
    /// The scale and instant flag of the zoom the user last edited; new zooms
    /// start from it, as in the studied build.
    last_zoom_style: Option<(f64, bool)>,
    /// The recording has pointer samples, so new zooms can follow the pointer.
    pointer_available: bool,
    /// Markers and pause points the recorder wrote for this recording, in
    /// source seconds.
    recording_marks: crate::recording::editor::sidecar::RecordingMarks,
    /// The Add zoom tool is armed: a click on the preview places a zoom.
    add_zoom_tool: bool,
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
    /// Playback speed of each cut piece, one entry per segment. The footage a
    /// piece covers stays what it is; its composition length is that footage
    /// divided by this. A missing or unusable entry plays at 1×.
    segment_speeds: Vec<f64>,
    /// Source starts of typing suggestions the user applied or removed. A
    /// suggestion whose start is listed here is not offered again.
    typing_suggestions_resolved: Vec<f64>,
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
    /// The camera layer, set once the editor has built one. Absent on a
    /// recording with no camera track.
    webcam: Option<super::webcam::WebcamModel>,
    /// The stage and the cursor, which a preset step reads and writes beside
    /// the edit list. Absent on a document built without the panels.
    background: Option<super::background::Background>,
    cursor: Option<super::cursor::CursorModel>,
    /// The look the last undo or redo put back, for the timeline to apply to
    /// the stage and the cursor it does not own.
    restored_look: Option<Look>,
    /// The layout change the Webcam panel is editing, when one is selected.
    selected_layout: Option<usize>,
    /// Bumped every time a layout change is selected, including a repeated
    /// click on the change already selected. The panel reveals itself on a
    /// new selection, and a counter is what tells that apart from a repaint.
    layout_selection_epoch: u64,
    /// When a layout slider last reported. One sweep is one undo step, and it
    /// must not fold into a geometry sweep that just ended.
    layout_burst: Option<Instant>,
    /// The secondary lane drawn at full height; the others rest thin.
    expanded_lane: TimelineLane,
    /// How expanded each lane is drawn right now, 0 thin .. 1 full.
    lane_progress: [f64; 3],
}

impl Default for Data {
    fn default() -> Self {
        Self {
            path: None,
            has_project: false,
            duration: 0.0,
            frame_size: None,
            fps: 0.0,
            trim_start: 0.0,
            trim_end: 0.0,
            playhead: 0.0,
            scale: 0.0,
            scroll: 0.0,
            playing: false,
            selected: false,
            hover: None,
            frames: Vec::new(),
            zoom_clips: Vec::new(),
            zoom_camera: super::zoom::ZoomCameraSettings::default(),
            effects: super::project::EffectsSettings::default(),
            hide_clips: Vec::new(),
            selected_zoom: None,
            zoom_selection_epoch: 0,
            selected_hide: None,
            clipboard: None,
            last_zoom_style: None,
            pointer_available: false,
            recording_marks: Default::default(),
            add_zoom_tool: false,
            hover_zoom: None,
            hover_hide: None,
            dragging_effect: false,
            cuts: Vec::new(),
            segment_starts: Vec::new(),
            segment_speeds: Vec::new(),
            typing_suggestions_resolved: Vec::new(),
            moving_segment: None,
            freeze_tail: 0.0,
            history: EditHistory::default(),
            webcam: None,
            background: None,
            cursor: None,
            restored_look: None,
            selected_layout: None,
            layout_selection_epoch: 0,
            layout_burst: None,
            expanded_lane: TimelineLane::Zoom,
            lane_progress: [1.0, 0.0, 0.0],
        }
    }
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
    /// Whether the recording had a project when the step was taken, so undoing
    /// an edit never takes the project's existence away with it.
    has_project: bool,
    duration: f64,
    frame_size: Option<(u32, u32)>,
    fps: f64,
    frames: Vec<gtk4::gdk_pixbuf::Pixbuf>,
    trim_start: f64,
    trim_end: f64,
    cuts: Vec<f64>,
    segment_starts: Vec<f64>,
    segment_speeds: Vec<f64>,
    typing_suggestions_resolved: Vec<f64>,
    freeze_tail: f64,
    zoom_clips: Vec<ZoomClip>,
    hide_clips: Vec<HideClip>,
    zoom_camera: super::zoom::ZoomCameraSettings,
    effects: super::project::EffectsSettings,
    /// The camera layer, which lives in the webcam model rather than here;
    /// the snapshot keeps the value so undo can put the bubble back.
    webcam: Option<super::webcam::LayerSettings>,
    /// The layout timeline, also kept in the webcam model. It is `Clone` rather
    /// than `Copy` because of the cue list, so the step carries its own copy.
    webcam_layouts: Option<super::webcam::LayoutSettings>,
    selected: bool,
    selected_zoom: Option<usize>,
    selected_hide: Option<usize>,
    selected_layout: Option<usize>,
    clipboard: Option<ClipTemplate>,
    /// The stage and cursor look, carried only by a step that applied a preset.
    look: Option<Look>,
}

/// The stage and the cursor look a preset writes. They live beside the edit
/// list, so a step that applies a preset carries them to take back or replay
/// with the document.
#[derive(Clone, PartialEq)]
struct Look {
    stage: (Option<String>, f64, f64),
    cursor: super::presets::CursorFile,
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
                    let (start, _) = self.segment_span(index)?;
                    let &(source_start, source_end) = boundaries.get(index)?;
                    let footage_end =
                        start + (source_end - source_start).max(0.0) / self.segment_speed(index);
                    Some((start, footage_end, source_start, source_end))
                })
                .collect(),
        )
    }

    fn frame_center(&self) -> (f64, f64) {
        self.frame_size
            .map(|(width, height)| (f64::from(width) / 2.0, f64::from(height) / 2.0))
            .unwrap_or((0.0, 0.0))
    }

    /// The recording frame's width over height, for a shape that reads the
    /// same on this footage. 16:9 while the frame size is unknown.
    fn frame_aspect(&self) -> f64 {
        self.frame_size
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| f64::from(width) / f64::from(height))
            .filter(|aspect| aspect.is_finite() && *aspect > 0.0)
            .unwrap_or(16.0 / 9.0)
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

    /// Playback speed of one cut piece. A speed the file cannot mean — a
    /// non-number, zero or a negative — plays at 1×, and the runtime bounds
    /// are wider than the panel offers so a file from another build still
    /// opens at something this one can play.
    fn segment_speed(&self, index: usize) -> f64 {
        self.segment_speeds
            .get(index)
            .copied()
            .filter(|speed| speed.is_finite() && *speed > 0.0)
            .map(|speed| speed.clamp(MIN_SEGMENT_SPEED, MAX_SEGMENT_SPEED))
            .unwrap_or(1.0)
    }

    /// Composition span of one segment, hold included on the last piece.
    fn segment_span(&self, index: usize) -> Option<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        let &(source_start, source_end) = boundaries.get(index)?;
        let start = self.segment_start(index);
        let mut end = start + (source_end - source_start).max(0.0) / self.segment_speed(index);
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
            let speed = self.segment_speed(index);
            let start = self.segment_start(index);
            let end = start + (source_end - source_start).max(0.0) / speed;
            if composition_time + 1e-9 >= start && composition_time <= end + 1e-9 {
                return Some(
                    (source_start + (composition_time - start) * speed)
                        .clamp(source_start, source_end),
                );
            }
        }
        None
    }

    /// Source time to composition time through the first segment that holds
    /// it. Footage the edit cut away answers `None`.
    fn composition_time_for_source(&self, source_time: f64) -> Option<f64> {
        for (index, &(source_start, source_end)) in self.segment_boundaries().iter().enumerate() {
            if source_time + 1e-9 >= source_start && source_time <= source_end + 1e-9 {
                return Some(
                    self.segment_start(index)
                        + (source_time - source_start).max(0.0) / self.segment_speed(index),
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
            let end = start + (source_end - source_start).max(0.0) / self.segment_speed(index);
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
        let duration = (source_end - source_start).max(0.0) / self.segment_speed(index);
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
                    let other_end = other_start + (s1 - s0).max(0.0) / self.segment_speed(other);
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
                .map(|(start, end)| (end - start).max(0.0) / self.segment_speed(index - 1))
                .unwrap_or(0.0);
            self.segment_starts.push(previous_start + previous_length);
        }
        if let Some(first) = self.segment_starts.first_mut() {
            if count <= 1 {
                *first = 0.0;
            }
        }
    }

    /// Keep one speed per cut piece. A list shorter than the segment count is
    /// filled with 1×, so a piece the file never named plays at normal speed.
    fn normalize_segment_speeds(&mut self) {
        let count = self.segment_count();
        self.segment_speeds.truncate(count);
        self.segment_speeds.resize(count, 1.0);
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
        let speed = self.segment_speed(last);
        let value = last_source_start + (desired.max(0.0) - last_start).max(0.0) * speed;
        self.trim_end = value.clamp(floor, duration);
        let footage_end = last_start + (self.trim_end - last_source_start).max(0.0) / speed;
        self.freeze_tail = (desired.max(0.0) - footage_end).max(0.0);
        self.clamp_playhead();
        self.clamp_cuts();
        self.reproject_anchored_zooms();
        self.clamp_effects();
        self.clamp_scroll();
    }

    fn reset_media(&mut self) {
        self.path = None;
        self.has_project = false;
        self.duration = 0.0;
        self.frame_size = None;
        self.fps = 0.0;
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
        self.last_zoom_style = None;
        self.pointer_available = false;
        self.recording_marks = Default::default();
        self.add_zoom_tool = false;
        self.hover_zoom = None;
        self.hover_hide = None;
        self.dragging_effect = false;
        self.zoom_camera = super::zoom::ZoomCameraSettings::default();
        self.effects = super::project::EffectsSettings::default();
        self.cuts.clear();
        self.segment_starts.clear();
        self.segment_speeds.clear();
        self.typing_suggestions_resolved.clear();
        self.moving_segment = None;
        self.freeze_tail = 0.0;
        self.selected_layout = None;
        self.layout_selection_epoch = 0;
        self.layout_burst = None;
        self.expanded_lane = TimelineLane::Zoom;
        self.lane_progress = [1.0, 0.0, 0.0];
    }

    /// The editable state, as one undo step keeps it.
    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            path: self.path.clone(),
            has_project: self.has_project,
            duration: self.duration,
            frame_size: self.frame_size,
            fps: self.fps,
            frames: self.frames.clone(),
            trim_start: self.trim_start,
            trim_end: self.trim_end,
            cuts: self.cuts.clone(),
            segment_starts: self.segment_starts.clone(),
            segment_speeds: self.segment_speeds.clone(),
            typing_suggestions_resolved: self.typing_suggestions_resolved.clone(),
            freeze_tail: self.freeze_tail,
            zoom_clips: self.zoom_clips.clone(),
            hide_clips: self.hide_clips.clone(),
            zoom_camera: self.zoom_camera,
            effects: self.effects.clone(),
            webcam: self.webcam.as_ref().and_then(|model| model.settings()),
            webcam_layouts: self.webcam_layouts(),
            selected: self.selected,
            selected_zoom: self.selected_zoom,
            selected_hide: self.selected_hide,
            selected_layout: self.selected_layout,
            clipboard: self.clipboard,
            look: None,
        }
    }

    /// Take the step a continuous edit is about to change.
    ///
    /// One trim drag or one slider sweep arrives as a stream of updates, so
    /// the step is taken on the first of the burst and the rest leave it
    /// alone. It also closes any layout sweep, because a different control
    /// reporting means that sweep is over and the next one is its own step.
    fn record_continuous_edit(&mut self) {
        self.layout_burst = None;
        let before = self.snapshot();
        self.history.record_continuous(before);
    }

    /// Record a step for a command that has already been applied, so an edit
    /// the document refused leaves nothing to undo.
    fn record_edit(&mut self, before: EditSnapshot) {
        self.layout_burst = None;
        self.history.record_command(before);
    }

    /// Take the step a layout edit is about to change.
    ///
    /// The transition and split sliders report continuously, so a sweep is one
    /// step. Every report restarts the burst window, so a drag longer than the
    /// window stays one step instead of splitting once per window. A sweep
    /// always opens a step of its own even when a geometry slider was the last
    /// thing to report: the two controls are separate edits, and folding them
    /// together would make one undo take back both.
    fn record_layout_edit(&mut self, continuous: bool) {
        if continuous {
            let fresh = self
                .layout_burst
                .is_none_or(|at| at.elapsed() > LAYOUT_EDIT_COALESCE);
            self.layout_burst = Some(Instant::now());
            if !fresh {
                return;
            }
        } else {
            self.layout_burst = None;
        }
        let before = self.snapshot();
        self.history.record_command(before);
    }

    /// Restore the document to how it stood before the last edit.
    ///
    /// Returns `false` when there is nothing left to take back, which leaves
    /// the document untouched.
    fn undo_edit(&mut self) -> bool {
        let current = self.current_state(self.history.peek_undo());
        let Some(previous) = self.history.undo(current) else {
            return false;
        };
        self.restore(previous);
        true
    }

    /// Replay the edit the last undo took back.
    fn redo_edit(&mut self) -> bool {
        let current = self.current_state(self.history.peek_redo());
        let Some(next) = self.history.redo(current) else {
            return false;
        };
        self.restore(next);
        true
    }

    /// The state an undo or redo leaves on the opposite stack. The live look
    /// goes with it only when the step being taken carries one, so an ordinary
    /// step never moves the stage when it is replayed.
    fn current_state(&self, step: Option<&EditSnapshot>) -> EditSnapshot {
        let mut current = self.snapshot();
        if step.is_some_and(|step| step.look.is_some()) {
            current.look = self.live_look();
        }
        current
    }

    /// The stage and the cursor as they stand now, or `None` before the panels
    /// exist.
    fn live_look(&self) -> Option<Look> {
        let background = self.background.as_ref()?;
        let cursor = self.cursor.as_ref()?;
        Some(Look {
            stage: background.snapshot(),
            cursor: super::presets::CursorFile::from_settings(cursor.settings()),
        })
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
        self.has_project = snapshot.has_project;
        self.duration = snapshot.duration;
        self.frame_size = snapshot.frame_size;
        self.fps = snapshot.fps;
        self.trim_start = snapshot.trim_start;
        self.trim_end = snapshot.trim_end;
        self.cuts = snapshot.cuts;
        self.segment_starts = snapshot.segment_starts;
        self.segment_speeds = snapshot.segment_speeds;
        self.typing_suggestions_resolved = snapshot.typing_suggestions_resolved;
        self.freeze_tail = snapshot.freeze_tail;
        self.zoom_clips = snapshot.zoom_clips;
        self.hide_clips = snapshot.hide_clips;
        self.zoom_camera = snapshot.zoom_camera;
        self.effects = snapshot.effects;
        if let (Some(webcam), Some(layer)) = (self.webcam.as_ref(), snapshot.webcam) {
            webcam.set_settings(layer);
        }
        if let (Some(webcam), Some(layouts)) = (self.webcam.as_ref(), snapshot.webcam_layouts) {
            webcam.set_layouts(layouts);
        }
        self.selected = snapshot.selected;
        self.selected_zoom = snapshot.selected_zoom;
        self.selected_hide = snapshot.selected_hide;
        self.selected_layout = snapshot.selected_layout;
        self.clipboard = snapshot.clipboard;
        self.restored_look = snapshot.look;
        self.layout_burst = None;
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
    /// "nothing changed" would only put an empty card in History. A session that
    /// already has a project keeps it even with nothing edited, so a stage reset
    /// to flat is not lost and the next open does not seed it again.
    ///
    /// The stage is part of the document, so `background` contributes its
    /// wallpaper, padding and radius: a recording opened on a polished stage
    /// has something worth saving even before an edit is made.
    fn project_file(
        &self,
        background: &super::background::Background,
    ) -> Option<super::project::EditorProjectFile> {
        let path = self.path.as_ref()?;
        let (stage_selection, background_padding, background_radius) = background.snapshot();
        let stage_edited = stage_selection.is_some()
            || (background_padding - super::background::DEFAULT_PADDING).abs() > f64::EPSILON
            || background_radius.abs() > f64::EPSILON;
        let layouts = self.webcam_layouts();
        let layouts_edited = layouts
            .as_ref()
            .is_some_and(|layouts| *layouts != super::webcam::LayoutSettings::default());
        let webcam = self
            .webcam
            .as_ref()
            .filter(|model| model.edited() || layouts_edited)
            .and_then(|model| model.settings())
            .map(|layer| {
                let layouts = layouts.clone().unwrap_or_default();
                super::project::WebcamLayerFile::from_layer(layer, &layouts)
            });
        let pristine = !stage_edited
            && webcam.is_none()
            && self.cuts.is_empty()
            && self
                .segment_speeds
                .iter()
                .all(|speed| (speed - 1.0).abs() <= 1e-9)
            && self.typing_suggestions_resolved.is_empty()
            && self.zoom_clips.is_empty()
            && self.hide_clips.is_empty()
            && self.freeze_tail <= f64::EPSILON
            && self.trim_start.abs() <= 0.001
            && (self.duration - self.trim_end).abs() <= 0.001
            && self.zoom_camera == super::zoom::ZoomCameraSettings::default()
            && self.effects == super::project::EffectsSettings::default();
        if pristine && !self.has_project {
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
            segment_speeds: self.segment_speeds.clone(),
            typing_suggestions_resolved: self.typing_suggestions_resolved.clone(),
            freeze_tail: self.freeze_tail,
            zoom_clips: self.zoom_clips.iter().map(|clip| clip.to_file()).collect(),
            zoom_camera: super::project::ZoomCameraFile {
                speed: self.zoom_camera.speed,
                smoothness: self.zoom_camera.smoothness,
                start_early: self.zoom_camera.start_early,
                motion_blur: self.zoom_camera.motion_blur,
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
            webcam,
            background: super::project::StageFile {
                selection: stage_selection,
            },
            background_padding,
            background_radius,
            playhead_seconds: self.playhead,
            timeline_scale: self.scale,
            timeline_scroll_seconds: self.scroll,
            cursor_click_sound: self.effects.click_sound.clone(),
            cursor_click_sound_volume: self.effects.click_sound_volume,
            captions_shortcuts: self.effects.captions.shortcuts,
            captions_show_single_letters: self.effects.captions.show_single_letters,
            cursor: Default::default(),
        })
    }

    /// Restore a saved edit list onto freshly loaded media. Every value is
    /// clamped to the recording that is on disk, so a project written against
    /// different footage — or one hand-edited into nonsense — still lands
    /// inside the composition instead of dragging the board off it.
    ///
    /// A camera block is read through its resolved layouts, so an older file
    /// written before layouts existed still opens on the picture its two
    /// switches described. The layer itself stays available even for a
    /// screen-only start, so a later cue can bring the camera back.
    ///
    /// Segment starts are completed before the trim clamp runs, so a file
    /// written before pieces carried their own starts spreads them out
    /// contiguously instead of stacking every piece at zero.
    fn apply_project(&mut self, file: &super::project::EditorProjectFile) {
        let duration = self.duration.max(0.0);
        self.trim_start = file.trim_start_seconds.clamp(0.0, duration);
        self.trim_end = file.trim_end_seconds.clamp(self.trim_start, duration);
        self.cuts = file
            .cuts
            .iter()
            .copied()
            .filter(|cut| cut.is_finite() && *cut > self.trim_start && *cut < self.trim_end)
            .collect();
        self.cuts.sort_by(f64::total_cmp);
        self.cuts
            .dedup_by(|left, right| (*left - *right).abs() <= 1e-9);
        self.segment_starts = file
            .segment_starts
            .iter()
            .copied()
            .filter(|start| start.is_finite() && *start >= 0.0)
            .collect();
        self.segment_speeds = file
            .segment_speeds
            .iter()
            .map(|speed| {
                if speed.is_finite() && *speed > 0.0 {
                    speed.clamp(MIN_SEGMENT_SPEED, MAX_SEGMENT_SPEED)
                } else {
                    1.0
                }
            })
            .collect();
        self.typing_suggestions_resolved = file
            .typing_suggestions_resolved
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
            motion_blur: file.zoom_camera.motion_blur,
            frame_rate: super::zoom::ZoomCameraSettings::default().frame_rate,
        }
        .clamped();
        if let (Some(webcam), Some(layer)) = (self.webcam.as_ref(), file.webcam.as_ref()) {
            let layouts = layer.resolved_layouts();
            webcam.set_settings(super::webcam::LayerSettings {
                // Only a file that carries the layouts block is describing the
                // layer's own switch. Older files used `enabled` for the
                // default layout, and their screen-only default already says
                // the camera was out of the picture.
                enabled: if layer.layouts.is_some() {
                    layer.enabled
                } else {
                    true
                },
                fullscreen: false,
                x: layer.x,
                y: layer.y,
                w: layer.w,
                h: layer.h,
                radius: layer.radius,
                mirror: layer.mirror,
                delay_seconds: layer.delay_seconds,
                zoomed_scale: layer.zoomed_scale,
                hide_during_silence: layer.hide_during_silence,
            });
            webcam.set_layouts(layouts);
        } else if let Some(webcam) = self.webcam.as_ref() {
            webcam.set_layouts(super::webcam::LayoutSettings::default());
        }
        self.selected_layout = None;
        self.effects = file.effects();
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
        self.normalize_segment_starts();
        self.clamp_cuts();
        self.clamp_effects();
        self.clamp_scroll();
        self.clamp_playhead();
        self.has_project = true;
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
        let speed = self.segment_speed(split_index);
        let right_start = left_start + (source - left_source_start).max(0.0) / speed;
        self.cuts.insert(split_index, source);
        if self.segment_starts.len() <= split_index {
            self.segment_starts.resize(split_index + 1, left_start);
        }
        self.segment_starts.insert(split_index + 1, right_start);
        if self.segment_speeds.len() <= split_index {
            self.segment_speeds.resize(split_index + 1, 1.0);
        }
        self.segment_speeds.insert(split_index + 1, speed);
        self.normalize_segment_starts();
        self.normalize_segment_speeds();
        self.reproject_anchored_zooms();
        self.record_edit(before);
        true
    }

    /// Apply typing suggestions as one undo step. Ranges that are not a real
    /// pause, or that are already resolved, change nothing.
    fn apply_typing_ranges(&mut self, ranges: &[(f64, f64)], speed: f64) -> bool {
        if ranges.is_empty() {
            return false;
        }
        let before = self.snapshot();
        let mut changed = false;
        for &(start, end) in ranges {
            changed |= self.apply_typing_range(start, end, speed);
        }
        if changed {
            self.record_edit(before);
        }
        changed
    }

    /// Slow one source range to `speed`: cut the range out of the pieces it
    /// straddles, give every piece inside it that speed, and pull the pieces
    /// after it back by the footage they no longer spend, keeping their gaps.
    fn apply_typing_range(&mut self, start: f64, end: f64, speed: f64) -> bool {
        if !start.is_finite() || !end.is_finite() || !speed.is_finite() || speed <= 0.0 {
            return false;
        }
        if !self.has_media() {
            return false;
        }
        let speed = speed.clamp(MIN_SEGMENT_SPEED, MAX_SEGMENT_SPEED);
        if start < self.trim_start - 1e-9 || end > self.trim_end + 1e-9 {
            return false;
        }
        let start = start.max(self.trim_start);
        let end = end.min(self.trim_end);
        if end - start <= 1e-6 {
            return false;
        }
        if self
            .typing_suggestions_resolved
            .iter()
            .any(|resolved| (resolved - start).abs() <= 1e-6)
        {
            return false;
        }
        let old_boundaries = self.segment_boundaries();
        if old_boundaries.is_empty() {
            return false;
        }
        let old_starts: Vec<f64> = (0..old_boundaries.len())
            .map(|index| self.segment_start(index))
            .collect();
        let old_speeds: Vec<f64> = (0..old_boundaries.len())
            .map(|index| self.segment_speed(index))
            .collect();
        let old_ends: Vec<f64> = old_boundaries
            .iter()
            .enumerate()
            .map(|(index, &(source_start, source_end))| {
                old_starts[index] + (source_end - source_start).max(0.0) / old_speeds[index]
            })
            .collect();

        let old_cuts = self.cuts.clone();
        let mut cuts = self.cuts.clone();
        for boundary in [start, end] {
            if boundary > self.trim_start + 1e-9
                && boundary < self.trim_end - 1e-9
                && !cuts.iter().any(|cut| (cut - boundary).abs() <= 1e-6)
            {
                let index = cuts.partition_point(|cut| *cut < boundary);
                cuts.insert(index, boundary);
            }
        }
        cuts.sort_by(f64::total_cmp);
        self.cuts = cuts;

        let boundaries = self.segment_boundaries();
        let mut owners = Vec::with_capacity(boundaries.len());
        for &(source_start, _) in &boundaries {
            let owner = old_boundaries
                .iter()
                .enumerate()
                .filter(|(_, &(old_start, old_end))| {
                    source_start + 1e-9 >= old_start && source_start <= old_end + 1e-9
                })
                .max_by(|(_, left), (_, right)| left.0.total_cmp(&right.0))
                .map(|(index, _)| index);
            let Some(owner) = owner else {
                self.cuts = old_cuts.clone();
                return false;
            };
            owners.push(owner);
        }

        let mut speeds = Vec::with_capacity(boundaries.len());
        for (index, &(source_start, source_end)) in boundaries.iter().enumerate() {
            let inside = source_start >= start - 1e-6 && source_end <= end + 1e-6;
            speeds.push(if inside {
                speed
            } else {
                old_speeds[owners[index]]
            });
        }

        let mut starts = Vec::with_capacity(boundaries.len());
        starts.push(old_starts[owners[0]]);
        for index in 1..boundaries.len() {
            let (source_start, source_end) = boundaries[index - 1];
            let previous_duration = (source_end - source_start).max(0.0) / speeds[index - 1];
            let gap = if owners[index] == owners[index - 1] {
                0.0
            } else {
                old_starts[owners[index]] - old_ends[owners[index - 1]]
            };
            starts.push(starts[index - 1] + previous_duration + gap);
        }

        self.segment_starts = starts;
        self.segment_speeds = speeds;
        self.normalize_segment_starts();
        self.normalize_segment_speeds();
        if !self
            .typing_suggestions_resolved
            .iter()
            .any(|resolved| (resolved - start).abs() <= 1e-6)
        {
            self.typing_suggestions_resolved.push(start);
        }
        self.reproject_anchored_zooms();
        self.clamp_effects();
        self.clamp_playhead();
        self.clamp_scroll();
        true
    }

    /// Stop offering the suggestion that starts at `start`, as one undo step.
    fn dismiss_typing_suggestion(&mut self, start: f64) -> bool {
        if !start.is_finite() || start < 0.0 {
            return false;
        }
        if self.typing_range_resolved(start, start) {
            return false;
        }
        let before = self.snapshot();
        self.typing_suggestions_resolved.push(start);
        self.record_edit(before);
        true
    }

    /// Whether a suggestion has already been applied or removed, by either the
    /// detected range's start or the start it is shown at after trimming.
    fn typing_range_resolved(&self, original_start: f64, clipped_start: f64) -> bool {
        self.typing_suggestions_resolved.iter().any(|resolved| {
            (resolved - original_start).abs() <= 1e-6 || (resolved - clipped_start).abs() <= 1e-6
        })
    }

    /// Move a resolved entry from the start a suggestion was shown at to the
    /// start the recording detected it at, so the same pause stays resolved
    /// when trimming changes where it is shown. Applied ranges record their
    /// resolved start before this runs, so undo keeps the step and redo keeps
    /// the final identifiers.
    fn replace_typing_resolved(&mut self, clipped_start: f64, original_start: f64) {
        for resolved in &mut self.typing_suggestions_resolved {
            if (*resolved - clipped_start).abs() <= 1e-6 {
                *resolved = original_start;
            }
        }
        let mut seen: Vec<f64> = Vec::with_capacity(self.typing_suggestions_resolved.len());
        for resolved in self.typing_suggestions_resolved.drain(..) {
            if !seen
                .iter()
                .any(|existing| (*existing - resolved).abs() <= 1e-6)
            {
                seen.push(resolved);
            }
        }
        self.typing_suggestions_resolved = seen;
    }

    /// Trimming can leave cuts outside the kept footage; those segments no
    /// longer exist, and their starts and speeds go with them. A piece that
    /// survives keeps the start and speed of the old piece whose source span
    /// holds its new first frame, so trimming across a cut never resets a
    /// moved or sped piece. The old topology comes from the cuts as they were
    /// before the trim, because the new trim can hide the very cut that
    /// identifies the piece.
    fn clamp_cuts(&mut self) {
        let old_cuts = self.cuts.clone();
        let old_starts = self.segment_starts.clone();
        let old_speeds = self.segment_speeds.clone();
        let (start, end) = (self.trim_start, self.trim_end);
        self.cuts
            .retain(|cut| *cut > start + 1e-9 && *cut < end - 1e-9);
        let after = self.segment_boundaries();
        let mut starts = Vec::with_capacity(after.len());
        let mut speeds = Vec::with_capacity(after.len());
        for &(source_start, _) in &after {
            let owner = old_cuts.partition_point(|cut| *cut <= source_start + 1e-9);
            starts.push(old_starts.get(owner).copied().unwrap_or(0.0));
            speeds.push(
                old_speeds
                    .get(owner)
                    .copied()
                    .filter(|speed| speed.is_finite() && *speed > 0.0)
                    .map(|speed| speed.clamp(MIN_SEGMENT_SPEED, MAX_SEGMENT_SPEED))
                    .unwrap_or(1.0),
            );
        }
        self.segment_starts = starts;
        self.segment_speeds = speeds;
        self.normalize_segment_starts();
        self.normalize_segment_speeds();
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
            // Selecting a clip puts the Add zoom tool away, as in the studied build.
            self.add_zoom_tool = false;
            self.expanded_lane = TimelineLane::of_effect(track);
        }
        if track == EffectTrack::Zoom && index.is_some() {
            self.zoom_selection_epoch = self.zoom_selection_epoch.wrapping_add(1);
        }
    }

    fn add_effect(&mut self, track: EffectTrack, start: f64) -> Option<usize> {
        self.add_effect_at(track, start, None)
    }

    /// Drop a default-length effect at `start`, in composition time.
    ///
    /// A zoom dropped inside another splits that one at the playhead, so the
    /// two halves keep their framing instead of stacking pills on one span. A
    /// new zoom starts from the scale and snap flag the user last edited, and
    /// it follows the pointer only when the recording has samples for it.
    /// A hide is refused when there is no room left or it would overlap a
    /// sibling, which is what the studied build does instead of stacking
    /// pills.
    ///
    /// A target point, in source pixels, makes the zoom Manual and aims it
    /// there; that is what a click on the preview with the Add zoom tool
    /// places.
    fn add_effect_at(
        &mut self,
        track: EffectTrack,
        start: f64,
        target: Option<(f64, f64)>,
    ) -> Option<usize> {
        if !self.has_media() {
            return None;
        }
        let limit = self.total_duration();
        if limit <= MIN_CLIP_SECONDS {
            return None;
        }
        let start = start.clamp(0.0, (limit - MIN_CLIP_SECONDS).max(0.0));
        let before = self.snapshot();
        match track {
            EffectTrack::Zoom => {
                let target = match (target, self.frame_size) {
                    (None, _) => None,
                    (Some((x, y)), Some((width, height))) if x.is_finite() && y.is_finite() => {
                        Some((
                            x.clamp(0.0, f64::from(width)),
                            y.clamp(0.0, f64::from(height)),
                        ))
                    }
                    _ => return None,
                };
                if let Some(index) = self
                    .zoom_clips
                    .iter()
                    .position(|clip| clip.start < start && start < clip.end)
                {
                    let clip = self.zoom_clips[index];
                    if start - clip.start < MIN_CLIP_SECONDS || clip.end - start < MIN_CLIP_SECONDS
                    {
                        return None;
                    }
                    self.zoom_clips[index].end = start;
                    let mut right = ZoomClip { start, ..clip };
                    if let Some(center) = target {
                        right.mode = ZoomMode::Manual;
                        right.center = center;
                    }
                    self.zoom_clips.insert(index + 1, right);
                    self.select_effect(track, Some(index + 1));
                    self.record_edit(before);
                    return Some(index + 1);
                }
                let next_start = self
                    .zoom_clips
                    .iter()
                    .filter(|clip| clip.start >= start)
                    .map(|clip| clip.start)
                    .reduce(f64::min)
                    .unwrap_or(limit);
                let end = (start + DEFAULT_ZOOM_SECONDS).min(next_start).min(limit);
                if end - start < MIN_CLIP_SECONDS {
                    return None;
                }
                let scale = self
                    .last_zoom_style
                    .map(|(scale, _)| scale)
                    .unwrap_or(DEFAULT_ZOOM_SCALE)
                    .clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
                let instant = self.last_zoom_style.is_some_and(|(_, instant)| instant);
                let mode = if target.is_none() && self.pointer_available {
                    ZoomMode::Auto
                } else {
                    ZoomMode::Manual
                };
                let index = self.insert_effect(track, start, end, scale, false)?;
                self.zoom_clips[index].mode = mode;
                self.zoom_clips[index].instant = instant;
                if let Some(center) = target {
                    self.zoom_clips[index].center = center;
                }
                self.select_effect(track, Some(index));
                self.record_edit(before);
                Some(index)
            }
            EffectTrack::Hide => {
                let end = (start + DEFAULT_EFFECT_SECONDS).min(limit);
                let index = self.insert_effect(track, start, end, DEFAULT_ZOOM_SCALE, false)?;
                self.select_effect(track, Some(index));
                self.record_edit(before);
                Some(index)
            }
        }
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

    /// The lane flags come from one snapshot, so the rows cannot disagree
    /// about which ones exist. Both effect rows are permanent drop targets:
    /// an empty lane still shows where a clip can go.
    fn lane_state(&self) -> LaneState {
        LaneState {
            camera: self
                .webcam
                .as_ref()
                .is_some_and(|webcam| webcam.has_track()),
            zoom: true,
            hide: true,
            expanded: self.expanded_lane,
        }
    }

    /// Whether `lane` is the one drawn at full height.
    fn lane_expanded(&self, lane: TimelineLane) -> bool {
        self.expanded_lane == lane
    }

    /// Make `lane` the full-height one. Returns whether it changed.
    fn expand_lane(&mut self, lane: TimelineLane) -> bool {
        let changed = self.expanded_lane != lane;
        self.expanded_lane = lane;
        changed
    }

    /// How far `lane` is drawn toward full height right now, 0..=1.
    fn lane_progress(&self, lane: TimelineLane) -> f64 {
        self.lane_progress[lane.index()]
    }

    /// Ease every lane toward its target (1 for the expanded lane, 0 for the
    /// rest) by `dt` seconds. Returns whether anything is still moving.
    fn step_lane_progress(&mut self, dt: f64) -> bool {
        // About 180 ms to settle: an exponential ease with a 45 ms constant.
        let amount = 1.0 - (-dt.max(0.0) / 0.045).exp();
        let mut next = self.lane_progress;
        let mut targets = [0.0; 3];
        for lane in TimelineLane::ALL {
            let index = lane.index();
            targets[index] = if self.expanded_lane == lane { 1.0 } else { 0.0 };
            next[index] += (targets[index] - next[index]) * amount;
        }
        // Snap only once every lane has arrived: snapping one early would
        // break their sum, which is what keeps the timeline's height fixed.
        let arrived = (0..3).all(|index| (targets[index] - next[index]).abs() < 0.002);
        self.lane_progress = if arrived { targets } else { next };
        !arrived
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

    /// Arm or put away the Add zoom tool. Arming pauses and clears the
    /// selection, so the preview shows the whole frame to aim at. Returns
    /// whether anything changed.
    fn set_add_zoom_tool(&mut self, armed: bool) -> bool {
        if armed && !self.has_media() {
            return false;
        }
        if self.add_zoom_tool == armed {
            return false;
        }
        self.add_zoom_tool = armed;
        if armed {
            self.playing = false;
            self.selected = false;
            self.selected_zoom = None;
            self.selected_hide = None;
        }
        true
    }

    /// The armed tool as the preview draws it. Playing hides it, as in the
    /// studied build, without putting it away.
    fn add_zoom_tool(&self) -> Option<AddZoomTool> {
        if !self.add_zoom_tool || self.playing {
            return None;
        }
        let scale = self
            .last_zoom_style
            .map(|(scale, _)| scale)
            .unwrap_or(DEFAULT_ZOOM_SCALE)
            .clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
        Some(AddZoomTool {
            scale,
            automatic: self.pointer_available,
        })
    }

    /// Put the camera layer on the playhead: the composition instant its
    /// layouts run on, how far the screen's zoom has shrunk it there, and the
    /// split's resting edge from the cursor.
    /// Both preview paths (the tick and a published edit) call this, so the
    /// camera never runs on a clock the shrink was not computed for.
    fn sync_webcam_to_playhead(
        &self,
        sidecar: impl FnOnce() -> Option<crate::recording::editor::sidecar::PointerSidecar>,
    ) {
        let Some(webcam) = self.webcam.as_ref() else {
            return;
        };
        webcam.set_composition_time(self.playhead);
        let unzoomed = self.placement().is_some() || self.add_zoom_tool().is_some();
        webcam.set_zoom_shrink(if unzoomed {
            0.0
        } else {
            super::webcam::zoom_shrink_at(
                &self.zoom_clips,
                self.playhead,
                self.zoom_camera.lead_in_seconds(),
            )
        });
        let fps = if self.fps > 0.0 { self.fps } else { 30.0 };
        webcam.set_split_rest_at(
            self.playhead,
            &self.zoom_clips,
            &self.camera_mapping(),
            self.total_duration(),
            super::zoom::ZoomCameraSettings {
                frame_rate: fps,
                ..self.zoom_camera
            },
            self.frame_size,
            sidecar,
        );
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
        let clip = self.zoom_clips[index];
        self.last_zoom_style = Some((clip.scale, clip.instant));
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
        let clip = self.zoom_clips[index];
        self.last_zoom_style = Some((clip.scale, clip.instant));
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

    fn update_effects(&mut self, update: impl FnOnce(&mut super::project::EffectsSettings)) {
        let mut effects = self.effects.clone();
        update(&mut effects);
        let effects = effects.clamped();
        if effects == self.effects {
            return;
        }
        self.record_continuous_edit();
        self.effects = effects;
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
        self.clamp_layout_cues();
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
        let speed = self.segment_speed(index);
        Some((
            composition_start + (start - source_start) / speed,
            composition_start + (end - source_start) / speed,
        ))
    }

    /// The footage a composition span plays, for anchoring a detected zoom to
    /// it. The span is clamped to the piece that owns its start: a click near a
    /// cut cannot claim footage the cut removed.
    fn source_anchor(&self, start: f64, end: f64) -> Option<(f64, f64)> {
        let boundaries = self.segment_boundaries();
        for (index, &(source_start, source_end)) in boundaries.iter().enumerate() {
            let composition_start = self.segment_start(index);
            let speed = self.segment_speed(index);
            let composition_end = composition_start + (source_end - source_start).max(0.0) / speed;
            let overlap_start = start.max(composition_start);
            let overlap_end = end.min(composition_end);
            if overlap_end - overlap_start <= 1e-9 {
                continue;
            }
            return Some((
                source_start + (overlap_start - composition_start) * speed,
                source_start + (overlap_end - composition_start) * speed,
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
        let speeds: Vec<f64> = (0..boundaries.len())
            .map(|index| self.segment_speed(index))
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
            let speed = speeds.get(index).copied().unwrap_or(1.0);
            reprojected.push(ZoomClip {
                start: composition_start + (start - source_start) / speed,
                end: composition_start + (end - source_start) / speed,
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

    /// Put the automatic zooms a fresh import starts with on the lane.
    ///
    /// The recorder's clicks seed the windows in encoded-video pixels, and a
    /// composition that has just opened starts at source time, so each
    /// placement is already the footage it frames. Seeding is a creation
    /// default rather than an edit, so nothing goes on the undo stack.
    fn seed_auto_zooms(
        &mut self,
        sidecar: &crate::recording::editor::sidecar::PointerSidecar,
        frame_size: (u32, u32),
        enabled: bool,
    ) {
        let (width, height) = frame_size;
        if !enabled || !self.zoom_clips.is_empty() || !self.has_media() || width < 2 || height < 2 {
            return;
        }
        let width = f64::from(width);
        let height = f64::from(height);
        let clicks = auto_zoom::clicks_from_sidecar(sidecar, width, height);
        let pointer = auto_zoom::pointer_from_sidecar(sidecar, width, height);
        let detections = auto_zoom::automatic_zooms(&clicks, &pointer, self.duration);
        let placements = place_auto_zooms(&self.zoom_clips, &detections, frame_size);
        self.set_frame_size(Some(frame_size));
        self.pointer_available = !sidecar.pointer.is_empty();
        let mut seeded = Vec::with_capacity(placements.len());
        for mut placement in placements {
            placement.anchor = self.source_anchor(placement.start, placement.end);
            seeded.push(placement);
        }
        self.zoom_clips = seeded;
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
        self.clamp_layout_cues();
    }

    /// Keep the layout changes inside the composition, and remember which one
    /// is selected.
    fn clamp_layout_cues(&mut self) {
        let Some(webcam) = self.webcam.clone() else {
            self.selected_layout = None;
            return;
        };
        if !webcam.has_track() {
            self.selected_layout = None;
            return;
        }
        let layouts = webcam.layouts();
        let selected = self
            .selected_layout
            .and_then(|index| layouts.cues.get(index))
            .map(|cue| (cue.at, cue.layout));
        let mut cues = layouts.cues.clone();
        normalize_layout_cues(&mut cues, self.total_duration());
        if cues != layouts.cues {
            webcam.set_layouts(super::webcam::LayoutSettings { cues, ..layouts });
        }
        let current = webcam.layouts();
        self.selected_layout = selected.and_then(|(at, layout)| {
            current
                .cues
                .iter()
                .position(|cue| (cue.at - at).abs() <= LAYOUT_CUE_EPSILON && cue.layout == layout)
        });
    }

    /// The camera layer's layout timeline, or `None` when the recording has no
    /// camera track to place.
    fn webcam_layouts(&self) -> Option<super::webcam::LayoutSettings> {
        let webcam = self.webcam.as_ref()?;
        webcam.has_track().then(|| webcam.layouts())
    }

    /// Where the recording's camera sits, or `None` when it captured none.
    fn webcam_settings(&self) -> Option<super::webcam::LayerSettings> {
        self.webcam.as_ref()?.settings()
    }

    /// Whether the camera layer draws at all. A switched-off layer occupies
    /// the recording but paints nothing, so every lane stays editable while
    /// the picture shows the screen alone.
    fn webcam_shown(&self) -> bool {
        self.webcam
            .as_ref()
            .filter(|webcam| webcam.has_track())
            .and_then(|webcam| webcam.settings())
            .is_some_and(|layer| layer.enabled)
    }

    /// Turn the camera layer on or off. Off paints the screen alone and leaves
    /// the recorded geometry and every layout change in place, so switching it
    /// back on restores the picture that was there. Returns whether it changed.
    fn set_webcam_enabled(&mut self, enabled: bool) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let Some(layer) = webcam.settings() else {
            return false;
        };
        if layer.enabled == enabled {
            return false;
        }
        self.record_layout_edit(false);
        webcam.set_settings(super::webcam::LayerSettings { enabled, ..layer });
        true
    }

    /// Edit the layout timeline. Slider sweeps pass `continuous`, so one drag
    /// is one undo step. Returns whether the timeline changed.
    fn update_layouts(
        &mut self,
        update: impl FnOnce(&mut super::webcam::LayoutSettings),
        continuous: bool,
    ) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let current = webcam.layouts();
        let mut next = current.clone();
        update(&mut next);
        let next = next.clamped();
        if next == current {
            return false;
        }
        self.record_layout_edit(continuous);
        webcam.set_layouts(next);
        self.clamp_layout_cues();
        true
    }

    /// Add a layout change at `at` and select it. The new cue keeps the layout
    /// already in force at that time, so adding one changes nothing on screen
    /// until a mode is chosen for it. A cue already starting there is selected
    /// instead of duplicated; either way the answer is its index.
    fn add_layout_cue(&mut self, at: f64) -> Option<usize> {
        let webcam = self.webcam.clone()?;
        if !webcam.has_track() {
            return None;
        }
        let layouts = webcam.layouts();
        let at = at.clamp(0.0, self.total_duration());
        if let Some(index) = layouts
            .cues
            .iter()
            .position(|cue| (cue.at - at).abs() <= LAYOUT_CUE_EPSILON)
        {
            self.selected_layout = Some(index);
            self.expanded_lane = TimelineLane::Layout;
            self.layout_selection_epoch = self.layout_selection_epoch.wrapping_add(1);
            return Some(index);
        }
        let mut next = layouts.clone();
        next.cues.push(super::webcam::LayoutCue {
            at,
            layout: layouts.layout_at(at),
            position: None,
            cutout_x: None,
        });
        let next = next.clamped();
        let index = next
            .cues
            .iter()
            .position(|cue| (cue.at - at).abs() <= LAYOUT_CUE_EPSILON);
        self.record_layout_edit(false);
        self.selected_layout = index;
        if index.is_some() {
            self.expanded_lane = TimelineLane::Layout;
            self.layout_selection_epoch = self.layout_selection_epoch.wrapping_add(1);
        }
        webcam.set_layouts(next);
        index
    }

    /// Remove one layout change. The playhead stays where it is.
    fn remove_layout_cue(&mut self, index: usize) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let layouts = webcam.layouts();
        if index >= layouts.cues.len() {
            return false;
        }
        let mut next = layouts;
        next.cues.remove(index);
        self.record_layout_edit(false);
        self.selected_layout = shift_selection(self.selected_layout, index);
        webcam.set_layouts(next);
        true
    }

    /// Give one layout change a different mode, in place. The change keeps its
    /// time, and no other cue is touched.
    fn set_layout_cue_layout(&mut self, index: usize, layout: super::webcam::CameraLayout) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let layouts = webcam.layouts();
        if layouts
            .cues
            .get(index)
            .is_none_or(|cue| cue.layout == layout)
        {
            return false;
        }
        let mut next = layouts;
        next.cues[index].layout = layout;
        self.record_layout_edit(false);
        webcam.set_layouts(next);
        true
    }

    /// Select a layout change and put the playhead on it. The playhead is a
    /// view, not content, so this is not an undo step.
    fn select_layout_cue(&mut self, index: usize) -> bool {
        let Some(layouts) = self.webcam_layouts() else {
            return false;
        };
        let Some(cue) = layouts.cues.get(index).copied() else {
            return false;
        };
        self.playing = false;
        self.selected_layout = Some(index);
        self.expanded_lane = TimelineLane::Layout;
        self.layout_selection_epoch = self.layout_selection_epoch.wrapping_add(1);
        self.playhead = cue.at.clamp(0.0, self.total_duration());
        true
    }

    /// Select a layout change where it is, the way a zoom or hide clip is
    /// selected on its lane: the playhead and playback stay as they are, so
    /// grabbing a change to move or resize it never jumps the playhead.
    fn select_layout_cue_in_place(&mut self, index: usize) -> bool {
        let Some(layouts) = self.webcam_layouts() else {
            return false;
        };
        if index >= layouts.cues.len() {
            return false;
        }
        self.selected_layout = Some(index);
        self.expanded_lane = TimelineLane::Layout;
        self.layout_selection_epoch = self.layout_selection_epoch.wrapping_add(1);
        true
    }

    /// Put the playhead inside the change at `index` when it is not already,
    /// at the middle of the span the change holds (the studied place a new
    /// change opens at), so editing a change from the panel shows it.
    /// Returns whether the playhead moved.
    fn show_layout_cue(&mut self, index: usize) -> bool {
        let Some(layouts) = self.webcam_layouts() else {
            return false;
        };
        let Some(cue) = layouts.cues.get(index).copied() else {
            return false;
        };
        let duration = self.total_duration();
        let end = layouts
            .cues
            .iter()
            .map(|other| other.at)
            .filter(|at| *at > cue.at)
            .fold(duration, f64::min);
        if self.playhead >= cue.at && self.playhead < end {
            return false;
        }
        self.playing = false;
        self.playhead = ((cue.at + end) / 2.0).clamp(0.0, duration);
        true
    }

    /// Drop the layout-change selection without moving the playhead and
    /// without taking an undo step.
    fn clear_layout_cue_selection(&mut self) -> bool {
        self.selected_layout.take().is_some()
    }

    /// Move one layout change to another composition instant.
    ///
    /// A time outside the composition is pulled back to its edge, and a time
    /// within one cue epsilon of another change is refused: one instant has
    /// one layout. The list stays ordered and whichever change was selected
    /// keeps that selection. A successful move pauses and seeks to the cue.
    /// Move a change to a typed time from the panel, which asks to go there:
    /// unlike a drag on the lane, the playhead follows it.
    fn set_layout_cue_time(&mut self, index: usize, at: f64) -> bool {
        let moved = self.move_layout_cue(index, at, false);
        if moved {
            self.playhead = at.clamp(0.0, self.total_duration());
        }
        moved
    }

    /// The earliest and latest instant the change at `index` can reach without
    /// crossing its neighbours or leaving the composition.
    fn layout_cue_limits(&self, index: usize) -> Option<(f64, f64)> {
        let layouts = self.webcam_layouts()?;
        layouts.cues.get(index)?;
        let duration = self.total_duration();
        if !duration.is_finite() || duration <= 0.0 {
            return None;
        }
        let low = index
            .checked_sub(1)
            .and_then(|previous| layouts.cues.get(previous))
            .map(|cue| cue.at + LAYOUT_CUE_EPSILON)
            .unwrap_or(0.0);
        let high = layouts
            .cues
            .get(index + 1)
            .map(|cue| cue.at - LAYOUT_CUE_EPSILON)
            .unwrap_or(duration);
        (high >= low).then_some((low.max(0.0), high.min(duration)))
    }

    /// Move one change to another instant. A drag passes `continuous`, so the
    /// reports beside each other share one undo step the way a slider sweep
    /// does. Returns whether the document changed.
    fn move_layout_cue(&mut self, index: usize, at: f64, continuous: bool) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let layouts = webcam.layouts();
        if self
            .selected_layout
            .is_some_and(|selected| selected >= layouts.cues.len())
        {
            self.selected_layout = None;
        }
        let Some(cue) = layouts.cues.get(index).copied() else {
            return false;
        };
        if !at.is_finite() {
            return false;
        }
        let limit = self.total_duration();
        if !limit.is_finite() || limit <= 0.0 {
            return false;
        }
        let at = at.clamp(0.0, limit);
        if (cue.at - at).abs() <= 1e-9 {
            return false;
        }
        if layouts.cues.iter().enumerate().any(|(other, other_cue)| {
            other != index && (other_cue.at - at).abs() <= LAYOUT_CUE_EPSILON
        }) {
            return false;
        }
        let selected = self
            .selected_layout
            .and_then(|selected| layouts.cues.get(selected))
            .map(|cue| (cue.at, cue.layout));
        let mut next = layouts;
        next.cues[index].at = at;
        let next = next.clamped();
        let selected = if self.selected_layout == Some(index) {
            Some((at, cue.layout))
        } else {
            selected
        };
        self.record_layout_edit(continuous);
        webcam.set_layouts(next);
        self.selected_layout = selected.and_then(|(at, layout)| {
            self.webcam_layouts().and_then(|layouts| {
                layouts
                    .cues
                    .iter()
                    .position(|cue| (cue.at - at).abs() <= 1e-9 && cue.layout == layout)
            })
        });
        self.playing = false;
        self.clamp_layout_cues();
        true
    }

    /// Give the selected layout change a mode, or the default layout when no
    /// valid change is selected. An explicit overlay shape also updates the
    /// camera layer's global shape, as one undo step with the layout.
    ///
    /// The shape is fitted to the recording's frame aspect so a circle stays
    /// round on screen, at the size the layer already had (its longest side
    /// against the frame's shortest), and both dimensions scale together to
    /// stay inside the layer's size bounds. The layer keeps the alignment it
    /// had, so a preset taken at an edge stays on that edge instead of
    /// drifting off it.
    fn set_webcam_editing_layout(
        &mut self,
        layout: super::webcam::CameraLayout,
        overlay_shape: Option<u8>,
    ) -> bool {
        let Some(webcam) = self.webcam.clone() else {
            return false;
        };
        if !webcam.has_track() {
            return false;
        }
        let layouts = webcam.layouts();
        let mut selection_changed = false;
        if self
            .selected_layout
            .is_some_and(|selected| selected >= layouts.cues.len())
        {
            self.selected_layout = None;
            selection_changed = true;
        }
        let selected = self.selected_layout;
        if let Some(index) = selected.filter(|_| overlay_shape.is_none()) {
            let changed = self.set_layout_cue_layout(index, layout);
            return changed || selection_changed;
        }
        let mut next = layouts.clone();
        match selected {
            Some(index) => next.cues[index].layout = layout,
            None => next.default_layout = layout,
        }
        let layouts_changed = next != layouts;

        let mut next_layer = webcam.settings();
        let mut layer_changed = false;
        if let (Some(shape), Some(layer)) = (overlay_shape, next_layer.as_mut()) {
            let previous = (layer.w, layer.h, layer.radius);
            let radius = super::webcam::radius_for_shape(shape);
            layer.set_overlay_aspect(
                1.0 / super::webcam::aspect_for_shape(shape),
                self.frame_aspect(),
            );
            layer.radius = radius;
            layer_changed = (layer.w - previous.0).abs() > f64::EPSILON
                || (layer.h - previous.1).abs() > f64::EPSILON
                || (layer.radius - previous.2).abs() > f64::EPSILON;
        }
        // Asking for a layout that shows the camera also switches the layer
        // back on: a mode chosen while it is off would otherwise look like it
        // did nothing.
        if layouts_changed || overlay_shape.is_some() {
            let shows_camera = layout != super::webcam::CameraLayout::ScreenOnly;
            if let Some(layer) = next_layer.as_mut().filter(|_| shows_camera) {
                if !layer.enabled {
                    layer.enabled = true;
                    layer_changed = true;
                }
            }
        }

        if !layouts_changed && !layer_changed {
            return selection_changed;
        }
        self.record_layout_edit(false);
        if let (true, Some(layer)) = (layer_changed, next_layer) {
            webcam.set_settings(layer);
        }
        webcam.set_layouts(next);
        self.clamp_layout_cues();
        true
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

/// Keep a layout change list usable: finite times inside the composition, in
/// order, with one cue per instant.
///
/// A cue marks the instant a layout begins, so a cue past the last frame can
/// never be reached. Trimming that footage away drops the cue rather than
/// pinning it to the final moment, where it would silently take over the end of
/// the recording; undo puts it back with the trim it belonged to. Two cues at
/// the same instant keep the later one, so a single moment has one answer.
fn normalize_layout_cues(cues: &mut Vec<super::webcam::LayoutCue>, limit: f64) {
    if limit <= 0.0 {
        cues.clear();
        return;
    }
    cues.retain(|cue| cue.at.is_finite() && cue.at >= 0.0 && cue.at <= limit + LAYOUT_CUE_EPSILON);
    cues.sort_by(|left, right| left.at.total_cmp(&right.at));
    let mut ordered: Vec<super::webcam::LayoutCue> = Vec::with_capacity(cues.len());
    for cue in cues.drain(..) {
        match ordered.last_mut() {
            Some(last) if cue.at - last.at <= LAYOUT_CUE_EPSILON => *last = cue,
            _ => ordered.push(cue),
        }
    }
    *cues = ordered;
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

/// The armed Add zoom tool, as the preview draws it: the scale a new zoom
/// would take, and whether the recording can follow the pointer, which is
/// what offers the Automatic zoom button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AddZoomTool {
    pub scale: f64,
    pub automatic: bool,
}

/// One tick of preview state: what the stage shows and what it outlines.
#[derive(Clone, Copy)]
pub(super) struct PreviewFrame {
    pub source_seconds: f64,
    /// The composition's length, for the export size estimate.
    pub composition_seconds: f64,
    /// Where the playhead stands in composition time, which is the clock the
    /// timed camera layouts run on.
    pub composition_time: f64,
    pub playing: bool,
    pub camera: super::zoom::Camera,
    pub camera_before: super::zoom::Camera,
    pub fps: f64,
    pub motion_blur: f64,
    /// Set while a Manual zoom's focus point is being placed: the clip's scale
    /// and its focus point, outlined over an unzoomed preview.
    pub placement: Option<(f64, (f64, f64))>,
    /// Set while the Add zoom tool is armed and the transport is stopped.
    pub add_zoom: Option<AddZoomTool>,
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

/// What the Webcam panel shows: the layer, its layout timeline, and where the
/// playhead and the cue selection stand. The playhead is part of the state so
/// the panel's "at the playhead" line follows a scrub or playback.
#[derive(Clone, PartialEq)]
pub(super) struct WebcamPanelState {
    /// The camera layer, absent when the recording has no camera track.
    pub layer: Option<super::webcam::LayerSettings>,
    /// The layout timeline, absent with no camera track.
    pub layouts: Option<super::webcam::LayoutSettings>,
    pub playhead: f64,
    /// The composition's length, which decides whether a change can be added.
    pub duration: f64,
    /// The recording frame's width over height, so the panel can show and
    /// label an overlay shape the way the frame will draw it. 16:9 when the
    /// frame size is unknown.
    pub frame_aspect: f64,
    /// The screen pane's share of the automatic split on the current stage,
    /// which a widened or padded stage changes.
    pub auto_split_share: f64,
    /// The layout change the cue chooser edits.
    pub selected: Option<usize>,
    /// Changes on every selection of a layout change, including a second click
    /// on the change already selected.
    pub selection_epoch: u64,
    /// Whether playback is running, so the "starts at the playhead" line only
    /// appears for a resting playhead instead of flashing past a cue.
    pub playing: bool,
    /// The layout the composition shows at the playhead.
    pub effective: super::webcam::CameraLayout,
    /// A layout change starting at the playhead, when one is there.
    pub cue_at_playhead: Option<usize>,
    /// The camera layer's last decode or person-matte failure. Part of the
    /// state so the panel notices a failure appearing or clearing.
    pub error: Option<String>,
}

/// The composition an export reads.
pub(super) struct ExportFootage {
    pub source: PathBuf,
    pub trim_start: f64,
    pub trim_end: f64,
    pub cuts: Vec<f64>,
    pub segment_speeds: Vec<f64>,
    pub freeze_tail: f64,
    pub zooms: Vec<ZoomClip>,
    pub zoom_camera: super::zoom::ZoomCameraSettings,
    pub effects: super::project::EffectsSettings,
}

type PreviewFrameSink = Rc<RefCell<Option<Rc<dyn Fn(PreviewFrame)>>>>;
type ChangeListener = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

#[derive(Clone)]
pub(super) struct Timeline {
    pub root: GtkBox,
    data: Rc<RefCell<Data>>,
    cursor: super::cursor::CursorModel,
    camera_cache: Rc<RefCell<super::zoom::ZoomCameraCache>>,
    generation: Rc<Cell<u64>>,
    zoom_scale: Scale,
    repaint: Rc<dyn Fn()>,
    frame_sink: PreviewFrameSink,
    zoom_listener: ChangeListener,
    webcam_listener: ChangeListener,
    effects_listener: ChangeListener,
    /// Told when the typing suggestions or the ranges already resolved change,
    /// so the Cursor panel can rebuild its cards without doing so on every
    /// playback repaint.
    typing_listener: ChangeListener,
    /// Told when a document opens with a different cursor look, so the Cursor
    /// panel shows the look the file carries rather than its starting one.
    cursor_listener: ChangeListener,
    /// Bumped when the cursor sidecar is loaded for a document, which is the
    /// one change the kept range and the resolved list cannot describe.
    typing_epoch: Rc<Cell<u64>>,
    /// The stage behind the video: the wallpaper, its padding and the card's
    /// corner radius. The project file keeps it with the rest of the edit list,
    /// and its only listener holds the project sync handle, so the panel never
    /// keeps the timeline or its document alive.
    background: super::background::Background,
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
        self.cursor.set_cuts(&data.cuts);
        self.cursor.set_end(data.trim_end);
        self.cursor.set_caption_settings(data.effects.captions);
        let placement = data.placement();
        let add_zoom = data.add_zoom_tool();
        let unzoomed = placement.is_some() || add_zoom.is_some();
        data.sync_webcam_to_playhead(|| self.cursor.sidecar());
        self.cursor
            .set_captions_active(data.camera_mapping().source_at(data.playhead).is_some());
        let fps = if data.fps > 0.0 { data.fps } else { 30.0 };
        let camera_settings = super::zoom::ZoomCameraSettings {
            frame_rate: fps,
            ..data.zoom_camera
        };
        let camera = if unzoomed {
            (1.0, data.frame_center())
        } else {
            let frame = data
                .frame_size
                .map(|(width, height)| (f64::from(width), f64::from(height)))
                .unwrap_or((0.0, 0.0));
            self.camera_cache.borrow_mut().evaluate(
                &data.zoom_clips,
                frame,
                &data.camera_mapping(),
                camera_settings,
                data.playhead,
                || self.cursor.sidecar(),
            )
        };
        let camera_before =
            if unzoomed || super::zoom::instant_blur_disabled(&data.zoom_clips, data.playhead) {
                camera
            } else {
                let frame = data
                    .frame_size
                    .map(|(w, h)| (f64::from(w), f64::from(h)))
                    .unwrap_or((0.0, 0.0));
                self.camera_cache.borrow_mut().evaluate(
                    &data.zoom_clips,
                    frame,
                    &data.camera_mapping(),
                    camera_settings,
                    (data.playhead - 1.0 / fps).max(0.0),
                    || self.cursor.sidecar(),
                )
            };
        PreviewFrame {
            source_seconds: data.playhead_source_time(),
            composition_seconds: data.total_duration(),
            composition_time: data.playhead,
            playing: data.playing,
            camera,
            camera_before,
            fps,
            motion_blur: data.zoom_camera.motion_blur,
            placement,
            add_zoom,
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
            segment_speeds: data.segment_speeds.clone(),
            freeze_tail: data.freeze_tail,
            zooms: data.zoom_clips.clone(),
            zoom_camera: data.zoom_camera,
            effects: data.effects.clone(),
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

    /// The click sound and keyboard caption settings the document carries.
    pub fn effects_settings(&self) -> super::project::EffectsSettings {
        self.data.borrow().effects.clone()
    }

    /// Ask to be told whenever the effects change, including on load and on
    /// undo. The first call delivers the state already on screen.
    pub fn set_effects_listener(&self, listener: Rc<dyn Fn()>) {
        *self.effects_listener.borrow_mut() = Some(listener.clone());
        listener();
    }

    /// Change the click sound or the captions. The change is one undo step,
    /// unless it leaves the settings as they were.
    pub fn update_effects(&self, update: impl FnOnce(&mut super::project::EffectsSettings)) {
        {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.update_effects(update);
        }
        (self.repaint)();
        self.publish_frame();
    }

    /// Every typing pause the key stream detects that still touches the kept
    /// footage, as `(shown_start, shown_end, detected_start)`. The detected
    /// start identifies the pause even when trimming moves where it is shown.
    fn typing_ranges(&self) -> Vec<(f64, f64, f64)> {
        let Some(sidecar) = self.cursor.sidecar() else {
            return Vec::new();
        };
        let data = self.data.borrow();
        if !data.has_media() {
            return Vec::new();
        }
        let (kept_start, kept_end) = (data.trim_start, data.trim_end);
        super::typing::detect(&sidecar.keys)
            .into_iter()
            .filter_map(|(start, end)| {
                if !start.is_finite() || !end.is_finite() {
                    return None;
                }
                let clipped_start = start.max(kept_start);
                let clipped_end = end.min(kept_end);
                (clipped_end - clipped_start > 1e-6).then_some((clipped_start, clipped_end, start))
            })
            .collect()
    }

    /// The typing pauses the recording's key stream suggests, in source
    /// seconds. Ranges the user applied or removed, and ranges that no longer
    /// touch the kept footage, are left out.
    pub fn typing_suggestions(&self) -> Vec<(f64, f64)> {
        let ranges = self.typing_ranges();
        let data = self.data.borrow();
        ranges
            .into_iter()
            .filter(|&(clipped_start, _, original_start)| {
                !data.typing_range_resolved(original_start, clipped_start)
            })
            .map(|(clipped_start, clipped_end, _)| (clipped_start, clipped_end))
            .collect()
    }

    /// Ask to be told whenever the typing suggestions or the resolved ranges
    /// change, including on load and on undo. The first call delivers the
    /// state already on screen.
    pub fn set_typing_listener(&self, listener: Rc<dyn Fn()>) {
        *self.typing_listener.borrow_mut() = Some(listener.clone());
        listener();
    }

    /// Tell the panel the document is open, after the cursor sidecar has been
    /// loaded, so the typing cards read the new recording's key stream.
    pub fn notify_typing_changed(&self) {
        self.typing_epoch
            .set(self.typing_epoch.get().wrapping_add(1));
        if let Some(listener) = self.typing_listener.borrow().as_ref() {
            listener();
        }
    }

    /// What decides which typing suggestions are on offer apart from the key
    /// stream: the document, the kept source range and the ranges already
    /// resolved. The panel compares it so a playback repaint does not run a
    /// detection pass on every tick.
    pub fn typing_signature(&self) -> (u64, u64, u64, u64, Vec<f64>) {
        let data = self.data.borrow();
        (
            self.generation.get(),
            self.typing_epoch.get(),
            data.trim_start.to_bits(),
            data.trim_end.to_bits(),
            data.typing_suggestions_resolved.clone(),
        )
    }

    /// Slow one suggested source range to `speed` as a single undo step.
    pub fn apply_typing_suggestion(&self, start: f64, end: f64, speed: f64) {
        let Some(original_start) = self
            .typing_ranges()
            .into_iter()
            .find(|&(clipped_start, _, _)| (clipped_start - start).abs() <= 1e-6)
            .map(|(_, _, original_start)| original_start)
        else {
            return;
        };
        let changed = {
            let mut data = self.data.borrow_mut();
            if data.typing_range_resolved(original_start, start) {
                return;
            }
            data.playing = false;
            let changed = data.apply_typing_ranges(&[(start, end)], speed);
            if changed {
                data.replace_typing_resolved(start, original_start);
            }
            changed
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Slow every suggestion still on offer to `speed` as a single undo step.
    pub fn apply_all_typing_suggestions(&self, speed: f64) {
        let ranges: Vec<(f64, f64, f64)> = {
            let data = self.data.borrow();
            self.typing_ranges()
                .into_iter()
                .filter(|&(clipped_start, _, original_start)| {
                    !data.typing_range_resolved(original_start, clipped_start)
                })
                .collect()
        };
        if ranges.is_empty() {
            return;
        }
        let changed = {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            let spans: Vec<(f64, f64)> = ranges
                .iter()
                .map(|&(clipped_start, clipped_end, _)| (clipped_start, clipped_end))
                .collect();
            let changed = data.apply_typing_ranges(&spans, speed);
            if changed {
                for &(clipped_start, _, original_start) in &ranges {
                    data.replace_typing_resolved(clipped_start, original_start);
                }
            }
            changed
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Stop offering the suggestion that starts at `start`, as one undo step.
    pub fn dismiss_typing_suggestion(&self, start: f64) {
        let Some(original_start) = self
            .typing_ranges()
            .into_iter()
            .find(|&(clipped_start, _, _)| (clipped_start - start).abs() <= 1e-6)
            .map(|(_, _, original_start)| original_start)
        else {
            return;
        };
        let changed = {
            let mut data = self.data.borrow_mut();
            data.playing = false;
            data.dismiss_typing_suggestion(original_start)
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// The camera layer this document carries, or `None` when the recording
    /// has no camera track to place.
    pub fn webcam_settings(&self) -> Option<super::webcam::LayerSettings> {
        let webcam = self.data.borrow().webcam.clone()?;
        webcam.settings()
    }

    /// The recording frame's width over height, for sizing the camera layer.
    pub fn webcam_frame_aspect(&self) -> f64 {
        self.data.borrow().frame_aspect()
    }

    /// Change the camera layer: the preview redraws from it and the autosave
    /// writes it, both because every mutation passes through the repaint.
    pub fn update_webcam(&self, update: impl FnOnce(&mut super::webcam::LayerSettings)) {
        let webcam = self.data.borrow().webcam.clone();
        let Some(webcam) = webcam else { return };
        let Some(mut settings) = webcam.settings() else {
            return;
        };
        update(&mut settings);
        self.data.borrow_mut().record_continuous_edit();
        webcam.set_settings(settings);
        (self.repaint)();
    }

    /// Put the layer back where the recording captured it. The layout
    /// timeline is not a placement, so a reset leaves the default layout and
    /// its changes alone.
    pub fn reset_webcam(&self) {
        let webcam = self.data.borrow().webcam.clone();
        let Some(webcam) = webcam else { return };
        if !webcam.has_track() {
            return;
        }
        let layouts = webcam.layouts();
        {
            let mut data = self.data.borrow_mut();
            data.record_continuous_edit();
        }
        webcam.reset();
        webcam.set_layouts(layouts);
        (self.repaint)();
    }

    /// Ask to be told whenever the camera layer changes, including when a
    /// document opens onto a saved one.
    pub fn set_webcam_listener(&self, listener: Rc<dyn Fn()>) {
        *self.webcam_listener.borrow_mut() = Some(listener.clone());
        listener();
    }

    /// Ask to be told when a document opens with a different cursor look,
    /// including the look the document already open carries.
    pub fn set_cursor_listener(&self, listener: Rc<dyn Fn()>) {
        *self.cursor_listener.borrow_mut() = Some(listener.clone());
        listener();
    }

    /// The camera layer's layout timeline, or `None` when this recording has
    /// no camera track to place.
    pub fn webcam_layouts(&self) -> Option<super::webcam::LayoutSettings> {
        self.data.borrow().webcam_layouts()
    }

    /// The camera layer's last decode or person-matte failure, if it has one.
    /// The panel shows it in place of its usual hint.
    pub fn webcam_error(&self) -> Option<String> {
        let webcam = self.data.borrow().webcam.clone()?;
        webcam.error()
    }

    /// Everything the Webcam panel shows.
    pub fn webcam_panel_state(&self) -> WebcamPanelState {
        let layer = self.webcam_settings();
        let error = self.webcam_error();
        let layouts = self.webcam_layouts();
        let data = self.data.borrow();
        let playhead = data.playhead;
        WebcamPanelState {
            layer,
            error,
            selected: data.selected_layout,
            selection_epoch: data.layout_selection_epoch,
            playing: data.playing,
            effective: layouts
                .as_ref()
                .map(|layouts| layouts.layout_at(playhead))
                .unwrap_or(super::webcam::CameraLayout::CameraOverlay),
            cue_at_playhead: layouts.as_ref().and_then(|layouts| {
                layouts
                    .cues
                    .iter()
                    .position(|cue| (cue.at - playhead).abs() <= LAYOUT_CUE_EPSILON)
            }),
            layouts,
            playhead,
            duration: data.total_duration(),
            frame_aspect: data.frame_aspect(),
            auto_split_share: data
                .webcam
                .as_ref()
                .map(|webcam| webcam.auto_split_share(data.frame_aspect()))
                .unwrap_or(0.5),
        }
    }

    /// Change the layout timeline as one undo step. The mode choosers use
    /// this, so each click is its own step.
    pub fn update_webcam_layouts(&self, update: impl FnOnce(&mut super::webcam::LayoutSettings)) {
        self.apply_webcam_layouts(update, false);
    }

    /// Change the layout timeline from a slider, which reports continuously:
    /// one sweep is one undo step, and it never joins a geometry sweep.
    pub fn update_webcam_layouts_continuous(
        &self,
        update: impl FnOnce(&mut super::webcam::LayoutSettings),
    ) {
        self.apply_webcam_layouts(update, true);
    }

    fn apply_webcam_layouts(
        &self,
        update: impl FnOnce(&mut super::webcam::LayoutSettings),
        continuous: bool,
    ) {
        let changed = {
            let mut data = self.data.borrow_mut();
            data.update_layouts(update, continuous)
        };
        if changed {
            (self.repaint)();
        }
    }

    /// Add a layout change at the playhead, or select the one already there.
    /// The new cue starts on the layout that is already in force, so the
    /// picture only changes once a mode is chosen for it. Adding pauses the
    /// transport, because the next thing to do is choose that mode.
    pub fn add_webcam_layout_cue(&self) {
        let selected = {
            let mut data = self.data.borrow_mut();
            let at = data.playhead;
            let index = data.add_layout_cue(at);
            if index.is_some() {
                data.playing = false;
            }
            index.is_some()
        };
        if selected {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Put the playhead on a layout change and select it for editing.
    pub fn select_webcam_layout_cue(&self, index: usize) {
        let selected = {
            let mut data = self.data.borrow_mut();
            data.select_layout_cue(index)
        };
        if selected {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Select a layout change from the timeline lane without moving the
    /// playhead.
    pub fn select_webcam_layout_cue_in_place(&self, index: usize) {
        let selected = {
            let mut data = self.data.borrow_mut();
            data.select_layout_cue_in_place(index)
        };
        if selected {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Bring the playhead into a layout change before the panel edits it, so
    /// the preview shows the change being edited.
    pub fn show_webcam_layout_cue(&self, index: usize) {
        let moved = {
            let mut data = self.data.borrow_mut();
            data.show_layout_cue(index)
        };
        if moved {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Remove one layout change. The playhead stays where it is.
    pub fn remove_webcam_layout_cue(&self, index: usize) {
        let removed = {
            let mut data = self.data.borrow_mut();
            data.remove_layout_cue(index)
        };
        if removed {
            (self.repaint)();
        }
    }

    /// Drop the layout-change selection. This moves no playhead and takes no
    /// undo step; it repaints only when there was a selection to clear.
    /// Returns whether there was one.
    pub fn clear_webcam_layout_cue_selection(&self) -> bool {
        let changed = self.data.borrow_mut().clear_layout_cue_selection();
        if changed {
            (self.repaint)();
        }
        changed
    }

    /// Give the selected layout change a mode, or the default layout when no
    /// valid change is selected. An optional overlay shape also updates the
    /// camera layer's global radius and height, fitted to the recording's
    /// frame aspect and anchored where the layer already sits.
    ///
    /// Both halves are one undo step, so undo puts the layout and the layer
    /// geometry back together. Returns whether anything changed.
    pub fn set_webcam_editing_layout(
        &self,
        layout: super::webcam::CameraLayout,
        overlay_shape: Option<u8>,
    ) -> bool {
        let changed = {
            let mut data = self.data.borrow_mut();
            data.set_webcam_editing_layout(layout, overlay_shape)
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// Turn the camera layer on or off. Off keeps the recorded geometry and
    /// every layout change, and paints the screen alone.
    pub fn set_webcam_enabled(&self, enabled: bool) {
        let changed = {
            let mut data = self.data.borrow_mut();
            data.set_webcam_enabled(enabled)
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
    }

    /// Move a layout change to another composition instant. A time outside the
    /// composition is pulled back to its edge, and a collision with another
    /// change is refused, leaving the document and the undo history untouched.
    ///
    /// A successful move pauses, seeks to the change, and repaints so the
    /// inspector and the time read-outs follow it.
    pub fn set_webcam_layout_cue_time(&self, index: usize, at: f64) -> bool {
        let changed = {
            let mut data = self.data.borrow_mut();
            data.set_layout_cue_time(index, at)
        };
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
        changed
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

    /// Escape puts the armed Add zoom tool away. Returns whether it was armed.
    pub fn disarm_add_zoom_tool(&self) -> bool {
        let changed = self.data.borrow_mut().set_add_zoom_tool(false);
        if changed {
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// A click on the preview with the Add zoom tool armed: a Manual zoom at
    /// the playhead aimed at `target`, in source pixels.
    pub fn place_zoom_at(&self, target: (f64, f64)) {
        self.add_zoom_at_playhead(Some(target));
    }

    /// The armed tool's Automatic zoom button: the playhead add, following
    /// the pointer.
    pub fn add_automatic_zoom(&self) {
        self.add_zoom_at_playhead(None);
    }

    fn add_zoom_at_playhead(&self, target: Option<(f64, f64)>) {
        {
            let mut data = self.data.borrow_mut();
            if !data.add_zoom_tool {
                return;
            }
            data.pointer_available = self
                .cursor
                .sidecar()
                .is_some_and(|sidecar| !sidecar.pointer.is_empty());
            let at = data.playhead;
            data.add_effect_at(EffectTrack::Zoom, at, target);
        }
        (self.repaint)();
        self.publish_frame();
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
        let fps = if data.fps > 0.0 { data.fps } else { 30.0 };
        Some(self.camera_cache.borrow_mut().evaluate(
            &data.zoom_clips,
            frame,
            &data.camera_mapping(),
            super::zoom::ZoomCameraSettings {
                frame_rate: fps,
                ..data.zoom_camera
            },
            data.playhead,
            || self.cursor.sidecar(),
        ))
    }

    /// Pause playback before opening export options or running the encoder.
    pub fn pause(&self) {
        self.data.borrow_mut().playing = false;
        (self.repaint)();
        self.publish_frame();
    }

    /// Note that the cursor's look changed, so the autosave writes it with
    /// the rest of the edit list instead of waiting for another edit.
    pub fn mark_cursor_changed(&self) {
        self.project.mark_changed();
    }

    /// Save this project's look as a preset the preset store keeps.
    ///
    /// The look is the stage, the cursor's current settings, the camera's
    /// styling, the captions and the click sound; the footage and the edit
    /// list stay out of it. The open document, the settings and the history
    /// are not touched, and the store is written only because this was asked
    /// for by a person pressing the button that calls it.
    pub fn save_preset(&self, name: &str) -> Result<super::presets::Preset, String> {
        let preset = self.captured_look(name)?;
        super::presets::save(&preset)?;
        Ok(preset)
    }

    /// Write this project's look over an existing preset, keeping its id and
    /// name, so the list keeps its place and the name stays the one it had.
    pub fn update_preset(
        &self,
        existing: &super::presets::Preset,
    ) -> Result<super::presets::Preset, String> {
        let preset = super::presets::replacing(existing, self.captured_look(&existing.name)?);
        super::presets::save(&preset)?;
        Ok(preset)
    }

    /// The open project's look as a preset under `name`, not yet written.
    fn captured_look(&self, name: &str) -> Result<super::presets::Preset, String> {
        let mut file = self
            .data
            .borrow()
            .project_file(&self.background)
            .ok_or_else(|| "Open a readable recording before saving a preset".to_owned())?;
        let (layer, layouts) = {
            let data = self.data.borrow();
            (
                data.webcam_settings(),
                data.webcam_layouts().unwrap_or_default(),
            )
        };
        fill_camera_from_recording(&mut file, layer, layouts);
        let cursor = self.cursor.settings();
        file.cursor = super::presets::CursorFile::from_settings(cursor);
        super::presets::from_project(&file, cursor, name)
    }

    /// Put a saved look on the open project as one undo step.
    ///
    /// The camera's styling, the captions and the click sound go through the
    /// edit list. The stage and the cursor are applied beside it, and the same
    /// step carries them, so one undo puts every part back.
    pub fn apply_preset(&self, preset: &super::presets::Preset) {
        let look = {
            let mut data = self.data.borrow_mut();
            if !data.has_media() {
                return;
            }
            let mut before = data.snapshot();
            before.look = data.live_look();
            data.playing = false;
            data.has_project = true;
            let Some(mut file) = data.project_file(&self.background) else {
                return;
            };
            fill_camera_from_recording(
                &mut file,
                data.webcam_settings(),
                data.webcam_layouts().unwrap_or_default(),
            );
            let cues = file
                .webcam
                .as_ref()
                .map(|webcam| webcam.resolved_layouts().cues);
            super::presets::apply_to(&mut file, preset);
            if let (Some(webcam), Some(cues)) = (file.webcam.as_mut(), cues) {
                let mut layouts = webcam.resolved_layouts();
                layouts.cues = cues;
                webcam.layouts = Some(layouts);
            }
            data.record_edit(before);
            data.apply_project(&file);
            Look {
                stage: (
                    file.background.selection.clone(),
                    file.background_padding,
                    file.background_radius,
                ),
                cursor: file.cursor,
            }
        };
        self.apply_look(&look);
        (self.repaint)();
        self.publish_frame();
    }

    /// Put a look's stage and cursor on the open project and tell the panels
    /// that show them. The background's listeners mark the project changed,
    /// and the cursor's change is marked here.
    fn apply_look(&self, look: &Look) {
        let (selection, padding, radius) = &look.stage;
        self.background
            .apply(selection.as_deref(), *padding, *radius);
        self.cursor.set_settings(look.cursor.settings());
        if let Some(listener) = self.cursor_listener.borrow().as_ref() {
            listener();
        }
        self.project.mark_changed();
    }

    /// Put the stage and the cursor back when the step the last undo or redo
    /// restored carries them.
    fn apply_restored_look(&self) {
        let look = self.data.borrow_mut().restored_look.take();
        if let Some(look) = look {
            self.apply_look(&look);
        }
    }

    /// Write this recording's edits now. Called when another recording replaces
    /// this one and when the window closes, both of which land before the
    /// repaint-driven autosave's next tick.
    pub fn flush_project(&self) {
        self.project.sync(
            snapshot_with_cursor(
                self.data.borrow().project_file(&self.background),
                self.cursor.settings(),
            ),
            true,
        );
    }

    /// Take the document back to how it stood before the last edit.
    ///
    /// Returns `false` when there is nothing left to take back, so the key
    /// that asked can travel on to whoever else might handle it.
    pub fn undo(&self) -> bool {
        let changed = self.data.borrow_mut().undo_edit();
        if changed {
            self.apply_restored_look();
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// Replay the edit the last undo took back.
    pub fn redo(&self) -> bool {
        let changed = self.data.borrow_mut().redo_edit();
        if changed {
            self.apply_restored_look();
            (self.repaint)();
            self.publish_frame();
        }
        changed
    }

    /// Swap the recording the timeline describes. `None` returns it to the
    /// empty placeholder scale.
    ///
    /// The outgoing recording's edits are written first, then the incoming
    /// recording's saved edit list is restored on top of its probed lengths. A
    /// recording with no project yet is seeded with the import stage and saved
    /// at once, a recording that already has one keeps the stage it stored, and
    /// an empty or unreadable document opens flat. The stage lands after the
    /// document borrow is gone, so nothing redraws against a half-swapped
    /// document.
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
        let mut imported = None;
        if let Some(source) = &source {
            duration = ffmpeg::probe_duration(source).unwrap_or(0.0);
            data.fps = ffmpeg::probe_frame_rate(source)
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .unwrap_or(30.0);
            data.path = Some(source.clone());
            data.recording_marks =
                crate::recording::editor::sidecar::RecordingMarks::for_video(source);
            data.duration = duration;
            data.trim_end = duration;
            // The camera layer follows the document: its own track, its own
            // length, and the placement the recording captured, which the
            if let Some(webcam) = data.webcam.clone() {
                webcam.load(Some(source), duration);
                webcam.set_layouts(super::webcam::LayoutSettings::default());
            }
            if duration > 0.0 {
                restored = super::project::load_project(source);
                if let Some(file) = &restored {
                    data.apply_project(file);
                } else if duration.is_finite() {
                    data.has_project = true;
                    if crate::config::load_config().rec_video_auto_zoom_on_import {
                        if let (Some(sidecar), Some(frame_size)) = (
                            crate::recording::editor::sidecar::PointerSidecar::load_next_to_video(
                                source,
                            ),
                            ffmpeg::probe_frame_size(source),
                        ) {
                            data.seed_auto_zooms(&sidecar, frame_size, true);
                        }
                    }
                    if let Some(mut file) = data.project_file(&self.background) {
                        fill_camera_from_recording(
                            &mut file,
                            data.webcam_settings(),
                            data.webcam_layouts().unwrap_or_default(),
                        );
                        super::import::seed(&mut file);
                        data.apply_project(&file);
                        imported = Some(file);
                    }
                    data.has_project = imported.is_some();
                }
            }
        } else if let Some(webcam) = data.webcam.clone() {
            // Closing the document takes the camera layer with it.
            webcam.load(None, 0.0);
            webcam.set_layouts(super::webcam::LayoutSettings::default());
        }
        data.clamp_cuts();
        data.clamp_scroll();
        data.clamp_playhead();
        let scale = data.scale;
        drop(data);

        self.project.adopt(restored.clone());
        let incoming = restored.or(imported.clone());
        self.cursor.set_settings(incoming_cursor(incoming.as_ref()));
        // The cursor model draws the captions, so it takes the opened
        // project's caption settings now rather than at the next edit.
        let captions = self.data.borrow().effects.captions;
        self.cursor.set_caption_settings(captions);
        if let Some(listener) = self.cursor_listener.borrow().as_ref() {
            listener();
        }
        match &incoming {
            Some(file) => self.background.apply(
                file.background.selection.as_deref(),
                file.background_padding,
                file.background_radius,
            ),
            None => self.background.apply(
                None,
                super::background::DEFAULT_PADDING,
                super::background::DEFAULT_RADIUS,
            ),
        }
        if let Some(file) = imported {
            self.project.sync(Some(file), true);
        }
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

/// The cursor look a document opens on: the one its file carries, or the
/// panel's starting look when the document brought none.
fn incoming_cursor(file: Option<&super::project::EditorProjectFile>) -> CursorSettings {
    file.map(|file| file.cursor.settings()).unwrap_or_default()
}

/// A snapshot with this session's cursor look in it.
///
/// The cursor model lives beside the edit list rather than inside it, so the
/// snapshot the data builds carries the starting look, and the owner of the
/// model puts the real one in before anything is written.
fn snapshot_with_cursor(
    snapshot: Option<super::project::EditorProjectFile>,
    cursor: CursorSettings,
) -> Option<super::project::EditorProjectFile> {
    snapshot.map(|file| super::project::EditorProjectFile {
        cursor: super::presets::CursorFile::from_settings(cursor),
        ..file
    })
}

/// Give a snapshot the camera layer the recording captured when the edit list
/// has none, so a saved look or a new project keeps the recorded placement
/// rather than claiming the recording carried no camera.
fn fill_camera_from_recording(
    file: &mut super::project::EditorProjectFile,
    layer: Option<super::webcam::LayerSettings>,
    layouts: super::webcam::LayoutSettings,
) {
    if file.webcam.is_some() {
        return;
    }
    if let Some(layer) = layer {
        file.webcam = Some(super::project::WebcamLayerFile::from_layer(layer, &layouts));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    window: &ApplicationWindow,
    canvas: &AspectFrame,
    scale: f64,
    prefers_dark: bool,
    cursor: super::cursor::CursorModel,
    webcam: super::webcam::WebcamModel,
    background: super::background::Background,
    source_size: Rc<Cell<Option<(u32, u32)>>>,
) -> Timeline {
    let data = Rc::new(RefCell::new(Data::default()));
    // The shared sheet sizes the board and the filmstrip/effect lanes for this
    // card, but it has no camera-lane rule. The scoped sheet below fills that
    // gap.
    install_timeline_css(scale);
    // The document needs the camera layer even when this recording has no
    // track: opening a different one has to be able to load it.
    {
        let mut data = data.borrow_mut();
        data.webcam = Some(webcam);
        data.background = Some(background.clone());
        data.cursor = Some(cursor.clone());
    }
    let camera_cache = Rc::new(RefCell::new(super::zoom::ZoomCameraCache::default()));
    let project = super::project::ProjectSync::default();
    {
        let project = project.clone();
        background.connect(Rc::new(move || project.mark_changed()));
    }

    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.add_css_class("recording-editor-timeline-compact");
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
        &t("Add zoom: click the preview to place it"),
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
    ruler.set_size_request(-1, (RULER_HEIGHT * scale).round() as i32);
    ruler.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_ruler(context, width, height, &data.borrow(), prefers_dark, scale);
        }
    });

    let video_track = DrawingArea::new();
    video_track.add_css_class("recording-editor-card-video-track");
    video_track.set_hexpand(true);
    video_track.set_size_request(-1, (VIDEO_LANE_HEIGHT * scale).round() as i32);
    video_track.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            draw_video_clip(context, width, height, &data.borrow(), prefers_dark, scale);
        }
    });

    let zoom_track_lane = DrawingArea::new();
    zoom_track_lane.add_css_class("recording-editor-card-zoom-track");
    zoom_track_lane.set_hexpand(true);
    zoom_track_lane.set_size_request(-1, (EFFECT_LANE_HEIGHT * scale).round() as i32);
    zoom_track_lane.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            let data = data.borrow();
            let lane = TimelineLane::of_effect(EffectTrack::Zoom);
            let progress = data.lane_progress(lane);
            let thin = |context: &Context| {
                let bars: Vec<_> = data
                    .effect_spans(EffectTrack::Zoom)
                    .into_iter()
                    .map(|(index, start, end)| {
                        let tone = effect_tone(
                            EffectTrack::Zoom,
                            prefers_dark,
                            data.effect_hidden(EffectTrack::Zoom, index),
                        );
                        (start, end, tone.fill)
                    })
                    .collect();
                draw_thin_lane(
                    context,
                    width,
                    height,
                    &data,
                    &t("Zoom"),
                    &bars,
                    prefers_dark,
                    scale,
                );
            };
            let full = |context: &Context| {
                draw_effect_clips(
                    context,
                    width,
                    height,
                    &data,
                    EffectTrack::Zoom,
                    prefers_dark,
                    scale,
                );
            };
            if progress >= 0.999 {
                full(context);
            } else if progress <= 0.001 {
                thin(context);
            } else {
                context.push_group();
                thin(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(1.0 - progress);
                context.push_group();
                full(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(progress);
            }
        }
    });

    let hide_track = DrawingArea::new();
    hide_track.add_css_class("recording-editor-card-hide-track");
    hide_track.set_hexpand(true);
    hide_track.set_size_request(-1, (THIN_LANE_HEIGHT * scale).round() as i32);
    hide_track.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            let data = data.borrow();
            let lane = TimelineLane::of_effect(EffectTrack::Hide);
            let progress = data.lane_progress(lane);
            let thin = |context: &Context| {
                let bars: Vec<_> = data
                    .effect_spans(EffectTrack::Hide)
                    .into_iter()
                    .map(|(index, start, end)| {
                        let tone = effect_tone(
                            EffectTrack::Hide,
                            prefers_dark,
                            data.effect_hidden(EffectTrack::Hide, index),
                        );
                        (start, end, tone.fill)
                    })
                    .collect();
                draw_thin_lane(
                    context,
                    width,
                    height,
                    &data,
                    &t("Hide"),
                    &bars,
                    prefers_dark,
                    scale,
                );
            };
            let full = |context: &Context| {
                draw_effect_clips(
                    context,
                    width,
                    height,
                    &data,
                    EffectTrack::Hide,
                    prefers_dark,
                    scale,
                );
            };
            if progress >= 0.999 {
                full(context);
            } else if progress <= 0.001 {
                thin(context);
            } else {
                context.push_group();
                thin(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(1.0 - progress);
                context.push_group();
                full(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(progress);
            }
        }
    });

    let layout_track = DrawingArea::new();
    layout_track.add_css_class("recording-editor-card-layout-track");
    layout_track.set_hexpand(true);
    layout_track.set_size_request(-1, (THIN_LANE_HEIGHT * scale).round() as i32);
    layout_track.set_visible(false);
    layout_track.set_tooltip_text(Some(&t("Camera layout changes")));
    layout_track.set_draw_func({
        let data = data.clone();
        move |_, context, width, height| {
            let data = data.borrow();
            let progress = data.lane_progress(TimelineLane::Layout);
            let thin = |context: &Context| {
                let bar = if prefers_dark {
                    (0.10, 0.155, 0.15, 1.0)
                } else {
                    (0.87, 0.915, 0.905, 1.0)
                };
                let tick = if prefers_dark {
                    (0.45, 0.80, 0.70, 0.95)
                } else {
                    (0.15, 0.50, 0.42, 0.95)
                };
                let mut bars = vec![(0.0, data.total_duration(), bar)];
                if let Some(layouts) = data.webcam_layouts() {
                    for cue in &layouts.cues {
                        bars.push((cue.at, cue.at, tick));
                    }
                }
                draw_thin_lane(
                    context,
                    width,
                    height,
                    &data,
                    &t("Layout"),
                    &bars,
                    prefers_dark,
                    scale,
                );
            };
            let full = |context: &Context| {
                draw_layout_lane(context, width, height, &data, prefers_dark, scale);
            };
            if progress >= 0.999 {
                full(context);
            } else if progress <= 0.001 {
                thin(context);
            } else {
                context.push_group();
                thin(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(1.0 - progress);
                context.push_group();
                full(context);
                let _ = context.pop_group_to_source();
                let _ = context.paint_with_alpha(progress);
            }
        }
    });

    let lanes = GtkBox::new(Orientation::Vertical, (LANE_GAP * scale).round() as i32);
    lanes.add_css_class("recording-editor-card-tracks");
    lanes.set_hexpand(true);
    lanes.append(&video_track);
    lanes.append(&zoom_track_lane);
    lanes.append(&hide_track);
    lanes.append(&layout_track);

    lanes.add_tick_callback({
        let data = data.clone();
        let zoom = zoom_track_lane.clone();
        let hide = hide_track.clone();
        let layout = layout_track.clone();
        let last = Rc::new(Cell::new(None::<i64>));
        // The heights on screen. Every tick derives them from the progress
        // and which lanes show, and applies them only when they change, so a
        // lane appearing or the first switch never jumps the total a pixel.
        let applied = Rc::new(Cell::new([0i32; 3]));
        move |_, clock| {
            let now = clock.frame_time();
            let dt = last
                .replace(Some(now))
                .map(|previous| (now - previous) as f64 / 1_000_000.0)
                .unwrap_or(0.0)
                .min(0.1);
            let (moving, progress) = {
                let mut data = data.borrow_mut();
                let before = data.lane_progress;
                let moving = data.step_lane_progress(dt);
                (moving || before != data.lane_progress, data.lane_progress)
            };
            let heights = lane_heights(
                progress,
                [zoom.is_visible(), hide.is_visible(), layout.is_visible()],
                scale,
            );
            if heights != applied.get() {
                applied.set(heights);
                zoom.set_size_request(-1, heights[0]);
                hide.set_size_request(-1, heights[1]);
                layout.set_size_request(-1, heights[2]);
            }
            if moving {
                zoom.queue_draw();
                hide.queue_draw();
                layout.queue_draw();
            }
            gtk4::glib::ControlFlow::Continue
        }
    });

    // The frosted extend area is one layer over the lanes, so it spans the
    // video, zoom, hide and camera-layout rows (and the gaps between them)
    // instead of stopping at the video lane's seam. It sits above the lanes
    // but lets input through, because the lanes own the pointer.
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

    let content = GtkBox::new(Orientation::Vertical, (LANE_GAP * scale).round() as i32);
    content.set_hexpand(true);
    content.append(&ruler);
    content.append(&lanes_overlay);

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

    // The playhead covers the lane column, so its one line crosses the ruler
    // and every visible row at the same coordinate the lanes use. The board
    // keeps the padding below it.
    let content_overlay = Overlay::new();
    content_overlay.set_hexpand(true);
    content_overlay.set_child(Some(&content));
    content_overlay.add_overlay(&playhead);

    let tracks = GtkBox::new(Orientation::Horizontal, 0);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&content_overlay);

    let board = Overlay::new();
    board.add_css_class("recording-editor-card-board");
    board.set_hexpand(true);
    board.set_child(Some(&tracks));

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
    let lane_state: Rc<Cell<Option<LaneState>>> = Rc::new(Cell::new(None));
    // The Zoom panel follows every change to the zoom track or its selection.
    // Repainting is the one place every mutation already passes through.
    let zoom_listener: ChangeListener = Rc::new(RefCell::new(None));
    let webcam_listener: ChangeListener = Rc::new(RefCell::new(None));
    let effects_listener: ChangeListener = Rc::new(RefCell::new(None));
    let typing_listener: ChangeListener = Rc::new(RefCell::new(None));
    let cursor_listener: ChangeListener = Rc::new(RefCell::new(None));
    let repaint: Rc<dyn Fn()> = {
        let data = data.clone();
        let ruler = ruler.clone();
        let video_track = video_track.clone();
        let zoom_track_lane = zoom_track_lane.clone();
        let hide_track = hide_track.clone();
        let layout_track = layout_track.clone();
        let playhead = playhead.clone();
        let extend = extend.clone();
        let playhead_clock = playhead_clock.clone();
        let duration_clock = duration_clock.clone();
        let adjustment = adjustment.clone();
        let scroll_syncing = scroll_syncing.clone();
        let lane_state = lane_state.clone();
        let zoom_listener = zoom_listener.clone();
        let webcam_listener = webcam_listener.clone();
        let effects_listener = effects_listener.clone();
        let typing_listener = typing_listener.clone();
        let project = project.clone();
        Rc::new(move || {
            let state = {
                let data = data.borrow();
                playhead_clock.set_text(&format_clock(data.playhead));
                duration_clock.set_text(&format_clock(data.total_duration()));
                scroll_syncing.set(true);
                adjustment.set_upper(data.canvas_seconds().max(data.visible_span()).max(0.001));
                adjustment.set_page_size(data.visible_span().max(0.001));
                adjustment.set_value(data.scroll);
                scroll_syncing.set(false);
                data.lane_state()
            };
            ruler.queue_draw();
            video_track.queue_draw();
            zoom_track_lane.queue_draw();
            hide_track.queue_draw();
            layout_track.queue_draw();
            extend.queue_draw();
            playhead.queue_draw();
            // The camera row follows the track; the two effect rows never
            // move. A playback tick leaves the state alone and with it the
            // lane heights.
            if lane_state.get() != Some(state) {
                lane_state.set(Some(state));
                apply_lane_state(state, &layout_track, &zoom_track_lane, &hide_track);
            }
            if let Some(listener) = zoom_listener.borrow().as_ref() {
                listener();
            }
            if let Some(listener) = webcam_listener.borrow().as_ref() {
                listener();
            }
            if let Some(listener) = effects_listener.borrow().as_ref() {
                listener();
            }
            if let Some(listener) = typing_listener.borrow().as_ref() {
                listener();
            }
            // Every mutation repaints, so this is the one place that can tell
            // the autosave a change may be waiting to be written.
            project.mark_changed();
        })
    };

    // A widened or padded stage changes the automatic split's share, which
    // the Webcam panel shows; the stage is settled by the preview, not here.
    let webcam = data.borrow().webcam.clone();
    if let Some(webcam) = webcam {
        webcam.set_stage_listener(repaint.clone());
    }

    // Autosave. The repaint above only notes that something may have changed;
    // this tick decides whether it is worth a write by comparing against the
    // file on disk, so a hover, a scroll or a playhead tick never rewrites it.
    {
        let data = data.clone();
        let background = background.clone();
        let project = project.clone();
        let cursor = cursor.clone();
        gtk4::glib::timeout_add_local(Duration::from_secs(2), move || {
            if project.take_changed() {
                let snapshot = data.borrow().project_file(&background);
                project.sync(snapshot_with_cursor(snapshot, cursor.settings()), false);
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
        &content,
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
        cursor.clone(),
        source_size.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
        scale,
    );
    bind_effect_lane(
        &hide_track,
        EffectTrack::Hide,
        data.clone(),
        cursor.clone(),
        source_size.clone(),
        repaint.clone(),
        sync_play_icon.clone(),
        scale,
    );

    hide.connect_clicked({
        let data = data.clone();
        let cursor = cursor.clone();
        let source_size = source_size.clone();
        let repaint = repaint.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                data.set_frame_size(source_size.get());
                data.pointer_available = cursor
                    .sidecar()
                    .is_some_and(|sidecar| !sidecar.pointer.is_empty());
                let at = data.playhead;
                data.add_effect(EffectTrack::Hide, at);
            }
            repaint();
        }
    });

    // The Zoom button arms the Add zoom tool rather than adding at once, as in
    // the studied build: the next click on the preview places the zoom.
    zoom.connect_clicked({
        let data = data.clone();
        let cursor = cursor.clone();
        let repaint = repaint.clone();
        move |_| {
            {
                let mut data = data.borrow_mut();
                data.pointer_available = cursor
                    .sidecar()
                    .is_some_and(|sidecar| !sidecar.pointer.is_empty());
                let armed = !data.add_zoom_tool;
                data.set_add_zoom_tool(armed);
            }
            repaint();
        }
    });

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
    let frame_sink: PreviewFrameSink = Rc::new(RefCell::new(None));
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
        let zoom_button = zoom.clone();
        let last_armed = Rc::new(Cell::new(false));
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
                data.sync_webcam_to_playhead(|| cursor.sidecar());
                // The tick builds its own frame, so it keeps the captions in
                // step the way `current_frame` does: playback crosses cuts and
                // the freeze tail without any edit to republish the frame.
                cursor.set_caption_settings(data.effects.captions);
                cursor.set_captions_active(
                    data.camera_mapping().source_at(composition_time).is_some(),
                );
                let placement = data.placement();
                let add_zoom = data.add_zoom_tool();
                let unzoomed = placement.is_some() || add_zoom.is_some();
                let fps = if data.fps > 0.0 { data.fps } else { 30.0 };
                let camera_settings = super::zoom::ZoomCameraSettings {
                    frame_rate: fps,
                    ..data.zoom_camera
                };
                let camera = if unzoomed {
                    (1.0, data.frame_center())
                } else {
                    let frame = data
                        .frame_size
                        .map(|(width, height)| (f64::from(width), f64::from(height)))
                        .unwrap_or((0.0, 0.0));
                    camera_cache.borrow_mut().evaluate(
                        &data.zoom_clips,
                        frame,
                        &data.camera_mapping(),
                        camera_settings,
                        composition_time,
                        || cursor.sidecar(),
                    )
                };
                let camera_before = if unzoomed
                    || super::zoom::instant_blur_disabled(&data.zoom_clips, composition_time)
                {
                    camera
                } else {
                    let frame = data
                        .frame_size
                        .map(|(w, h)| (f64::from(w), f64::from(h)))
                        .unwrap_or((0.0, 0.0));
                    camera_cache.borrow_mut().evaluate(
                        &data.zoom_clips,
                        frame,
                        &data.camera_mapping(),
                        camera_settings,
                        (composition_time - 1.0 / fps).max(0.0),
                        || cursor.sidecar(),
                    )
                };
                (
                    data.playing,
                    was_playing || data.playing,
                    PreviewFrame {
                        source_seconds: data.playhead_source_time(),
                        composition_seconds: data.total_duration(),
                        composition_time,
                        playing: data.playing,
                        camera,
                        camera_before,
                        fps,
                        motion_blur: data.zoom_camera.motion_blur,
                        placement,
                        add_zoom,
                    },
                )
            };
            if last_playing.get() != playing {
                set_play_icon(&play_icon, playing);
                last_playing.set(playing);
            }
            let armed = data.borrow().add_zoom_tool;
            if last_armed.get() != armed {
                if armed {
                    zoom_button.add_css_class("video-editor-tool-armed");
                } else {
                    zoom_button.remove_css_class("video-editor-tool-armed");
                }
                last_armed.set(armed);
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

    let timeline = Timeline {
        root: dock,
        data: data.clone(),
        cursor,
        camera_cache,
        generation: Rc::new(Cell::new(0)),
        zoom_scale,
        repaint,
        frame_sink,
        zoom_listener,
        webcam_listener,
        effects_listener,
        typing_listener,
        cursor_listener,
        typing_epoch: Rc::new(Cell::new(0)),
        background,
        project,
    };
    bind_layout_lane(&layout_track, data, timeline.clone(), sync_play_icon, scale);
    timeline
}

/// A lane's height for how expanded it is drawn, in device pixels. Linear
/// in `progress`: the lanes' progress always sums to one expanded lane, so
/// linear heights keep the timeline's total height fixed while they trade.
fn lane_height_exact(progress: f64, scale: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    (THIN_LANE_HEIGHT + (EFFECT_LANE_HEIGHT - THIN_LANE_HEIGHT) * progress) * scale
}

/// [`lane_height_exact`] rounded to whole pixels on its own.
fn lane_height(progress: f64, scale: f64) -> i32 {
    lane_height_exact(progress, scale).round() as i32
}

/// Whole-pixel heights for the visible lanes whose sum never changes while
/// they trade height: each lane takes the difference between the rounded
/// running totals, so a pixel one lane loses to rounding the next one gains.
/// Rounding each lane alone made the total wobble by a pixel a frame, and
/// the preview above resized with it.
fn lane_heights(progress: [f64; 3], visible: [bool; 3], scale: f64) -> [i32; 3] {
    let mut heights = [0; 3];
    let shown: Vec<usize> = (0..3).filter(|&index| visible[index]).collect();
    // The resting total: one lane full and the rest thin, in whole pixels.
    // Pinning the sum to it keeps a total that lands on half a pixel from
    // flipping between frames on floating-point noise.
    let resting = (lane_height_exact(1.0, scale)
        + lane_height_exact(0.0, scale) * shown.len().saturating_sub(1) as f64)
        .round() as i32;
    let mut total = 0.0;
    let mut placed = 0;
    for (position, &index) in shown.iter().enumerate() {
        if position + 1 == shown.len() {
            heights[index] = resting - placed;
            break;
        }
        total += lane_height_exact(progress[index], scale);
        let rounded = total.round() as i32;
        heights[index] = rounded - placed;
        placed = rounded;
    }
    for index in 0..3 {
        if !visible[index] {
            heights[index] = lane_height(progress[index], scale);
        }
    }
    heights
}

/// Apply a lane snapshot to the drawing areas. Only called when the snapshot
/// changes, so playback never re-measures the card.
fn apply_lane_state(
    state: LaneState,
    layout_track: &DrawingArea,
    zoom_track: &DrawingArea,
    hide_track: &DrawingArea,
) {
    layout_track.set_visible(state.camera);
    // Both effect lanes are permanent drop targets, so their rows never
    // collapse, even while empty.
    zoom_track.set_visible(state.zoom);
    hide_track.set_visible(state.hide);
}

/// The one stylesheet this card adds on top of the shared editor sheet.
///
/// It is scoped to the timeline dock and uses only properties this theme
/// parser accepts: `min-height`, `padding`, `color` and `font-size`. The
/// shared sheet's floors are sized for a taller timeline; the Windows editor
/// keeps its timeline compact so the preview gets the height, so every lane
/// floor and the board's own floor are restated here at the compact sizes.
/// The lanes rest thin and one of them expands, so their floors never stop a
/// lane from shrinking.
fn timeline_css(scale: f64) -> String {
    let px = |value: f64| format!("{}px", (value * scale).round() as i32);
    // A lane's floor rounds down: the animation hands each lane whole pixels
    // that sum to a fixed total, so a lane may be asked for the pixel below
    // its exact height. A floor rounded up would refuse that pixel, grow
    // the timeline by one and make the preview above it shake.
    let floor = |value: f64| format!("{}px", (value * scale).floor() as i32);
    let board = RULER_HEIGHT
        + LANE_GAP
        + VIDEO_LANE_HEIGHT
        + EFFECT_LANE_HEIGHT
        + THIN_LANE_HEIGHT
        + 2.0 * LANE_GAP;
    format!(
        "
.recording-editor-timeline-compact .recording-editor-card-board {{
    min-height: {board};
    padding: 0 0 {board_padding} 0;
}}
.recording-editor-timeline-compact .recording-editor-card-ruler {{
    min-height: {ruler};
}}
.recording-editor-timeline-compact .recording-editor-card-video-track {{
    min-height: {video};
}}
.recording-editor-timeline-compact .recording-editor-card-zoom-track,
.recording-editor-timeline-compact .recording-editor-card-hide-track {{
    min-height: {effect};
}}
.recording-editor-timeline-compact .recording-editor-card-layout-track {{
    min-height: {camera};
}}
",
        board = px(board),
        board_padding = px(4.0),
        ruler = px(RULER_HEIGHT),
        video = px(VIDEO_LANE_HEIGHT),
        effect = floor(THIN_LANE_HEIGHT),
        camera = floor(THIN_LANE_HEIGHT),
    )
}

fn install_timeline_css(scale: f64) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&timeline_css(scale));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
    );
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
                scale,
                data.clone(),
                repaint.clone(),
                None,
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
                        let speed = data.segment_speed(0);
                        data.set_trim_start(anchor.get() + delta * speed);
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

/// The red read-out line belongs to the content column, not to one lane:
/// motion over the ruler, a gap, a lane or a pill all report the same time, so
/// the line stays put as the pointer crosses between them instead of blinking
/// out. The column starts at the ruler's origin, so it never reports a time.
fn bind_board_hover(
    content: &GtkBox,
    data: Rc<RefCell<Data>>,
    playhead: DrawingArea,
    extend: DrawingArea,
    band_hover: Rc<Cell<bool>>,
    scale: f64,
) {
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let data = data.clone();
        let content = content.clone();
        let playhead = playhead.clone();
        let extend = extend.clone();
        let band_hover = band_hover.clone();
        move |_, x, _| {
            let width = content.allocated_width().max(1) as f64;
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
    content.add_controller(motion);
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

#[allow(clippy::too_many_arguments)]
fn bind_effect_lane(
    lane: &DrawingArea,
    track: EffectTrack,
    data: Rc<RefCell<Data>>,
    cursor: super::cursor::CursorModel,
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
        let cursor = cursor.clone();
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
                let lane = TimelineLane::of_effect(track);
                if !data.lane_expanded(lane) {
                    data.expand_lane(lane);
                    if let Some(index) = effect_at(&data, track, width, x, scale) {
                        data.select_effect(track, Some(index));
                    }
                    drop(data);
                    sync_play_icon();
                    repaint();
                    return;
                }
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
                    if data.paste_effect_at(track, at).is_none() {
                        data.pointer_available = cursor
                            .sidecar()
                            .is_some_and(|sidecar| !sidecar.pointer.is_empty());
                        if data.add_effect(track, at).is_none() {
                            data.select_effect(track, None);
                        }
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
            if !data.borrow().lane_expanded(TimelineLane::of_effect(track)) {
                return;
            }
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
            show_clip_menu(
                &lane,
                target,
                x,
                y,
                scale,
                data.clone(),
                repaint.clone(),
                None,
            );
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
            if !data.borrow().lane_expanded(TimelineLane::of_effect(track)) {
                drag_kind.set(EffectDrag::None);
                return;
            }
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
            if !data.borrow().lane_expanded(TimelineLane::of_effect(track)) {
                data.borrow_mut().set_hover_time(track, None);
                lane.set_cursor_from_name(Some("pointer"));
                lane.queue_draw();
                return;
            }
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

/// Make the camera-layout lane's chips select their change through the same
/// path the panel uses: pause, seek, select, and reveal the inspector on the
/// new selection epoch.
fn bind_layout_lane(
    lane: &DrawingArea,
    data: Rc<RefCell<Data>>,
    timeline: Timeline,
    sync_play_icon: Rc<dyn Fn()>,
    scale: f64,
) {
    // A press only selects. The row is continuous, so there is no empty space
    // to place a change in: a press on the resting band goes back to editing
    // the default, and a change is added from the panel or the menu.
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let lane = lane.clone();
        let data = data.clone();
        let timeline = timeline.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |_, _, x, _| {
            if !data.borrow().lane_expanded(TimelineLane::Layout) {
                data.borrow_mut().expand_lane(TimelineLane::Layout);
                (timeline.repaint)();
            }
            let width = lane.allocated_width().max(1) as f64;
            let hit = {
                let data = data.borrow();
                layout_cue_at(&data, width, x, scale)
            };
            match hit {
                Some(index) => timeline.select_webcam_layout_cue_in_place(index),
                None => {
                    timeline.clear_webcam_layout_cue_selection();
                }
            }
            sync_play_icon();
        }
    });
    lane.add_controller(click);

    // A drag carries the change under the pointer through time. Grabbing a
    // chip's trailing edge widens or narrows it by moving the change after it,
    // which is what resizing a pill does in the effect lanes.
    let grab = Rc::new(Cell::new(LayoutGrab::None));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let data = data.clone();
        let grab = grab.clone();
        let timeline = timeline.clone();
        let sync_play_icon = sync_play_icon.clone();
        move |gesture, x, _| {
            if !data.borrow().lane_expanded(TimelineLane::Layout) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let grabbed = {
                let data = data.borrow();
                let Some(index) = layout_cue_at(&data, width, x, scale) else {
                    return;
                };
                let Some((start, end)) = layout_chip_bounds(&data, width, index, scale) else {
                    return;
                };
                let edge = (LAYOUT_EDGE_GRAB * scale).min((end - start) / 2.0);
                let can_resize = data
                    .webcam_layouts()
                    .is_some_and(|layouts| index + 1 < layouts.cues.len());
                if can_resize && (x - end).abs() <= edge {
                    LayoutGrab::Resize(index)
                } else {
                    LayoutGrab::Move(index)
                }
            };
            grab.set(grabbed);
            match grabbed {
                LayoutGrab::Move(index) | LayoutGrab::Resize(index) => {
                    timeline.select_webcam_layout_cue_in_place(index);
                    data.borrow_mut().playing = false;
                }
                LayoutGrab::None => {}
            }
            sync_play_icon();
        }
    });
    drag.connect_drag_update({
        let data = data.clone();
        let grab = grab.clone();
        let lane = lane.clone();
        let timeline = timeline.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = lane.allocated_width().max(1) as f64;
            {
                let mut data = data.borrow_mut();
                let time = data.x_to_time(start_x + offset_x, width);
                let time = snap_time_to_playhead(&data, width, time, scale);
                let index = match grab.get() {
                    LayoutGrab::Move(index) => index,
                    LayoutGrab::Resize(index) => index + 1,
                    LayoutGrab::None => return,
                };
                let Some((low, high)) = data.layout_cue_limits(index) else {
                    return;
                };
                data.move_layout_cue(index, time.clamp(low, high), true);
            }
            (timeline.repaint)();
            timeline.publish_frame();
        }
    });
    drag.connect_drag_end({
        let data = data.clone();
        let grab = grab.clone();
        let timeline = timeline.clone();
        move |_, _, _| {
            grab.set(LayoutGrab::None);
            data.borrow_mut().playing = false;
            (timeline.repaint)();
        }
    });
    lane.add_controller(drag);

    // Right-click acts on the chip or the default band under the pointer, the
    // same way the Zoom and Hide lanes act on their pills. A miss opens
    // nothing rather than acting on whatever happened to be selected.
    let menu = GestureClick::new();
    menu.set_button(3);
    menu.connect_pressed({
        let lane = lane.clone();
        let data = data.clone();
        let timeline = timeline.clone();
        let sync_play_icon = sync_play_icon.clone();
        let repaint = timeline.repaint.clone();
        move |_, _, x, y| {
            let width = lane.allocated_width().max(1) as f64;
            let target = {
                let data = data.borrow();
                match layout_cue_at(&data, width, x, scale) {
                    Some(index) => MenuTarget::Layout(Some(index)),
                    None => {
                        if !layout_default_at(&data, width, x) {
                            return;
                        }
                        MenuTarget::Layout(None)
                    }
                }
            };
            match target {
                MenuTarget::Layout(Some(index)) => {
                    timeline.select_webcam_layout_cue_in_place(index)
                }
                _ => {
                    timeline.clear_webcam_layout_cue_selection();
                }
            }
            sync_play_icon();
            show_clip_menu(
                &lane,
                target,
                x,
                y,
                scale,
                data.clone(),
                repaint.clone(),
                Some(&timeline),
            );
        }
    });
    lane.add_controller(menu);

    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let lane = lane.clone();
        let data = data.clone();
        move |_, x, _| {
            let width = lane.allocated_width().max(1) as f64;
            let (cursor, tooltip) = {
                let data = data.borrow();
                let hit = layout_cue_at(&data, width, x, scale);
                let default = hit.is_none() && layout_default_at(&data, width, x);
                let cursor = if hit.is_some() || default {
                    "pointer"
                } else {
                    "default"
                };
                let tooltip = layout_lane_tooltip(&data, width, x, scale);
                (cursor, tooltip)
            };
            lane.set_cursor_from_name(Some(cursor));
            let current = lane.tooltip_text().map(|text| text.to_string());
            if current != tooltip {
                lane.set_tooltip_text(tooltip.as_deref());
            }
        }
    });
    motion.connect_leave({
        let lane = lane.clone();
        move |_| {
            lane.set_cursor_from_name(None);
            lane.set_tooltip_text(None);
        }
    });
    lane.add_controller(motion);
}

/// What a drag on the camera lane picked up: a change to carry through time,
/// or a chip's trailing edge, which resizes it by moving the change after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutGrab {
    None,
    Move(usize),
    Resize(usize),
}

/// What a right-click landed on. The menu is built per target, the way the
/// source editor's single clip menu covers all three tracks.
#[derive(Clone, Copy)]
enum MenuTarget {
    Video,
    Zoom(usize),
    Hide(usize),
    /// The camera lane: one of its layout changes, or the row's default.
    Layout(Option<usize>),
}

struct MenuItem {
    icon: &'static str,
    label: String,
    action: Rc<dyn Fn()>,
}

/// Pop up the clip menu at `(x, y)` in `anchor`'s space. It opens upward: the
/// tracks sit at the bottom of the editor, so a downward menu would fall off
/// the window. The surface and rows are the shared select menu's, so this menu
/// reads like the Motion timeline's.
#[allow(clippy::too_many_arguments)]
fn show_clip_menu(
    anchor: &impl IsA<gtk4::Widget>,
    target: MenuTarget,
    x: f64,
    y: f64,
    scale: f64,
    data: Rc<RefCell<Data>>,
    repaint: Rc<dyn Fn()>,
    timeline: Option<&Timeline>,
) {
    // The menu is drawn at its authored size like the Motion timeline's, so a
    // high-DPI display does not inflate rows the shared sheet already sizes.
    let scale = scale.min(1.0);
    let popover = Popover::new();
    popover.add_css_class("history-action-popover");
    popover.add_css_class("settings-select-popover");
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);

    let card = GtkBox::new(Orientation::Vertical, (2.0 * scale).round() as i32);
    card.add_css_class("settings-select-menu");
    card.set_size_request((160.0 * scale).round() as i32, -1);

    let mut items = Vec::new();
    let mut offer_paste = false;
    match target {
        MenuTarget::Video => items.push(MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete"),
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
        MenuTarget::Layout(cue) => {
            items = layout_menu_items(cue, timeline, &data);
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

    for item in items {
        card.append(&clip_menu_row(item, &popover, scale));
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

/// The camera lane's rows: the five layouts a change — or the row's default —
/// can take, the layer's own switch, and the change's removal.
///
/// The layouts are listed in the lane's own order and named with the lane's own
/// words, so a menu row and the chip it produces read the same. Every row goes
/// through the timeline API, which is what keeps the menu, the panel and the
/// preview in step.
fn layout_menu_items(
    cue: Option<usize>,
    timeline: Option<&Timeline>,
    data: &Rc<RefCell<Data>>,
) -> Vec<MenuItem> {
    let Some(timeline) = timeline else {
        return Vec::new();
    };
    let shown = data.borrow().webcam_shown();
    let mut items: Vec<MenuItem> = LAYOUT_MENU_ORDER
        .iter()
        .map(|&layout| MenuItem {
            icon: layout_menu_icon(layout),
            label: t(layout_lane_label(layout)),
            action: {
                let timeline = timeline.clone();
                Rc::new(move || {
                    match cue {
                        Some(index) => timeline.select_webcam_layout_cue_in_place(index),
                        None => {
                            timeline.clear_webcam_layout_cue_selection();
                        }
                    }
                    timeline.set_webcam_editing_layout(layout, None);
                })
            },
        })
        .collect();

    items.push(MenuItem {
        icon: "list-add-symbolic",
        label: t("Add change at playhead"),
        action: {
            let timeline = timeline.clone();
            Rc::new(move || timeline.add_webcam_layout_cue())
        },
    });

    items.push(MenuItem {
        icon: if shown {
            icon_names::shipped::EYE_OFF_REGULAR
        } else {
            icon_names::shipped::EYE_REGULAR
        },
        label: if shown {
            t("Turn camera off")
        } else {
            t("Turn camera on")
        },
        action: {
            let timeline = timeline.clone();
            Rc::new(move || timeline.set_webcam_enabled(!shown))
        },
    });

    if let Some(index) = cue {
        items.push(MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete change"),
            action: {
                let timeline = timeline.clone();
                Rc::new(move || timeline.remove_webcam_layout_cue(index))
            },
        });
    }
    items
}

/// The lane's layouts in the order its menu offers them: the bubble first,
/// then camera-only, then the two ways of sharing the frame, and the screen
/// alone last before the cutout.
const LAYOUT_MENU_ORDER: [super::webcam::CameraLayout; 5] = [
    super::webcam::CameraLayout::CameraOverlay,
    super::webcam::CameraLayout::FullscreenCamera,
    super::webcam::CameraLayout::SplitScreen,
    super::webcam::CameraLayout::ScreenOnly,
    super::webcam::CameraLayout::CameraCutout,
];

/// A glyph per layout, so the menu's rows differ at a glance the way their
/// pictures do.
fn layout_menu_icon(layout: super::webcam::CameraLayout) -> &'static str {
    match layout {
        super::webcam::CameraLayout::CameraOverlay => {
            icon_names::custom::OVERLAPPING_WINDOWS_SYMBOLIC
        }
        super::webcam::CameraLayout::FullscreenCamera => icon_names::custom::WEBCAM_SYMBOLIC,
        super::webcam::CameraLayout::SplitScreen => {
            icon_names::custom::SQUARE_SPLIT_HORIZONTAL_SYMBOLIC
        }
        super::webcam::CameraLayout::ScreenOnly => icon_names::custom::RECORD_SCREEN_SYMBOLIC,
        super::webcam::CameraLayout::CameraCutout => icon_names::custom::CROP_SYMBOLIC,
    }
}

fn clip_menu_row(item: MenuItem, popover: &Popover, scale: f64) -> Button {
    let button = Button::new();
    button.add_css_class("history-action-btn");
    button.set_has_frame(false);
    button.set_focus_on_click(false);
    button.set_size_request(-1, (32.0 * scale).round() as i32);

    let row = GtkBox::new(Orientation::Horizontal, (10.0 * scale).round() as i32);
    let icon = Image::from_icon_name(item.icon);
    icon.set_pixel_size((15.0 * scale).round() as i32);
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

    context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Normal);
    context.set_font_size((11.0 * scale).round());
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
    // Where the recording resumed after a pause, and the moments the person
    // marked while recording, as the studied editor shows them.
    for &source in &data.recording_marks.pauses {
        let Some(time) = data.composition_time_for_source(source) else {
            continue;
        };
        let x = data.time_to_x(time, width).floor() + 0.5;
        if x < -4.0 || x > width + 4.0 {
            continue;
        }
        context.set_source_rgba(
            tick_r,
            tick_g,
            tick_b,
            if prefers_dark { 0.55 } else { 0.6 },
        );
        context.set_dash(&[2.0 * scale, 2.0 * scale], 0.0);
        context.move_to(x, 0.0);
        context.line_to(x, height);
        let _ = context.stroke();
        context.set_dash(&[], 0.0);
    }
    for &source in &data.recording_marks.markers {
        let Some(time) = data.composition_time_for_source(source) else {
            continue;
        };
        let x = data.time_to_x(time, width).floor() + 0.5;
        if x < -8.0 || x > width + 8.0 {
            continue;
        }
        context.set_source_rgba(0.98, 0.74, 0.18, 1.0);
        context.move_to(x, 0.0);
        context.line_to(x, height);
        let _ = context.stroke();
        let flag = (6.0 * scale).round();
        context.move_to(x, 1.0);
        context.line_to(x + flag, 1.0 + flag * 0.5);
        context.line_to(x, 1.0 + flag);
        context.close_path();
        let _ = context.fill();
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
        let y = 2.0 * scale;
        let pill_h = (height - 4.0 * scale).max(10.0);
        let tone = effect_tone(track, prefers_dark, data.effect_hidden(track, index));

        rounded_rect(context, x0, y, pill_w, pill_h, 8.0 * scale);
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
            context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Normal);
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

/// A resting lane, drawn the studied way: its name sits at the left edge and
/// each clip is a slim bar at its time. A bar that runs under the name fades
/// in from transparent behind it, so the name never sits on solid colour.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_thin_lane(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    label: &str,
    bars: &[(f64, f64, (f64, f64, f64, f64))],
    prefers_dark: bool,
    scale: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Bold);
    context.set_font_size((12.0 * scale).round());
    let extents = context.text_extents(label).ok();
    let text_w = extents
        .map(|extents| extents.x_advance())
        .unwrap_or(40.0 * scale);
    let text_h = extents
        .map(|extents| extents.height())
        .unwrap_or(9.0 * scale);
    let label_x = 6.0 * scale;
    let label_end = label_x + text_w;
    let fade_end = label_end + 40.0 * scale;

    let bar_h = (height - 4.0 * scale).max(3.0);
    let y = (height - bar_h) / 2.0;
    for (start, end, fill) in bars {
        let x0 = data.time_to_x(*start, width);
        let x1 = data.time_to_x(*end, width).max(x0 + 4.0 * scale);
        rounded_rect(
            context,
            x0,
            y,
            x1 - x0,
            bar_h,
            (bar_h * 0.35).min(5.0 * scale),
        );
        if x0 < fade_end {
            let fade = gtk4::cairo::LinearGradient::new(0.0, 0.0, fade_end, 0.0);
            fade.add_color_stop_rgba(0.0, fill.0, fill.1, fill.2, 0.0);
            fade.add_color_stop_rgba(label_end / fade_end, fill.0, fill.1, fill.2, fill.3 * 0.15);
            fade.add_color_stop_rgba(1.0, fill.0, fill.1, fill.2, fill.3);
            let _ = context.set_source(&fade);
        } else {
            context.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
        }
        let _ = context.fill();
    }
    let ink = if prefers_dark { 1.0 } else { 0.0 };
    context.set_source_rgba(ink, ink, ink, 0.6);
    context.move_to(label_x, (height + text_h) / 2.0);
    let _ = context.show_text(label);
}

/// The clip held on the clipboard, drawn where a click would drop it.
///
/// A placeable ghost reads as the real pill, so what the click will produce is
/// obvious. One that would collide keeps the outline and drops the fill,
/// matching the hollow treatment a disabled clip already uses.
#[allow(clippy::too_many_arguments)]
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
    let y = 2.0 * scale;
    let pill_h = (height - 4.0 * scale).max(10.0);
    let free = data.paste_spot_free(track, placed, duration);
    let (base_r, base_g, base_b) = match track {
        EffectTrack::Zoom => (0.0, 0.0, 1.0),
        EffectTrack::Hide => (0.4, 0.0, 0.2),
    };
    let fill_alpha = if free { 0.45 } else { 0.10 };

    rounded_rect(context, x0, y, pill_w, pill_h, 8.0 * scale);
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
        context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Normal);
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

/// The short layout names the camera lane uses on its chips and in its
/// tooltips.
fn layout_lane_label(layout: super::webcam::CameraLayout) -> &'static str {
    match layout {
        super::webcam::CameraLayout::CameraOverlay => "Overlay",
        super::webcam::CameraLayout::FullscreenCamera => "Fullscreen",
        super::webcam::CameraLayout::SplitScreen => "Split",
        super::webcam::CameraLayout::ScreenOnly => "Screen",
        super::webcam::CameraLayout::CameraCutout => "Cutout",
    }
}

/// `m:ss.s`, the clock the camera lane and its tooltips use.
fn format_layout_clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}

/// The width a layout chip takes from its text and the distance to the next
/// cue. The gap is a ceiling, never a target: a short label stays a short chip,
/// and neighbouring chips keep a seam between them.
/// Pixel bounds of one layout-change chip. A change owns the row from its own
/// instant to the next one, so the chips tile the lane the way the effect
/// pills do and a chip is as wide as the layout it holds.
fn layout_chip_bounds(data: &Data, width: f64, index: usize, scale: f64) -> Option<(f64, f64)> {
    let layouts = data.webcam_layouts()?;
    let cue = layouts.cues.get(index)?;
    let duration = data.total_duration();
    if duration <= 0.0 {
        return None;
    }
    let start = data.time_to_x(cue.at, width);
    let next = layouts
        .cues
        .get(index + 1)
        .map(|next| next.at)
        .unwrap_or(duration);
    let seam = LAYOUT_CHIP_GAP * scale;
    let end = (data.time_to_x(next, width) - seam).max(start + 2.0 * scale);
    Some((start, end))
}

/// The layout change whose chip sits under `x`. A click near the cue time
/// still lands on a chip squeezed narrow by its neighbour.
fn layout_cue_at(data: &Data, width: f64, x: f64, scale: f64) -> Option<usize> {
    if !x.is_finite() || width <= 0.0 || x < 0.0 || x > width {
        return None;
    }
    let layouts = data.webcam_layouts()?;
    if data.total_duration() <= 0.0 {
        return None;
    }
    let reach = LAYOUT_CHIP_HIT * scale.max(0.0);
    layouts
        .cues
        .iter()
        .enumerate()
        .filter_map(|(index, _cue)| {
            let (start, end) = layout_chip_bounds(data, width, index, scale)?;
            let hit_start = start - reach;
            let hit_end = end.max(start + reach);
            (x >= hit_start && x <= hit_end).then_some(((x - start).abs(), index))
        })
        .min_by(|(after, _), (before, _)| after.total_cmp(before))
        .map(|(_, index)| index)
}

/// Where the default chip ends: the first cue, or the composition's end when
/// no cue has been placed.
fn layout_default_end(data: &Data, width: f64) -> Option<f64> {
    let layouts = data.webcam_layouts()?;
    let duration = data.total_duration();
    if duration <= 0.0 {
        return None;
    }
    let end = layouts.cues.first().map(|cue| cue.at).unwrap_or(duration);
    Some(data.time_to_x(end, width))
}

/// Whether `x` sits on the muted default chip.
fn layout_default_at(data: &Data, width: f64, x: f64) -> bool {
    if !x.is_finite() || width <= 0.0 {
        return false;
    }
    let Some(end) = layout_default_end(data, width) else {
        return false;
    };
    let start = data.time_to_x(0.0, width).max(0.0);
    let end = end.min(width);
    start < end && x >= start && x <= end
}

fn layout_lane_tooltip(data: &Data, width: f64, x: f64, scale: f64) -> Option<String> {
    let layouts = data.webcam_layouts()?;
    if let Some(index) = layout_cue_at(data, width, x, scale) {
        let cue = layouts.cues.get(index)?;
        return Some(format!(
            "{} {} · {}",
            t("Layout change"),
            format_layout_clock(cue.at),
            t(layout_lane_label(cue.layout))
        ));
    }
    layout_default_at(data, width, x).then(|| {
        format!(
            "{} · {}",
            t("Default layout"),
            t(layout_lane_label(layouts.default_layout))
        )
    })
}

/// Solid fill for a layout chip. The camera lane has its own teal instead of
/// the theme accent — a colour the accent already marks as selected elsewhere,
/// and one that sits too close to the Zoom lane's blue. The selected chip is
/// the saturated end of the hue; a placed change sits in a deeper, quieter
/// version of it so it does not read as selected.
fn layout_chip_fill(selected: bool, prefers_dark: bool) -> (f64, f64, f64, f64) {
    match (selected, prefers_dark) {
        (true, true) => (0.15, 0.46, 0.41, 1.0),
        (true, false) => (0.10, 0.40, 0.35, 1.0),
        (false, true) => (0.13, 0.28, 0.26, 1.0),
        (false, false) => (0.80, 0.89, 0.87, 1.0),
    }
}

/// The one-pixel ring the selected chip carries. It is the same hue as the
/// fill, lifted until it reads against it on either theme, rather than the
/// near-white that used to outline an accent chip.
fn layout_chip_ring(prefers_dark: bool) -> (f64, f64, f64, f64) {
    if prefers_dark {
        (0.72, 0.96, 0.90, 0.95)
    } else {
        (0.86, 1.0, 0.96, 0.95)
    }
}

fn layout_chip_label_color(selected: bool, prefers_dark: bool) -> (f64, f64, f64, f64) {
    match (selected, prefers_dark) {
        (true, _) => (1.0, 1.0, 1.0, 0.98),
        (false, true) => (0.90, 0.98, 0.96, 0.92),
        (false, false) => (0.07, 0.22, 0.20, 0.88),
    }
}

/// Truncate `text` to `max_width` with an ellipsis. `None` means not even the
/// ellipsis fits, so the chip stays bare.
fn ellipsized_text(context: &Context, text: &str, max_width: f64) -> Option<String> {
    if !max_width.is_finite() || max_width <= 0.0 {
        return None;
    }
    let width = |value: &str| {
        context
            .text_extents(value)
            .map(|extents| extents.width())
            .unwrap_or(f64::INFINITY)
    };
    if width(text) <= max_width {
        return Some(text.to_owned());
    }
    let ellipsis_width = width("…");
    if !ellipsis_width.is_finite() || ellipsis_width > max_width {
        return None;
    }
    let mut fitted = String::new();
    for character in text.chars() {
        let mut candidate = fitted.clone();
        candidate.push(character);
        candidate.push('…');
        if width(&candidate) > max_width {
            break;
        }
        fitted.push(character);
    }
    (!fitted.is_empty()).then(|| format!("{fitted}…"))
}

/// Paint the camera lane: a muted band for the default layout, then one chip
/// per layout change.
fn draw_layout_lane(
    context: &Context,
    width: i32,
    height: i32,
    data: &Data,
    prefers_dark: bool,
    scale: f64,
) {
    let Some(layouts) = data.webcam_layouts() else {
        return;
    };
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    if data.total_duration() <= 0.0 {
        return;
    }
    // A switched-off layer keeps its chips on the row, painted back so the row
    // still says what is stored while the picture shows the screen alone.
    let fade = if data.webcam_shown() { 1.0 } else { 0.45 };
    // The same pill inset, height and radius as the Zoom and Hide lanes, so
    // the camera row reads as one of the same family of tracks.
    let y = LAYOUT_CHIP_INSET * scale;
    let pill_height = (height - LAYOUT_CHIP_INSET * 2.0 * scale).max(10.0);
    let radius = LAYOUT_CHIP_RADIUS * scale;
    context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Normal);
    context.set_font_size((11.0 * scale).round());

    // The default chip runs from zero to the first change. With no changes it
    // is the whole row. It names the layout it stands for, without the word
    // "Default": the panel already says which scope is being edited, and the
    // band only has room for the layout itself.
    let start_x = data.time_to_x(0.0, width);
    let default_end_x = layout_default_end(data, width).unwrap_or(start_x);
    let default_width = default_end_x - start_x;
    if default_width > 1.0 {
        rounded_rect(
            context,
            start_x,
            y,
            default_width.max(2.0),
            pill_height,
            radius,
        );
        if prefers_dark {
            context.set_source_rgba(0.10, 0.155, 0.15, fade);
        } else {
            context.set_source_rgba(0.87, 0.915, 0.905, fade);
        }
        let _ = context.fill();

        let label = t(layout_lane_label(layouts.default_layout));
        let visible_start = start_x.max(0.0);
        let visible_width = default_end_x.min(width) - visible_start;
        let label_width = visible_width - LAYOUT_CHIP_LABEL_PADDING * scale;
        if let Some(fitted) = ellipsized_text(context, &label, label_width) {
            // Quieter than a placed change's label: the band is the resting
            // state, and its changes are what the row is for.
            let (r, g, b, a) = layout_chip_label_color(false, prefers_dark);
            context.set_source_rgba(r, g, b, a * fade * 0.78);
            context.move_to(
                visible_start + LAYOUT_CHIP_LABEL_PADDING * 0.5 * scale,
                y + pill_height * 0.62,
            );
            let _ = context.show_text(&fitted);
        }
    }

    // One discrete chip per layout change, sized by its label and stopped
    // before the next change. The gap after the last chip is the row surface,
    // not a stretched span.
    for (index, cue) in layouts.cues.iter().enumerate() {
        let label = t(layout_lane_label(cue.layout));
        let Some((chip_x, chip_end)) = layout_chip_bounds(data, width, index, scale) else {
            continue;
        };
        if chip_end < 0.0 || chip_x > width {
            continue;
        }
        let selected = data.selected_layout == Some(index);
        let chip_width = chip_end - chip_x;
        if chip_width <= 0.0 {
            continue;
        }

        rounded_rect(context, chip_x, y, chip_width, pill_height, radius);
        let (fill_r, fill_g, fill_b, fill_a) = layout_chip_fill(selected, prefers_dark);
        context.set_source_rgba(fill_r, fill_g, fill_b, fill_a * fade);
        let _ = context.fill();

        if selected {
            // The selected chip alone carries the lane's colour, lifted into a
            // one-pixel ring at its edge.
            rounded_rect(
                context,
                chip_x + 0.5,
                y + 0.5,
                (chip_width - 1.0).max(0.0),
                (pill_height - 1.0).max(0.0),
                (radius - 0.5).max(0.0),
            );
            let (ring_r, ring_g, ring_b, ring_a) = layout_chip_ring(prefers_dark);
            context.set_source_rgba(ring_r, ring_g, ring_b, ring_a * fade);
            context.set_line_width(1.0);
            let _ = context.stroke();
        }

        let visible_start = chip_x.max(0.0);
        let visible_width = chip_end.min(width) - visible_start;
        let label_width = visible_width - LAYOUT_CHIP_LABEL_PADDING * scale;
        let Some(fitted) = ellipsized_text(context, &label, label_width) else {
            continue;
        };
        let (r, g, b, a) = layout_chip_label_color(selected, prefers_dark);
        context.set_source_rgba(r, g, b, a * fade);
        context.move_to(
            visible_start + LAYOUT_CHIP_LABEL_PADDING * 0.5 * scale,
            y + pill_height * 0.62,
        );
        let _ = context.show_text(&fitted);
    }
}

/// The scale presets the Zoom panel offers, on the same stops as the shared
/// editor. Anything between two stops keeps its own scale; the panel then
/// lights no chip rather than rounding the clip to a value nobody picked.
pub(super) const ZOOM_SCALE_PRESETS: [(&str, f64); 6] = [
    ("1.25×", 1.25),
    ("1.5×", 1.5),
    ("2×", 2.0),
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
    context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Normal);
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
        let speed = data.segment_speed(index);
        let footage = (source_end - source_start).max(0.0) / speed;
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
                let tile_x = data.time_to_x(segment_start + (from - source_start) / speed, width);
                let tile_right = data.time_to_x(segment_start + (to - source_start) / speed, width);
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

        if (speed - 1.0).abs() > 1e-9 {
            let label = speed_label(speed);
            context.select_font_face("DM Sans", FontSlant::Normal, FontWeight::Bold);
            context.set_font_size((10.0 * scale).round());
            if let Ok(extents) = context.text_extents(&label) {
                let pad = 5.0 * scale;
                let badge_w = extents.width() + pad * 2.0;
                let badge_h = (14.0 * scale).min(clip_height - 4.0 * scale);
                if badge_h > 0.0 && badge_w + 8.0 * scale <= body_width {
                    let badge_x = right - badge_w - 4.0 * scale;
                    let badge_y = y + clip_height - badge_h - 4.0 * scale;
                    rounded_rect(
                        context,
                        badge_x,
                        badge_y,
                        badge_w,
                        badge_h,
                        (badge_h / 2.0).min(4.0 * scale),
                    );
                    context.set_source_rgba(0.05, 0.06, 0.08, 0.72);
                    let _ = context.fill();
                    context.set_source_rgba(1.0, 1.0, 1.0, 0.92);
                    let text_top = badge_y + (badge_h - extents.height()) / 2.0;
                    context.move_to(badge_x + pad, text_top - extents.y_bearing());
                    let _ = context.show_text(&label);
                }
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

/// A speed as the timeline chip labels it: whole numbers stay whole, and a
/// fractional speed keeps its own digits.
fn speed_label(speed: f64) -> String {
    let rounded = (speed * 100.0).round() / 100.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}×", rounded.round() as i64)
    } else {
        format!("{rounded}×")
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
            context.set_line_width(1.0);
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
    context.set_line_width(1.0);
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
    use super::super::background::{Background, DEFAULT_PADDING, DEFAULT_RADIUS};
    use super::super::project;
    use super::super::webcam::{CameraLayout, LayoutSettings};
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, PointerSidecar, WebcamTrack,
    };
    use gtk4::cairo::{Format, ImageSurface};

    fn loaded() -> Data {
        Data {
            path: Some(PathBuf::from("clip.mp4")),
            duration: 10.0,
            frame_size: Some((1920, 1080)),
            trim_end: 10.0,
            ..Data::default()
        }
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
        let background = Background::default();

        assert!(data.project_file(&background).is_none());

        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        let file = data
            .project_file(&background)
            .expect("an edited session is saved");
        assert_eq!(file.zoom_clips.len(), 1);
        assert_eq!(file.source_path, video);
    }

    #[test]
    fn a_stage_alone_is_saved_with_the_project() {
        let video = recording("stage-only");
        let mut data = loaded();
        data.path = Some(video.clone());
        let background = Background::default();

        assert!(
            data.project_file(&background).is_none(),
            "an untouched flat stage is still a freshly opened recording"
        );

        background.apply(Some("wallpaper-004.jpg"), 48.0, 6.0);
        let file = data
            .project_file(&background)
            .expect("a staged session is saved");
        assert_eq!(
            file.background.selection.as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert!((file.background_padding - 48.0).abs() < 1e-9);
        assert!((file.background_radius - 6.0).abs() < 1e-9);
        assert_eq!(file.source_path, video);
    }

    #[test]
    fn a_padding_only_stage_is_saved_and_resets_to_pristine() {
        let video = recording("stage-padding-only");
        let mut data = loaded();
        data.path = Some(video);
        let background = Background::default();

        background.apply(None, 48.0, DEFAULT_RADIUS);
        let file = data
            .project_file(&background)
            .expect("a padded stage is saved");
        assert!(file.background.selection.is_none());
        assert!((file.background_padding - 48.0).abs() < 1e-9);
        assert!((file.background_radius - DEFAULT_RADIUS).abs() < 1e-9);

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS);
        assert!(data.project_file(&background).is_none());
    }

    #[test]
    fn a_radius_only_stage_is_saved_and_resets_to_pristine() {
        let video = recording("stage-radius-only");
        let mut data = loaded();
        data.path = Some(video);
        let background = Background::default();

        background.apply(None, DEFAULT_PADDING, 6.0);
        let file = data
            .project_file(&background)
            .expect("a rounded stage is saved");
        assert!(file.background.selection.is_none());
        assert!((file.background_padding - DEFAULT_PADDING).abs() < 1e-9);
        assert!((file.background_radius - 6.0).abs() < 1e-9);

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS);
        assert!(data.project_file(&background).is_none());
    }

    #[test]
    fn an_initialised_project_keeps_a_flat_stage() {
        let video = recording("seeded-reset");
        let mut data = loaded();
        data.path = Some(video);
        data.has_project = true;
        let background = Background::default();

        background.apply(Some("wallpaper-004.jpg"), 40.0, 0.0);
        assert!(data.project_file(&background).is_some());

        background.apply(None, DEFAULT_PADDING, DEFAULT_RADIUS);
        let file = data
            .project_file(&background)
            .expect("a reset stage keeps its project");
        assert!(file.background.selection.is_none());
        assert!((file.background_padding - DEFAULT_PADDING).abs() < 1e-9);
        assert!((file.background_radius - DEFAULT_RADIUS).abs() < 1e-9);
    }

    #[test]
    fn a_saved_flat_project_is_not_seeded_again() {
        let video = recording("flat-v2");
        let mut data = loaded();
        data.path = Some(video.clone());
        data.apply_project(&project::EditorProjectFile {
            version: 2,
            source_path: video,
            source_size: 0,
            source_mtime_secs: 0,
            trim_start_seconds: 0.0,
            trim_end_seconds: 10.0,
            cuts: Vec::new(),
            segment_starts: Vec::new(),
            segment_speeds: Vec::new(),
            typing_suggestions_resolved: Vec::new(),
            freeze_tail: 0.0,
            zoom_clips: Vec::new(),
            zoom_camera: project::ZoomCameraFile::default(),
            cursor_hide_clips: Vec::new(),
            webcam: None,
            background: project::StageFile::default(),
            background_padding: DEFAULT_PADDING,
            background_radius: DEFAULT_RADIUS,
            playhead_seconds: 0.0,
            timeline_scale: 0.0,
            timeline_scroll_seconds: 0.0,
            cursor_click_sound: None,
            cursor_click_sound_volume: 0.25,
            captions_shortcuts: false,
            captions_show_single_letters: false,
            cursor: Default::default(),
        });

        let background = Background::default();
        let file = data
            .project_file(&background)
            .expect("a loaded project is kept even when flat");
        assert!(file.background.selection.is_none());
        assert!((file.background_padding - DEFAULT_PADDING).abs() < 1e-9);
        assert!((file.background_radius - DEFAULT_RADIUS).abs() < 1e-9);

        let text = serde_json::to_string(&file).unwrap();
        assert_eq!(
            serde_json::from_str::<project::EditorProjectFile>(&text).unwrap(),
            file
        );
    }

    #[test]
    fn resetting_the_media_clears_the_project_marker() {
        let mut data = loaded();
        data.has_project = true;

        data.reset_media();

        assert!(!data.has_project);
    }

    #[test]
    fn an_undo_step_keeps_the_project_initialised() {
        let mut data = loaded();
        data.has_project = true;

        let before = data.snapshot();
        data.has_project = false;
        data.restore(before);

        assert!(data.has_project);
    }

    fn tuned_cursor() -> CursorSettings {
        CursorSettings {
            theme: crate::recording::editor::model::CursorTheme::White,
            size: 1.6,
            click_effect: crate::recording::editor::model::ClickEffect::Circle,
            stop_movement_in_last_part_ms: 800.0,
            ..CursorSettings::default()
        }
    }

    fn editable(name: &str) -> (Data, Background) {
        let video = recording(name);
        let mut data = loaded();
        data.path = Some(video);
        assert!(data.add_effect(EffectTrack::Zoom, 2.0).is_some());
        (data, Background::default())
    }

    #[test]
    fn a_document_opens_on_the_cursor_look_its_file_stored() {
        let (data, background) = editable("cursor-open");
        let file = data
            .project_file(&background)
            .expect("an edited session is saved");
        let stored = snapshot_with_cursor(Some(file), tuned_cursor()).expect("a snapshot");

        assert_eq!(incoming_cursor(Some(&stored)), tuned_cursor());
        assert_eq!(incoming_cursor(None), CursorSettings::default());
    }

    #[test]
    fn a_saved_snapshot_carries_the_cursor_this_session_holds() {
        let (data, background) = editable("cursor-save");
        let file = data
            .project_file(&background)
            .expect("an edited session is saved");
        assert_eq!(file.cursor, super::super::presets::CursorFile::default());

        let saved = snapshot_with_cursor(Some(file), tuned_cursor()).expect("a snapshot");
        assert_eq!(saved.cursor.settings(), tuned_cursor());
        assert_eq!(
            saved.cursor,
            super::super::presets::CursorFile::from_settings(tuned_cursor())
        );
        assert_eq!(snapshot_with_cursor(None, tuned_cursor()), None);
    }

    #[test]
    fn noting_a_cursor_change_writes_nothing_until_the_sync_runs() {
        let (data, background) = editable("cursor-tick");
        let video = data.path.clone().expect("the session has a recording");
        let directory = video
            .parent()
            .expect("a recording sits in a folder")
            .to_path_buf();
        let snapshot = snapshot_with_cursor(data.project_file(&background), tuned_cursor());
        let path = project::project_path_for_video(&video);
        project::delete_project(&video);
        let sync = project::ProjectSync::default();
        sync.adopt(None);

        sync.mark_changed();
        assert!(
            sync.take_changed(),
            "the autosave tick is what sees the note"
        );
        assert!(!path.exists(), "noting a change writes nothing on its own");

        assert!(sync.sync(snapshot, false));
        let saved = project::load_project(&video).expect("the project reloads");
        assert_eq!(saved.cursor.settings(), tuned_cursor());

        project::delete_project(&video);
        clean_up(vec![video, directory]);
    }

    #[test]
    fn a_document_with_nothing_to_save_carries_no_look_for_a_preset() {
        let data = Data::default();
        let background = Background::default();
        assert!(data.project_file(&background).is_none());
    }

    #[test]
    fn a_captured_look_keeps_the_camera_the_recording_shows() {
        let (data, background) = editable("preset-camera");
        let mut file = data
            .project_file(&background)
            .expect("an edited session is saved");
        assert!(file.webcam.is_none(), "an unmoved camera saves no block");

        fill_camera_from_recording(
            &mut file,
            Some(super::super::webcam::LayerSettings {
                enabled: true,
                fullscreen: false,
                x: 0.62,
                y: 0.31,
                w: 0.18,
                h: 0.24,
                radius: 0.4,
                mirror: true,
                delay_seconds: 0.02,
                zoomed_scale: 0.7,
                hide_during_silence: Some(4.0),
            }),
            LayoutSettings::default(),
        );
        let camera = file.webcam.as_ref().expect("the recorded camera travels");
        assert_eq!(camera.x, 0.62);
        assert_eq!(camera.radius, 0.4);
        assert!(camera.mirror);
        assert!(camera.layouts.is_some());

        fill_camera_from_recording(&mut file, None, LayoutSettings::default());
        assert_eq!(
            file.webcam.as_ref().expect("the block stays").x,
            0.62,
            "a saved block is never replaced by the recorded one"
        );
    }

    #[test]
    fn a_legacy_project_without_piece_starts_opens_contiguous_at_normal_speed() {
        let mut data = loaded();
        data.apply_project(&project::EditorProjectFile {
            version: 2,
            source_path: PathBuf::from("clip.mp4"),
            source_size: 0,
            source_mtime_secs: 0,
            trim_start_seconds: 0.0,
            trim_end_seconds: 10.0,
            cuts: vec![3.0, 6.0],
            segment_starts: Vec::new(),
            segment_speeds: Vec::new(),
            typing_suggestions_resolved: Vec::new(),
            freeze_tail: 0.0,
            zoom_clips: Vec::new(),
            zoom_camera: project::ZoomCameraFile::default(),
            cursor_hide_clips: Vec::new(),
            webcam: None,
            background: project::StageFile::default(),
            background_padding: DEFAULT_PADDING,
            background_radius: DEFAULT_RADIUS,
            playhead_seconds: 0.0,
            timeline_scale: 0.0,
            timeline_scroll_seconds: 0.0,
            cursor_click_sound: None,
            cursor_click_sound_volume: 0.25,
            captions_shortcuts: false,
            captions_show_single_letters: false,
            cursor: Default::default(),
        });

        assert_eq!(data.segment_starts, vec![0.0, 3.0, 6.0]);
        assert_eq!(data.segment_speeds, vec![1.0, 1.0, 1.0]);
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
            segment_speeds: vec![1.0, 1.0],
            typing_suggestions_resolved: Vec::new(),
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
                motion_blur: 1.0,
            },
            cursor_hide_clips: vec![project::HideClipFile {
                start: 1.0,
                end: 4.0,
                hidden: false,
            }],
            webcam: None,
            background: project::StageFile::default(),
            background_padding: DEFAULT_PADDING,
            background_radius: DEFAULT_RADIUS,
            playhead_seconds: 99.0,
            timeline_scale: 400.0,
            timeline_scroll_seconds: 12.0,
            cursor_click_sound: None,
            cursor_click_sound_volume: 0.25,
            captions_shortcuts: false,
            captions_show_single_letters: false,
            cursor: Default::default(),
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
        assert!((zoom.end - 5.0).abs() < 1e-9);
        assert!((zoom.scale - DEFAULT_ZOOM_SCALE).abs() < 1e-9);
        assert_eq!(zoom.center, (960.0, 540.0));
        assert_eq!(data.selected_zoom, Some(0));
        assert_eq!(data.selected_hide, Some(0));
        assert!(!data.selected);
    }

    #[test]
    fn effects_do_not_stack_on_each_other() {
        let mut data = loaded();
        assert_eq!(data.add_effect(EffectTrack::Zoom, 1.0), Some(0));
        assert!((data.zoom_clips[0].start - 1.0).abs() < 1e-9);
        assert!((data.zoom_clips[0].end - 4.0).abs() < 1e-9);

        // A drop inside the zoom splits it at the playhead and selects the
        // right half instead of stacking a second pill on the same span.
        assert_eq!(data.add_effect(EffectTrack::Zoom, 2.0), Some(1));
        assert_eq!(data.zoom_clips.len(), 2);
        assert!((data.zoom_clips[0].start - 1.0).abs() < 1e-9);
        assert!((data.zoom_clips[0].end - 2.0).abs() < 1e-9);
        assert!((data.zoom_clips[1].start - 2.0).abs() < 1e-9);
        assert!((data.zoom_clips[1].end - 4.0).abs() < 1e-9);
        assert_eq!(data.selected_zoom, Some(1));

        // Past the split zoom, a new one runs its default length.
        assert_eq!(data.add_effect(EffectTrack::Zoom, 4.0), Some(2));
        assert!((data.zoom_clips[2].start - 4.0).abs() < 1e-9);
        assert!((data.zoom_clips[2].end - 7.0).abs() < 1e-9);

        // Hide still refuses a drop on top of another hide.
        assert!(data.add_effect(EffectTrack::Hide, 1.0).is_some());
        assert!(data.add_effect(EffectTrack::Hide, 2.0).is_none());
    }

    #[test]
    fn a_new_zoom_runs_three_seconds_or_to_the_next_zoom() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 5.0).is_some());
        assert!((data.zoom_clips[0].start - 5.0).abs() < 1e-9);
        assert!((data.zoom_clips[0].end - 8.0).abs() < 1e-9);

        // The next zoom's start caps the new clip's length.
        assert!(data.add_effect(EffectTrack::Zoom, 3.0).is_some());
        assert_eq!(data.zoom_clips.len(), 2);
        assert!((data.zoom_clips[0].start - 3.0).abs() < 1e-9);
        assert!((data.zoom_clips[0].end - 5.0).abs() < 1e-9);
    }

    #[test]
    fn adding_inside_a_zoom_splits_it_at_the_playhead() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_scale(2.5);
        assert_eq!(data.add_effect(EffectTrack::Zoom, 2.5), Some(1));
        assert_eq!(data.zoom_clips.len(), 2);
        assert!((data.zoom_clips[0].start - 1.0).abs() < 1e-9);
        assert!((data.zoom_clips[0].end - 2.5).abs() < 1e-9);
        assert!((data.zoom_clips[1].start - 2.5).abs() < 1e-9);
        assert!((data.zoom_clips[1].end - 4.0).abs() < 1e-9);
        assert_eq!(data.selected_zoom, Some(1));
        assert!((data.zoom_clips[0].scale - 2.5).abs() < 1e-9);
        assert!((data.zoom_clips[1].scale - 2.5).abs() < 1e-9);
    }

    #[test]
    fn new_zooms_inherit_the_last_edited_scale_and_instant_flag() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        data.set_selected_zoom_scale(3.0);
        data.set_selected_zoom_instant(true);

        assert!(data.add_effect(EffectTrack::Zoom, 6.0).is_some());
        let clip = data.zoom_clips[1];
        assert!((clip.scale - 3.0).abs() < 1e-9);
        assert!(clip.instant);
    }

    #[test]
    fn new_zooms_are_automatic_only_with_pointer_data() {
        let mut data = loaded();
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Manual);

        data.pointer_available = true;
        assert!(data.add_effect(EffectTrack::Zoom, 5.0).is_some());
        assert_eq!(data.zoom_clips[1].mode, ZoomMode::Auto);
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

    fn click_sidecar(clicks: &[(f64, f64, f64)]) -> PointerSidecar {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for &(t, x, y) in clicks {
            sidecar.clicks.push(ClickSample {
                t,
                x,
                y,
                button: 0,
                cursor_id: None,
            });
        }
        sidecar
    }

    #[test]
    fn a_fresh_import_seeds_its_clicks_automatic_zooms() {
        let mut data = loaded();
        let sidecar = click_sidecar(&[
            (1.0, 480.0, 270.0),
            (7.0, 1440.0, 810.0),
            (9.5, 960.0, 540.0),
        ]);

        data.seed_auto_zooms(&sidecar, (1920, 1080), true);

        assert_eq!(data.zoom_clips.len(), 2);
        let first = data.zoom_clips[0];
        assert!((first.start - 0.7).abs() < 1e-9);
        assert!((first.end - 3.5).abs() < 1e-9);
        assert!((first.scale - 2.0).abs() < 1e-9);
        assert_eq!(first.center, (480.0, 270.0));
        assert_eq!(first.mode, ZoomMode::Auto);
        let (anchor_start, anchor_end) = first.anchor.expect("a seeded clip carries its footage");
        assert!((anchor_start - 0.7).abs() < 1e-9);
        assert!((anchor_end - 3.5).abs() < 1e-9);

        let second = data.zoom_clips[1];
        assert!((second.start - 6.7).abs() < 1e-9);
        assert!((second.end - 9.2).abs() < 1e-9);
        assert_eq!(second.center, (1440.0, 810.0));
        assert_eq!(second.mode, ZoomMode::Auto);
        let (anchor_start, anchor_end) = second.anchor.expect("a seeded clip carries its footage");
        assert!((anchor_start - 6.7).abs() < 1e-9);
        assert!((anchor_end - 9.2).abs() < 1e-9);
        assert_eq!(data.frame_size, Some((1920, 1080)));
    }

    #[test]
    fn seeding_stays_off_when_the_setting_is_off() {
        let mut data = loaded();
        let sidecar = click_sidecar(&[(1.0, 480.0, 270.0)]);

        data.seed_auto_zooms(&sidecar, (1920, 1080), false);

        assert!(data.zoom_clips.is_empty());
    }

    #[test]
    fn seeding_again_keeps_the_clips_the_first_import_placed() {
        let mut data = loaded();
        data.seed_auto_zooms(&click_sidecar(&[(1.0, 480.0, 270.0)]), (1920, 1080), true);
        let seeded = data.zoom_clips.clone();

        data.seed_auto_zooms(&click_sidecar(&[(5.0, 100.0, 100.0)]), (1280, 720), true);

        assert_eq!(data.zoom_clips, seeded);
        assert_eq!(data.frame_size, Some((1920, 1080)));
    }

    #[test]
    fn clicks_in_the_final_second_seed_nothing() {
        let mut data = loaded();
        let sidecar = click_sidecar(&[(9.5, 960.0, 540.0)]);

        data.seed_auto_zooms(&sidecar, (1920, 1080), true);

        assert!(data.zoom_clips.is_empty());
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
        assert!((second.end - 6.0).abs() < 1e-9);
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
        assert!((clip.end - 9.0).abs() < 1e-9);
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
        assert!((data.zoom_clips[1].start - 3.0).abs() < 1e-9);
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
    fn recording_marks_follow_the_footage_through_cuts_and_moves() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.set_segment_start(1, 6.0);
        assert!((data.composition_time_for_source(2.0).unwrap() - 2.0).abs() < 1e-9);
        assert!((data.composition_time_for_source(4.5).unwrap() - 6.5).abs() < 1e-9);
        assert_eq!(data.composition_time_for_source(42.0), None);
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
    fn a_trim_that_crosses_a_cut_keeps_the_piece_it_lands_in() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        assert!(data.add_cut(8.0));
        data.segment_starts = vec![0.0, 4.0, 8.0];
        data.segment_speeds = vec![2.0, 4.0, 1.0];

        data.set_trim_start(5.0);

        assert_eq!(data.segment_boundaries(), vec![(5.0, 8.0), (8.0, 10.0)]);
        assert_eq!(data.segment_speeds, vec![4.0, 1.0]);
        assert!((data.segment_start(0) - 4.0).abs() < 1e-9);
        assert!((data.segment_start(1) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn a_suggestion_next_to_the_clip_edge_survives_save_and_undo() {
        let video = recording("typing-edge");
        let mut data = loaded();
        data.path = Some(video.clone());
        assert!(data.apply_typing_ranges(&[(0.05, 1.0)], 2.0));

        let background = Background::default();
        let file = data
            .project_file(&background)
            .expect("an edge suggestion is saved");
        assert_eq!(file.cuts, vec![0.05, 1.0]);
        assert_eq!(file.segment_speeds, vec![1.0, 2.0, 1.0]);

        let mut reopened = loaded();
        reopened.path = Some(video);
        reopened.apply_project(&file);
        assert_eq!(reopened.cuts, vec![0.05, 1.0]);
        assert_eq!(reopened.segment_speeds, vec![1.0, 2.0, 1.0]);

        assert!(data.undo_edit());
        assert!(data.cuts.is_empty());
        assert!(data.segment_speeds.is_empty() || data.segment_speeds == vec![1.0]);
        assert!(data.redo_edit());
        assert_eq!(data.cuts, vec![0.05, 1.0]);
        assert_eq!(data.segment_speeds, vec![1.0, 2.0, 1.0]);
    }

    #[test]
    fn a_resolved_detected_range_stays_hidden_when_the_trim_expands() {
        let mut data = loaded();
        assert!(data.apply_typing_ranges(&[(3.0, 5.0)], 2.0));
        data.replace_typing_resolved(3.0, 2.0);
        assert_eq!(data.typing_suggestions_resolved, vec![2.0]);

        data.trim_start = 3.0;
        data.clamp_cuts();
        assert!(data.typing_range_resolved(2.0, 3.0));

        data.trim_start = 0.0;
        data.clamp_cuts();
        assert!(data.typing_range_resolved(2.0, 2.0));
        assert!(!data.typing_range_resolved(3.0, 3.0));
    }

    #[test]
    fn mixed_speeds_change_each_pieces_composition_length() {
        let mut data = loaded();
        assert!(data.add_cut(2.0));
        assert!(data.add_cut(4.0));
        data.segment_speeds = vec![2.0, 4.0, 1.0];
        data.segment_starts = vec![0.0, 1.0, 1.5];

        assert!((data.total_duration() - 7.5).abs() < 1e-9);
        assert!((data.segment_span(0).unwrap().1 - 1.0).abs() < 1e-9);
        assert!((data.segment_span(1).unwrap().0 - 1.0).abs() < 1e-9);
        assert!((data.segment_span(1).unwrap().1 - 1.5).abs() < 1e-9);
        assert!((data.segment_span(2).unwrap().0 - 1.5).abs() < 1e-9);

        assert!((data.source_time_at(0.5).unwrap() - 1.0).abs() < 1e-9);
        assert!((data.source_time_at(1.25).unwrap() - 3.0).abs() < 1e-9);
        assert!((data.composition_time_for_source(3.0).unwrap() - 1.25).abs() < 1e-9);

        data.playhead = 2.0;
        assert!((data.playhead_source_time() - 4.5).abs() < 1e-9);
    }

    #[test]
    fn splitting_a_sped_piece_inherits_its_speed() {
        let mut data = loaded();
        data.segment_speeds = vec![3.0];

        assert!(data.add_cut(2.0));

        assert_eq!(data.cuts, vec![6.0]);
        assert_eq!(data.segment_speeds, vec![3.0, 3.0]);
        assert!((data.segment_start(1) - 2.0).abs() < 1e-9);
        assert!((data.segment_span(1).unwrap().1 - (2.0 + 4.0 / 3.0)).abs() < 1e-9);
        assert!((data.total_duration() - 10.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn trimming_keeps_the_speed_of_the_piece_it_keeps() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.segment_speeds = vec![2.0, 4.0];

        data.trim_start = 1.0;
        data.clamp_cuts();
        assert_eq!(data.segment_boundaries(), vec![(1.0, 4.0), (4.0, 10.0)]);
        assert_eq!(data.segment_speeds, vec![2.0, 4.0]);
        assert!((data.segment_start(0) - 0.0).abs() < 1e-9);

        data.trim_start = 5.0;
        data.clamp_cuts();
        assert_eq!(data.segment_boundaries(), vec![(5.0, 10.0)]);
        assert_eq!(data.segment_speeds, vec![4.0]);
        assert!((data.segment_start(0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn speeding_a_range_keeps_the_gaps_between_moved_pieces() {
        let mut data = loaded();
        assert!(data.add_cut(3.0));
        assert!(data.add_cut(6.0));
        data.segment_starts = vec![0.0, 3.0, 8.0];

        assert!(data.apply_typing_ranges(&[(3.0, 6.0)], 3.0));

        assert!((data.segment_start(1) - 3.0).abs() < 1e-9);
        assert!((data.segment_start(2) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_suggestion_next_to_the_clip_edge_skips_the_split_guard() {
        let mut data = loaded();

        assert!(data.apply_typing_ranges(&[(0.05, 1.0)], 2.0));

        assert_eq!(data.cuts, vec![0.05, 1.0]);
        assert_eq!(data.segment_speeds, vec![1.0, 2.0, 1.0]);
    }

    #[test]
    fn applying_a_typing_range_splits_and_reflows_as_one_undo() {
        let mut data = loaded();

        assert!(data.apply_typing_ranges(&[(3.0, 6.0)], 3.0));

        assert_eq!(
            data.segment_boundaries(),
            vec![(0.0, 3.0), (3.0, 6.0), (6.0, 10.0)]
        );
        assert_eq!(data.segment_speeds, vec![1.0, 3.0, 1.0]);
        assert!((data.segment_start(0) - 0.0).abs() < 1e-9);
        assert!((data.segment_start(1) - 3.0).abs() < 1e-9);
        assert!((data.segment_start(2) - 4.0).abs() < 1e-9);
        assert!((data.total_duration() - 8.0).abs() < 1e-9);
        assert_eq!(data.typing_suggestions_resolved, vec![3.0]);

        assert!(data.undo_edit());
        assert_eq!(data.segment_boundaries(), vec![(0.0, 10.0)]);
        assert!(data.segment_speeds.is_empty() || data.segment_speeds == vec![1.0]);
        assert!(data.typing_suggestions_resolved.is_empty());
        assert!((data.total_duration() - 10.0).abs() < 1e-9);
        assert!(!data.undo_edit());

        assert!(data.redo_edit());
        assert_eq!(data.segment_speeds, vec![1.0, 3.0, 1.0]);
        assert_eq!(data.typing_suggestions_resolved, vec![3.0]);
        assert!(!data.apply_typing_ranges(&[(3.0, 6.0)], 3.0));
    }

    #[test]
    fn applying_all_suggestions_is_one_undo_and_skips_resolved_ranges() {
        let mut data = loaded();
        data.typing_suggestions_resolved.push(7.0);

        assert!(data.apply_typing_ranges(&[(1.0, 2.0), (4.0, 5.0), (7.0, 8.0)], 2.0));

        assert_eq!(
            data.segment_boundaries(),
            vec![(0.0, 1.0), (1.0, 2.0), (2.0, 4.0), (4.0, 5.0), (5.0, 10.0)]
        );
        assert_eq!(data.segment_speeds, vec![1.0, 2.0, 1.0, 2.0, 1.0]);
        assert_eq!(data.typing_suggestions_resolved, vec![7.0, 1.0, 4.0]);
        assert!((data.total_duration() - 9.0).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!(data.cuts.is_empty());
        assert_eq!(data.typing_suggestions_resolved, vec![7.0]);
        assert!(!data.undo_edit());
    }

    #[test]
    fn a_dismissed_suggestion_is_not_applied_and_undo_brings_it_back() {
        let mut data = loaded();

        assert!(data.dismiss_typing_suggestion(3.0));
        assert!(!data.apply_typing_ranges(&[(3.0, 6.0)], 2.0));
        assert!(data.cuts.is_empty());

        assert!(data.undo_edit());
        assert!(data.typing_suggestions_resolved.is_empty());
        assert!(data.apply_typing_ranges(&[(3.0, 6.0)], 2.0));
    }

    #[test]
    fn non_finite_typing_ranges_and_speeds_are_refused() {
        let mut data = loaded();

        assert!(!data.apply_typing_ranges(&[(f64::NAN, 3.0)], 3.0));
        assert!(!data.apply_typing_ranges(&[(1.0, f64::INFINITY)], 3.0));
        assert!(!data.apply_typing_ranges(&[(1.0, 2.0)], f64::NAN));
        assert!(!data.apply_typing_ranges(&[(1.0, 2.0)], 0.0));
        assert!(!data.apply_typing_ranges(&[(1.0, 2.0)], -2.0));

        assert!(data.cuts.is_empty());
        assert!(data.typing_suggestions_resolved.is_empty());
    }

    #[test]
    fn resolved_typing_ranges_round_trip_with_the_project() {
        let video = recording("typing-resolved");
        let mut data = loaded();
        data.path = Some(video.clone());
        assert!(data.apply_typing_ranges(&[(2.0, 4.0)], 2.0));

        let background = Background::default();
        let file = data
            .project_file(&background)
            .expect("an applied suggestion is saved");
        assert_eq!(file.segment_speeds, vec![1.0, 2.0, 1.0]);
        assert_eq!(file.typing_suggestions_resolved, vec![2.0]);

        let mut reopened = loaded();
        reopened.path = Some(video);
        reopened.apply_project(&file);
        assert_eq!(reopened.segment_speeds, vec![1.0, 2.0, 1.0]);
        assert_eq!(reopened.typing_suggestions_resolved, vec![2.0]);
    }

    #[test]
    fn anchored_zooms_follow_footage_through_a_sped_piece() {
        let mut data = loaded();
        assert!(data.add_cut(4.0));
        data.segment_speeds = vec![1.0, 3.0];
        data.zoom_clips = vec![ZoomClip {
            start: 0.0,
            end: 1.0,
            scale: 2.0,
            center: (960.0, 540.0),
            hidden: false,
            mode: ZoomMode::Auto,
            instant: false,
            anchor: Some((4.5, 5.5)),
        }];

        data.reproject_anchored_zooms();

        let clip = data.zoom_clips[0];
        assert!((clip.start - (4.0 + 0.5 / 3.0)).abs() < 1e-9);
        assert!((clip.end - (4.0 + 1.5 / 3.0)).abs() < 1e-9);
        assert!((super::super::zoom::CAMERA_FOLLOW_SPRING.stiffness - 125.0).abs() < 1e-9);
        assert!((super::super::zoom::CAMERA_FOLLOW_SPRING.damping - 12.0).abs() < 1e-9);
        assert!((super::super::zoom::CAMERA_FOLLOW_SPRING.mass - 1.5).abs() < 1e-9);
        assert!((crate::recording::editor::auto_zoom::TAIL_SECONDS - 2.5).abs() < 1e-9);
    }

    #[test]
    fn speed_labels_read_whole_and_fractional_speeds() {
        assert_eq!(speed_label(3.0), "3×");
        assert_eq!(speed_label(1.5), "1.5×");
        assert_eq!(speed_label(1.0), "1×");
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
    fn a_preview_click_adds_a_manual_zoom_aimed_at_the_point() {
        let mut data = loaded();
        data.pointer_available = true;
        assert!(data.set_add_zoom_tool(true));
        assert_eq!(
            data.add_zoom_tool(),
            Some(AddZoomTool {
                scale: DEFAULT_ZOOM_SCALE,
                automatic: true
            })
        );
        assert_eq!(
            data.add_effect_at(EffectTrack::Zoom, 2.0, Some((300.0, 5000.0))),
            Some(0)
        );
        let zoom = data.zoom_clips[0];
        assert_eq!(zoom.mode, ZoomMode::Manual);
        assert_eq!(zoom.center, (300.0, 1080.0));
        assert!((zoom.end - 5.0).abs() < 1e-9);
        assert_eq!(data.selected_zoom, Some(0));
        assert!(
            !data.add_zoom_tool,
            "placing a zoom selects it, which disarms the tool"
        );
        assert_eq!(data.add_zoom_tool(), None);
    }

    #[test]
    fn a_preview_click_inside_a_zoom_splits_it_into_a_manual_half() {
        let mut data = loaded();
        data.pointer_available = true;
        assert!(data.add_effect(EffectTrack::Zoom, 1.0).is_some());
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Auto);
        assert!(data.set_add_zoom_tool(true));
        assert_eq!(data.selected_zoom, None);
        assert_eq!(
            data.add_effect_at(EffectTrack::Zoom, 2.0, Some((100.0, 200.0))),
            Some(1)
        );
        assert_eq!(data.zoom_clips[0].mode, ZoomMode::Auto);
        assert_eq!(data.zoom_clips[1].mode, ZoomMode::Manual);
        assert_eq!(data.zoom_clips[1].center, (100.0, 200.0));
    }

    #[test]
    fn the_add_zoom_tool_hides_while_playing_and_needs_media() {
        let mut empty = Data::default();
        assert!(!empty.set_add_zoom_tool(true));
        let mut data = loaded();
        data.playing = true;
        assert!(data.set_add_zoom_tool(true));
        assert!(!data.playing, "arming pauses");
        data.playing = true;
        assert_eq!(data.add_zoom_tool(), None);
        assert!(data.add_zoom_tool);
        assert!(data.set_add_zoom_tool(false));
        assert!(!data.set_add_zoom_tool(false));
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

    /// A document with the camera track the recorder would have written, so
    /// the layout timeline has a model to live in. The returned paths are the
    /// scratch files the test has to remove.
    fn webcam_loaded(name: &str) -> (Data, Vec<PathBuf>) {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-timeline-layouts-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let video = directory.join("clip.mp4");
        std::fs::write(&video, b"footage").unwrap();

        let sidecar_path = PointerSidecar::sidecar_path(&video);
        let track_path = sidecar_path
            .parent()
            .map(|parent| parent.join(format!("camera-layout-{name}.mp4")))
            .unwrap_or_else(|| directory.join("camera-layout.mp4"));
        if let Some(parent) = track_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&track_path, b"track").unwrap();
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.webcam = Some(WebcamTrack {
            file: track_path
                .file_name()
                .map(|file| file.to_string_lossy().into_owned())
                .unwrap_or_default(),
            width: 640,
            height: 480,
            duration: 10.0,
            start: 0.0,
            x: 0.7,
            y: 0.55,
            w: 0.2,
            h: 0.25,
            shape: 0,
            flip: false,
        });
        sidecar.write(&sidecar_path).unwrap();

        let model = super::super::webcam::WebcamModel::new(Rc::new(|| {}));
        model.load(Some(&video), 10.0);
        let mut data = loaded();
        data.path = Some(video);
        data.webcam = Some(model);
        (data, vec![sidecar_path, track_path, directory])
    }

    fn saved_layouts(data: &Data) -> LayoutSettings {
        data.webcam_layouts()
            .expect("the camera track carries a layout timeline")
    }

    fn clean_up(paths: Vec<PathBuf>) {
        for path in paths {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    #[test]
    fn switching_the_camera_layer_off_keeps_every_layout_change() {
        let (mut data, scratch) = webcam_loaded("camera-off");
        assert!(data.add_layout_cue(4.0).is_some());
        assert!(data.set_webcam_editing_layout(CameraLayout::SplitScreen, None));
        let before = saved_layouts(&data);
        assert!(data.webcam_shown());

        assert!(data.set_webcam_enabled(false));
        assert!(!data.webcam_shown());
        assert_eq!(
            saved_layouts(&data),
            before,
            "the layout timeline survives the switch"
        );
        assert!(
            !data.set_webcam_enabled(false),
            "switching an off layer off is nothing to undo"
        );

        assert!(data.set_webcam_enabled(true));
        assert!(data.webcam_shown());
        clean_up(scratch);
    }

    #[test]
    fn choosing_a_camera_layout_switches_the_layer_back_on() {
        let (mut data, scratch) = webcam_loaded("camera-off-choice");
        assert!(data.set_webcam_enabled(false));

        assert!(data.set_webcam_editing_layout(CameraLayout::SplitScreen, None));
        assert!(data.webcam_shown(), "the chosen layout is the camera's");
        assert_eq!(
            saved_layouts(&data).default_layout,
            CameraLayout::SplitScreen
        );
        clean_up(scratch);
    }

    #[test]
    fn layout_changes_keep_only_the_moments_the_composition_can_reach() {
        let mut cues = vec![
            super::super::webcam::LayoutCue {
                at: 8.0,
                layout: CameraLayout::SplitScreen,
                position: None,
                cutout_x: None,
            },
            super::super::webcam::LayoutCue {
                at: 1.0,
                layout: CameraLayout::ScreenOnly,
                position: None,
                cutout_x: None,
            },
            super::super::webcam::LayoutCue {
                at: f64::NAN,
                layout: CameraLayout::CameraOverlay,
                position: None,
                cutout_x: None,
            },
            super::super::webcam::LayoutCue {
                at: 4.0,
                layout: CameraLayout::CameraOverlay,
                position: None,
                cutout_x: None,
            },
            super::super::webcam::LayoutCue {
                at: 4.004,
                layout: CameraLayout::FullscreenCamera,
                position: None,
                cutout_x: None,
            },
            super::super::webcam::LayoutCue {
                at: 40.0,
                layout: CameraLayout::SplitScreen,
                position: None,
                cutout_x: None,
            },
        ];
        normalize_layout_cues(&mut cues, 10.0);

        let times: Vec<f64> = cues.iter().map(|cue| cue.at).collect();
        assert_eq!(times, vec![1.0, 4.004, 8.0]);
        assert_eq!(cues[1].layout, CameraLayout::FullscreenCamera);

        normalize_layout_cues(&mut cues, 0.0);
        assert!(cues.is_empty());
    }

    #[test]
    fn a_layout_change_keeps_the_layout_in_force_where_it_was_added() {
        let (mut data, scratch) = webcam_loaded("add");
        assert!(data.update_layouts(
            |layouts| layouts.default_layout = CameraLayout::SplitScreen,
            false
        ));
        data.playhead = 5.0;

        let index = data.add_layout_cue(5.0).expect("the track takes a cue");
        assert_eq!(data.selected_layout, Some(index));
        let layouts = saved_layouts(&data);
        assert!((layouts.cues[index].at - 5.0).abs() < 1e-9);
        assert_eq!(layouts.cues[index].layout, CameraLayout::SplitScreen);

        assert_eq!(data.add_layout_cue(5.0), Some(index));
        assert_eq!(saved_layouts(&data).cues.len(), 1);
        clean_up(scratch);
    }

    #[test]
    fn a_cue_edit_touches_only_the_cue_it_was_given() {
        let (mut data, scratch) = webcam_loaded("target");
        let first = data.add_layout_cue(2.0).expect("the first cue");
        let second = data.add_layout_cue(6.0).expect("the second cue");
        assert_ne!(first, second);

        assert!(data.set_layout_cue_layout(second, CameraLayout::ScreenOnly));
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.cues[first].layout, CameraLayout::CameraOverlay);
        assert_eq!(layouts.cues[second].layout, CameraLayout::ScreenOnly);

        assert!(!data.set_layout_cue_layout(second, CameraLayout::ScreenOnly));
        clean_up(scratch);
    }

    #[test]
    fn selecting_a_layout_change_puts_the_playhead_on_it() {
        let (mut data, scratch) = webcam_loaded("seek");
        let index = data.add_layout_cue(3.5).expect("the cue");
        data.playhead = 0.0;
        data.selected_layout = None;

        assert!(data.select_layout_cue(index));
        assert!((data.playhead - 3.5).abs() < 1e-9);
        assert_eq!(data.selected_layout, Some(index));
        assert!(!data.select_layout_cue(99));
        clean_up(scratch);
    }

    #[test]
    fn adding_a_layout_change_expands_its_lane() {
        let (mut data, scratch) = webcam_loaded("lanes");
        assert!(data.add_layout_cue(2.0).is_some());
        assert!(data.lane_expanded(TimelineLane::Layout));
        clean_up(scratch);
    }

    #[test]
    fn a_layout_change_undoes_and_redoes() {
        let (mut data, scratch) = webcam_loaded("undo");
        let index = data.add_layout_cue(3.0).expect("the cue");
        assert!(data.set_layout_cue_layout(index, CameraLayout::FullscreenCamera));
        assert_eq!(
            saved_layouts(&data).cues[0].layout,
            CameraLayout::FullscreenCamera
        );

        assert!(data.undo_edit());
        assert_eq!(
            saved_layouts(&data).cues[0].layout,
            CameraLayout::CameraOverlay
        );
        assert!(data.undo_edit());
        assert!(saved_layouts(&data).cues.is_empty());
        assert_eq!(data.selected_layout, None);

        assert!(data.redo_edit());
        assert_eq!(saved_layouts(&data).cues.len(), 1);
        assert!(data.redo_edit());
        assert_eq!(
            saved_layouts(&data).cues[0].layout,
            CameraLayout::FullscreenCamera
        );
        clean_up(scratch);
    }

    #[test]
    fn a_layout_sweep_is_its_own_step_after_a_geometry_sweep() {
        let (mut data, scratch) = webcam_loaded("sweep");
        data.record_continuous_edit();
        data.trim_start = 1.0;
        assert!(data.update_layouts(|layouts| layouts.transition_seconds = 0.8, true));
        assert!(data.update_layouts(|layouts| layouts.transition_seconds = 1.0, true));
        assert!((saved_layouts(&data).transition_seconds - 1.0).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!((saved_layouts(&data).transition_seconds - 0.45).abs() < 1e-9);
        assert!((data.trim_start - 1.0).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!(data.trim_start.abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn a_coalesced_layout_report_restarts_the_burst_window() {
        let (mut data, scratch) = webcam_loaded("window");
        let before = Instant::now();
        data.layout_burst = Some(before - Duration::from_millis(300));

        assert!(data.update_layouts(|layouts| layouts.transition_seconds = 0.6, true));
        let refreshed = data.layout_burst.expect("the sweep stays open");
        assert!(
            refreshed >= before,
            "a coalesced report must restart the burst window"
        );
        clean_up(scratch);
    }

    #[test]
    fn a_long_layout_sweep_stays_one_undo_step() {
        let (mut data, scratch) = webcam_loaded("long-sweep");
        for step in 1..=6 {
            let value = 0.5 + step as f64 * 0.05;
            assert!(data.update_layouts(|layouts| layouts.transition_seconds = value, true));
            let opened = data.layout_burst.expect("the sweep stays open");
            data.layout_burst = Some(opened - Duration::from_millis(200));
        }
        assert!((saved_layouts(&data).transition_seconds - 0.8).abs() < 1e-9);

        assert!(data.undo_edit());
        assert!((saved_layouts(&data).transition_seconds - 0.45).abs() < 1e-9);
        assert!(!data.undo_edit());
        clean_up(scratch);
    }

    #[test]
    fn a_recording_without_a_camera_has_no_layout_timeline() {
        let mut data = loaded();
        assert!(data.webcam.is_none());
        assert!(data.webcam_layouts().is_none());
        assert_eq!(data.add_layout_cue(2.0), None);
        assert!(!data.update_layouts(|layouts| layouts.transition_seconds = 1.0, false));
        assert!(!data.update_layouts(|layouts| layouts.transition_seconds = 1.0, true));
        assert!(!data.remove_layout_cue(0));
        assert!(!data.select_layout_cue(0));
    }

    #[test]
    fn a_layout_only_edit_is_saved_with_the_placement_it_did_not_touch() {
        let (mut data, scratch) = webcam_loaded("saved");
        let background = Background::default();
        assert!(data.project_file(&background).is_none());

        assert!(data.update_layouts(|layouts| layouts.transition_seconds = 0.8, false));
        let file = data
            .project_file(&background)
            .expect("a layout edit is saved");
        let webcam = file.webcam.expect("the camera block is written");
        let group = webcam.layouts.expect("the layout timeline is written");
        assert!((group.transition_seconds - 0.8).abs() < 1e-9);
        assert!((webcam.x - 0.7).abs() < 1e-9);
        assert!((webcam.y - 0.55).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn trimming_drops_layout_changes_it_can_no_longer_reach() {
        let (mut data, scratch) = webcam_loaded("trim");
        data.add_layout_cue(2.0);
        data.add_layout_cue(8.0);
        assert_eq!(data.selected_layout, Some(1));

        data.set_visible_length(5.0);
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.cues.len(), 1);
        assert!((layouts.cues[0].at - 2.0).abs() < 1e-9);
        assert_eq!(data.selected_layout, None);

        assert!(data.undo_edit());
        assert_eq!(saved_layouts(&data).cues.len(), 2);
        clean_up(scratch);
    }

    #[test]
    fn a_legacy_project_restores_the_layout_its_switches_describe() {
        let (mut data, scratch) = webcam_loaded("legacy");
        data.apply_project(&project::EditorProjectFile {
            version: project::PROJECT_VERSION,
            source_path: PathBuf::from("clip.mp4"),
            source_size: 0,
            source_mtime_secs: 0,
            trim_start_seconds: 0.0,
            trim_end_seconds: 10.0,
            cuts: Vec::new(),
            segment_starts: Vec::new(),
            segment_speeds: Vec::new(),
            typing_suggestions_resolved: Vec::new(),
            freeze_tail: 0.0,
            zoom_clips: Vec::new(),
            zoom_camera: project::ZoomCameraFile::default(),
            cursor_hide_clips: Vec::new(),
            webcam: Some(project::WebcamLayerFile {
                enabled: false,
                fullscreen: false,
                x: 0.6,
                y: 0.4,
                w: 0.25,
                h: 0.25,
                radius: 0.5,
                mirror: true,
                delay_seconds: 0.0,
                zoomed_scale: super::super::webcam::ZOOMED_CAMERA_SCALE,
                hide_during_silence: None,
                layouts: None,
            }),
            background: project::StageFile::default(),
            background_padding: DEFAULT_PADDING,
            background_radius: DEFAULT_RADIUS,
            playhead_seconds: 0.0,
            timeline_scale: 0.0,
            timeline_scroll_seconds: 0.0,
            cursor_click_sound: None,
            cursor_click_sound_volume: 0.25,
            captions_shortcuts: false,
            captions_show_single_letters: false,
            cursor: Default::default(),
        });

        let layouts = saved_layouts(&data);
        assert_eq!(layouts.default_layout, CameraLayout::ScreenOnly);
        assert!(layouts.cues.is_empty());
        let layer = data
            .webcam
            .as_ref()
            .and_then(|model| model.settings())
            .expect("the track still has a layer");
        assert!(layer.enabled);
        assert!(!layer.fullscreen);
        assert!(layer.mirror);
        assert!((layer.x - 0.6).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn layout_chips_follow_zoom_and_scroll_and_stay_inside_the_view() {
        let (mut data, scratch) = webcam_loaded("chips");
        for at in [2.0, 5.0, 9.0] {
            data.add_layout_cue(at).expect("the track takes a cue");
        }
        let width = 1000.0;
        let (first_start, first_end) =
            layout_chip_bounds(&data, width, 0, 1.0).expect("the first chip");
        assert!(
            first_end < data.time_to_x(5.0, width),
            "the chip stops short of the change after it"
        );
        assert!(first_end - first_start > 200.0, "and owns its own span");
        assert_eq!(
            layout_cue_at(&data, width, first_start + 20.0, 1.0),
            Some(0)
        );
        assert_eq!(layout_cue_at(&data, width, first_start + 9.0, 1.0), Some(0));
        assert_eq!(
            layout_cue_at(&data, width, 400.0, 1.0),
            Some(0),
            "a press anywhere in a change's span picks it up"
        );
        assert_eq!(layout_cue_at(&data, width, 500.0, 1.0), Some(1));
        let cue_tooltip =
            layout_lane_tooltip(&data, width, 200.0, 1.0).expect("a cue has a tooltip");
        assert!(cue_tooltip.contains(format_layout_clock(2.0).as_str()));
        assert!(cue_tooltip.contains(t("Overlay").as_str()));
        let default_tooltip =
            layout_lane_tooltip(&data, width, 50.0, 1.0).expect("default has a tooltip");
        assert!(default_tooltip.contains(t("Default layout").as_str()));
        assert!(
            layout_lane_tooltip(&data, width, 400.0, 1.0)
                .is_some_and(|tooltip| tooltip.contains(format_layout_clock(2.0).as_str())),
            "the span belongs to the change that starts it"
        );

        data.scale = 100.0;
        data.scroll = 4.0;
        assert_eq!(layout_cue_at(&data, width, 800.0, 1.0), Some(1));
        assert_eq!(
            layout_cue_at(&data, width, 0.0, 1.0),
            Some(0),
            "scrolled into the first change's own span"
        );
        // The last change owns its span all the way to the end of the
        // composition, so only the space past the footage itself is empty.
        data.scroll = 9.0;
        let end_of_footage = data.time_to_x(data.total_duration(), width);
        let past_the_footage = (end_of_footage + 100.0).min(width - 1.0);
        assert_eq!(
            layout_cue_at(&data, width, past_the_footage, 1.0),
            None,
            "past the end of the composition no change owns the row"
        );
        data.scroll = 4.0;

        data.scroll = 5.0;
        assert_eq!(layout_cue_at(&data, width, 0.0, 1.0), Some(1));
        data.scroll = 3.75;
        assert_eq!(layout_cue_at(&data, width, width, 1.0), Some(1));
        assert_eq!(layout_cue_at(&data, width, width + 0.1, 1.0), None);
        assert_eq!(layout_cue_at(&data, width, -0.1, 1.0), None);
        assert_eq!(layout_cue_at(&data, width, f64::NAN, 1.0), None);
        clean_up(scratch);
    }

    #[test]
    fn selecting_a_layout_chip_pauses_and_announces_every_click() {
        let (mut data, scratch) = webcam_loaded("epoch");
        let index = data.add_layout_cue(3.5).expect("the cue");
        let after_add = data.layout_selection_epoch;
        data.selected_layout = None;
        data.playhead = 0.0;
        data.playing = true;

        assert!(data.select_layout_cue(index));
        assert!(!data.playing);
        assert_eq!(data.selected_layout, Some(index));
        assert!((data.playhead - 3.5).abs() < 1e-9);
        assert_eq!(data.layout_selection_epoch, after_add + 1);

        data.playing = true;
        assert!(data.select_layout_cue(index));
        assert!(!data.playing);
        assert_eq!(data.layout_selection_epoch, after_add + 2);

        let before_add = data.layout_selection_epoch;
        assert_eq!(data.add_layout_cue(3.5), Some(index));
        assert_eq!(data.layout_selection_epoch, before_add + 1);

        data.playhead = 1.0;
        data.playing = true;
        assert!(data.clear_layout_cue_selection());
        assert_eq!(data.selected_layout, None);
        assert!((data.playhead - 1.0).abs() < 1e-9);
        assert!(data.playing);
        assert!(!data.clear_layout_cue_selection());
        assert!(data.undo_edit());
        assert!(saved_layouts(&data).cues.is_empty());
        clean_up(scratch);
    }

    /// A timeline over a recording with a camera track whose layout timeline
    /// has two cues, built on the GTK thread. The stage is the bundled one.
    fn timeline_with_camera_cues(name: &str) -> (Timeline, Background, Vec<PathBuf>) {
        let (data, paths) = webcam_loaded(name);
        let video = data.path.clone().expect("a recording");
        let webcam = data.webcam.clone().expect("a camera model");
        webcam.set_layouts(LayoutSettings {
            cues: vec![
                super::super::webcam::LayoutCue {
                    at: 1.5,
                    layout: CameraLayout::FullscreenCamera,
                    position: None,
                    cutout_x: None,
                },
                super::super::webcam::LayoutCue {
                    at: 4.25,
                    layout: CameraLayout::CameraOverlay,
                    position: None,
                    cutout_x: None,
                },
            ],
            ..LayoutSettings::default()
        });

        let background = Background::default();
        static NEXT_APP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let app_number = NEXT_APP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let app = gtk4::Application::builder()
            .application_id(format!("com.apexshot.Tests.PresetApply{app_number}"))
            .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gtk4::gio::Cancellable>).unwrap();
        let window = gtk4::ApplicationWindow::new(&app);
        let canvas = gtk4::AspectFrame::new(0.5, 0.5, 1.0, false);
        let timeline = build(
            &window,
            &canvas,
            1.0,
            false,
            super::super::cursor::CursorModel::new(),
            webcam,
            background.clone(),
            Rc::new(Cell::new(None)),
        );
        {
            let mut document = timeline.data.borrow_mut();
            document.path = Some(video);
            document.duration = 10.0;
            document.trim_end = 10.0;
            document.frame_size = Some((1920, 1080));
        }
        (timeline, background, paths)
    }

    /// The stage of a look captured from the open project: a wallpaper, an
    /// inset and a radius that differ from the bundled stage.
    fn captured_stage_look(
        timeline: &Timeline,
        background: &Background,
    ) -> super::super::presets::Preset {
        background.apply(Some("wallpaper-019.jpg"), 66.0, 9.0);
        let file = timeline
            .data
            .borrow()
            .project_file(background)
            .expect("an edited recording has a project");
        super::super::presets::from_project(&file, CursorSettings::default(), "Look")
            .expect("the look captures")
    }

    #[test]
    fn applying_a_preset_keeps_the_layout_cues_of_the_recording() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (timeline, background, paths) = timeline_with_camera_cues("preset-cues");
            let look = captured_stage_look(&timeline, &background);
            background.apply(Some("wallpaper-004.jpg"), 40.0, 5.0);

            timeline.apply_preset(&look);

            assert_eq!(
                background.snapshot(),
                (Some("wallpaper-019.jpg".to_owned()), 66.0, 9.0)
            );
            let cues: Vec<f64> = timeline
                .webcam_layouts()
                .expect("the camera keeps its layout timeline")
                .cues
                .iter()
                .map(|cue| cue.at)
                .collect();
            assert_eq!(cues, [1.5, 4.25]);
            clean_up(paths);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn undoing_a_preset_puts_the_stage_back_and_redo_applies_it_again() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (timeline, background, paths) = timeline_with_camera_cues("preset-undo");
            let look = captured_stage_look(&timeline, &background);
            background.apply(Some("wallpaper-004.jpg"), 40.0, 5.0);
            timeline.apply_preset(&look);

            assert!(timeline.undo());
            assert_eq!(
                background.snapshot(),
                (Some("wallpaper-004.jpg".to_owned()), 40.0, 5.0)
            );
            assert_eq!(saved_layouts(&timeline.data.borrow()).cues.len(), 2);

            assert!(timeline.redo());
            assert_eq!(
                background.snapshot(),
                (Some("wallpaper-019.jpg".to_owned()), 66.0, 9.0)
            );
            clean_up(paths);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn an_edit_made_after_a_preset_undoes_without_moving_the_stage() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (timeline, background, paths) = timeline_with_camera_cues("preset-edit");
            let look = captured_stage_look(&timeline, &background);
            background.apply(Some("wallpaper-004.jpg"), 40.0, 5.0);
            timeline.apply_preset(&look);
            timeline
                .data
                .borrow_mut()
                .add_effect(EffectTrack::Zoom, 2.0)
                .expect("a zoom lands on the recording");

            assert!(timeline.undo());
            assert!(timeline.data.borrow().zoom_clips.is_empty());
            assert_eq!(
                background.snapshot(),
                (Some("wallpaper-019.jpg".to_owned()), 66.0, 9.0),
                "undoing the zoom leaves the preset's stage where it is"
            );

            assert!(timeline.undo());
            assert_eq!(
                background.snapshot(),
                (Some("wallpaper-004.jpg".to_owned()), 40.0, 5.0),
                "the preset's own step puts the stage back"
            );
            clean_up(paths);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn a_layout_preset_edits_the_cue_and_global_shape_as_one_step() {
        let (mut data, scratch) = webcam_loaded("preset");
        let before = data
            .webcam
            .as_ref()
            .and_then(|model| model.settings())
            .expect("the track has a layer");
        let cue = data.add_layout_cue(2.0).expect("the cue");
        assert_eq!(data.selected_layout, Some(cue));

        assert!(data.set_webcam_editing_layout(CameraLayout::SplitScreen, Some(2)));
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.default_layout, CameraLayout::CameraOverlay);
        assert_eq!(layouts.cues[cue].layout, CameraLayout::SplitScreen);
        let after = data
            .webcam
            .as_ref()
            .and_then(|model| model.settings())
            .expect("the track has a layer");
        assert!((after.radius - super::super::webcam::radius_for_shape(2)).abs() < 1e-9);
        let expected_height =
            after.w * data.frame_aspect() * super::super::webcam::aspect_for_shape(2);
        assert!((after.h - expected_height).abs() < 1e-9);
        assert!(
            (after.overlay_size_ratio(data.frame_aspect())
                - before.overlay_size_ratio(data.frame_aspect()))
            .abs()
                < 1e-9
        );
        let (before_x, before_y) = before.overlay_alignment();
        let (after_x, after_y) = after.overlay_alignment();
        assert!((after_x - before_x).abs() < 1e-9);
        assert!(
            (after_y - before_y).abs() < 1e-9,
            "the taller shape keeps the layer's alignment instead of drifting"
        );
        assert_eq!(after.mirror, before.mirror);

        assert!(data.undo_edit());
        assert_eq!(
            saved_layouts(&data).cues[cue].layout,
            CameraLayout::CameraOverlay
        );
        let restored = data
            .webcam
            .as_ref()
            .and_then(|model| model.settings())
            .expect("the track has a layer");
        assert!((restored.radius - before.radius).abs() < 1e-9);
        assert!((restored.h - before.h).abs() < 1e-9);
        assert!((restored.w - before.w).abs() < 1e-9);
        assert!((restored.x - before.x).abs() < 1e-9);
        assert!((restored.y - before.y).abs() < 1e-9);
        assert_eq!(restored.mirror, before.mirror);

        assert!(data.redo_edit());
        assert_eq!(
            saved_layouts(&data).cues[cue].layout,
            CameraLayout::SplitScreen
        );
        assert!(data.undo_edit());
        assert!(data.undo_edit());
        assert!(saved_layouts(&data).cues.is_empty());
        clean_up(scratch);
    }

    #[test]
    fn a_layout_preset_targets_the_selection_or_the_default() {
        let (mut data, scratch) = webcam_loaded("scope");
        let cue = data.add_layout_cue(2.0).expect("the cue");

        assert!(data.set_webcam_editing_layout(CameraLayout::ScreenOnly, None));
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.default_layout, CameraLayout::CameraOverlay);
        assert_eq!(layouts.cues[cue].layout, CameraLayout::ScreenOnly);

        data.selected_layout = Some(99);
        assert!(data.set_webcam_editing_layout(CameraLayout::SplitScreen, None));
        assert_eq!(data.selected_layout, None);
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.default_layout, CameraLayout::SplitScreen);
        assert_eq!(layouts.cues[cue].layout, CameraLayout::ScreenOnly);

        assert!(!data.set_webcam_editing_layout(CameraLayout::SplitScreen, None));
        assert!(data.undo_edit());
        let layouts = saved_layouts(&data);
        assert_eq!(layouts.default_layout, CameraLayout::CameraOverlay);
        assert_eq!(layouts.cues[cue].layout, CameraLayout::ScreenOnly);
        assert!(data.undo_edit());
        assert_eq!(
            saved_layouts(&data).cues[cue].layout,
            CameraLayout::CameraOverlay
        );
        clean_up(scratch);
    }

    #[test]
    fn a_circle_preset_follows_the_frame_aspect_and_fits_the_layer_bounds() {
        let (mut data, scratch) = webcam_loaded("shape");
        data.selected_layout = None;
        assert!(data.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(0)));
        let layer = data
            .webcam
            .as_ref()
            .and_then(|model| model.settings())
            .expect("the track has a layer");
        assert!((layer.radius - 1.0).abs() < 1e-9);
        assert!((layer.h - layer.w * data.frame_aspect()).abs() < 1e-9);

        let model = data.webcam.clone().expect("the track has a model");
        let mut settings = model.settings().expect("the track has a layer");
        settings.w = 0.9;
        settings.h = 0.5;
        model.set_settings(settings);
        assert!(data.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(0)));
        let layer = model.settings().expect("the track has a layer");
        let max = super::super::webcam::MAX_SIZE;
        assert!(layer.w <= max + 1e-9);
        assert!(layer.h <= max + 1e-9);
        assert!((layer.h - layer.w * data.frame_aspect()).abs() < 1e-9);
        assert!((layer.h - max).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn a_shape_preset_keeps_the_alignment_the_layer_had() {
        let (mut data, scratch) = webcam_loaded("anchored");
        let model = data.webcam.clone().expect("the track has a model");
        let mut corner = model.settings().expect("the track has a layer");
        corner.w = 0.2;
        corner.h = 0.25;
        corner.x = 0.8;
        corner.y = 0.75;
        model.set_settings(corner);
        let corner = model.settings().expect("the track has a layer");
        assert_eq!(corner.overlay_alignment(), (1.0, 1.0));

        assert!(data.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(2)));
        let shaped = model.settings().expect("the track has a layer");
        let (x, y) = shaped.overlay_alignment();
        assert!((x - 1.0).abs() < 1e-9, "the shape stays on the right edge");
        assert!((y - 1.0).abs() < 1e-9, "the shape stays on the bottom edge");
        assert!((shaped.x + shaped.w - 1.0).abs() < 1e-9);
        assert!((shaped.y + shaped.h - 1.0).abs() < 1e-9);
        assert!(
            (shaped.overlay_size_ratio(data.frame_aspect())
                - corner.overlay_size_ratio(data.frame_aspect()))
            .abs()
                < 1e-9
        );

        let mut centred = shaped;
        centred.set_overlay_alignment(0.5, 0.5);
        model.set_settings(centred);
        assert!(data.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(3)));
        let shaped = model.settings().expect("the track has a layer");
        let (x, y) = shaped.overlay_alignment();
        assert!((x - 0.5).abs() < 1e-9, "the shape stays centred across");
        assert!((y - 0.5).abs() < 1e-9, "the shape stays centred down");
        assert!((shaped.x + shaped.w / 2.0 - 0.5).abs() < 1e-9);
        assert!((shaped.y + shaped.h / 2.0 - 0.5).abs() < 1e-9);

        let mut wide = shaped;
        wide.w = 0.9;
        wide.h = 0.9;
        wide.x = 0.1;
        wide.y = 0.1;
        model.set_settings(wide);
        assert!(data.set_webcam_editing_layout(CameraLayout::CameraOverlay, Some(0)));
        let fitted = model.settings().expect("the track has a layer");
        let max = super::super::webcam::MAX_SIZE;
        let min = super::super::webcam::MIN_SIZE;
        assert!(fitted.w <= max + 1e-9 && fitted.h <= max + 1e-9);
        assert!(fitted.w >= min && fitted.h >= min);
        assert!((fitted.h - fitted.w * data.frame_aspect()).abs() < 1e-9);
        assert!((fitted.overlay_alignment().0 - 1.0).abs() < 1e-9);
        assert!((fitted.overlay_alignment().1 - 1.0).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn a_cutout_placement_undoes_and_redoes_with_the_project() {
        let (mut data, scratch) = webcam_loaded("cutout-position");
        let background = Background::default();
        assert!((saved_layouts(&data).cutout_position_x - 0.5).abs() < 1e-9);

        assert!(data.update_layouts(|layouts| layouts.cutout_position_x = 0.2, false));
        assert!((saved_layouts(&data).cutout_position_x - 0.2).abs() < 1e-9);
        let saved = data
            .project_file(&background)
            .expect("the layout edit is saved");
        assert!(
            (saved
                .webcam
                .expect("the camera block is written")
                .layouts
                .expect("the layout timeline is written")
                .cutout_position_x
                - 0.2)
                .abs()
                < 1e-9
        );

        assert!(data.undo_edit());
        assert!((saved_layouts(&data).cutout_position_x - 0.5).abs() < 1e-9);
        assert!(data.redo_edit());
        assert!((saved_layouts(&data).cutout_position_x - 0.2).abs() < 1e-9);

        assert!(data.update_layouts(|layouts| layouts.cutout_position_x = 4.0, false));
        assert!((saved_layouts(&data).cutout_position_x - 1.0).abs() < 1e-9);
        assert!(!data.update_layouts(|layouts| layouts.cutout_position_x = 1.5, false));
        clean_up(scratch);
    }

    #[test]
    fn moving_a_layout_change_keeps_the_order_and_refuses_collisions() {
        let (mut data, scratch) = webcam_loaded("move");
        let first = data.add_layout_cue(2.0).expect("the first cue");
        let second = data.add_layout_cue(5.0).expect("the second cue");
        data.selected_layout = Some(first);
        data.playing = true;

        assert!(data.set_layout_cue_time(first, 8.0));
        let layouts = saved_layouts(&data);
        let times: Vec<f64> = layouts.cues.iter().map(|cue| cue.at).collect();
        assert_eq!(times, vec![5.0, 8.0]);
        assert_eq!(data.selected_layout, Some(1));
        assert!(!data.playing);
        assert!((data.playhead - 8.0).abs() < 1e-9);

        let moved = data.selected_layout.expect("the moved cue stays selected");
        assert!(!data.set_layout_cue_time(moved, 5.0));
        assert!(!data.set_layout_cue_time(moved, 5.004));
        assert!(!data.set_layout_cue_time(moved, f64::NAN));
        assert!(!data.set_layout_cue_time(moved, 8.0));
        assert!(data.undo_edit());
        let layouts = saved_layouts(&data);
        let times: Vec<f64> = layouts.cues.iter().map(|cue| cue.at).collect();
        assert_eq!(times, vec![2.0, 5.0]);
        assert_eq!(data.selected_layout, Some(first));

        assert!(data.set_layout_cue_time(second, 99.0));
        let layouts = saved_layouts(&data);
        let times: Vec<f64> = layouts.cues.iter().map(|cue| cue.at).collect();
        assert_eq!(times, vec![2.0, 10.0]);
        assert_eq!(data.selected_layout, Some(first));

        data.selected_layout = Some(1);
        assert!(data.set_layout_cue_time(1, 1.0));
        let layouts = saved_layouts(&data);
        let times: Vec<f64> = layouts.cues.iter().map(|cue| cue.at).collect();
        assert_eq!(times, vec![1.0, 2.0]);
        assert_eq!(data.selected_layout, Some(0));
        assert!((data.playhead - 1.0).abs() < 1e-9);

        assert!(data.set_layout_cue_time(0, -4.0));
        let layouts = saved_layouts(&data);
        assert!((layouts.cues[0].at - 0.0).abs() < 1e-9);
        assert_eq!(data.selected_layout, Some(0));
        clean_up(scratch);
    }

    #[test]
    fn both_effect_lanes_stay_open_without_clips() {
        // The lane flags drive the drawing areas, so both true means both drop
        // targets are on screen from the start.
        let data = loaded();
        let state = data.lane_state();
        assert!(state.zoom);
        assert!(state.hide);
        assert!(!state.camera);
    }

    #[test]
    fn a_clip_still_draws_in_its_lane() {
        for track in [EffectTrack::Zoom, EffectTrack::Hide] {
            let [_, _, _, alpha] = pill_pixel(track);
            assert_eq!(alpha, 255, "the pill covers the lane pixel");
        }
    }

    #[test]
    fn the_camera_row_appears_only_with_a_camera_track() {
        let data = loaded();
        assert!(!data.lane_state().camera);
        let (data, scratch) = webcam_loaded("lane-camera");
        assert!(data.lane_state().camera);
        clean_up(scratch);
    }

    #[test]
    fn the_lanes_share_the_rulers_origin() {
        let (mut data, scratch) = webcam_loaded("lanes");
        for at in [1.0, 4.0, 7.0] {
            data.add_layout_cue(at).expect("the track takes a cue");
        }
        assert!(data.add_effect(EffectTrack::Zoom, 0.0).is_some());
        // What a lane actually gets: the full width of the content column.
        let width = 900.0;

        for (scale, scroll) in [(0.0, 0.0), (40.0, 0.5), (80.0, 0.5)] {
            data.scale = scale;
            data.scroll = scroll;
            let origin = data.time_to_x(0.0, width);
            assert!(
                (data.time_to_x(data.segment_start(0), width) - origin).abs() < 1e-9,
                "the filmstrip starts on the ruler's origin"
            );
            let effect_start = data
                .effect_span(EffectTrack::Zoom, 0)
                .expect("the zoom pill is there")
                .0;
            assert!(
                (data.time_to_x(effect_start, width) - origin).abs() < 1e-9,
                "the pill lane uses the same origin"
            );
            assert!((data.x_to_time(origin, width) - 0.0).abs() < 1e-9);

            let layouts = saved_layouts(&data);
            for (index, _cue) in layouts.cues.iter().enumerate() {
                let Some((start, end)) = layout_chip_bounds(&data, width, index, 1.0) else {
                    continue;
                };
                let visible_start = start.max(0.0);
                let visible_end = end.min(width);
                if visible_end > visible_start {
                    let probe = (visible_start + visible_end) / 2.0;
                    assert_eq!(
                        layout_cue_at(&data, width, probe, 1.0),
                        Some(index),
                        "the camera chip uses the lane's own coordinate"
                    );
                }
            }

            // The playhead and the first cue chip share that one coordinate.
            data.playhead = 1.0;
            let playhead_x = data.time_to_x(data.playhead, width);
            let (chip_start, chip_end) =
                layout_chip_bounds(&data, width, 0, 1.0).expect("the first cue is there");
            let reach = LAYOUT_CHIP_HIT;
            if playhead_x >= chip_start - reach && playhead_x <= chip_end.max(chip_start + reach) {
                assert_eq!(layout_cue_at(&data, width, playhead_x, 1.0), Some(0));
            }
        }
        clean_up(scratch);
    }

    #[test]
    fn a_change_moves_only_between_its_neighbours() {
        let (mut data, scratch) = webcam_loaded("cue-limits");
        for at in [2.0, 5.0, 8.0] {
            data.add_layout_cue(at).expect("the track takes a cue");
        }

        let (low, high) = data.layout_cue_limits(1).expect("the middle change moves");
        assert!((low - (2.0 + LAYOUT_CUE_EPSILON)).abs() < 1e-9);
        assert!((high - (8.0 - LAYOUT_CUE_EPSILON)).abs() < 1e-9);

        assert!(data.move_layout_cue(1, 4.0, false));
        assert!((saved_layouts(&data).cues[1].at - 4.0).abs() < 1e-9);
        assert!(
            (data.layout_cue_limits(0).expect("the first change moves").1
                - (4.0 - LAYOUT_CUE_EPSILON))
                .abs()
                < 1e-9,
            "the change before it shrank to meet it"
        );
        clean_up(scratch);
    }

    #[test]
    fn the_last_change_stops_at_the_end_of_the_composition() {
        let (mut data, scratch) = webcam_loaded("cue-end");
        data.add_layout_cue(2.0).expect("the first cue");
        data.add_layout_cue(9.0).expect("the last cue");

        let (low, high) = data.layout_cue_limits(1).expect("the last change moves");
        assert!(low > 2.0);
        assert!((high - data.total_duration()).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn a_change_owns_the_row_until_the_next_one() {
        let (mut data, scratch) = webcam_loaded("tiling");
        data.add_layout_cue(2.0).expect("the first cue");
        data.add_layout_cue(5.0).expect("the second cue");
        let width = 1000.0;
        let (first_start, first_end) =
            layout_chip_bounds(&data, width, 0, 1.0).expect("the first chip");
        let (second_start, second_end) =
            layout_chip_bounds(&data, width, 1, 1.0).expect("the second chip");
        assert!((first_start - data.time_to_x(2.0, width)).abs() < 1e-9);
        assert!(
            (first_end - (data.time_to_x(5.0, width) - LAYOUT_CHIP_GAP)).abs() < 1e-9,
            "the chip runs to the seam before the next change"
        );
        assert!((second_start - data.time_to_x(5.0, width)).abs() < 1e-9);
        assert!(
            (second_end - (width - LAYOUT_CHIP_GAP)).abs() < 1e-9,
            "the last chip runs to the end of the composition"
        );
        clean_up(scratch);
    }

    #[test]
    fn adjacent_cues_keep_their_chips_apart() {
        let (mut data, scratch) = webcam_loaded("adjacent-chips");
        data.add_layout_cue(1.0).expect("the first cue");
        data.add_layout_cue(1.4).expect("the second cue");
        let width = 1000.0;
        let (first_start, first_end) =
            layout_chip_bounds(&data, width, 0, 1.0).expect("the first chip");
        let (second_start, _) = layout_chip_bounds(&data, width, 1, 1.0).expect("the second chip");
        assert!(first_end > first_start);
        assert!(
            first_end <= second_start - LAYOUT_CHIP_GAP + 1e-9,
            "adjacent chips leave their seam"
        );
        clean_up(scratch);
    }

    #[test]
    fn only_the_selected_chip_uses_the_lane_colour() {
        // The camera lane used to borrow the theme accent, so its selected chip
        // matched the selection colour of every other surface and sat close to
        // the Zoom lane's blue. The lane carries its own teal now: the selected
        // chip is the saturated end of it, and a placed change keeps the hue at
        // a depth that cannot be mistaken for selected.
        let selected_dark = layout_chip_fill(true, true);
        let selected_light = layout_chip_fill(true, false);
        assert_eq!(selected_dark, (0.15, 0.46, 0.41, 1.0));
        assert_eq!(selected_light, (0.10, 0.40, 0.35, 1.0));
        assert_eq!(selected_dark.3, 1.0, "the fill is solid");
        assert_eq!(selected_light.3, 1.0, "the fill is solid");

        let unselected_dark = layout_chip_fill(false, true);
        let unselected_light = layout_chip_fill(false, false);
        assert_eq!(unselected_dark, (0.13, 0.28, 0.26, 1.0));
        assert_eq!(unselected_light, (0.80, 0.89, 0.87, 1.0));
        assert_ne!(
            unselected_dark, selected_dark,
            "the selected chip is a deeper teal than its neighbours"
        );
        assert_ne!(
            unselected_light, selected_light,
            "selection reads on the light theme too"
        );

        let accent = (1.0, 0.404, 0.051, 1.0);
        let zoom = (0.0, 0.0, 1.0, 1.0);
        let hide = (0.4, 0.0, 0.2, 1.0);
        for (theme, selected) in [("dark", selected_dark), ("light", selected_light)] {
            assert_ne!(
                selected, accent,
                "the accent orange is not the camera lane's colour ({theme})"
            );
            assert_ne!(
                selected, zoom,
                "the camera lane stays clear of the Zoom lane's blue ({theme})"
            );
            assert_ne!(
                selected, hide,
                "the camera lane stays clear of the Hide lane's red ({theme})"
            );
        }
    }

    #[test]
    fn a_click_on_a_narrow_chip_body_selects_its_cue() {
        let (mut data, scratch) = webcam_loaded("narrow-chip");
        data.add_layout_cue(1.0).expect("the first cue");
        data.add_layout_cue(1.4).expect("the second cue");
        let width = 1000.0;
        let (start, end) = layout_chip_bounds(&data, width, 0, 1.0).expect("the first chip");
        assert!(
            end - start < 60.0,
            "a change a fraction of a second long barely has a chip"
        );
        let probe = start + (end - start) / 2.0;
        assert_eq!(layout_cue_at(&data, width, probe, 1.0), Some(0));
        clean_up(scratch);
    }

    #[test]
    fn editing_a_change_from_the_panel_brings_the_playhead_into_it() {
        let (mut data, scratch) = webcam_loaded("show");
        let first = data.add_layout_cue(2.0).expect("the first cue");
        data.add_layout_cue(6.0).expect("the second cue");
        data.playhead = 1.0;
        assert!(data.show_layout_cue(first));
        assert!(
            (data.playhead - 4.0).abs() < 1e-9,
            "the middle of 2..6: {}",
            data.playhead
        );
        data.playhead = 3.0;
        assert!(!data.show_layout_cue(first), "already inside, it stays");
        assert!((data.playhead - 3.0).abs() < 1e-9);
        clean_up(scratch);
    }

    #[test]
    fn grabbing_a_layout_change_on_its_lane_leaves_the_playhead() {
        let (mut data, scratch) = webcam_loaded("grab");
        let index = data.add_layout_cue(2.0).expect("the cue");
        data.playhead = 5.0;
        assert!(data.select_layout_cue_in_place(index));
        assert_eq!(data.selected_layout, Some(index));
        assert!((data.playhead - 5.0).abs() < 1e-9, "the playhead stays put");
        assert!(data.move_layout_cue(index, 3.0, true));
        assert!(
            (data.playhead - 5.0).abs() < 1e-9,
            "moving the change leaves it too"
        );
        clean_up(scratch);
    }

    #[test]
    fn one_lane_is_expanded_and_a_selection_brings_its_lane_forward() {
        let mut data = Data::default();
        assert!(
            data.lane_expanded(TimelineLane::Zoom),
            "zoom starts expanded"
        );
        assert!(!data.lane_expanded(TimelineLane::Hide));
        assert!(data.expand_lane(TimelineLane::Hide));
        assert!(!data.expand_lane(TimelineLane::Hide), "already expanded");
        assert!(data.lane_expanded(TimelineLane::Hide) && !data.lane_expanded(TimelineLane::Zoom));
        data.select_effect(EffectTrack::Zoom, Some(0));
        assert!(
            data.lane_expanded(TimelineLane::Zoom),
            "selecting a zoom expands its lane"
        );
        assert_eq!(data.lane_state().expanded, TimelineLane::Zoom);
    }

    #[test]
    fn the_lanes_keep_one_total_height_while_they_trade() {
        let mut data = Data::default();
        let resting: i32 = lane_heights(data.lane_progress, [true; 3], 1.25)
            .iter()
            .sum();
        data.expand_lane(TimelineLane::Hide);
        for frame in 0..40 {
            if frame == 6 {
                // A second switch mid-move keeps three lanes moving at once.
                data.expand_lane(TimelineLane::Layout);
            }
            data.step_lane_progress(0.016);
            let total: i32 = lane_heights(data.lane_progress, [true; 3], 1.25)
                .iter()
                .sum();
            assert_eq!(total, resting, "frame {frame}: {:?}", data.lane_progress);
        }
    }

    #[test]
    fn a_lane_eases_to_its_new_height_and_settles() {
        let mut data = Data::default();
        assert_eq!(data.lane_progress, [1.0, 0.0, 0.0]);
        data.expand_lane(TimelineLane::Hide);
        assert!(data.step_lane_progress(0.016), "a change starts moving");
        let zoom = data.lane_progress(TimelineLane::Zoom);
        let hide = data.lane_progress(TimelineLane::Hide);
        assert!(zoom < 1.0 && zoom > 0.5, "zoom shrinks gradually: {zoom}");
        assert!(hide > 0.0 && hide < 0.5, "hide grows gradually: {hide}");
        let mut steps = 0;
        while data.step_lane_progress(0.016) {
            steps += 1;
            assert!(steps < 100, "it settles");
        }
        assert_eq!(data.lane_progress, [0.0, 1.0, 0.0]);
        assert!(steps > 5, "it takes several frames, not a jump: {steps}");
        assert_eq!(lane_height(0.0, 1.0), THIN_LANE_HEIGHT as i32);
        assert_eq!(lane_height(1.0, 1.0), EFFECT_LANE_HEIGHT as i32);
    }

    #[test]
    fn the_scoped_stylesheet_keeps_the_timeline_compact() {
        let css = timeline_css(1.25);
        assert!(css.contains("recording-editor-card-layout-track"));
        assert!(
            css.contains("min-height: 22px"),
            "a resting lane's 18px floor rounds down to 22.5 -> 22"
        );
        assert!(css.contains("min-height: 53px"), "the filmstrip is 42px");
        assert!(css.contains("min-height: 193px"), "the board fits its rows");
        assert!(!css.contains("min-height: 0"));
        // The lane rail is gone, so the sheet carries no gutter rules at all.
        assert!(!css.contains("recording-editor-timeline-gutter"));
        for rejected in ["max-height", "max-width", "overflow"] {
            assert!(
                !css.contains(rejected),
                "{rejected} is rejected by this theme parser"
            );
        }
    }

    #[test]
    fn the_layout_lane_needs_a_track_and_its_default_survives_without_cues() {
        let data = loaded();
        let width = 1000.0;
        assert_eq!(layout_cue_at(&data, width, 0.0, 1.0), None);
        assert!(!layout_default_at(&data, width, 0.0));
        assert!(layout_lane_tooltip(&data, width, 0.0, 1.0).is_none());

        let (data, scratch) = webcam_loaded("empty-lane");
        assert!(saved_layouts(&data).cues.is_empty());
        assert_eq!(layout_cue_at(&data, width, 500.0, 1.0), None);
        assert!(layout_default_at(&data, width, 500.0));
        let tooltip =
            layout_lane_tooltip(&data, width, 500.0, 1.0).expect("the default has a tooltip");
        assert!(tooltip.contains(t("Default layout").as_str()));
        clean_up(scratch);
    }

    #[test]
    fn frame_aspect_falls_back_when_the_source_size_is_unknown() {
        let mut data = loaded();
        assert!((data.frame_aspect() - 16.0 / 9.0).abs() < 1e-9);
        data.frame_size = Some((1080, 1920));
        assert!((data.frame_aspect() - 9.0 / 16.0).abs() < 1e-9);
        data.frame_size = None;
        assert!((data.frame_aspect() - 16.0 / 9.0).abs() < 1e-9);
        data.frame_size = Some((0, 1080));
        assert!((data.frame_aspect() - 16.0 / 9.0).abs() < 1e-9);
    }

    #[test]
    fn effects_travel_with_the_project_file_and_return_on_load() {
        let video = recording("effects-file");
        let background = Background::default();
        let mut data = loaded();
        data.path = Some(video);
        data.update_effects(|effects| {
            effects.click_sound = Some("soft".to_owned());
            effects.click_sound_volume = 0.6;
            effects.captions.shortcuts = true;
        });
        let file = data
            .project_file(&background)
            .expect("an effect change is an edit");
        assert_eq!(file.cursor_click_sound.as_deref(), Some("soft"));
        assert!(file.captions_shortcuts);

        let mut reopened = loaded();
        reopened.apply_project(&file);
        assert_eq!(reopened.effects, data.effects);
    }

    #[test]
    fn an_effects_change_undoes_and_an_unchanged_one_adds_no_step() {
        let mut data = loaded();
        data.update_effects(|effects| effects.captions.shortcuts = true);
        data.update_effects(|effects| effects.captions.shortcuts = true);
        assert!(data.undo_edit());
        assert_eq!(data.effects, project::EffectsSettings::default());
        assert!(!data.undo_edit());
        assert!(data.redo_edit());
        assert!(data.effects.captions.shortcuts);
    }

    #[test]
    fn opening_another_recording_replaces_its_effects() {
        let video = recording("effects-switch");
        let background = Background::default();
        let mut with_effects = loaded();
        with_effects.path = Some(video.clone());
        with_effects.update_effects(|effects| {
            effects.click_sound = Some("soft".to_owned());
            effects.captions.show_single_letters = true;
        });
        let first = with_effects
            .project_file(&background)
            .expect("the first recording is saved");
        let mut plain = loaded();
        plain.path = Some(video);
        plain.has_project = true;
        let second = plain
            .project_file(&background)
            .expect("the second recording keeps a project");

        let mut data = loaded();
        data.apply_project(&first);
        assert_eq!(data.effects, with_effects.effects);
        data.apply_project(&second);
        assert_eq!(data.effects, project::EffectsSettings::default());
        data.apply_project(&first);
        data.reset_media();
        assert_eq!(data.effects, project::EffectsSettings::default());
    }
}
