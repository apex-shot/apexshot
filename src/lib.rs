#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::items_after_test_module,
    clippy::arc_with_non_send_sync
)]

pub mod annotations;
pub mod app_identity;
pub mod backend;
pub mod capture;
pub mod capture_overlay;
pub mod compositor;
pub mod config;
pub mod daemon;
pub mod distro;
pub mod gnome_integration;
pub mod gnome_shell;
pub mod hotkeys;
pub mod ocr;
pub mod onboarding;
pub mod overlay;
pub mod qr;
pub mod recording;
pub mod settings;
pub mod tray;
pub mod utils;

// Re-export commonly used types
pub use backend::{CaptureData, DisplayBackend, DisplayError, DisplayResult, PixelFormat};
pub use capture::{quick_save, save_capture, ImageFormat, SaveConfig, SaveError, SaveResult};
pub use config::{
    config_path, load_config, save_config, AppConfig, DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS,
    MAX_PREVIEW_AUTO_CLOSE_SECONDS, MIN_PREVIEW_AUTO_CLOSE_SECONDS,
};
pub use ocr::{
    copy_to_clipboard, extract_text, extract_text_from_path, ContentSource, OcrConfig, OcrError,
    OcrOutput, OcrResult,
};
pub use overlay::{
    select_area, select_area_from_capture, select_area_from_image, AreaSelector, SelectionArea,
    SelectionError, SelectionResult,
};

pub use onboarding::{is_onboarding_complete, show_onboarding_window};
pub use recording::{start_recording, RecordError, RecordResult, RecordingConfig};
pub use settings::show_settings_window;
