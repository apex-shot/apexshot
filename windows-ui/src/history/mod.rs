//! Windows History window.
//!
//! The gallery and window shell are shared with the Linux build. Cloud and
//! saved-edit pages stay out of the Windows stack until their editors and cloud
//! transport are available there.

pub mod actions;
#[path = "../../../src/history/local_page.rs"]
pub mod local_page;
#[cfg(target_os = "windows")]
pub mod placeholder_page;
#[path = "../../../src/history/scan.rs"]
pub mod scan;
#[path = "../../../src/history/thumbnails.rs"]
pub mod thumbnails;
#[path = "../../../src/history/window.rs"]
pub mod window;

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::Application;

/// Scale the History shell exactly like the Windows Settings window.
pub(super) fn window_scale() -> f64 {
    crate::platform::ui_scale()
}

/// Layer the DPI-scaled Settings metrics after the shared base stylesheet.
pub(super) fn install_platform_window_css() {
    crate::settings::install_window_css();
}

/// One page in the shared History window.
pub struct HistoryPage {
    pub widget: gtk4::Widget,
    pub refresh: Rc<dyn Fn()>,
    pub search_placeholder: String,
    pub searchable: bool,
}

/// Open the History window as its own GTK application.
pub fn show_history_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|application| {
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        window::build_history_window(application);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}
