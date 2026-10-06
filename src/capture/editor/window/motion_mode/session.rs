use image::RgbaImage;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::capture::editor::render::rgba_image_to_surface;
use crate::capture::editor::state::{EditorState, MotionCropHistoryChange};
use crate::capture::editor::types::Rect;
use crate::recording::editor::model::{
    MotionAppearance, MotionBackgroundFillType, MotionSegment, MotionState,
    MotionTextCoordinateSpace, MotionTextSegment,
};

/// Undo steps kept for the Motion timeline. Snapshots are cheap: MotionState
/// is plain data plus a handful of small surfaces-by-name, so a hundred steps
/// stay far below the decoded still's footprint.
const MOTION_HISTORY_LIMIT: usize = 100;
/// Edits arriving within this window (one slider drag, one text burst) share
/// a single undo step instead of flooding the stack.
const MOTION_EDIT_COALESCE: Duration = Duration::from_millis(350);
const AUTO_PREVIEW_QUIET_PERIOD: Duration = Duration::from_millis(300);

/// Cached scene-only preview. Motion's card, text, and watermark remain
/// dynamic, but the checkerboard/background layer can be reused for every
/// timeline frame until its Appearance or viewport changes.
pub(in crate::capture::editor::window) struct MotionBackdropCache {
    pub(in crate::capture::editor::window) width: i32,
    pub(in crate::capture::editor::window) height: i32,
    pub(in crate::capture::editor::window) prefers_dark: bool,
    /// Scene panel rectangle the pixels were painted for: source size, Frame
    /// preset and canvas dimensions all land here, so any of them changing
    /// invalidates the cache without keying on foreground-only card styling.
    pub(in crate::capture::editor::window) scene_rect: (f64, f64, f64, f64),
    pub(in crate::capture::editor::window) appearance: MotionAppearance,
    pub(in crate::capture::editor::window) surface: gtk4::cairo::ImageSurface,
}

/// Empty effect lane names, used to paint the row's add affordance.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::capture::editor::window) enum MotionHoverTrack {
    Motion,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::capture::editor::window) enum AutoPreviewLane {
    Motion,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::capture::editor::window) struct PendingAutoPreview {
    pub(in crate::capture::editor::window) lane: AutoPreviewLane,
    pub(in crate::capture::editor::window) start: f64,
    pub(in crate::capture::editor::window) end: f64,
    pub(in crate::capture::editor::window) pose_time: f64,
    clip_end: f64,
    pub(in crate::capture::editor::window) ready_at: Instant,
}

impl PendingAutoPreview {
    fn matches_selection(self, motion: &MotionState) -> bool {
        let selected_range = match self.lane {
            AutoPreviewLane::Motion => motion
                .selected_segment()
                .map(|segment| (segment.start, segment.end)),
            AutoPreviewLane::Text => motion
                .selected_text_segment()
                .map(|segment| (segment.start, segment.end)),
        };
        selected_range.is_some_and(|(start, end)| {
            (start - self.start).abs() <= 1e-6 && (end - self.clip_end).abs() <= 1e-6
        })
    }
}

/// A finished background-thread composite. The preview paint blits this;
/// the UI thread never composites (see motion_mode/preview.rs).
pub(in crate::capture::editor::window) struct PreviewFrame {
    pub(in crate::capture::editor::window) width: i32,
    pub(in crate::capture::editor::window) height: i32,
    pub(in crate::capture::editor::window) time: f64,
    pub(in crate::capture::editor::window) live_preview: bool,
    pub(in crate::capture::editor::window) content_gen: u64,
    pub(in crate::capture::editor::window) surface: gtk4::cairo::ImageSurface,
}

/// Raw surface pixels. Cairo surfaces are not `Send`, so this is what
/// crosses the thread boundary; each side rebuilds its own surface.
pub(in crate::capture::editor::window) struct PreviewPixels {
    pub(in crate::capture::editor::window) width: i32,
    pub(in crate::capture::editor::window) height: i32,
    pub(in crate::capture::editor::window) stride: i32,
    pub(in crate::capture::editor::window) bytes: Vec<u8>,
}

/// A finished frame posted by the background compositor, as pixels for the
/// UI thread to upload.
pub(in crate::capture::editor::window) struct PreviewResult {
    pub(in crate::capture::editor::window) width: i32,
    pub(in crate::capture::editor::window) height: i32,
    pub(in crate::capture::editor::window) time: f64,
    pub(in crate::capture::editor::window) live_preview: bool,
    pub(in crate::capture::editor::window) content_gen: u64,
    pub(in crate::capture::editor::window) stride: i32,
    pub(in crate::capture::editor::window) bytes: Vec<u8>,
}

pub(in crate::capture::editor::window) struct MotionRuntime {
    pub(in crate::capture::editor::window) snapshot: Option<RgbaImage>,
    pub(in crate::capture::editor::window) card: Option<gtk4::cairo::ImageSurface>,
    /// Downscaled card texture built once per snapshot for the live preview.
    /// Scrubbing samples the card on every pointer event; sampling the
    /// full-resolution still dominated scrub frame time. Export keeps `card`.
    pub(in crate::capture::editor::window) card_preview: Option<gtk4::cairo::ImageSurface>,
    /// Pixel scale from `card` to `card_preview` (1.0 when no preview texture).
    pub(in crate::capture::editor::window) card_scale: f64,
    pub(in crate::capture::editor::window) background_surface: Option<gtk4::cairo::ImageSurface>,
    /// Path whose pixels `background_surface` holds, so the static canvas can
    /// reuse the inspector's decode instead of decoding the same file again on
    /// the UI thread.
    pub(in crate::capture::editor::window) background_surface_path: Option<String>,
    /// True while `background_surface` only holds the cached thumbnail shown
    /// until the full-size decode lands. The static canvas keeps its previous
    /// surface while this is set instead of stretching a 256px thumb.
    pub(in crate::capture::editor::window) background_surface_is_preview: bool,
    pub(in crate::capture::editor::window) watermark_surface: Option<gtk4::cairo::ImageSurface>,
    pub(in crate::capture::editor::window) backdrop_cache: Option<MotionBackdropCache>,
    pub(in crate::capture::editor::window) motion: MotionState,
    /// Motion states as they were before each edit; the top is the state to
    /// restore on the next Undo.
    undo_stack: Vec<MotionState>,
    /// States undone by Undo, replayed by Redo.
    redo_stack: Vec<MotionState>,
    last_edit: Option<Instant>,
    pub(in crate::capture::editor::window) playing: bool,
    pub(in crate::capture::editor::window) live_preview: bool,
    pub(in crate::capture::editor::window) last_tick: Option<Instant>,
    pub(in crate::capture::editor::window) pending_auto_preview: Option<PendingAutoPreview>,
    /// Playhead time at which an edit-triggered transition preview stops.
    pub(in crate::capture::editor::window) preview_end: Option<f64>,
    /// UI-only selection of the source (image) lane. Motion and Text selection
    /// live on the model; this one never needs undo.
    pub(in crate::capture::editor::window) source_selected: bool,
    /// Pointer read-out time, drawn as the red hover hairline. `None` when the
    /// pointer is outside the timeline.
    pub(in crate::capture::editor::window) hover_time: Option<f64>,
    /// Which track row the pointer is over, so an empty row can show its add
    /// affordance. UI-only.
    pub(in crate::capture::editor::window) hover_track: Option<MotionHoverTrack>,
    /// Content generation: bumped on every model mutation a playhead move
    /// alone would not reveal, so a cached preview frame cannot go stale.
    pub(in crate::capture::editor::window) preview_content_gen: u64,
    /// Latest finished background composite. Painted by the preview widget.
    pub(in crate::capture::editor::window) preview_frame: Option<PreviewFrame>,
    /// A composite is in flight; draws set `preview_dirty` instead of
    /// spawning more work.
    pub(in crate::capture::editor::window) preview_busy: bool,
    /// A newer frame was requested while a composite was in flight.
    pub(in crate::capture::editor::window) preview_dirty: bool,
    pub(in crate::capture::editor::window) preview_tx: Option<mpsc::Sender<Option<PreviewResult>>>,
    pub(in crate::capture::editor::window) preview_rx:
        Option<mpsc::Receiver<Option<PreviewResult>>>,
}

impl MotionRuntime {
    fn new() -> Self {
        Self {
            snapshot: None,
            card: None,
            card_preview: None,
            card_scale: 1.0,
            background_surface: None,
            background_surface_path: None,
            background_surface_is_preview: false,
            watermark_surface: None,
            backdrop_cache: None,
            motion: MotionState::default(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            playing: false,
            live_preview: false,
            last_tick: None,
            pending_auto_preview: None,
            preview_end: None,
            source_selected: false,
            hover_time: None,
            hover_track: None,
            preview_content_gen: 0,
            preview_frame: None,
            preview_busy: false,
            preview_dirty: false,
            preview_tx: None,
            preview_rx: None,
        }
    }

    /// Record the current state before a (possibly continuous) edit. A burst
    /// of updates from one gesture or slider drag collapses into one step:
    /// the first update inside the coalesce window pushes the checkpoint and
    /// the rest reuse it.
    pub(in crate::capture::editor::window) fn begin_motion_edit(&mut self) {
        self.stop_auto_preview();
        self.preview_content_gen = self.preview_content_gen.wrapping_add(1);
        let new_burst = self
            .last_edit
            .is_none_or(|at| at.elapsed() > MOTION_EDIT_COALESCE);
        if new_burst {
            self.push_motion_history();
        }
        self.last_edit = Some(Instant::now());
    }

    pub(in crate::capture::editor::window) fn begin_motion_command(&mut self) {
        self.last_edit = None;
        self.begin_motion_edit();
        self.last_edit = None;
    }

    pub(in crate::capture::editor::window) fn update_motion_drag(
        &mut self,
        checkpointed: &mut bool,
        update: impl FnOnce(&mut MotionState),
    ) -> bool {
        let mut candidate = self.motion.clone();
        update(&mut candidate);
        if candidate == self.motion {
            return false;
        }
        if *checkpointed {
            self.stop_auto_preview();
            self.preview_content_gen = self.preview_content_gen.wrapping_add(1);
        } else {
            self.begin_motion_command();
            *checkpointed = true;
        }
        self.motion = candidate;
        true
    }

    fn push_motion_history(&mut self) {
        if self.undo_stack.last() == Some(&self.motion) {
            return;
        }
        self.undo_stack.push(self.motion.clone());
        if self.undo_stack.len() > MOTION_HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Record the pixels behind the current Wallpaper/Image fill. The static
    /// canvas reads this instead of decoding the same file on the UI thread;
    /// `is_preview` marks the cached thumbnail shown until the full-size decode
    /// lands, which the canvas must not stretch over a whole canvas.
    pub(in crate::capture::editor::window) fn set_background_surface(
        &mut self,
        path: Option<String>,
        surface: Option<gtk4::cairo::ImageSurface>,
        is_preview: bool,
    ) {
        self.background_surface = surface;
        self.background_surface_path = path;
        self.background_surface_is_preview = is_preview;
        self.backdrop_cache = None;
        // A new fill must paint on the next Motion draw instead of showing
        // the previous worker frame while the new composite renders.
        // Clearing here forces the inline first-frame path, so Motion updates
        // immediately like Static instead of looking stuck.
        self.preview_frame = None;
    }

    pub(in crate::capture::editor::window) fn undo_motion(&mut self) -> bool {
        self.stop_auto_preview();
        // A drag that ended without changing anything still leaves a
        // checkpoint behind; skip those so Undo always makes progress.
        while self.undo_stack.last() == Some(&self.motion) {
            self.undo_stack.pop();
        }
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack
            .push(std::mem::replace(&mut self.motion, previous));
        self.last_edit = None;
        self.preview_content_gen = self.preview_content_gen.wrapping_add(1);
        self.refresh_motion_surfaces();
        true
    }

    pub(in crate::capture::editor::window) fn redo_motion(&mut self) -> bool {
        self.stop_auto_preview();
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack
            .push(std::mem::replace(&mut self.motion, next));
        self.last_edit = None;
        self.preview_content_gen = self.preview_content_gen.wrapping_add(1);
        self.refresh_motion_surfaces();
        true
    }

    pub(in crate::capture::editor::window) fn motion_history_availability(&self) -> (bool, bool) {
        (!self.undo_stack.is_empty(), !self.redo_stack.is_empty())
    }

    pub(in crate::capture::editor::window) fn queue_auto_preview(
        &mut self,
        lane: AutoPreviewLane,
        start: f64,
        end: f64,
        pose_time: f64,
        now: Instant,
    ) {
        if self.playing && self.preview_end.is_none() {
            self.cancel_auto_preview();
            return;
        }

        let clip_end = match lane {
            AutoPreviewLane::Motion => self.motion.selected_segment().map(|segment| segment.end),
            AutoPreviewLane::Text => self
                .motion
                .selected_text_segment()
                .map(|segment| segment.end),
        };

        self.cancel_auto_preview();
        self.playing = false;
        self.last_tick = None;
        self.preview_end = None;
        self.hover_time = None;
        self.hover_track = None;
        self.live_preview = false;
        self.motion.playhead = pose_time;
        let Some(clip_end) = clip_end else {
            return;
        };
        if end <= start {
            return;
        }
        self.pending_auto_preview = Some(PendingAutoPreview {
            lane,
            start,
            end,
            pose_time,
            clip_end,
            ready_at: now + AUTO_PREVIEW_QUIET_PERIOD,
        });
    }

    pub(in crate::capture::editor::window) fn take_ready_auto_preview(
        &mut self,
        now: Instant,
        drag_active: bool,
    ) -> Option<PendingAutoPreview> {
        self.cancel_stale_auto_preview();
        let pending = self.pending_auto_preview?;
        if drag_active || now < pending.ready_at {
            return None;
        }
        self.pending_auto_preview.take()
    }

    pub(in crate::capture::editor::window) fn cancel_stale_auto_preview(&mut self) {
        let Some(pending) = self.pending_auto_preview else {
            return;
        };
        if (self.playing && self.preview_end.is_none())
            || !pending.matches_selection(&self.motion)
            || (self.motion.playhead - pending.pose_time).abs() > 1e-6
        {
            self.cancel_auto_preview();
        }
    }

    pub(in crate::capture::editor::window) fn cancel_auto_preview(&mut self) {
        self.pending_auto_preview = None;
    }

    pub(in crate::capture::editor::window) fn stop_auto_preview(&mut self) {
        self.cancel_auto_preview();
        if self.preview_end.is_some() {
            self.playing = false;
            self.last_tick = None;
            self.preview_end = None;
        }
    }

    /// Entering or leaving Motion starts a fresh history: the track is
    /// cleared and there is nothing sensible to undo across the mode switch.
    pub(in crate::capture::editor::window) fn reset_motion_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_edit = None;
    }

    /// Restored appearance or watermark state may name a different image;
    /// rebuild the decoded surfaces exactly like entering Motion does.
    pub(in crate::capture::editor::window) fn refresh_motion_surfaces(&mut self) {
        self.backdrop_cache = None;
        let scene_path = match self.motion.appearance.background_fill_type {
            MotionBackgroundFillType::Wallpaper => {
                self.motion.appearance.wallpaper_image_name.as_deref()
            }
            MotionBackgroundFillType::Image => {
                self.motion.appearance.custom_background_image.as_deref()
            }
            _ => None,
        };
        let surface = scene_path.and_then(|path| {
            super::super::motion_render::load_motion_background_preview_surface(
                path,
                super::super::background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
            )
        });
        self.set_background_surface(scene_path.map(str::to_owned), surface, false);
        self.watermark_surface =
            self.motion
                .watermark
                .image_file_name
                .as_deref()
                .and_then(|path| {
                    super::super::motion_render::load_motion_background_preview_surface(
                        path,
                        super::super::background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
                    )
                });
    }
}

#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionSession {
    pub(in crate::capture::editor::window) runtime: Rc<RefCell<MotionRuntime>>,
    pub(super) prefers_dark: bool,
}

impl MotionSession {
    pub(in crate::capture::editor::window) fn new(
        prefers_dark: bool,
        background_padding: f64,
    ) -> Self {
        let runtime = Rc::new(RefCell::new(MotionRuntime::new()));
        // Per-image padding (0px for a fresh image) seeds the shared runtime;
        // the Motion model's 96px card framing stays a video-editor default
        // and the global prefs padding is never applied here.
        runtime.borrow_mut().motion.appearance.background_padding = background_padding;
        // Single shared background: never inject a default wallpaper here.
        // Motion must look like Static on entry — a fresh image with no
        // background stays on None (checkerboard in preview) instead of
        // gaining a wallpaper Static never had. Users pick a fill explicitly;
        // `capture_snapshot` preserves a Static background when one exists.
        Self {
            runtime,
            prefers_dark,
        }
    }

    pub(in crate::capture::editor::window) fn has_segments(&self) -> bool {
        self.runtime.borrow().motion.has_segments()
    }

    pub(in crate::capture::editor::window) fn duration(&self) -> f64 {
        self.runtime.borrow().motion.duration
    }

    pub(in crate::capture::editor::window) fn capture_snapshot(&self, state: &EditorState) {
        // Single shared tool: Static and Motion edit the same MotionRuntime
        // directly (same Appearance builder), and the Static tick mirrors it
        // into EditorState. Re-importing Static here would clobber Motion's
        // own Frame/appearance through lossy converters (Custom/social presets
        // collapse to the nearest static crop and never round-trip), so entry
        // must NOT reseed. Only refresh the background-free card (screenshot +
        // annotations, never the fill) and its surfaces.
        let mut runtime = self.runtime.borrow_mut();
        refresh_motion_card_snapshot(&mut runtime, state);
        runtime.motion.playhead = 0.0;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.cancel_auto_preview();
        runtime.preview_end = None;
        runtime.source_selected = false;
        runtime.hover_time = None;
        runtime.hover_track = None;
        runtime.preview_content_gen = runtime.preview_content_gen.wrapping_add(1);
        runtime.preview_frame = None;
        runtime.preview_busy = false;
        runtime.preview_dirty = false;
        runtime.reset_motion_history();
        // Motion starts with an empty effects track; clips appear
        // when the user clicks or drags the timeline.
    }

    pub(in crate::capture::editor::window) fn rebase_source_coordinates_for_crop(
        &self,
        crop: Rect,
        old_width: u32,
        old_height: u32,
    ) {
        let mut runtime = self.runtime.borrow_mut();
        rebase_motion_coordinates(&mut runtime.motion, crop, old_width, old_height);
        runtime.preview_content_gen = runtime.preview_content_gen.wrapping_add(1);
        runtime.preview_frame = None;
        runtime.preview_busy = false;
        runtime.preview_dirty = false;
    }

    pub(in crate::capture::editor::window) fn motion_state(&self) -> MotionState {
        self.runtime.borrow().motion.clone()
    }

    pub(in crate::capture::editor::window) fn restore_crop_history_state(
        &self,
        state: &EditorState,
        change: MotionCropHistoryChange,
    ) {
        let mut runtime = self.runtime.borrow_mut();
        rebase_motion_history_geometry(&mut runtime.motion, &change);
        refresh_motion_card_snapshot(&mut runtime, state);
        runtime.reset_motion_history();
    }

    pub(in crate::capture::editor::window) fn export_mp4(
        &self,
        source_image: &std::path::Path,
    ) -> Result<PathBuf, String> {
        let runtime = self.runtime.borrow();
        let snapshot = runtime
            .snapshot
            .as_ref()
            .ok_or_else(|| "Motion has no still to export".to_string())?;
        super::super::motion_render::export_motion_mp4(
            snapshot,
            &runtime.motion,
            self.prefers_dark,
            source_image,
        )
    }

    pub(in crate::capture::editor::window) fn clear_snapshot(&self) {
        let mut runtime = self.runtime.borrow_mut();
        runtime.snapshot = None;
        runtime.card = None;
        runtime.card_preview = None;
        runtime.card_scale = 1.0;
        // Appearance-owned surfaces survive the switch: the Static canvas
        // reuses the decoded wallpaper/watermark after leaving Motion, and
        // re-entering refreshes them anyway. Clearing them here left Static
        // with no pixels behind a fill chosen in Motion.
        runtime.backdrop_cache = None;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.cancel_auto_preview();
        runtime.preview_end = None;
        runtime.motion.playhead = 0.0;
        runtime.motion.segments.clear();
        runtime.motion.text_segments.clear();
        runtime.motion.selected = None;
        runtime.motion.selected_text = None;
        runtime.source_selected = false;
        runtime.hover_time = None;
        runtime.hover_track = None;
        runtime.preview_content_gen = runtime.preview_content_gen.wrapping_add(1);
        runtime.preview_frame = None;
        runtime.preview_busy = false;
        runtime.preview_dirty = false;
        runtime.reset_motion_history();
    }
}

fn refresh_motion_card_snapshot(runtime: &mut MotionRuntime, state: &EditorState) {
    let snapshot = state.to_motion_card_image().ok();
    runtime.card = snapshot.as_ref().and_then(rgba_image_to_surface);
    runtime.card_preview = None;
    runtime.card_scale = 1.0;
    if let Some(card) = runtime.card.as_ref() {
        if let Some((preview, scale)) = super::super::motion_render::scaled_card_preview(card) {
            runtime.card_preview = Some(preview);
            runtime.card_scale = scale;
        }
    }
    runtime.refresh_motion_surfaces();
    runtime.snapshot = snapshot;
    runtime.preview_content_gen = runtime.preview_content_gen.wrapping_add(1);
    runtime.preview_frame = None;
    runtime.preview_busy = false;
    runtime.preview_dirty = false;
}

fn rebase_motion_coordinates(
    motion: &mut MotionState,
    crop: Rect,
    old_width: u32,
    old_height: u32,
) {
    for segment in &mut motion.segments {
        segment.zoom_anchor_x =
            map_source_x(segment.zoom_anchor_x, crop, old_width, false).clamp(0.0, 1.0);
        segment.zoom_anchor_y =
            map_source_y(segment.zoom_anchor_y, crop, old_height, false).clamp(0.0, 1.0);
    }
    for text in &mut motion.text_segments {
        if text.annotation_coordinate_space != MotionTextCoordinateSpace::Canvas {
            text.pos_x = map_source_x(text.pos_x, crop, old_width, false).clamp(0.05, 0.95);
            text.pos_y = map_source_y(text.pos_y, crop, old_height, false).clamp(0.05, 0.95);
        }
    }
    motion.watermark.position.0 =
        map_source_x(motion.watermark.position.0, crop, old_width, false).clamp(0.0, 1.0);
    motion.watermark.position.1 =
        map_source_y(motion.watermark.position.1, crop, old_height, false).clamp(0.0, 1.0);
    let source_scale = old_width as f64 / crop.width as f64;
    motion.watermark.size = (motion.watermark.size * source_scale).clamp(0.02, 0.80);
    motion.watermark.inset = (motion.watermark.inset * source_scale).clamp(0.0, 0.45);
}

fn rebase_motion_history_geometry(motion: &mut MotionState, change: &MotionCropHistoryChange) {
    let (source, target) = if change.undo {
        (&change.after, &change.before)
    } else {
        (&change.before, &change.after)
    };
    let segment_count = motion.segments.len();
    let mut used_segments = vec![false; source.segments.len()];
    for (index, segment) in motion.segments.iter_mut().enumerate() {
        let matched = match_motion_segment(
            segment,
            index,
            segment_count,
            &source.segments,
            &mut used_segments,
        );
        if let Some(source_index) = matched {
            let source_segment = &source.segments[source_index];
            let target_segment = &target.segments[source_index];
            if same_coordinate(segment.zoom_anchor_x, source_segment.zoom_anchor_x) {
                segment.zoom_anchor_x = target_segment.zoom_anchor_x;
            } else {
                segment.zoom_anchor_x = map_source_x(
                    segment.zoom_anchor_x,
                    change.rect,
                    change.old_width,
                    change.undo,
                )
                .clamp(0.0, 1.0);
            }
            if same_coordinate(segment.zoom_anchor_y, source_segment.zoom_anchor_y) {
                segment.zoom_anchor_y = target_segment.zoom_anchor_y;
            } else {
                segment.zoom_anchor_y = map_source_y(
                    segment.zoom_anchor_y,
                    change.rect,
                    change.old_height,
                    change.undo,
                )
                .clamp(0.0, 1.0);
            }
        } else {
            segment.zoom_anchor_x = map_source_x(
                segment.zoom_anchor_x,
                change.rect,
                change.old_width,
                change.undo,
            )
            .clamp(0.0, 1.0);
            segment.zoom_anchor_y = map_source_y(
                segment.zoom_anchor_y,
                change.rect,
                change.old_height,
                change.undo,
            )
            .clamp(0.0, 1.0);
        }
    }

    let text_count = motion.text_segments.len();
    let mut used_text = vec![false; source.text_segments.len()];
    for (index, text) in motion.text_segments.iter_mut().enumerate() {
        if text.annotation_coordinate_space == MotionTextCoordinateSpace::Canvas {
            continue;
        }
        let matched = match_motion_text(
            text,
            index,
            text_count,
            &source.text_segments,
            &mut used_text,
        );
        if let Some(source_index) = matched {
            let source_text = &source.text_segments[source_index];
            let target_text = &target.text_segments[source_index];
            if same_coordinate(text.pos_x, source_text.pos_x) {
                text.pos_x = target_text.pos_x;
            } else {
                text.pos_x = map_source_x(text.pos_x, change.rect, change.old_width, change.undo)
                    .clamp(0.05, 0.95);
            }
            if same_coordinate(text.pos_y, source_text.pos_y) {
                text.pos_y = target_text.pos_y;
            } else {
                text.pos_y = map_source_y(text.pos_y, change.rect, change.old_height, change.undo)
                    .clamp(0.05, 0.95);
            }
        } else {
            text.pos_x = map_source_x(text.pos_x, change.rect, change.old_width, change.undo)
                .clamp(0.05, 0.95);
            text.pos_y = map_source_y(text.pos_y, change.rect, change.old_height, change.undo)
                .clamp(0.05, 0.95);
        }
    }

    if same_coordinate(motion.watermark.position.0, source.watermark.position.0) {
        motion.watermark.position.0 = target.watermark.position.0;
    } else {
        motion.watermark.position.0 = map_source_x(
            motion.watermark.position.0,
            change.rect,
            change.old_width,
            change.undo,
        )
        .clamp(0.0, 1.0);
    }
    if same_coordinate(motion.watermark.position.1, source.watermark.position.1) {
        motion.watermark.position.1 = target.watermark.position.1;
    } else {
        motion.watermark.position.1 = map_source_y(
            motion.watermark.position.1,
            change.rect,
            change.old_height,
            change.undo,
        )
        .clamp(0.0, 1.0);
    }
    let crop_scale = change.rect.width as f64 / change.old_width as f64;
    let source_scale = change.old_width as f64 / change.rect.width as f64;
    motion.watermark.size = if same_coordinate(motion.watermark.size, source.watermark.size) {
        target.watermark.size
    } else if change.undo {
        (motion.watermark.size * crop_scale).clamp(0.02, 0.80)
    } else {
        (motion.watermark.size * source_scale).clamp(0.02, 0.80)
    };
    motion.watermark.inset = if same_coordinate(motion.watermark.inset, source.watermark.inset) {
        target.watermark.inset
    } else if change.undo {
        (motion.watermark.inset * crop_scale).clamp(0.0, 0.45)
    } else {
        (motion.watermark.inset * source_scale).clamp(0.0, 0.45)
    };
}

fn match_motion_segment(
    current: &MotionSegment,
    index: usize,
    current_len: usize,
    source: &[MotionSegment],
    used: &mut [bool],
) -> Option<usize> {
    let same_range = source
        .iter()
        .enumerate()
        .find_map(|(candidate_index, candidate)| {
            (!used[candidate_index]
                && same_coordinate(current.start, candidate.start)
                && same_coordinate(current.end, candidate.end)
                && current.zoom_mode == candidate.zoom_mode)
                .then_some(candidate_index)
        });
    let match_index = same_range.or_else(|| {
        source
            .iter()
            .enumerate()
            .filter(|(candidate_index, candidate)| {
                !used[*candidate_index]
                    && same_coordinate(current.zoom_anchor_x, candidate.zoom_anchor_x)
                    && same_coordinate(current.zoom_anchor_y, candidate.zoom_anchor_y)
            })
            .min_by_key(|(candidate_index, _)| candidate_index.abs_diff(index))
            .map(|(candidate_index, _)| candidate_index)
    });
    let match_index = match_index.or_else(|| {
        (current_len == source.len() && index < source.len() && !used[index]).then_some(index)
    });
    if let Some(match_index) = match_index {
        used[match_index] = true;
    }
    match_index
}

fn match_motion_text(
    current: &MotionTextSegment,
    index: usize,
    current_len: usize,
    source: &[MotionTextSegment],
    used: &mut [bool],
) -> Option<usize> {
    let same_clip = source
        .iter()
        .enumerate()
        .find_map(|(candidate_index, candidate)| {
            (!used[candidate_index]
                && same_coordinate(current.start, candidate.start)
                && same_coordinate(current.end, candidate.end)
                && current.text == candidate.text
                && current.annotation_coordinate_space == candidate.annotation_coordinate_space)
                .then_some(candidate_index)
        });
    let match_index = same_clip.or_else(|| {
        source
            .iter()
            .enumerate()
            .filter(|(candidate_index, candidate)| {
                !used[*candidate_index]
                    && current.annotation_coordinate_space == candidate.annotation_coordinate_space
                    && same_coordinate(current.pos_x, candidate.pos_x)
                    && same_coordinate(current.pos_y, candidate.pos_y)
            })
            .min_by_key(|(candidate_index, _)| candidate_index.abs_diff(index))
            .map(|(candidate_index, _)| candidate_index)
    });
    let match_index = match_index.or_else(|| {
        (current_len == source.len() && index < source.len() && !used[index]).then_some(index)
    });
    if let Some(match_index) = match_index {
        used[match_index] = true;
    }
    match_index
}

fn same_coordinate(left: f64, right: f64) -> bool {
    (left - right).abs() <= f64::EPSILON
}

fn map_source_x(value: f64, crop: Rect, old_width: u32, undo: bool) -> f64 {
    if undo {
        (value * crop.width as f64 + crop.x as f64) / old_width as f64
    } else {
        (value * old_width as f64 - crop.x as f64) / crop.width as f64
    }
}

fn map_source_y(value: f64, crop: Rect, old_height: u32, undo: bool) -> f64 {
    if undo {
        (value * crop.height as f64 + crop.y as f64) / old_height as f64
    } else {
        (value * old_height as f64 - crop.y as f64) / crop.height as f64
    }
}

#[cfg(test)]
mod crop_tests {
    use super::{rebase_motion_coordinates, rebase_motion_history_geometry};
    use crate::capture::editor::state::MotionCropHistoryChange;
    use crate::capture::editor::types::Rect;
    use crate::recording::editor::model::{
        MotionBackgroundFillType, MotionEffectTransformTiming, MotionSegment, MotionState,
        MotionTextAnimation, MotionTextCoordinateSpace, MotionTextFormat, MotionTextScope,
        MotionTextSegment, MotionTransform, MotionZoomMode,
    };

    fn segment(start: f64, end: f64, x: f64, y: f64) -> MotionSegment {
        MotionSegment {
            start,
            end,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: x,
            zoom_anchor_y: y,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform::default(),
            timing: MotionEffectTransformTiming::default(),
        }
    }

    fn text(
        start: f64,
        end: f64,
        text: &str,
        space: MotionTextCoordinateSpace,
        x: f64,
        y: f64,
    ) -> MotionTextSegment {
        MotionTextSegment {
            start,
            end,
            text: text.into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Line,
            typewriter_time: 0.0,
            is_disabled: false,
            annotation_coordinate_space: space,
            pos_x: x,
            pos_y: y,
            size: 1.0,
            format: MotionTextFormat::default(),
            transition_duration: 0.2,
        }
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn crop_rebases_source_local_motion_coordinates_but_keeps_canvas_titles() {
        let mut motion = MotionState::default();
        motion.segments.push(MotionSegment {
            start: 0.0,
            end: 1.0,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: 0.5,
            zoom_anchor_y: 0.5,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform::default(),
            timing: MotionEffectTransformTiming::default(),
        });
        let text = |space, x, y| MotionTextSegment {
            start: 0.0,
            end: 1.0,
            text: "Title".into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Line,
            typewriter_time: 0.0,
            is_disabled: false,
            annotation_coordinate_space: space,
            pos_x: x,
            pos_y: y,
            size: 1.0,
            format: MotionTextFormat::default(),
            transition_duration: 0.2,
        };
        motion
            .text_segments
            .push(text(MotionTextCoordinateSpace::CanonicalSource, 0.25, 0.25));
        motion
            .text_segments
            .push(text(MotionTextCoordinateSpace::MotionCanvasLocal, 0.5, 0.5));
        motion
            .text_segments
            .push(text(MotionTextCoordinateSpace::Canvas, 0.8, 0.7));
        motion.watermark.position = (0.5, 0.5);
        motion.watermark.size = 0.2;
        motion.watermark.inset = 0.1;

        rebase_motion_coordinates(
            &mut motion,
            Rect {
                x: 20,
                y: 16,
                width: 50,
                height: 40,
            },
            100,
            80,
        );

        assert_eq!(
            (
                motion.segments[0].zoom_anchor_x,
                motion.segments[0].zoom_anchor_y
            ),
            (0.6, 0.6)
        );
        assert_eq!(
            (motion.text_segments[0].pos_x, motion.text_segments[0].pos_y),
            (0.1, 0.1)
        );
        assert_eq!(
            (motion.text_segments[1].pos_x, motion.text_segments[1].pos_y),
            (0.6, 0.6)
        );
        assert_eq!(
            (motion.text_segments[2].pos_x, motion.text_segments[2].pos_y),
            (0.8, 0.7)
        );
        assert_eq!(motion.watermark.position, (0.6, 0.6));
        assert_eq!(motion.watermark.size, 0.4);
        assert_eq!(motion.watermark.inset, 0.2);
    }

    #[test]
    fn crop_undo_and_redo_merge_geometry_without_erasing_later_motion_edits() {
        let crop = Rect {
            x: 20,
            y: 16,
            width: 50,
            height: 40,
        };
        let mut before = MotionState::default();
        before.segments.push(segment(0.0, 1.0, 0.5, 0.5));
        before.text_segments.push(text(
            0.0,
            1.0,
            "Image title",
            MotionTextCoordinateSpace::CanonicalSource,
            0.5,
            0.5,
        ));
        before.text_segments.push(text(
            0.0,
            1.0,
            "Canvas title",
            MotionTextCoordinateSpace::Canvas,
            0.8,
            0.7,
        ));
        before.watermark.position = (0.5, 0.5);
        before.watermark.size = 0.2;
        before.watermark.inset = 0.1;

        let mut after = before.clone();
        rebase_motion_coordinates(&mut after, crop, 100, 80);
        let mut current = after.clone();
        current.segments[0].zoom_anchor_x = 0.7;
        current.segments.push(segment(2.0, 3.0, 0.25, 0.75));
        current.text_segments[0].pos_x = 0.8;
        current.text_segments[1].pos_x = 0.3;
        current.text_segments[1].text = "Edited canvas title".into();
        current.text_segments.push(text(
            2.0,
            3.0,
            "New image title",
            MotionTextCoordinateSpace::MotionCanvasLocal,
            0.2,
            0.8,
        ));
        current.watermark.image_file_name = Some("new-mark.png".into());
        current.watermark.position.0 = 0.75;
        current.watermark.size = 0.5;
        current.watermark.inset = 0.3;
        current.appearance.background_fill_type = MotionBackgroundFillType::Color;
        current.appearance.background_color = [0.2, 0.3, 0.4, 1.0];
        current.appearance.background_padding = 137.0;

        let mut undo_change = MotionCropHistoryChange {
            before: before.clone(),
            after: after.clone(),
            rect: crop,
            old_width: 100,
            old_height: 80,
            undo: true,
        };
        rebase_motion_history_geometry(&mut current, &undo_change);
        assert_near(current.segments[0].zoom_anchor_x, 0.55);
        assert_near(current.segments[0].zoom_anchor_y, 0.5);
        assert_near(current.segments[1].zoom_anchor_x, 0.325);
        assert_near(current.segments[1].zoom_anchor_y, 0.575);
        assert_near(current.text_segments[0].pos_x, 0.6);
        assert_near(current.text_segments[0].pos_y, 0.5);
        assert_near(current.text_segments[1].pos_x, 0.3);
        assert_eq!(current.text_segments[1].text, "Edited canvas title");
        assert_near(current.text_segments[2].pos_x, 0.3);
        assert_near(current.text_segments[2].pos_y, 0.6);
        assert_eq!(
            current.watermark.image_file_name.as_deref(),
            Some("new-mark.png")
        );
        assert_near(current.watermark.position.0, 0.575);
        assert_near(current.watermark.position.1, 0.5);
        assert_near(current.watermark.size, 0.25);
        assert_near(current.watermark.inset, 0.15);
        assert_eq!(current.appearance.background_color, [0.2, 0.3, 0.4, 1.0]);
        assert_eq!(current.appearance.background_padding, 137.0);

        undo_change.undo = false;
        rebase_motion_history_geometry(&mut current, &undo_change);
        assert_near(current.segments[0].zoom_anchor_x, 0.7);
        assert_near(current.segments[0].zoom_anchor_y, 0.6);
        assert_near(current.segments[1].zoom_anchor_x, 0.25);
        assert_near(current.segments[1].zoom_anchor_y, 0.75);
        assert_near(current.text_segments[0].pos_x, 0.8);
        assert_near(current.text_segments[0].pos_y, 0.6);
        assert_near(current.text_segments[1].pos_x, 0.3);
        assert_eq!(current.text_segments[1].text, "Edited canvas title");
        assert_near(current.text_segments[2].pos_x, 0.2);
        assert_near(current.text_segments[2].pos_y, 0.8);
        assert_near(current.watermark.position.0, 0.75);
        assert_near(current.watermark.position.1, 0.6);
        assert_near(current.watermark.size, 0.5);
        assert_near(current.watermark.inset, 0.3);
        assert_eq!(current.appearance.background_color, [0.2, 0.3, 0.4, 1.0]);
        assert_eq!(current.appearance.background_padding, 137.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_at(lane: AutoPreviewLane) -> (AutoPreviewLane, f64, f64, f64) {
        (lane, 0.0, 0.3, 0.2)
    }

    fn runtime_with_clip() -> MotionRuntime {
        let mut runtime = MotionRuntime::new();
        runtime.motion.add_segment_at(0.0).expect("motion clip");
        runtime
    }

    #[test]
    fn many_auto_preview_requests_coalesce_until_the_latest_quiet_deadline() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        let request = request_at(AutoPreviewLane::Motion);
        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);
        runtime.queue_auto_preview(
            request.0,
            request.1,
            request.2,
            request.3,
            now + Duration::from_millis(200),
        );

        assert!(runtime
            .take_ready_auto_preview(now + Duration::from_millis(499), false)
            .is_none());
        assert_eq!(
            runtime
                .take_ready_auto_preview(now + Duration::from_millis(500), false)
                .map(|pending| pending.ready_at),
            Some(now + Duration::from_millis(500))
        );
        assert!(runtime
            .take_ready_auto_preview(now + Duration::from_secs(1), false)
            .is_none());
    }

    #[test]
    fn held_drag_defers_a_ready_auto_preview_until_release() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        let request = request_at(AutoPreviewLane::Motion);
        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);

        assert!(runtime
            .take_ready_auto_preview(now + AUTO_PREVIEW_QUIET_PERIOD, true)
            .is_none());
        assert!(!runtime.playing);
        assert!(runtime.pending_auto_preview.is_some());
        assert!(runtime
            .take_ready_auto_preview(now + AUTO_PREVIEW_QUIET_PERIOD, false)
            .is_some());
    }

    #[test]
    fn new_edits_stop_automatic_playback_and_show_the_final_pose() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.playing = true;
        runtime.last_tick = Some(now);
        runtime.preview_end = Some(2.0);
        runtime.motion.playhead = 0.1;

        runtime.queue_auto_preview(AutoPreviewLane::Motion, 0.0, 0.3, 0.2, now);

        assert!(!runtime.playing);
        assert_eq!(runtime.last_tick, None);
        assert_eq!(runtime.preview_end, None);
        assert_eq!(runtime.motion.playhead, 0.2);
        assert!(runtime.pending_auto_preview.is_some());
    }

    #[test]
    fn zero_length_auto_preview_keeps_the_pose_without_scheduling_playback() {
        let mut runtime = runtime_with_clip();
        runtime
            .motion
            .add_segment_at(1.0)
            .expect("adjacent motion clip");
        let now = Instant::now();
        runtime.motion.playhead = 0.4;

        runtime.queue_auto_preview(AutoPreviewLane::Motion, 1.0, 1.0, 1.001, now);

        assert_eq!(runtime.motion.playhead, 1.001);
        assert!(!runtime.playing);
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn manual_playback_ignores_auto_preview_requests_without_seeking() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.playing = true;
        runtime.last_tick = Some(now);
        runtime.motion.playhead = 0.7;

        runtime.queue_auto_preview(AutoPreviewLane::Motion, 1.0, 1.3, 1.2, now);

        assert!(runtime.playing);
        assert_eq!(runtime.last_tick, Some(now));
        assert_eq!(runtime.motion.playhead, 0.7);
        assert_eq!(runtime.preview_end, None);
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn selection_changes_and_seeks_cancel_pending_auto_preview() {
        let mut runtime = runtime_with_clip();
        runtime
            .motion
            .add_segment_at(3.0)
            .expect("second motion clip");
        runtime.motion.selected = Some(0);
        let now = Instant::now();
        let request = request_at(AutoPreviewLane::Motion);
        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);

        runtime.motion.selected = Some(1);
        assert!(runtime
            .take_ready_auto_preview(now + AUTO_PREVIEW_QUIET_PERIOD, false)
            .is_none());
        assert!(runtime.pending_auto_preview.is_none());

        runtime.motion.selected = Some(0);
        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);
        runtime.motion.playhead = 0.5;
        assert!(runtime
            .take_ready_auto_preview(now + AUTO_PREVIEW_QUIET_PERIOD, false)
            .is_none());
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn resizing_the_selected_clip_invalidates_a_preview_with_old_bounds() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.queue_auto_preview(AutoPreviewLane::Motion, 0.0, 0.3, 0.2, now);
        assert!(runtime.pending_auto_preview.is_some());

        runtime.motion.set_segment_range(0, 0.0, 0.8);

        assert!(runtime
            .take_ready_auto_preview(now + AUTO_PREVIEW_QUIET_PERIOD, false)
            .is_none());
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn accepted_edits_preserve_manual_playback_but_stop_automatic_preview() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.playing = true;
        runtime.last_tick = Some(now);
        runtime.preview_end = None;
        let mut checkpointed = false;

        assert!(runtime.update_motion_drag(&mut checkpointed, |motion| {
            motion.set_motion_blur(0.2);
        }));
        assert!(runtime.playing);
        assert_eq!(runtime.last_tick, Some(now));
        assert_eq!(runtime.preview_end, None);

        runtime.preview_end = Some(1.0);
        runtime.pending_auto_preview = Some(PendingAutoPreview {
            lane: AutoPreviewLane::Motion,
            start: 0.0,
            end: 0.3,
            pose_time: 0.2,
            clip_end: 1.0,
            ready_at: now,
        });
        assert!(runtime.update_motion_drag(&mut checkpointed, |motion| {
            motion.set_motion_blur(0.3);
        }));
        assert!(!runtime.playing);
        assert_eq!(runtime.last_tick, None);
        assert_eq!(runtime.preview_end, None);
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn no_op_edit_does_not_interrupt_an_automatic_preview_or_create_history() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.playing = true;
        runtime.last_tick = Some(now);
        runtime.preview_end = Some(1.0);
        let mut checkpointed = false;

        assert!(!runtime.update_motion_drag(&mut checkpointed, |_| {}));

        assert!(runtime.playing);
        assert_eq!(runtime.last_tick, Some(now));
        assert_eq!(runtime.preview_end, Some(1.0));
        assert!(!checkpointed);
        assert_eq!(runtime.motion_history_availability(), (false, false));
    }

    #[test]
    fn a_different_model_edit_cancels_an_obsolete_auto_preview() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        runtime.queue_auto_preview(AutoPreviewLane::Motion, 0.0, 0.3, 0.2, now);
        assert!(runtime.pending_auto_preview.is_some());
        runtime.begin_motion_edit();
        runtime.motion.set_motion_blur(0.2);
        assert!(runtime.pending_auto_preview.is_none());
    }

    #[test]
    fn undo_and_mode_exit_cancel_pending_auto_preview() {
        let mut runtime = runtime_with_clip();
        let now = Instant::now();
        let request = request_at(AutoPreviewLane::Motion);
        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);
        runtime.undo_motion();
        assert!(runtime.pending_auto_preview.is_none());

        runtime.queue_auto_preview(request.0, request.1, request.2, request.3, now);
        let session = MotionSession {
            runtime: Rc::new(RefCell::new(runtime)),
            prefers_dark: true,
        };
        session.clear_snapshot();
        assert!(session.runtime.borrow().pending_auto_preview.is_none());
    }

    #[test]
    fn undo_restores_the_pre_edit_track_and_redo_replays_it() {
        let mut runtime = runtime_with_clip();

        runtime.begin_motion_edit();
        assert!(runtime.motion.remove_selected());
        assert!(runtime.motion.segments.is_empty());
        assert_eq!(runtime.motion_history_availability(), (true, false));

        assert!(runtime.undo_motion());
        assert_eq!(runtime.motion.segments.len(), 1);
        assert_eq!(runtime.motion_history_availability(), (false, true));

        assert!(runtime.redo_motion());
        assert!(runtime.motion.segments.is_empty());
        assert_eq!(runtime.motion_history_availability(), (true, false));
    }

    #[test]
    fn edits_within_the_coalesce_window_share_one_undo_step() {
        let mut runtime = runtime_with_clip();
        let initial_timing = runtime.motion.selected_transform_timing();

        runtime.begin_motion_edit();
        runtime.motion.set_selected_transition_ms(120);
        // Same burst: no additional checkpoint, so one Undo reaches the start.
        runtime.last_edit = Some(Instant::now());
        runtime.begin_motion_edit();
        runtime.motion.set_selected_transition_ms(240);

        runtime.undo_motion();
        assert_eq!(runtime.motion.selected_transform_timing(), initial_timing);

        runtime.redo_motion();
        assert_eq!(
            runtime
                .motion
                .selected_transform_timing()
                .transition_duration,
            240.0 / 1000.0
        );
    }

    #[test]
    fn undo_skips_checkpoints_from_drags_that_changed_nothing() {
        let mut runtime = MotionRuntime::new();
        runtime.begin_motion_edit();
        runtime.motion.add_segment_at(0.0);
        // A second gesture checkpoints but ends without changing anything;
        // the first Undo must still reach the empty track, not re-apply
        // the redundant checkpoint.
        runtime.begin_motion_edit();

        assert!(runtime.undo_motion());
        assert!(runtime.motion.segments.is_empty());
    }

    #[test]
    fn new_edits_drop_the_redo_branch() {
        let mut runtime = runtime_with_clip();
        runtime.begin_motion_edit();
        runtime.motion.remove_selected();
        runtime.undo_motion();
        assert!(
            runtime.motion_history_availability().1,
            "an undone edit leaves Redo available"
        );

        runtime.begin_motion_edit();
        runtime.motion.add_segment_at(1.0);
        assert_eq!(runtime.motion_history_availability(), (true, false));
    }

    #[test]
    fn discrete_clip_commands_have_separate_undo_steps() {
        let mut runtime = runtime_with_clip();
        runtime.begin_motion_edit();
        runtime.motion.set_selected_end_scale(1.5);
        runtime.begin_motion_command();
        runtime.motion.set_selected_disabled(true);
        runtime.begin_motion_command();
        runtime.motion.remove_selected();

        assert!(runtime.undo_motion());
        assert!(runtime.motion.selected_segment().unwrap().is_disabled);
        assert!(runtime.undo_motion());
        let segment = runtime.motion.selected_segment().unwrap();
        assert!(!segment.is_disabled);
        assert_eq!(segment.to.scale, 1.5);
    }

    #[test]
    fn text_position_drag_keeps_one_checkpoint_after_a_long_pause() {
        let mut runtime = MotionRuntime::new();
        runtime.motion.add_text_at(0.0).expect("text clip");
        let initial = {
            let text = runtime.motion.selected_text_segment().unwrap();
            (text.pos_x, text.pos_y)
        };
        let mut checkpointed = false;

        assert!(runtime.update_motion_drag(&mut checkpointed, |motion| {
            motion.set_selected_text_pos(0.6, 0.5);
        }));
        runtime.last_edit = Some(Instant::now() - MOTION_EDIT_COALESCE - Duration::from_secs(1));
        assert!(runtime.update_motion_drag(&mut checkpointed, |motion| {
            motion.set_selected_text_pos(0.7, 0.5);
        }));

        assert!(runtime.undo_motion());
        let text = runtime.motion.selected_text_segment().unwrap();
        assert_eq!((text.pos_x, text.pos_y), initial);
    }
}
