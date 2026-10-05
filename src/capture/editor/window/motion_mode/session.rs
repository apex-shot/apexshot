use image::RgbaImage;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::capture::editor::render::rgba_image_to_surface;
use crate::capture::editor::state::EditorState;
use crate::recording::editor::model::{MotionAppearance, MotionBackgroundFillType, MotionState};

/// Undo steps kept for the Motion timeline. Snapshots are cheap: MotionState
/// is plain data plus a handful of small surfaces-by-name, so a hundred steps
/// stay far below the decoded still's footprint.
const MOTION_HISTORY_LIMIT: usize = 100;
/// Edits arriving within this window (one slider drag, one text burst) share
/// a single undo step instead of flooding the stack.
const MOTION_EDIT_COALESCE: Duration = Duration::from_millis(350);

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
        let snapshot = state.to_motion_card_image().ok();
        let mut runtime = self.runtime.borrow_mut();
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
        runtime.motion.playhead = 0.0;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_with_clip() -> MotionRuntime {
        let mut runtime = MotionRuntime::new();
        runtime.motion.add_segment_at(0.0).expect("motion clip");
        runtime
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
}
