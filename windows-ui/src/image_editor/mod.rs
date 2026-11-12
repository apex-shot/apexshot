//! Windows image-editor shell.
//!
//! This module owns the editor's window chrome, the display canvas, the
//! floating tool bar and the inspector frame. The annotation model, its
//! rendering and export live in the annotation controller and shared editor
//! core; the remaining Appearance tools and Motion shell are separate from
//! that document.

use gtk4::prelude::*;
use gtk4::Application;
use std::ffi::OsString;
use std::path::PathBuf;

mod annotate;
mod arrow_control;
mod canvas;
mod color_control;
mod cursor;
mod custom_color;
mod effect_control;
mod effect_mode;
mod inspector;
mod mode_confirm;
mod motion;
mod motion_controls;
mod motion_text;
mod number_control;
mod picker_placement;
mod sections;
mod stroke_control;
mod sub_bar;
mod text_controls;
mod timeline;
mod toolbar;
mod ui_support;
mod watermark;
mod window;

/// Open the Windows image editor, optionally with an image to annotate.
pub fn show_image_editor_window(initial: Option<PathBuf>) -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
        crate::platform::sharpen_text();
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        window::build_window(application, initial.as_deref());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

/// The first non-option argument is the image path, matching the shared
/// `image-editor [path]` command shape.
pub fn initial_image_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter()
        .find(|arg| !arg.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::initial_image_from_args;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn first_non_option_argument_is_the_initial_image() {
        assert_eq!(
            initial_image_from_args([
                OsString::from("--debug"),
                OsString::from(r"C:\Shots\capture.png"),
            ]),
            Some(PathBuf::from(r"C:\Shots\capture.png"))
        );
    }

    #[test]
    fn launching_without_a_path_opens_the_empty_shell() {
        assert_eq!(initial_image_from_args(Vec::<OsString>::new()), None);
        assert_eq!(initial_image_from_args([OsString::from("--verbose")]), None);
    }
}
