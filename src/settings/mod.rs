use crate::config::{
    load_config, save_config, AppConfig, MAX_PREVIEW_AUTO_CLOSE_SECONDS,
    MIN_PREVIEW_AUTO_CLOSE_SECONDS,
};
use gtk4::{
    glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Label,
    Orientation, SpinButton,
};

pub fn show_settings_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id("com.cleanshitx.settings")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(build_settings_window);
    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_settings_window(app: &Application) {
    let config = load_config().sanitized();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("CleanShotX Settings")
        .default_width(420)
        .default_height(220)
        .resizable(false)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let heading = Label::new(Some("Preferences"));
    heading.set_halign(Align::Start);
    heading.add_css_class("title-3");

    let auto_close_row = GtkBox::new(Orientation::Horizontal, 12);
    auto_close_row.set_halign(Align::Fill);

    let auto_close_label = Label::new(Some("Preview auto-dismiss (seconds)"));
    auto_close_label.set_halign(Align::Start);
    auto_close_label.set_hexpand(true);

    let auto_close_spin = SpinButton::with_range(
        MIN_PREVIEW_AUTO_CLOSE_SECONDS as f64,
        MAX_PREVIEW_AUTO_CLOSE_SECONDS as f64,
        1.0,
    );
    auto_close_spin.set_value(config.preview_auto_close_seconds as f64);
    auto_close_spin.set_halign(Align::End);
    auto_close_spin.set_numeric(true);
    auto_close_spin.set_width_chars(4);

    auto_close_row.append(&auto_close_label);
    auto_close_row.append(&auto_close_spin);

    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.set_halign(Align::End);

    let cancel_btn = Button::with_label("Cancel");
    let save_btn = Button::with_label("Save");
    save_btn.add_css_class("suggested-action");

    footer.append(&cancel_btn);
    footer.append(&save_btn);

    root.append(&heading);
    root.append(&auto_close_row);
    root.append(&footer);

    window.set_child(Some(&root));

    let window_weak_cancel = window.downgrade();
    cancel_btn.connect_clicked(move |_| {
        if let Some(window) = window_weak_cancel.upgrade() {
            window.close();
        }
    });

    let spin_save = auto_close_spin.clone();
    let window_weak_save = window.downgrade();
    save_btn.connect_clicked(move |_| {
        let mut config = load_config();
        config.preview_auto_close_seconds = spin_save.value_as_int().max(0) as u32;
        let config = AppConfig {
            preview_auto_close_seconds: config.preview_auto_close_seconds,
        }
        .sanitized();

        if let Err(e) = save_config(&config) {
            eprintln!("[settings] Failed to save config: {e}");
        }

        if let Some(window) = window_weak_save.upgrade() {
            window.close();
        }
    });

    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });

    window.present();
}
