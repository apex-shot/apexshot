use gtk4::gdk::Key;
use gtk4::{glib, prelude::*, Application, ApplicationWindow, EventControllerKey, Label};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Show a fullscreen 3-2-1 countdown overlay before recording starts.
/// Blocks until the countdown completes.
pub fn run_countdown_overlay(seconds: u32) {
    let remaining = Rc::new(Cell::new(seconds));
    let done = Rc::new(Cell::new(false));

    let app = Application::builder()
        .application_id("com.apexshot.countdown")
        .build();

    let remaining_act = remaining.clone();
    let done_act = done.clone();
    app.connect_activate(move |application| {
        let window = ApplicationWindow::builder()
            .application(application)
            .decorated(false)
            .resizable(false)
            .build();

        window.fullscreen();
        window.set_css_classes(&["countdown-overlay"]);

        let label = Label::new(Some(&format!("{}", remaining_act.get())));
        label.set_css_classes(&["countdown-number"]);
        label.set_xalign(0.5);
        label.set_yalign(0.5);
        label.set_hexpand(true);
        label.set_vexpand(true);

        // Add CSS provider for styling
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(
            ".countdown-overlay { background-color: rgba(0, 0, 0, 0.7); } \
             .countdown-number { font-size: 120px; font-weight: bold; color: white; }",
        );
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("No display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        window.set_child(Some(&label));

        // Allow Esc to skip countdown
        let window_esc = window.downgrade();
        let key_controller = EventControllerKey::builder()
            .propagation_phase(gtk4::PropagationPhase::Capture)
            .build();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == Key::Escape {
                if let Some(win) = window_esc.upgrade() {
                    win.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key_controller);

        window.present();

        // Tick down every second
        let remaining_timer = remaining_act.clone();
        let done_timer = done_act.clone();
        let window_ref = window.downgrade();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let val = remaining_timer.get();
            if val <= 1 || done_timer.get() {
                if let Some(win) = window_ref.upgrade() {
                    win.close();
                }
                done_timer.set(true);
                return glib::ControlFlow::Break;
            }
            remaining_timer.set(val - 1);
            label.set_text(&format!("{}", val - 1));
            glib::ControlFlow::Continue
        });
    });

    // Run the app — blocks until window closes
    app.run_with_args::<String>(&[]);
}
