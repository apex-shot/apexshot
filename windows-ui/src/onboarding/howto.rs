//! How to capture: where captures start, and the bindings already saved.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};

use super::ui::heading;
use super::{px, Nav, SECTION_GAP};
use crate::config::load_config;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let scale = nav.scale;
    body.append(&heading(
        scale,
        &t("One shortcut away."),
        &t("Start a capture from your keyboard or the tray menu."),
    ));

    let config = load_config().sanitized();
    let area = display_shortcut(&config.shortcut_capture_area);
    let screen = display_shortcut(&config.shortcut_capture_fullscreen);

    let hotkeys_block = GtkBox::new(Orientation::Vertical, px(scale, 8.0));
    hotkeys_block.set_margin_top(px(scale, SECTION_GAP));
    hotkeys_block.set_halign(Align::Fill);

    let hotkeys_hint = Label::new(Some(&t(
        "Shortcuts work while ApexShot runs in the background.",
    )));
    hotkeys_hint.set_halign(Align::Start);
    hotkeys_hint.set_xalign(0.0);
    hotkeys_hint.set_wrap(true);
    hotkeys_hint.set_max_width_chars(44);
    hotkeys_hint.add_css_class("settings-sub-option");

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-hotkey-table");
    frame.set_hexpand(true);
    frame.append(&build_hotkey_row(scale, &t("Area capture"), &area));
    frame.append(&build_hotkey_row(scale, &t("Full screen"), &screen));
    hotkeys_block.append(&frame);
    hotkeys_hint.set_margin_top(px(scale, 8.0));
    hotkeys_block.append(&hotkeys_hint);
    body.append(&hotkeys_block);

    let continue_btn = Button::with_label(&t("Continue"));
    continue_btn.add_css_class("settings-primary-btn");
    let advance = nav.advance.clone();
    continue_btn.connect_clicked(move |_| advance());
    actions.append(&continue_btn);
}

fn display_shortcut(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        t("Disabled")
    } else {
        trimmed.to_string()
    }
}

fn build_hotkey_row(scale: f64, action: &str, shortcut: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, px(scale, 16.0));
    row.add_css_class("settings-table-row");
    row.set_hexpand(true);
    row.set_halign(Align::Fill);

    let action_label = Label::new(Some(action));
    action_label.set_xalign(0.0);
    action_label.set_halign(Align::Start);
    action_label.set_hexpand(true);

    let shortcut_box = GtkBox::new(Orientation::Horizontal, px(scale, 4.0));
    shortcut_box.set_halign(Align::End);
    shortcut_box.set_hexpand(false);

    {
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
