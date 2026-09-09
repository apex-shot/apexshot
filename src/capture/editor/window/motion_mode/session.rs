use image::RgbaImage;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use crate::capture::editor::render::rgba_image_to_surface;
use crate::capture::editor::state::EditorState;
use crate::recording::editor::model::{MotionBackgroundFillType, MotionState};

pub(in crate::capture::editor::window) struct MotionRuntime {
    pub(in crate::capture::editor::window) snapshot: Option<RgbaImage>,
    pub(in crate::capture::editor::window) card: Option<gtk4::cairo::ImageSurface>,
    pub(in crate::capture::editor::window) background_surface: Option<gtk4::cairo::ImageSurface>,
    pub(in crate::capture::editor::window) watermark_surface: Option<gtk4::cairo::ImageSurface>,
    pub(in crate::capture::editor::window) motion: MotionState,
    pub(in crate::capture::editor::window) playing: bool,
    pub(in crate::capture::editor::window) live_preview: bool,
    pub(in crate::capture::editor::window) last_tick: Option<Instant>,
    /// Playhead time at which an edit-triggered transition preview stops.
    pub(in crate::capture::editor::window) preview_end: Option<f64>,
}

impl MotionRuntime {
    fn new() -> Self {
        Self {
            snapshot: None,
            card: None,
            background_surface: None,
            watermark_surface: None,
            motion: MotionState::default(),
            playing: false,
            live_preview: false,
            last_tick: None,
            preview_end: None,
        }
    }
}

#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionSession {
    pub(super) runtime: Rc<RefCell<MotionRuntime>>,
    pub(super) prefers_dark: bool,
}

impl MotionSession {
    pub(in crate::capture::editor::window) fn new(prefers_dark: bool) -> Self {
        Self {
            runtime: Rc::new(RefCell::new(MotionRuntime::new())),
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
        let snapshot = state.to_final_image().ok();
        let mut runtime = self.runtime.borrow_mut();
        runtime.card = snapshot.as_ref().and_then(rgba_image_to_surface);
        let scene_path = match runtime.motion.appearance.background_fill_type {
            MotionBackgroundFillType::Wallpaper => {
                runtime.motion.appearance.wallpaper_image_name.as_deref()
            }
            MotionBackgroundFillType::Image => {
                runtime.motion.appearance.custom_background_image.as_deref()
            }
            _ => None,
        };
        runtime.background_surface =
            scene_path.and_then(super::super::motion_render::load_motion_background_surface);
        runtime.watermark_surface = runtime
            .motion
            .watermark
            .image_file_name
            .as_deref()
            .and_then(super::super::motion_render::load_motion_background_surface);
        runtime.snapshot = snapshot;
        runtime.motion.playhead = 0.0;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.preview_end = None;
        // Shotbase enters Motion with an empty effects track; clips appear
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
        runtime.background_surface = None;
        runtime.watermark_surface = None;
        runtime.playing = false;
        runtime.live_preview = false;
        runtime.last_tick = None;
        runtime.preview_end = None;
        runtime.motion.playhead = 0.0;
        runtime.motion.segments.clear();
        runtime.motion.text_segments.clear();
        runtime.motion.selected = None;
        runtime.motion.selected_text = None;
    }
}
