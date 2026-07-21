use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};

use super::ui::feature_card_list;
use crate::config::load_config;
use crate::daemon::{ensure_daemon_running, trigger_daemon_action_sync};

pub fn build(content: &GtkBox) {
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>How to capture</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let subtitle = Label::new(Some(
        "ApexShot runs in the background with a tray icon and hotkeys.\n\
         After setup, you do not need to open Settings every time.",
    ));
    subtitle.set_halign(Align::Center);
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk4::Justification::Center);
    subtitle.set_width_request(520);
    subtitle.add_css_class("settings-sub-option");
    subtitle.set_margin_bottom(12);
    content.append(&subtitle);

    let config = load_config().sanitized();
    let area = display_shortcut(&config.shortcut_capture_area, "Shift+Super+4");
    let screen = display_shortcut(&config.shortcut_capture_fullscreen, "Shift+Super+3");
    let record = display_shortcut(&config.shortcut_open_recording_ui, "Ctrl+Alt+R");

    let tips = feature_card_list(&[
        (
            "◉",
            "Tray icon",
            "Right-click for Area, Screen, Window, and Record",
        ),
        (
            "☰",
            "App menu",
            "Opening ApexShot shows Settings; captures stay on tray and hotkeys",
        ),
    ]);
    tips.set_margin_bottom(14);
    content.append(&tips);

    // Hotkeys table
    let hotkeys_block = GtkBox::new(Orientation::Vertical, 6);
    hotkeys_block.set_halign(Align::Center);
    hotkeys_block.set_width_request(480);

    let hotkeys_title = Label::new(None);
    hotkeys_title.set_markup("<span weight='bold'>Hotkeys</span>");
    hotkeys_title.set_halign(Align::Start);
    hotkeys_block.append(&hotkeys_title);

    let hotkeys_hint = Label::new(Some(
        "Defaults below. Change them anytime in Settings → Shortcuts.",
    ));
    hotkeys_hint.set_halign(Align::Start);
    hotkeys_hint.set_wrap(true);
    hotkeys_hint.add_css_class("settings-sub-option");
    hotkeys_hint.set_margin_bottom(2);
    hotkeys_block.append(&hotkeys_hint);

    let frame = GtkBox::new(Orientation::Vertical, 0);
    frame.add_css_class("settings-table-frame");
    frame.set_halign(Align::Fill);
    frame.set_hexpand(true);

    // Header row
    frame.append(&build_hotkey_row("Action", "Shortcut", true, false));
    frame.append(&build_hotkey_row("Area capture", &area, false, false));
    frame.append(&build_hotkey_row("Full screen", &screen, false, true));
    frame.append(&build_hotkey_row("Record UI", &record, false, false));

    hotkeys_block.append(&frame);
    content.append(&hotkeys_block);

    let try_btn = Button::with_label("Take a test screenshot");
    try_btn.add_css_class("settings-primary-btn");
    try_btn.set_halign(Align::Center);
    try_btn.set_margin_top(28);
    try_btn.set_tooltip_text(Some(
        "Starts the tray daemon if needed, then opens area capture",
    ));
    try_btn.connect_clicked(|_| {
        std::thread::spawn(|| {
            if !ensure_daemon_running() {
                eprintln!("[onboarding] Could not start daemon for test capture");
                // Fallback: one-shot CLI path without daemon.
                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
                let _ = std::process::Command::new(exe)
                    .args(["capture", "area"])
                    .spawn();
                return;
            }
            if !trigger_daemon_action_sync("capture_area") {
                eprintln!("[onboarding] Daemon did not accept capture_area");
                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
                let _ = std::process::Command::new(exe)
                    .args(["capture", "area"])
                    .spawn();
            }
        });
    });
    content.append(&try_btn);

    let hint = Label::new(Some(
        "Optional. You can skip this and try a capture after finishing setup.",
    ));
    hint.set_halign(Align::Center);
    hint.set_wrap(true);
    hint.set_margin_top(10);
    hint.add_css_class("dim-label");
    content.append(&hint);
}

fn display_shortcut(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        format!("{fallback} (default)")
    } else {
        trimmed.to_string()
    }
}

fn build_hotkey_row(action: &str, shortcut: &str, is_header: bool, muted: bool) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 16);
    row.add_css_class("settings-table-row");
    if muted {
        row.add_css_class("settings-table-row-muted");
    }
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
    action_label.set_width_request(160);

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

fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
