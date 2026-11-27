#![allow(deprecated)]

//! Rust UI shell for the Windows build.
//!
//! This crate owns the GTK4 surfaces that the Windows package runs as a
//! separate process. The settings pages are shared verbatim with the Linux
//! binary (`#[path]` includes), while the platform modules below provide the
//! handful of Linux APIs those pages expect.

pub mod ai_names;
pub mod app_identity;

#[path = "../../src/app_links.rs"]
pub mod app_links;

/// Re-editable annotation sidecars: the files and format the Linux editor
/// writes, so an exported image reopens with its annotations as objects on
/// either platform.
#[allow(clippy::too_many_arguments, clippy::wrong_self_convention, dead_code)]
pub mod annotations {
    #[path = "../../../src/annotations/schema.rs"]
    #[rustfmt::skip]
    mod schema;
    #[path = "../../../src/annotations/storage.rs"]
    #[rustfmt::skip]
    mod storage;

    pub use schema::{AnnotationFile, ShadowProfileSettings};
    pub use storage::{
        delete_annotations, load_annotations, load_original_image, save_annotations,
        serializable_to_action, AnnotationError, ImageSessionSnapshot,
    };
}
pub mod capture;
pub mod capture_overlay;
pub mod cloud;
#[path = "../../src/config.rs"]
pub mod config;
pub mod css_scale;
pub mod daemon;
#[path = "../../src/dither_field.rs"]
pub(crate) mod dither_field;
pub mod history;
pub mod i18n;
pub mod icons;
pub mod image_editor;
pub mod onboarding;
pub mod platform;
pub mod recording;
pub mod settings;
pub mod studio_entitlement;
#[path = "../../src/studio_trial.rs"]
pub mod studio_trial;
#[path = "../../src/studio_trial_toast.rs"]
pub(crate) mod studio_trial_toast;
#[path = "../../src/studio_upsell.rs"]
pub(crate) mod studio_upsell;
#[cfg(test)]
#[path = "../../src/test_support.rs"]
pub mod test_support;
#[path = "../../src/typography.rs"]
#[rustfmt::skip]
pub mod typography;
pub mod update;
pub mod update_ui;
pub mod usage_telemetry;
pub mod utils;
pub mod video_editor;
pub mod whats_new;
pub mod whats_new_window;
