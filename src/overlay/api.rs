use super::background::{
    background_frame_from_capture, background_frame_from_image, BackgroundFrame,
};
use super::window::setup_window;
use super::{
    icons::TOOLBAR_WINDOW_INDEX,
    state::{OverlayMode, SelectorState},
};
use crate::backend::CaptureData;
use crate::capture_overlay::RecordingRequest;
use gtk4::{prelude::*, Application};
use image::RgbaImage;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub struct SelectionArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl SelectionArea {
    /// Normalize the selection (handle negative width/height from dragging)
    pub fn normalize(mut self) -> Self {
        if self.width < 0 {
            self.x += self.width;
            self.width = self.width.abs();
        }
        if self.height < 0 {
            self.y += self.height;
            self.height = self.height.abs();
        }
        self
    }

    /// Check if the selection is valid (has positive area)
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

#[derive(Debug, Clone)]
pub enum OverlaySelection {
    Area(Option<SelectionArea>),
    Recording(RecordingRequest),
}

/// Result of area selection
pub type SelectionResult = Result<OverlaySelection, SelectionError>;

#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("GTK initialization failed: {0}")]
    InitError(String),

    #[error("{0}")]
    Blocked(String),

    #[error("Selection was cancelled by user")]
    Cancelled,

    #[error("Window capture requested from toolbar")]
    WindowCaptureRequested,

    #[error("OCR requested on selection area")]
    OcrRequested(SelectionArea),
}

pub struct AreaSelector {
    state: Arc<Mutex<SelectorState>>,
}

impl AreaSelector {
    /// Create a new area selector
    pub fn new() -> Self {
        let mut state = SelectorState::default();

        // Populate recording defaults from app config so the Rust overlay
        // recording checkboxes stay in sync with Settings > After Capture.
        let app_config = crate::config::load_config();
        state.recording.rec_controls = app_config.rec_controls;
        state.recording.display_rec_time = app_config.rec_display_time;
        state.recording.do_not_disturb = app_config.rec_notifications;
        state.recording.remember_selection = app_config.rec_remember_selection;
        state.recording.dim_screen = app_config.rec_dim_screen;
        state.recording.show_countdown = app_config.rec_countdown;
        state.recording.video_max_res = app_config.rec_video_max_res as usize;
        state.recording.video_fps = app_config.rec_video_fps as usize;
        state.recording.record_mono = app_config.rec_video_mono;
        state.recording.open_editor = app_config.rec_video_open_editor;
        state.recording.gif_fps = app_config.rec_gif_fps as f64;
        state.recording.gif_quality = app_config.rec_gif_quality;
        state.recording.optimize_gif = app_config.rec_gif_optimize;
        state.recording.gif_size_idx = app_config.rec_gif_size_idx as usize;

        // Populate windows from compositor if available
        if let Some(compositor) = crate::compositor::detect_compositor() {
            if let Ok(windows) = compositor.get_windows() {
                let active_ws = compositor.get_active_workspace().ok().flatten();
                state.windows = if let Some(ref ws) = active_ws {
                    windows.into_iter().filter(|w| w.workspace == *ws).collect()
                } else {
                    windows
                };
            }
        }

        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    fn new_with_mode(mode: OverlayMode) -> Self {
        let selector = Self::new();
        selector.state.lock().unwrap().overlay_mode = mode;
        selector
    }

    fn new_window_picker() -> Self {
        let selector = Self::new();
        {
            let mut st = selector.state.lock().unwrap();
            st.active_tool_index = TOOLBAR_WINDOW_INDEX;
            st.window_picker_open = true;
            st.hovered_window_picker_entry = -1;
        }
        selector
    }

    /// Run the area selection dialog
    ///
    /// Returns `Ok(Some(area))` if user selected an area
    /// Returns `Ok(None)` if user cancelled (ESC)
    /// Returns `Err` if initialization failed
    pub fn run(&self) -> SelectionResult {
        self.run_with_background(None)
    }

    fn run_with_background(&self, background: Option<BackgroundFrame>) -> SelectionResult {
        let state = self.state.clone();
        let (result_tx, result_rx) = std::sync::mpsc::channel();

        // Create application.
        // NON_UNIQUE: skip the single-instance check so the overlay can be
        // launched multiple times without GApplication refusing to activate.
        let app = Application::builder()
            .application_id(crate::app_identity::app_id())
            .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
            .build();

        // NOTE: We intentionally do NOT clear DESKTOP_STARTUP_ID here.
        // On GNOME Wayland, clearing it strips the XDG activation token that
        // allows the compositor to grant keyboard focus and raise the window.
        // Without it, window.present() is silently ignored by GNOME Shell.

        // Clone state for the activate handler
        let state_activate = state.clone();
        let background_activate = background.clone();
        app.connect_activate(move |application| {
            setup_window(
                application,
                state_activate.clone(),
                result_tx.clone(),
                background_activate.clone(),
            );
        });

        // Run the application
        let _ = app.run_with_args::<String>(&[]);

        // Check for window capture request (set by toolbar Window button)
        {
            let st = state.lock().unwrap();
            if st.window_capture_requested {
                return Err(SelectionError::WindowCaptureRequested);
            }
        }

        // Get the result
        match result_rx.recv() {
            Ok(Ok(OverlaySelection::Area(area))) => {
                // Check if OCR was requested on this selection
                let st = state.lock().unwrap();
                if st.intent == crate::overlay::recording::state::OverlayIntent::Ocr {
                    if let Some(a) = area {
                        return Err(SelectionError::OcrRequested(a));
                    }
                }
                Ok(OverlaySelection::Area(area))
            }
            Ok(other) => other,
            Err(_) => Err(SelectionError::InitError("No result received".into())),
        }
    }
}

/// Setup the overlay window (standalone function to avoid lifetime issues)
/// Install CSS that disables GTK-side animations on the overlay window so it
/// appears and disappears instantly (no fade / scale shutter effect).
impl Default for AreaSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the interactive area selector.
///
/// **Primary path:** launches the native C++ Qt5 `apexshot-capture` binary.
/// This works reliably on both X11 and Wayland (GNOME, KDE, Sway, etc.)
/// because Qt handles compositor quirks natively.
///
/// **Fallback:** if the C++ binary is not found, falls back to the GTK4
/// overlay implementation (legacy path, may be unreliable on GNOME Wayland).
pub fn select_area() -> SelectionResult {
    match crate::capture_overlay::run_capture_overlay(None) {
        Ok(result) => return Ok(result),
        Err(e) => {
            eprintln!(
                "[overlay] C++ capture overlay unavailable ({e}), falling back to GTK4 selector"
            );
        }
    }
    // GTK4 fallback
    let selector = AreaSelector::new();
    selector.run()
}

/// Run area selection against a static screenshot image.
///
/// Saves the image to a temp file, passes it to `apexshot-capture --background`,
/// then deletes the temp file. Falls back to the GTK4 path if unavailable.
pub fn select_area_from_image(image: &RgbaImage) -> SelectionResult {
    // Write the image to a temp PNG for the C++ binary to load
    let tmp_path = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let write_ok = image.save(&tmp_path).is_ok();
    if write_ok {
        let result = crate::capture_overlay::run_capture_overlay(Some(&tmp_path));
        let _ = std::fs::remove_file(&tmp_path);
        match result {
            Ok(r) => return Ok(r),
            Err(e) => eprintln!("[overlay] C++ capture overlay failed ({e}), falling back to GTK4"),
        }
    }
    // GTK4 fallback
    let selector = AreaSelector::new();
    let background = background_frame_from_image(image)?;
    selector.run_with_background(Some(background))
}

/// Convert `CaptureData` pixels directly to Cairo ARGB bytes in one parallel pass.
///
/// Avoids the intermediate `RgbaImage` allocation that `capture_to_rgba_image` would
/// produce, saving one full-resolution copy (~8 MB for 1080p).
pub fn select_area_from_capture(capture: &CaptureData) -> SelectionResult {
    // Write to temp PNG for the C++ binary
    let tmp_path = std::env::temp_dir().join(format!(
        "apexshot_bg_{}.png",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    // Convert CaptureData to RGBA image for saving
    let write_ok = (|| -> Option<()> {
        use crate::backend::PixelFormat;
        let w = capture.width;
        let h = capture.height;
        let fmt = capture.format;
        let src_stride = capture.stride as usize;
        let bpp = fmt.bytes_per_pixel as usize;
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            let row_start = y * src_stride;
            for x in 0..w as usize {
                let si = row_start + x * bpp;
                let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                    (
                        capture.pixels[si],
                        capture.pixels[si + 1],
                        capture.pixels[si + 2],
                    )
                } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                    (
                        capture.pixels[si + 2],
                        capture.pixels[si + 1],
                        capture.pixels[si],
                    )
                } else if fmt == PixelFormat::RGB24 {
                    (
                        capture.pixels[si],
                        capture.pixels[si + 1],
                        capture.pixels[si + 2],
                    )
                } else {
                    (
                        capture.pixels[si + 2],
                        capture.pixels[si + 1],
                        capture.pixels[si],
                    )
                };
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let img: image::RgbaImage = image::ImageBuffer::from_raw(w, h, rgba)?;
        img.save(&tmp_path).ok()?;
        Some(())
    })()
    .is_some();

    if write_ok {
        let result = crate::capture_overlay::run_capture_overlay(Some(&tmp_path));
        let _ = std::fs::remove_file(&tmp_path);
        match result {
            Ok(r) => return Ok(r),
            Err(e) => eprintln!("[overlay] C++ capture overlay failed ({e}), falling back to GTK4"),
        }
    }

    // GTK4 fallback
    let selector = AreaSelector::new();
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}

pub fn select_area_from_capture_with_gtk(capture: &CaptureData) -> SelectionResult {
    let selector = AreaSelector::new();
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}

pub fn select_crosshair_from_capture_with_gtk(capture: &CaptureData) -> SelectionResult {
    let selector = AreaSelector::new_with_mode(OverlayMode::CrosshairCapture);
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}

pub fn select_window_from_capture_with_gtk(capture: &CaptureData) -> SelectionResult {
    let selector = AreaSelector::new_window_picker();
    let background = background_frame_from_capture(capture)?;
    selector.run_with_background(Some(background))
}
