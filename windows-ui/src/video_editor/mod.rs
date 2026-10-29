//! Windows video-editor shell.
//!
//! The shell, the tool-specific side panels and the launch contract live here;
//! playback, the editing model and export are their own slices, and the saved
//! edit list is [`project`].

use gtk4::prelude::*;
use gtk4::Application;
mod background;
mod cursor;
mod custom_fill;
mod cutout;
mod export;
mod export_card;
mod history;
mod pixels;
mod press;
mod preview;
mod project;
mod rounded_clip;
mod side_panel;
mod timeline;
mod ui_support;
mod webcam;
mod window;
mod zoom;

use std::ffi::OsString;
use std::path::PathBuf;

/// Open the Windows video editor, optionally with a recording to name.
pub fn show_video_editor_window(initial: Option<PathBuf>) -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |application| {
        crate::capture::editor::ui_support::apply_theme_to_gtk();
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        window::build_window(application, initial.as_deref());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

/// One saved edit list, as the History window sees it.
pub(crate) struct SavedEdit {
    pub name: String,
    pub source_path: PathBuf,
}

/// Every saved edit whose recording is still on disk, newest edit first.
pub(crate) fn saved_edits() -> Vec<SavedEdit> {
    project::list_projects()
        .into_iter()
        .map(|file| SavedEdit {
            name: file.display_name(),
            source_path: file.source_path,
        })
        .collect()
}

/// The first non-option argument is the recording path, matching the shared
/// `video-editor [path]` command shape.
pub fn initial_video_from_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter()
        .find(|arg| !arg.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::initial_video_from_args;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn first_non_option_argument_is_the_initial_video() {
        assert_eq!(
            initial_video_from_args([
                OsString::from("--debug"),
                OsString::from(r"C:\Videos\recording.mp4"),
            ]),
            Some(PathBuf::from(r"C:\Videos\recording.mp4"))
        );
    }

    #[test]
    fn launching_without_a_path_opens_an_empty_shell() {
        assert_eq!(initial_video_from_args(Vec::<OsString>::new()), None);
        assert_eq!(initial_video_from_args([OsString::from("--verbose")]), None);
    }
}
