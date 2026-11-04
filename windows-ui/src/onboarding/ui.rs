//! Shared onboarding building blocks (headings, feature rows, option cards,
//! tips), scaled to this display's DPI.

use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, Image, Label, Orientation};

use super::px;
use crate::i18n::{t, tfmt};

/// Open `url` in the browser. `status` stays hidden unless that fails, so a
/// working browser adds no text to the page.
pub fn open_link(url: &str, status: &Label) {
    status.set_visible(false);
    let (tx, rx) = async_channel::bounded(1);
    let url = url.to_string();
    std::thread::spawn(move || {
        let _ = tx.send_blocking(crate::utils::open::open_url(&url));
    });
    let status = status.clone();
    glib::spawn_future_local(async move {
        let Ok(result) = rx.recv().await else {
            return;
        };
        if let Err(error) = result {
            status.set_text(&tfmt(
                "Couldn't open a browser: {error}. Use Copy link and open it yourself.",
                &[("error", &error.to_string())],
            ));
            status.set_visible(true);
        }
    });
}

pub fn copy_with_feedback(button: &Button, value: &str, idle: &str) {
    let feedback = match crate::utils::clipboard::copy_text_to_gtk_clipboard(value) {
        Ok(()) => t("Copied"),
        Err(error) => {
            eprintln!("[onboarding] failed to copy: {error}");
            t("Copy failed")
        }
    };
    button.set_label(&feedback);
    let button = button.clone();
    let idle = idle.to_string();
    glib::timeout_add_local_once(Duration::from_millis(1600), move || {
        button.set_label(&idle);
    });
}

/// Heading block: title over supporting copy, centered under the app mark.
pub fn heading(scale: f64, title: &str, copy: &str) -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, px(scale, 8.0));
    block.set_halign(Align::Center);

    // The character caps keep a wrapped label's natural width inside the
    // pane's design width; without one the label would ask for its full
    // unwrapped width and stretch the pane.
    for (text, class, max_chars) in [
        (title, "onboarding-headline", 26),
        (copy, "onboarding-copy", 36),
    ] {
        let label = Label::new(Some(text));
        label.add_css_class(class);
        label.set_halign(Align::Center);
        label.set_xalign(0.5);
        label.set_justify(gtk4::Justification::Center);
        label.set_wrap(true);
        label.set_max_width_chars(max_chars);
        block.append(&label);
    }

    block
}

/// A quiet, unframed list of icon-and-copy rows.
pub fn feature_card_list(scale: f64, items: &[(&str, &str, &str)]) -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-card-list");
    frame.set_halign(Align::Fill);
    frame.set_hexpand(true);

    for (icon, title, subtitle) in items {
        frame.append(&feature_card_row(scale, icon, title, subtitle));
    }

    frame
}

fn feature_card_row(scale: f64, icon: &str, title: &str, subtitle: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, px(scale, 14.0));
    row.add_css_class("settings-table-row");
    row.add_css_class("onboarding-feature-row");
    row.set_hexpand(true);

    let badge = Image::from_icon_name(icon);
    badge.add_css_class("onboarding-icon-badge");
    badge.set_pixel_size(px(scale, 18.0));
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    badge.set_size_request(px(scale, 34.0), px(scale, 34.0));
    row.append(&badge);

    let text = GtkBox::new(Orientation::Vertical, px(scale, 2.0));
    text.set_halign(Align::Start);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_label = Label::new(None);
    title_label.set_markup(&format!(
        "<span weight='bold'>{}</span>",
        escape_markup(title)
    ));
    title_label.set_halign(Align::Start);
    title_label.set_xalign(0.0);
    text.append(&title_label);

    if !subtitle.is_empty() {
        let sub = Label::new(Some(subtitle));
        sub.add_css_class("settings-sub-option");
        sub.set_halign(Align::Start);
        sub.set_xalign(0.0);
        sub.set_wrap(true);
        sub.set_max_width_chars(40);
        text.append(&sub);
    }

    row.append(&text);
    row
}

/// Small pill/badge label (e.g. the plan name on the connected view).
pub fn status_pill(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("onboarding-status-pill");
    label.set_halign(Align::Start);
    label
}

pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The one-line reason a device-authorization sign-in failed, for the status
/// line. Shared by the onboarding connect view and the Settings sign-in
/// dialog.
pub fn login_error_message(error: &crate::cloud::auth::LoginError) -> String {
    use crate::cloud::auth::LoginError;
    match error {
        LoginError::Expired => t("This code expired. Try again to get a new one."),
        LoginError::Denied => t("Authorization was denied in the browser."),
        LoginError::NotConfigured | LoginError::HttpRequest(_) | LoginError::Server(_) => {
            t("Couldn't reach ApexShot Cloud. Check your connection and try again.")
        }
        LoginError::Local(_) => t("Signed in, but this device could not save the account."),
    }
}
