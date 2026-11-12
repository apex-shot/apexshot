//! Windows History window.
//!
//! The gallery, the Cloud page, and the window shell are shared with the
//! Linux build. Motion edits stay a placeholder until the Windows Motion
//! editor keeps its projects; its exported videos already list under
//! Recordings.

pub mod actions;
#[path = "../../../src/history/cloud_page.rs"]
pub mod cloud_page;
#[path = "../../../src/history/local_page.rs"]
pub mod local_page;
#[cfg(target_os = "windows")]
pub mod placeholder_page;
#[path = "../../../src/history/scan.rs"]
pub mod scan;
#[path = "../../../src/history/thumbnails.rs"]
pub mod thumbnails;
mod type_scale;
pub mod video_edits_page;
#[path = "../../../src/history/window.rs"]
pub mod window;

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::Application;

/// Scale the History shell exactly like the Windows Settings window.
pub(super) fn window_scale() -> f64 {
    crate::platform::ui_scale()
}

/// Layer the DPI-scaled Settings metrics after the shared base stylesheet:
/// the window chrome first, then the shell (sidebar, card) metrics so the
/// cascade resolves the same way it does in the Settings window, and finally
/// the History type sheet over both.
pub(super) fn install_platform_window_css() {
    crate::settings::install_window_css();
    crate::settings::shell::install_css();
    type_scale::install();
}

/// One page in the shared History window.
pub struct HistoryPage {
    pub widget: gtk4::Widget,
    pub refresh: Rc<dyn Fn()>,
    pub search_placeholder: String,
    pub searchable: bool,
}

/// The Windows page set: a placeholder for Motion projects, which the
/// Windows Motion editor does not keep yet, the page the video editor's saved
/// lists feed, and the shared Cloud page.
///
/// It lives here rather than in the shared window so the copy a Windows-only
/// branch shows stays in the crate that shows it — the Linux catalog check
/// reads `src/` and would otherwise demand translations for strings no Linux
/// build can display.
pub fn windows_history_pages(
    toast: window::HistoryToast,
    search: &gtk4::Entry,
) -> [HistoryPage; 3] {
    use crate::capture::editor::window::icon_names::custom;
    use crate::i18n::t;

    [
        placeholder_page::build_placeholder_page(
            custom::SPLINE_SYMBOLIC,
            &t("Motion videos you export appear in Recordings. Saved Motion projects are not kept on Windows yet."),
        ),
        video_edits_page::build_video_edits_page(toast.clone(), search),
        cloud_page::build_cloud_page(toast),
    ]
}

/// Open the History window as its own GTK application.
pub fn show_history_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|application| {
        crate::platform::sharpen_text();
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        window::build_history_window(application);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use gtk4::prelude::*;

    #[test]
    fn history_styles_parse() {
        let Some(errors) = crate::test_support::with_gtk(|| {
            let provider = gtk4::CssProvider::new();
            let errors = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let captured = std::rc::Rc::clone(&errors);
            provider.connect_parsing_error(move |_, _, error| {
                captured.borrow_mut().push(error.message().to_string());
            });
            provider.load_from_data(include_str!(
                "../../../src/settings/css/10-history-shell.css"
            ));
            let result = errors.borrow().clone();
            result
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert!(errors.is_empty(), "CSS parsing errors: {errors:?}");
    }

    #[test]
    fn the_loading_skeleton_fills_the_grid_with_placeholder_cards() {
        let Some((cards, targetable)) = crate::test_support::with_gtk(|| {
            let skeleton = super::local_page::build_loading_skeleton();
            let mut cards = 0;
            let mut child = skeleton.first_child();
            while let Some(widget) = child {
                cards += 1;
                child = widget.next_sibling();
            }
            (cards, skeleton.can_target())
        }) else {
            eprintln!("skipping: no display available");
            return;
        };
        assert_eq!(cards, 12);
        assert!(!targetable);
    }
}
