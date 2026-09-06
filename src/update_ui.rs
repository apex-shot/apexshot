//! Compact, non-blocking update card shared by ApexShot windows.

use gtk4::{
    glib::{self, ControlFlow},
    prelude::*,
    Align, Box as GtkBox, Button, CssProvider, Label, Orientation, Overlay,
};
use std::{sync::mpsc, time::Duration};

const UPDATE_CSS: &str = r#"
    .apexshot-update-surface {
        /* Matches .settings-table-frame: #141414 behind alpha(white, 0.04). */
        background: #1d1d1d;
        color: #f1f1f3;
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 14px;
        box-shadow: 0 18px 50px rgba(0, 0, 0, 0.42);
        padding: 16px;
        font-family: 'Inter', 'Noto Sans', system-ui, sans-serif;
    }
    .apexshot-update-title { font-size: 14px; font-weight: 700; }
    .apexshot-update-copy { font-size: 12px; color: rgba(241, 241, 243, 0.64); }
    button.apexshot-update-close {
        min-width: 28px; min-height: 28px; padding: 0; border-radius: 999px;
        border: none; background: transparent; color: rgba(241, 241, 243, 0.66);
    }
    button.apexshot-update-close:hover { background: rgba(255, 255, 255, 0.08); color: #fff; }
    button.apexshot-update-primary {
        min-height: 32px; padding: 4px 14px; border: none; border-radius: 7px;
        background: #b05c38; color: white; font-size: 12px; font-weight: 700;
    }
    button.apexshot-update-primary:hover { background: #c06540; }
    button.apexshot-update-link {
        padding: 0; min-height: 0; border: none; background: transparent;
        color: #d9815e; font-size: 12px;
    }
    button.apexshot-update-link:hover { color: #efa17e; text-decoration: underline; }
"#;

fn install_css() {
    if let Some(display) = gtk4::gdk::Display::default() {
        let provider = CssProvider::new();
        provider.load_from_data(UPDATE_CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Shows an in-window card once a cached/background check finds a newer
/// version. Using an Overlay pins it to the bottom right on Wayland and X11.
pub fn present_if_needed(overlay: &Overlay) {
    let overlay = overlay.clone();
    if let Some(update) = crate::update::cached_update() {
        present_card(&overlay, update);
        return;
    }

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(crate::update::check_for_update());
    });
    glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
        Ok(Some(update)) => {
            present_card(&overlay, update);
            ControlFlow::Break
        }
        Ok(None) | Err(mpsc::TryRecvError::Disconnected) => ControlFlow::Break,
        Err(mpsc::TryRecvError::Empty) => ControlFlow::Continue,
    });
}

fn present_card(overlay: &Overlay, update: crate::update::UpdateInfo) {
    if crate::update::prompt_is_snoozed(&update) {
        return;
    }
    install_css();

    let surface = GtkBox::new(Orientation::Vertical, 12);
    surface.add_css_class("apexshot-update-surface");
    surface.set_width_request(370);
    surface.set_halign(Align::End);
    surface.set_valign(Align::End);
    surface.set_margin_end(16);
    surface.set_margin_bottom(16);

    let heading = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some("Update available"));
    title.add_css_class("apexshot-update-title");
    title.set_halign(Align::Start);
    title.set_hexpand(true);
    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("apexshot-update-close");
    close.set_tooltip_text(Some("Close"));
    heading.append(&title);
    heading.append(&close);

    let copy = Label::new(Some(&format!(
        "ApexShot v{} is ready. Your current work will not be interrupted.",
        update.version
    )));
    copy.add_css_class("apexshot-update-copy");
    copy.set_wrap(true);
    copy.set_halign(Align::Start);
    copy.set_xalign(0.0);

    let release_notes = Button::with_label("Release notes");
    release_notes.add_css_class("apexshot-update-link");
    release_notes.set_halign(Align::Start);
    let release_url = update.release_url.clone();
    release_notes.connect_clicked(move |_| {
        let release_url = release_url.clone();
        std::thread::spawn(move || {
            let _ = crate::utils::open::open_url(&release_url);
        });
    });

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::Fill);
    actions.set_hexpand(true);
    let update_btn = Button::with_label("Update ApexShot");
    update_btn.add_css_class("apexshot-update-primary");
    let action_spacer = GtkBox::new(Orientation::Horizontal, 0);
    action_spacer.set_hexpand(true);
    actions.append(&release_notes);
    actions.append(&action_spacer);
    actions.append(&update_btn);

    surface.append(&heading);
    surface.append(&copy);
    surface.append(&actions);

    let close_surface = surface.clone();
    let close_overlay = overlay.clone();
    let update_for_close = update.clone();
    close.connect_clicked(move |_| {
        crate::update::snooze_prompt(&update_for_close);
        close_overlay.remove_overlay(&close_surface);
    });
    let update_for_action = update.clone();
    let update_surface = surface.clone();
    let update_overlay = overlay.clone();
    update_btn.connect_clicked(move |_| {
        crate::update::snooze_prompt(&update_for_action);
        if let Err(err) = crate::update::launch_update() {
            eprintln!("[update] {err}");
        }
        update_overlay.remove_overlay(&update_surface);
    });
    overlay.add_overlay(&surface);
    overlay.set_clip_overlay(&surface, false);
}
