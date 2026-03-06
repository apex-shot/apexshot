use gtk4::gdk::Key;
use gtk4::{
    glib::{self, clone},
    prelude::*,
    Application, ApplicationWindow, Button, EventControllerKey, Label, Orientation,
};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Debug, Error)]
pub enum StopOverlayError {
    #[error("GTK initialization failed: {0}")]
    InitError(String),
}

/// Shows a small window that lets the user stop an in-progress recording.
///
/// - Press `Esc` or click the button to stop.
/// - This is intended for Wayland hotkey workflows where you can't rely on Ctrl+C.
pub fn run_recording_stop_overlay(stop_tx: oneshot::Sender<()>) -> Result<(), StopOverlayError> {
    let stop_tx: Arc<Mutex<Option<oneshot::Sender<()>>>> = Arc::new(Mutex::new(Some(stop_tx)));

    let app = Application::builder()
        .application_id("com.cleanshitx.recording")
        .build();

    let stop_tx_activate = stop_tx.clone();
    app.connect_activate(move |application| {
        setup_window(application, stop_tx_activate.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn setup_window(app: &Application, stop_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Recording")
        .default_width(360)
        .default_height(120)
        .decorated(false)
        .resizable(false)
        .build();

    let vbox = gtk4::Box::new(Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let label = Label::new(Some("Recording… Press Esc to stop"));
    label.set_xalign(0.0);

    let btn = Button::with_label("Stop");

    let window_weak = window.downgrade();
    let stop_tx_btn = stop_tx.clone();
    btn.connect_clicked(clone!(
        #[strong]
        stop_tx_btn,
        move |_| {
            send_stop(&stop_tx_btn);
            if let Some(window) = window_weak.upgrade() {
                window.close();
            }
        }
    ));

    vbox.append(&label);
    vbox.append(&btn);

    window.set_child(Some(&vbox));

    let key_controller = EventControllerKey::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let stop_tx_key = stop_tx.clone();
    let window_weak_esc = window.downgrade();
    key_controller.connect_key_pressed(clone!(
        #[strong]
        stop_tx_key,
        move |_, key, _, _| {
            if key == Key::Escape {
                send_stop(&stop_tx_key);
                if let Some(window) = window_weak_esc.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));

    window.add_controller(key_controller);
    window.present();
}

fn send_stop(stop_tx: &Arc<Mutex<Option<oneshot::Sender<()>>>>) {
    if let Some(tx) = stop_tx.lock().ok().and_then(|mut g| g.take()) {
        let _ = tx.send(());
    }
}
