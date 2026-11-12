//! Windows History window.
//!
//! The gallery, the Cloud page, and the window shell are shared with the
//! Linux build. Motion edits stay a placeholder until that editor is
//! available on Windows.

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

/// The Windows page set: a placeholder for the Motion editor this build
/// does not have yet, the page the video editor's saved lists feed, and the
/// shared Cloud page.
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
            &t("Motion editing is not available in the Windows build yet."),
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
