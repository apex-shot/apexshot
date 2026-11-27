#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::items_after_test_module,
    clippy::arc_with_non_send_sync,
    // The crate builds against GTK 4.10+ because the video preview masks the
    // video card with `GtkSnapshot::push_mask` (4.10) to draw the editor's
    // squircle corners. GTK 4.10 also deprecated the `FileChooserNative`,
    // `MessageDialog`, and `StyleContext` APIs this codebase still uses;
    // migrating them is separate follow-up work, not a radius change.
    deprecated
)]

pub mod annotations;
pub mod app_identity;
pub mod app_links;
pub mod backend;
pub mod capture;
pub mod capture_overlay;
pub mod cloud;
pub mod compositor;
pub mod config;
pub mod daemon;
pub mod distro;
pub(crate) mod dither_field;
pub mod gnome_extension;
pub mod gnome_integration;
pub mod gnome_shell;
pub(crate) mod graceful_close;
pub mod history;
pub mod hotkeys;
pub mod i18n;
pub mod ocr;
pub mod onboarding;
pub mod overlay;
pub mod pipewire_engine;
pub mod preview_launch;
pub mod qr;
pub mod recording;
pub mod settings;
pub(crate) mod studio_entitlement;
pub(crate) mod studio_nudges;
pub(crate) mod studio_trial;
pub(crate) mod studio_trial_toast;
pub(crate) mod studio_upsell;

/// The CLI needs this machine's Studio Device ID, status line, and the entry
/// point for an activation link; nothing else about entitlements is exported.
pub use studio_entitlement::{activate_from_url, device_id, studio_status_line};
#[cfg(test)]
mod test_support;
mod typography;
pub mod update;
pub mod update_ui;
pub mod update_window;
pub mod usage_telemetry;
pub mod utils;
pub mod whats_new;
pub mod whats_new_window;

// Re-export commonly used types
pub use backend::{CaptureData, DisplayBackend, DisplayError, DisplayResult, PixelFormat};
pub use capture::{quick_save, save_capture, ImageFormat, SaveConfig, SaveError, SaveResult};
pub use config::{config_path, load_config, save_config, AppConfig};
pub use ocr::{
    copy_to_clipboard, extract_text, extract_text_from_path, ContentSource, OcrConfig, OcrError,
    OcrOutput, OcrResult,
};
pub use overlay::{
    select_area, select_area_from_capture, select_area_from_capture_with_gtk,
    select_area_from_image, select_crosshair_from_capture_with_gtk,
    select_window_from_capture_with_gtk, AreaSelector, OverlaySelection, SelectionArea,
    SelectionError, SelectionResult,
};

pub use onboarding::{is_onboarding_complete, show_onboarding_window};
pub use recording::{start_recording, RecordError, RecordResult, RecordingConfig};
pub use settings::show_settings_window;
pub mod tray;
