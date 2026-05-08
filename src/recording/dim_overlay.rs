use gtk4::{glib, prelude::*, Application, ApplicationWindow};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Show a fullscreen dim overlay during recording countdown.
/// Call `close()` on the returned handle to dismiss it.
pub fn run_dim_overlay(close_flag: Arc<AtomicBool>) {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    let flag_clone = close_flag.clone();
    app.connect_activate(move |application| {
        let window = ApplicationWindow::builder()
            .application(application)
            .decorated(false)
            .resizable(false)
            .build();

        window.fullscreen();
        window.set_opacity(0.5);

        // CSS for dark background
        let provider = gtk4::CssProvider::new();
        provider.load_from_data("window { background-color: black; }");
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("No display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.present();

        // Poll for close signal
        let window_ref = window.downgrade();
        let flag_ref = flag_clone.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if flag_ref.load(Ordering::Relaxed) {
                if let Some(win) = window_ref.upgrade() {
                    win.close();
                }
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    });

    // Run in a separate thread so it doesn't block the main app
    app.run_with_args::<String>(&[]);
}
