use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Label, Orientation,
};

#[allow(dead_code)]
pub struct ScreenshotsSettingsWidgets {
    pub section: GtkBox,
    pub export_location_entry: Entry,
    pub export_location_browse: Button,
    pub format_input: ComboBoxText,
    pub clipboard_mode_input: ComboBoxText,
    pub freeze_screen_check: CheckButton,
    pub crosshair_mode_input: ComboBoxText,
    pub show_magnifier_check: CheckButton,
    pub timer_interval_input: ComboBoxText,
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
    let export_title = Label::new(Some("Export"));
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
    let export_location_browse = Button::with_label("Browse");

    let export_location_hbox = GtkBox::new(Orientation::Horizontal, 12);
    export_location_hbox.set_hexpand(true);
    let export_label = Label::new(Some("Save location"));
    export_label.set_xalign(0.0);
    export_label.set_hexpand(true);
    let entry_row = GtkBox::new(Orientation::Horizontal, 8);
    entry_row.append(&export_location_entry);
    entry_row.append(&export_location_browse);
    export_location_hbox.append(&export_label);
    export_location_hbox.append(&entry_row);
    export_frame.append(&build_row!(&export_location_hbox, false));

    // File format
    let format_input = ComboBoxText::new();
    format_input.add_css_class("settings-select");
    for fmt in ["PNG", "JPEG", "WebP"] {
        format_input.append(Some(fmt), fmt);
    }
    format_input.set_active_id(Some(&config.screenshot_format));
    let format_hbox = GtkBox::new(Orientation::Horizontal, 12);
    format_hbox.set_hexpand(true);
    let format_label = Label::new(Some("File format"));
    format_label.set_xalign(0.0);
    format_label.set_hexpand(true);
    format_hbox.append(&format_label);
    format_hbox.append(&format_input);
    export_frame.append(&build_row!(&format_hbox, true));

    // Clipboard mode
    let clipboard_mode_input = ComboBoxText::new();
    clipboard_mode_input.add_css_class("settings-select");
    clipboard_mode_input.append(Some("File & Image (default)"), "File & Image (default)");
    clipboard_mode_input.append(Some("Image Only"), "Image Only");
    clipboard_mode_input.append(Some("File Path Only"), "File Path Only");
    clipboard_mode_input.set_active_id(Some(&config.adv_clipboard_mode));

    let clipboard_hbox = GtkBox::new(Orientation::Horizontal, 12);
    clipboard_hbox.set_hexpand(true);
    let clip_vbox = GtkBox::new(Orientation::Vertical, 4);
    clip_vbox.set_hexpand(true);
    let lbl_clip = Label::new(Some("Clipboard copy behavior"));
    lbl_clip.set_xalign(0.0);
    let clip_hint = Label::new(Some(
        "Choose whether screenshots copy as an image, a file URI, or both.",
    ));
    clip_hint.add_css_class("settings-sub-option-hint");
    clip_hint.set_xalign(0.0);
    clip_vbox.append(&lbl_clip);
    clip_vbox.append(&clip_hint);
    clipboard_hbox.append(&clip_vbox);
    clipboard_hbox.append(&clipboard_mode_input);
    export_frame.append(&build_row!(&clipboard_hbox, false));

    section.append(&export_frame);

    // --- Interface Group ---
    let interface_title = Label::new(Some("Interface"));
    interface_title.add_css_class("settings-group-title");
    interface_title.set_xalign(0.0);
    interface_title.set_halign(Align::Start);
    interface_title.set_margin_bottom(8);
    section.append(&interface_title);

    let interface_frame = build_frame();

    // Freeze screen
    let freeze_screen_check = CheckButton::new();
    freeze_screen_check.set_active(config.screenshot_freeze_screen);
    let freeze_hbox = GtkBox::new(Orientation::Horizontal, 12);
    freeze_hbox.set_hexpand(true);
    let freeze_option = Label::new(Some("Use frozen background during selection"));
    freeze_option.set_xalign(0.0);
    freeze_option.set_hexpand(true);
    freeze_hbox.append(&freeze_option);
    freeze_hbox.append(&freeze_screen_check);
    interface_frame.append(&build_row!(&freeze_hbox, false));

    // Crosshair mode
    let crosshair_mode_input = ComboBoxText::new();
    crosshair_mode_input.add_css_class("settings-select");
    for mode in ["Disabled", "Crosshair", "Magnifier"] {
        crosshair_mode_input.append(Some(mode), mode);
    }
    crosshair_mode_input.set_active_id(Some(&config.screenshot_crosshair_mode));
    let cross_hbox = GtkBox::new(Orientation::Horizontal, 12);
    cross_hbox.set_hexpand(true);
    let cross_label = Label::new(Some("Selection cursor"));
    cross_label.set_xalign(0.0);
    cross_label.set_hexpand(true);
    cross_hbox.append(&cross_label);
    cross_hbox.append(&crosshair_mode_input);
    interface_frame.append(&build_row!(&cross_hbox, true));

    // Show magnifier sub-option
    let show_magnifier_check = CheckButton::new();
    show_magnifier_check.set_active(config.screenshot_show_magnifier);
    let mag_hbox = GtkBox::new(Orientation::Horizontal, 12);
    mag_hbox.set_hexpand(true);
    let mag_option = Label::new(Some("Show zoom preview while selecting"));
    mag_option.set_xalign(0.0);
    mag_option.set_hexpand(true);
    mag_hbox.append(&mag_option);
    mag_hbox.append(&show_magnifier_check);
    interface_frame.append(&build_row!(&mag_hbox, false));

    section.append(&interface_frame);

    // --- Advanced Group ---
    let adv_title = Label::new(Some("Advanced"));
    adv_title.add_css_class("settings-group-title");
    adv_title.set_xalign(0.0);
    adv_title.set_halign(Align::Start);
    adv_title.set_margin_bottom(8);
    section.append(&adv_title);

    let adv_frame = build_frame();

    // Self-Timer interval
    let timer_interval_input = ComboBoxText::new();
    timer_interval_input.add_css_class("settings-select");
    for (id, label) in [
        ("0", "Off"),
        ("1", "1 Second"),
        ("3", "3 Seconds"),
        ("5", "5 Seconds"),
        ("10", "10 Seconds"),
    ] {
        timer_interval_input.append(Some(id), label);
    }
    timer_interval_input.set_active_id(Some(&config.screenshot_timer_interval.to_string()));
    let timer_hbox = GtkBox::new(Orientation::Horizontal, 12);
    timer_hbox.set_hexpand(true);
    let timer_label = Label::new(Some("Self-Timer interval"));
    timer_label.set_xalign(0.0);
    timer_label.set_hexpand(true);
    timer_hbox.append(&timer_label);
    timer_hbox.append(&timer_interval_input);
    adv_frame.append(&build_row!(&timer_hbox, false));

    // Cursor
    let show_cursor_check = CheckButton::new();
    show_cursor_check.set_active(config.screenshot_show_cursor);
    let cursor_hbox = GtkBox::new(Orientation::Horizontal, 12);
    cursor_hbox.set_hexpand(true);

    let cursor_vbox = GtkBox::new(Orientation::Vertical, 4);
    cursor_vbox.set_hexpand(true);
    let cursor_option = Label::new(Some("Include pointer when available"));
    cursor_option.set_xalign(0.0);
    let cursor_desc = Label::new(Some(
        "ApexShot includes the pointer only when the current capture flow provides cursor data.",
    ));
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
        freeze_screen_check,
        crosshair_mode_input,
        show_magnifier_check,
        timer_interval_input,
        show_cursor_check,
    }
}
