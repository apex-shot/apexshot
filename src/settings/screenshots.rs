use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};

use super::select::SettingsSelect;

#[allow(dead_code)]
pub struct ScreenshotsSettingsWidgets {
    pub section: GtkBox,
    pub export_location_entry: Entry,
    pub export_location_browse: Button,
    pub format_input: SettingsSelect,
    pub clipboard_mode_input: SettingsSelect,
    pub timer_interval_input: SettingsSelect,
    pub show_cursor_check: CheckButton,
}

pub fn build_screenshots_section(config: &AppConfig) -> ScreenshotsSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);
    section.set_vexpand(true);

    macro_rules! build_row {
        ($content:expr, $is_muted:expr) => {{
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            row.add_css_class("settings-table-row");
            if $is_muted {
                row.add_css_class("settings-table-row-muted");
            }
            row.set_hexpand(true);
            row.append($content);
            row
        }};
    }

    let build_frame = || -> gtk4::Box {
        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("settings-table-frame");
        frame.set_margin_bottom(24);
        frame.set_margin_start(4);
        frame.set_margin_end(4);
        frame
    };

    // --- Export Group ---
    let export_title = Label::new(Some(&t("Export")));
    export_title.add_css_class("settings-group-title");
    export_title.set_xalign(0.0);
    export_title.set_halign(Align::Start);
    export_title.set_margin_bottom(8);
    section.append(&export_title);

    let export_frame = build_frame();

    let export_location_entry = Entry::new();
    export_location_entry.set_hexpand(true);
    export_location_entry.set_width_chars(28);
    export_location_entry.set_placeholder_text(Some("Choose a folder"));
    export_location_entry.set_text(&config.screenshot_export_location);
    let export_location_browse = Button::with_label(&t("Browse"));

    let export_location_hbox = GtkBox::new(Orientation::Horizontal, 12);
    export_location_hbox.set_hexpand(true);
    let export_label = Label::new(Some(&t("Save location")));
    export_label.set_xalign(0.0);
    export_label.set_hexpand(true);
    let entry_row = GtkBox::new(Orientation::Horizontal, 8);
    entry_row.append(&export_location_entry);
    entry_row.append(&export_location_browse);
    export_location_hbox.append(&export_label);
    export_location_hbox.append(&entry_row);
    export_frame.append(&build_row!(&export_location_hbox, false));

    // File format
    let format_input = SettingsSelect::new(
        ["PNG", "JPEG", "WebP"].map(|fmt| (fmt, t(fmt))),
        &config.screenshot_format,
    );
    let format_hbox = GtkBox::new(Orientation::Horizontal, 12);
    format_hbox.set_hexpand(true);
    let format_label = Label::new(Some(&t("File format")));
    format_label.set_xalign(0.0);
    format_label.set_hexpand(true);
    format_hbox.append(&format_label);
    format_hbox.append(format_input.widget());
    export_frame.append(&build_row!(&format_hbox, true));

    // Clipboard mode
    let clipboard_mode_input = SettingsSelect::new(
        [
            ("File & Image (default)", t("File & Image (default)")),
            ("Image Only", t("Image Only")),
            ("File Path Only", t("File Path Only")),
        ],
        &config.adv_clipboard_mode,
    );

    let clipboard_hbox = GtkBox::new(Orientation::Horizontal, 12);
    clipboard_hbox.set_hexpand(true);
    let clip_vbox = GtkBox::new(Orientation::Vertical, 4);
    clip_vbox.set_hexpand(true);
    let lbl_clip = Label::new(Some(&t("Clipboard copy behavior")));
    lbl_clip.set_xalign(0.0);
    let clip_hint = Label::new(Some(&t(
        "Choose whether screenshots copy as an image, a file URI, or both.",
    )));
    clip_hint.add_css_class("settings-sub-option-hint");
    clip_hint.set_xalign(0.0);
    clip_vbox.append(&lbl_clip);
    clip_vbox.append(&clip_hint);
    clipboard_hbox.append(&clip_vbox);
    clipboard_hbox.append(clipboard_mode_input.widget());
    export_frame.append(&build_row!(&clipboard_hbox, false));

    section.append(&export_frame);

    // Interface options (frozen selection background, crosshair, magnifier) were
    // removed: they are hard to keep reliable across Linux compositors/distros.
    // Capture still uses fixed AppConfig defaults for those fields.

    // --- Advanced Group ---
    let adv_title = Label::new(Some(&t("Advanced")));
    adv_title.add_css_class("settings-group-title");
    adv_title.set_xalign(0.0);
    adv_title.set_halign(Align::Start);
    adv_title.set_margin_bottom(8);
    section.append(&adv_title);

    let adv_frame = build_frame();

    // Self-Timer interval
    let timer_interval_input = SettingsSelect::new(
        [
            ("0", t("Off")),
            ("1", t("1 Second")),
            ("3", t("3 Seconds")),
            ("5", t("5 Seconds")),
            ("10", t("10 Seconds")),
        ],
        &config.screenshot_timer_interval.to_string(),
    );
    let timer_hbox = GtkBox::new(Orientation::Horizontal, 12);
    timer_hbox.set_hexpand(true);
    let timer_label = Label::new(Some(&t("Self-Timer interval")));
    timer_label.set_xalign(0.0);
    timer_label.set_hexpand(true);
    timer_hbox.append(&timer_label);
    timer_hbox.append(timer_interval_input.widget());
    adv_frame.append(&build_row!(&timer_hbox, false));

    // Cursor
    let show_cursor_check = CheckButton::new();
    show_cursor_check.set_active(config.screenshot_show_cursor);
    let cursor_hbox = GtkBox::new(Orientation::Horizontal, 12);
    cursor_hbox.set_hexpand(true);

    let cursor_vbox = GtkBox::new(Orientation::Vertical, 4);
    cursor_vbox.set_hexpand(true);
    let cursor_option = Label::new(Some(&t("Include pointer when available")));
    cursor_option.set_xalign(0.0);
    let cursor_desc = Label::new(Some(&t(
        "ApexShot includes the pointer only when the current capture flow provides cursor data.",
    )));
    cursor_desc.set_xalign(0.0);
    cursor_desc.add_css_class("settings-sub-option-hint");
    cursor_vbox.append(&cursor_option);
    cursor_vbox.append(&cursor_desc);

    cursor_hbox.append(&cursor_vbox);
    cursor_hbox.append(&show_cursor_check);
    adv_frame.append(&build_row!(&cursor_hbox, true));

    section.append(&adv_frame);

    ScreenshotsSettingsWidgets {
        section,
        export_location_entry,
        export_location_browse,
        format_input,
        clipboard_mode_input,
        timer_interval_input,
        show_cursor_check,
    }
}
