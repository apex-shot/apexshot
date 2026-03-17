pub mod backend;
pub mod capture;
pub mod capture_overlay;
pub mod config;
pub mod daemon;
pub mod gnome_integration;
pub mod hotkeys;
pub mod ocr;
pub mod overlay;
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
    copy_to_clipboard, extract_text, extract_text_from_path, OcrConfig, OcrError, OcrOutput,
    OcrResult,
};
pub use overlay::{
    select_area, select_area_from_capture, select_area_from_image, AreaSelector, SelectionArea,
    SelectionError, SelectionResult,
};
pub use recording::{start_recording, RecordError, RecordResult, RecordingConfig};
pub use settings::show_settings_window;
