//! Shared onboarding building blocks: centred headings, flat icon rows and
//! the small status pill the cloud pages reuse.

use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, Image, Label, Orientation};

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

/// Heading block: title over supporting copy, centered under the page mark.
pub fn heading(title: &str, copy: &str) -> GtkBox {
    heading_parts(title, copy).block
}

pub struct HeadingParts {
    pub block: GtkBox,
    pub title: Label,
    pub copy: Label,
}

/// Heading block with its labels exposed for a page-specific text reveal.
pub fn heading_parts(title: &str, copy: &str) -> HeadingParts {
    let block = GtkBox::new(Orientation::Vertical, 8);
    block.set_halign(Align::Center);

    // The character caps keep a wrapped label's natural width inside the
    // pane's design width; without one the label would ask for its full
    // unwrapped width and stretch the pane.
    let mut title_label = None;
    let mut copy_label = None;
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
        if class == "onboarding-headline" {
            title_label = Some(label);
        } else {
            copy_label = Some(label);
        }
    }

    HeadingParts {
        block,
        title: title_label.expect("heading always has a title"),
        copy: copy_label.expect("heading always has supporting copy"),
    }
}

/// A quiet, unframed list of icon-and-copy rows.
pub fn feature_card_list(items: &[(&str, &str, &str)]) -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-card-list");
    frame.set_halign(Align::Fill);
    frame.set_hexpand(true);

    for (icon, title, subtitle) in items {
        frame.append(&feature_card_row(icon, title, subtitle));
    }

    frame
}

fn feature_card_row(icon: &str, title: &str, subtitle: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-table-row");
    row.add_css_class("onboarding-feature-row");
    row.set_hexpand(true);

    let badge = Image::from_icon_name(icon);
    badge.add_css_class("onboarding-icon-badge");
    badge.set_pixel_size(18);
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    badge.set_size_request(28, 28);
    row.append(&badge);

    let text = GtkBox::new(Orientation::Vertical, 2);
    text.set_halign(Align::Start);
    text.set_hexpand(true);
    text.set_valign(Align::Center);

    let title_label = Label::new(Some(title));
    title_label.add_css_class("onboarding-feature-title");
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
