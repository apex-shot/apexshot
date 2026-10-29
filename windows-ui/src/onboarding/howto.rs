//! How to capture: where captures start, and the bindings already saved.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};

use super::ui::{escape_markup, heading, tip_block};
use super::{px, Nav};
use crate::config::load_config;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let scale = nav.scale;
    body.append(&heading(
        scale,
        &t("How to capture"),
        &t("ApexShot runs in the background with a tray icon.\nAfter setup, you do not need to open Settings every time."),
    ));

    let config = load_config().sanitized();
    let area = display_shortcut(&config.shortcut_capture_area, "Shift+Super+4");
    let screen = display_shortcut(&config.shortcut_capture_fullscreen, "Shift+Super+3");

    let hotkeys_block = GtkBox::new(Orientation::Vertical, px(scale, 8.0));
    hotkeys_block.set_margin_top(px(scale, 28.0));
    hotkeys_block.set_halign(Align::Fill);

    let hotkeys_hint = Label::new(Some(&t(
        "Global shortcuts are not available in the Windows build yet. Bindings are saved for a future release.",
    )));
    hotkeys_hint.set_halign(Align::Start);
    hotkeys_hint.set_xalign(0.0);
    hotkeys_hint.set_wrap(true);
    hotkeys_hint.add_css_class("settings-sub-option");
    hotkeys_block.append(&hotkeys_hint);

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-hotkey-table");
    frame.set_hexpand(true);
    frame.append(&build_hotkey_row(scale, &t("Action"), &t("Shortcut"), true));
    frame.append(&build_hotkey_row(scale, &t("Area capture"), &area, false));
    frame.append(&build_hotkey_row(scale, &t("Full screen"), &screen, false));
    hotkeys_block.append(&frame);
    body.append(&hotkeys_block);

    let tips = GtkBox::new(Orientation::Vertical, px(scale, 14.0));
    tips.set_margin_top(px(scale, 24.0));
    tips.append(&tip_block(
        scale,
        &t("Tray icon"),
        &t("ApexShot stays in the tray; right-click it for the capture menu."),
    ));
    tips.append(&tip_block(
        scale,
        &t("Settings"),
        &t("Open ApexShot anytime for Settings and preferences"),
    ));
    body.append(&tips);

    let continue_btn = Button::with_label(&t("Continue"));
    continue_btn.add_css_class("settings-primary-btn");
    let advance = nav.advance.clone();
    continue_btn.connect_clicked(move |_| advance());
    actions.append(&continue_btn);
}

fn display_shortcut(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_hotkey_row(scale: f64, action: &str, shortcut: &str, is_header: bool) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, px(scale, 16.0));
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row.set_halign(Align::Fill);

    let action_label = Label::new(None);
    if is_header {
        action_label.set_markup(&format!(
            "<span weight='bold' size='small'>{}</span>",
            escape_markup(action)
        ));
        action_label.add_css_class("settings-table-header");
    } else {
        action_label.set_text(action);
    }
    action_label.set_xalign(0.0);
    action_label.set_halign(Align::Start);
    action_label.set_hexpand(true);

    let shortcut_box = GtkBox::new(Orientation::Horizontal, px(scale, 4.0));
    shortcut_box.set_halign(Align::End);
    shortcut_box.set_hexpand(false);

    if is_header {
        let shortcut_label = Label::new(None);
        shortcut_label.set_markup(&format!(
            "<span weight='bold' size='small'>{}</span>",
            escape_markup(shortcut)
        ));
        shortcut_label.add_css_class("settings-table-header");
        shortcut_label.set_xalign(1.0);
        shortcut_box.append(&shortcut_label);
    } else {
        // Split "Ctrl+Alt+R" into keycap chips for readability.
        let parts: Vec<&str> = shortcut
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                let plus = Label::new(Some("+"));
                plus.add_css_class("shortcut-capture-plus");
                plus.set_margin_start(px(scale, 2.0));
                plus.set_margin_end(px(scale, 2.0));
                shortcut_box.append(&plus);
            }
            let keycap = Label::new(Some(part));
            keycap.add_css_class("shortcut-capture-keycap");
            keycap.set_xalign(0.5);
            shortcut_box.append(&keycap);
        }
        if parts.is_empty() {
            let empty = Label::new(Some("—"));
            empty.add_css_class("dim-label");
            shortcut_box.append(&empty);
        }
    }

    row.append(&action_label);
    row.append(&shortcut_box);
    row
}
