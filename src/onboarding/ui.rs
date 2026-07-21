//! Shared onboarding UI building blocks (feature cards, option cards, tips).

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// Vertical stack of icon + title (+ optional subtitle) feature cards in a framed list.
pub fn feature_card_list(items: &[(&str, &str, &str)]) -> GtkBox {
    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-card-list");
    frame.set_halign(Align::Center);
    frame.set_width_request(480);
    frame.set_hexpand(false);

    for (idx, (icon, title, subtitle)) in items.iter().enumerate() {
        let muted = idx % 2 == 1;
        frame.append(&feature_card_row(icon, title, subtitle, muted));
    }

    frame
}

/// Single feature row: badge | title + subtitle.
pub fn feature_card_row(icon: &str, title: &str, subtitle: &str, muted: bool) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 14);
    row.add_css_class("settings-table-row");
    row.add_css_class("onboarding-feature-row");
    if muted {
        row.add_css_class("settings-table-row-muted");
    }
    row.set_hexpand(true);

    let badge = Label::new(Some(icon));
    badge.add_css_class("onboarding-icon-badge");
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    badge.set_size_request(40, 40);
    row.append(&badge);

    let text = GtkBox::new(Orientation::Vertical, 2);
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
        sub.set_width_request(360);
        text.append(&sub);
    }

    row.append(&text);
    row
}

/// Side-by-side or stacked choice cards (e.g. cloud destinations).
pub fn option_card(icon: &str, title: &str, body: &str) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 8);
    card.add_css_class("settings-table-frame");
    card.add_css_class("onboarding-option-card");
    card.set_halign(Align::Fill);
    card.set_hexpand(true);
    card.set_width_request(220);

    let header = GtkBox::new(Orientation::Horizontal, 10);
    header.set_halign(Align::Start);

    let badge = Label::new(Some(icon));
    badge.add_css_class("onboarding-icon-badge");
    badge.set_size_request(36, 36);
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
    body_label.set_width_request(200);
    card.append(&body_label);

    card
}

/// Compact tip block with title + body (used on How to capture, Complete).
pub fn tip_block(title: &str, body: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Vertical, 4);
    row.set_halign(Align::Start);
    row.set_width_request(480);

    let title_label = Label::new(None);
    title_label.set_markup(&format!(
        "<span weight='bold'>{}</span>",
        escape_markup(title)
    ));
    title_label.set_halign(Align::Start);
    row.append(&title_label);

    let body_label = Label::new(Some(body));
    body_label.set_halign(Align::Start);
    body_label.set_wrap(true);
    body_label.set_width_request(480);
    body_label.add_css_class("settings-sub-option");
    row.append(&body_label);

    row
}

/// Small pill/badge label (e.g. "Available now", "Optional").
pub fn status_pill(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("onboarding-status-pill");
    label.set_halign(Align::Center);
    label
}

pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
