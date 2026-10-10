use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};

use super::ui::{escape_markup, heading, tip_block};
use super::Nav;
use crate::config::load_config;
use crate::daemon::{ensure_daemon_running, trigger_daemon_action_sync};
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    body.append(&heading(
        &t("How to capture"),
        &t("ApexShot runs in the background with a tray icon and hotkeys.\nAfter setup, you do not need to open Settings every time."),
    ));

    let config = load_config().sanitized();
    let area = display_shortcut(&config.shortcut_capture_area, "Shift+Super+4");
    let screen = display_shortcut(&config.shortcut_capture_fullscreen, "Shift+Super+3");
    let record = display_shortcut(&config.shortcut_open_recording_ui, "Ctrl+Alt+R");

    let hotkeys_block = GtkBox::new(Orientation::Vertical, 8);
    hotkeys_block.set_margin_top(28);
    hotkeys_block.set_halign(Align::Fill);

    let hotkeys_hint = Label::new(Some(&if crate::app_identity::portal_only() {
        t("These defaults require desktop GlobalShortcuts portal support and approval. Use the tray if shortcuts are unavailable.")
    } else {
        t("Defaults below. Change them anytime in Settings → Shortcuts.")
    }));
    hotkeys_hint.set_halign(Align::Start);
    hotkeys_hint.set_xalign(0.0);
    hotkeys_hint.set_wrap(true);
    hotkeys_hint.add_css_class("settings-sub-option");
    hotkeys_block.append(&hotkeys_hint);

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.add_css_class("onboarding-hotkey-table");
    frame.set_hexpand(true);
    frame.append(&build_hotkey_row(&t("Action"), &t("Shortcut"), true));
    frame.append(&build_hotkey_row(&t("Area capture"), &area, false));
    frame.append(&build_hotkey_row(&t("Full screen"), &screen, false));
    frame.append(&build_hotkey_row(&t("Record UI"), &record, false));
    hotkeys_block.append(&frame);
    body.append(&hotkeys_block);

    let tips = GtkBox::new(Orientation::Vertical, 14);
    tips.set_margin_top(24);
    tips.append(&tip_block(
        &t("Tray icon"),
        &t("Right-click for Area, Screen, Window, and Record"),
    ));
    tips.append(&tip_block(
        &t("App menu"),
        &t("Opening ApexShot shows Settings; captures stay on tray and hotkeys"),
    ));
    body.append(&tips);

    let try_btn = Button::with_label(&t("Take a test screenshot"));
    try_btn.add_css_class("secondary-settings-button");
    try_btn.set_tooltip_text(Some(&t(
        "Starts the tray daemon if needed, then takes a full screenshot",
    )));
    try_btn.connect_clicked(|_| {
        std::thread::spawn(|| {
            if !ensure_daemon_running() {
                eprintln!("[onboarding] Could not start daemon for test capture");
                // Fallback: one-shot CLI path without daemon.
                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
                let _ = std::process::Command::new(exe)
                    .args(["capture", "screen"])
                    .spawn();
                return;
            }
            if !trigger_daemon_action_sync("capture_screen") {
                eprintln!("[onboarding] Daemon did not accept capture_screen");
                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
                let _ = std::process::Command::new(exe)
                    .args(["capture", "screen"])
                    .spawn();
            }
        });
    });
    actions.append(&try_btn);

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

fn build_hotkey_row(action: &str, shortcut: &str, is_header: bool) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 16);
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

    let shortcut_box = GtkBox::new(Orientation::Horizontal, 4);
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
            .filter(|p| !p.is_empty())
            .collect();
        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                let plus = Label::new(Some("+"));
                plus.add_css_class("shortcut-capture-plus");
                plus.set_margin_start(2);
                plus.set_margin_end(2);
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
