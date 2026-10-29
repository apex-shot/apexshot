//! Shared onboarding building blocks (headings, feature rows, option cards,
//! tips), scaled to this display's DPI.

use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, Image, Label, Orientation};

use super::px;
use crate::i18n::{t, tfmt};

pub fn open_link(url: &str, status: &Label) {
    status.set_text(&t("Opening your browser…"));
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
        let message = match result {
            Ok(()) => t("Sent to your browser. If no page opened, copy the link below."),
            Err(error) => tfmt(
                "Couldn't open a browser: {error}. Copy the link below and open it yourself.",
                &[("error", &error.to_string())],
            ),
        };
        status.set_text(&message);
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

/// Heading block: title over supporting copy, left-aligned for most steps.
pub fn heading(scale: f64, title: &str, copy: &str) -> GtkBox {
    heading_with_alignment(scale, title, copy, Align::Start)
}

/// Heading block centered like the Welcome step, for views that read as one
/// centered column.
pub fn heading_centered(scale: f64, title: &str, copy: &str) -> GtkBox {
    heading_with_alignment(scale, title, copy, Align::Center)
}

fn heading_with_alignment(scale: f64, title: &str, copy: &str, alignment: Align) -> GtkBox {
    let centered = alignment == Align::Center;
    let block = GtkBox::new(Orientation::Vertical, px(scale, 8.0));
    block.set_halign(if centered { Align::Center } else { Align::Fill });

    let label_align = if centered {
        Align::Center
    } else {
        Align::Start
    };
    let xalign = if centered { 0.5 } else { 0.0 };
    let justify = if centered {
        gtk4::Justification::Center
    } else {
        gtk4::Justification::Left
    };

    let title_label = Label::new(Some(title));
    title_label.add_css_class("onboarding-headline");
    title_label.set_halign(label_align);
    title_label.set_xalign(xalign);
    title_label.set_justify(justify);
    title_label.set_wrap(true);
    block.append(&title_label);

    let copy_label = Label::new(Some(copy));
    copy_label.add_css_class("onboarding-copy");
    copy_label.set_halign(label_align);
    copy_label.set_xalign(xalign);
    copy_label.set_justify(justify);
    copy_label.set_wrap(true);
    block.append(&copy_label);

    block
}

/// Vertical stack of icon + title (+ optional subtitle) rows in a framed list.
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
        text.append(&sub);
    }

    row.append(&text);
    row
}

/// Side-by-side choice card (e.g. cloud destinations).
pub fn option_card(scale: f64, icon: &str, title: &str, body: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, px(scale, 10.0));
    card.add_css_class("settings-table-frame");
    card.add_css_class("onboarding-option-card");
    card.set_halign(Align::Fill);
    card.set_hexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, px(scale, 10.0));
    header.set_halign(Align::Start);

    let badge = Image::from_icon_name(icon);
    badge.add_css_class("onboarding-option-icon");
    badge.set_pixel_size(px(scale, 22.0));
    badge.set_size_request(px(scale, 32.0), px(scale, 32.0));
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    header.append(&badge);

    let title_label = Label::new(None);
    title_label.set_markup(&format!(
        "<span weight='bold'>{}</span>",
        escape_markup(title)
    ));
    title_label.set_halign(Align::Start);
    title_label.set_valign(Align::Center);
    title_label.set_wrap(true);
    header.append(&title_label);

    card.append(&header);

    let body_label = Label::new(Some(body));
    body_label.add_css_class("settings-sub-option");
    body_label.set_halign(Align::Start);
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    card.append(&body_label);

    card
}

/// Compact tip block with title + body.
pub fn tip_block(scale: f64, title: &str, body: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, px(scale, 4.0));
    row.set_halign(Align::Start);

    let title_label = Label::new(None);
    title_label.set_markup(&format!(
        "<span weight='bold'>{}</span>",
        escape_markup(title)
    ));
    title_label.set_halign(Align::Start);
    title_label.set_xalign(0.0);
    row.append(&title_label);

    let body_label = Label::new(Some(body));
    body_label.set_halign(Align::Start);
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    body_label.add_css_class("settings-sub-option");
    row.append(&body_label);

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
