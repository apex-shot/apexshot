use gtk4::gdk::Key;
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, EventControllerKey, Label, Orientation, Window,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Show a fullscreen 3-2-1 countdown overlay before recording starts.
/// Blocks until the countdown completes.
pub fn run_countdown_overlay(seconds: u32) {
    if gtk4::init().is_err() {
        return;
    }

    let remaining = Rc::new(Cell::new(seconds));
    let done = Rc::new(Cell::new(false));
    let main_loop = glib::MainLoop::new(None, false);

    let window = Window::builder().decorated(false).resizable(false).build();
    window.fullscreen();
    window.set_css_classes(&["countdown-overlay"]);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_halign(Align::Center);
    root.set_valign(Align::Center);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let bubble = GtkBox::new(Orientation::Vertical, 0);
    bubble.set_halign(Align::Center);
    bubble.set_valign(Align::Center);
    bubble.set_css_classes(&["countdown-bubble"]);

    let label = Label::new(Some(&format!("{}", remaining.get())));
    label.set_css_classes(&["countdown-number"]);
    label.set_xalign(0.5);
    label.set_yalign(0.5);
    bubble.append(&label);
    root.append(&bubble);

    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        ".countdown-overlay { background-color: transparent; } \
         .countdown-bubble { min-width: 184px; min-height: 184px; padding: 0; border-radius: 92px; background-color: rgba(0, 0, 0, 0.94); } \
         .countdown-number { font-size: 92px; font-weight: 700; color: white; }",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    window.set_child(Some(&root));

    let loop_for_close = main_loop.clone();
    window.connect_close_request(move |_| {
        loop_for_close.quit();
        glib::Propagation::Proceed
    });

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

    // Start ticking only after the window is mapped so the first number is
    // actually visible for a full second (avoids "3" being skipped when map
    // latency eats the first tick).
    let timer_started = Rc::new(Cell::new(false));
    window.connect_map(glib::clone!(
        #[strong]
        remaining,
        #[strong]
        done,
        #[strong]
        main_loop,
        #[strong]
        label,
        #[strong]
        timer_started,
        #[weak]
        window,
        move |_| {
            if timer_started.replace(true) {
                return;
            }
            let remaining_timer = remaining.clone();
            let done_timer = done.clone();
            let loop_for_timer = main_loop.clone();
            let window_ref = window.downgrade();
            let label = label.clone();
            glib::timeout_add_local(Duration::from_secs(1), move || {
                let val = remaining_timer.get();
                if val <= 1 || done_timer.get() {
                    done_timer.set(true);
                    if let Some(win) = window_ref.upgrade() {
                        win.close();
                    }
                    loop_for_timer.quit();
                    return glib::ControlFlow::Break;
                }
                remaining_timer.set(val - 1);
                label.set_text(&format!("{}", val - 1));
                glib::ControlFlow::Continue
            });
        }
    ));

    window.present();
    main_loop.run();
}
