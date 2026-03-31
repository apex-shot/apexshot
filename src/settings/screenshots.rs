use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Grid, Label,
    Orientation, Separator,
};

#[allow(dead_code)]
pub struct ScreenshotsSettingsWidgets {
    pub section: GtkBox,
    pub export_location_entry: Entry,
    pub export_location_browse: Button,
    pub format_input: ComboBoxText,
    pub retina_scale_check: CheckButton,
    pub frame_border_check: CheckButton,
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

    let grid = Grid::new();
    grid.set_hexpand(true);
    grid.set_row_spacing(30);
    grid.set_column_spacing(12);

    // Spacers for centering
    let left_spacer = GtkBox::new(Orientation::Horizontal, 0);
    left_spacer.set_hexpand(true);
    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);

    grid.attach(&left_spacer, 0, 0, 1, 1);
    grid.attach(&right_spacer, 4, 0, 1, 1);

    let mut row = 0;

    // --- GROUP 1 ---
    let export_label = Label::new(Some("Save location:"));
    export_label.add_css_class("settings-group-title");
    export_label.set_xalign(1.0);
    export_label.set_size_request(165, -1);
    let export_location_entry = Entry::new();
    export_location_entry.set_hexpand(true);
    export_location_entry.set_width_chars(28);
    export_location_entry.set_placeholder_text(Some("Choose a folder"));
    export_location_entry.set_text(&config.screenshot_export_location);
    let export_location_browse = Button::with_label("Browse");
    let export_location_row = GtkBox::new(Orientation::Horizontal, 8);
    export_location_row.set_halign(Align::Start);
    export_location_row.append(&export_location_entry);
    export_location_row.append(&export_location_browse);
    grid.attach(&export_label, 1, row, 1, 1);
    grid.attach(&export_location_row, 3, row, 1, 1);

    row += 1;

    // File format
    let format_label = Label::new(Some("File format:"));
    format_label.add_css_class("settings-group-title");
    format_label.set_xalign(1.0);
    format_label.set_size_request(165, -1);
    let format_input = ComboBoxText::new();
    format_input.add_css_class("settings-select");
    for fmt in ["PNG", "JPEG", "WebP"] {
        format_input.append(Some(fmt), fmt);
    }
    format_input.set_active_id(Some(&config.screenshot_format));
    format_input.set_halign(Align::Start);
    grid.attach(&format_label, 1, row, 1, 1);
    grid.attach(&format_input, 3, row, 1, 1);

    row += 1;
    // Retina
    let retina_label = Label::new(Some("Retina:"));
    retina_label.add_css_class("settings-group-title");
    retina_label.set_xalign(1.0);
    let retina_scale_check = CheckButton::new();
    retina_scale_check.set_active(config.screenshot_retina_scale);
    let retina_cell = GtkBox::new(Orientation::Horizontal, 0);
    retina_cell.set_size_request(28, -1);
    retina_cell.set_halign(Align::Start);
    retina_cell.append(&retina_scale_check);
    let retina_option = Label::new(Some("Scale Retina screenshots to 1x"));
    retina_option.set_xalign(0.0);
    grid.attach(&retina_label, 1, row, 1, 1);
    grid.attach(&retina_cell, 2, row, 1, 1);
    grid.attach(&retina_option, 3, row, 1, 1);

    row += 1;
    // Frame
    let frame_label = Label::new(Some("Frame:"));
    frame_label.add_css_class("settings-group-title");
    frame_label.set_xalign(1.0);
    let frame_border_check = CheckButton::new();
    frame_border_check.set_active(config.screenshot_frame_border);
    let frame_cell = GtkBox::new(Orientation::Horizontal, 0);
    frame_cell.set_size_request(28, -1);
    frame_cell.set_halign(Align::Start);
    frame_cell.append(&frame_border_check);
    let frame_option = Label::new(Some("Add 1px border to all screenshots"));
    frame_option.set_xalign(0.0);
    grid.attach(&frame_label, 1, row, 1, 1);
    grid.attach(&frame_cell, 2, row, 1, 1);
    grid.attach(&frame_option, 3, row, 1, 1);

    row += 1;
    // SEPARATOR 1
    let sep1 = Separator::new(Orientation::Horizontal);
    sep1.set_margin_top(14);
    sep1.set_margin_bottom(14);
    sep1.set_hexpand(true);
    grid.attach(&sep1, 0, row, 5, 1);

    row += 1;
    // --- GROUP 2 ---

    // Freeze screen
    let freeze_label = Label::new(Some("Freeze screen:"));
    freeze_label.add_css_class("settings-group-title");
    freeze_label.set_xalign(1.0);
    let freeze_screen_check = CheckButton::new();
    freeze_screen_check.set_active(config.screenshot_freeze_screen);
    let freeze_cell = GtkBox::new(Orientation::Horizontal, 0);
    freeze_cell.set_size_request(28, -1);
    freeze_cell.set_halign(Align::Start);
    freeze_cell.append(&freeze_screen_check);
    let freeze_option = Label::new(Some("Freeze screen when taking a screenshot"));
    freeze_option.set_xalign(0.0);
    grid.attach(&freeze_label, 1, row, 1, 1);
    grid.attach(&freeze_cell, 2, row, 1, 1);
    grid.attach(&freeze_option, 3, row, 1, 1);

    row += 1;
    // Crosshair mode
    let cross_label = Label::new(Some("Crosshair mode:"));
    cross_label.add_css_class("settings-group-title");
    cross_label.set_xalign(1.0);
    let crosshair_mode_input = ComboBoxText::new();
    crosshair_mode_input.add_css_class("settings-select");
    for mode in ["Disabled", "Crosshair", "Magnifier"] {
        crosshair_mode_input.append(Some(mode), mode);
    }
    crosshair_mode_input.set_active_id(Some(&config.screenshot_crosshair_mode));
    crosshair_mode_input.set_halign(Align::Start);
    grid.attach(&cross_label, 1, row, 1, 1);
    grid.attach(&crosshair_mode_input, 3, row, 1, 1);

    row += 1;
    // Show magnifier sub-option
    let show_magnifier_check = CheckButton::new();
    show_magnifier_check.set_active(config.screenshot_show_magnifier);
    let mag_cell = GtkBox::new(Orientation::Horizontal, 0);
    mag_cell.set_size_request(28, -1);
    mag_cell.set_halign(Align::Start);
    mag_cell.append(&show_magnifier_check);
    let mag_option = Label::new(Some("Show magnifier"));
    mag_option.set_xalign(0.0);
    mag_option.add_css_class("settings-sub-option");
    grid.attach(&mag_cell, 2, row, 1, 1);
    grid.attach(&mag_option, 3, row, 1, 1);

    row += 1;
    // SEPARATOR 2
    let sep2 = Separator::new(Orientation::Horizontal);
    sep2.set_margin_top(14);
    sep2.set_margin_bottom(14);
    sep2.set_hexpand(true);
    grid.attach(&sep2, 0, row, 5, 1);

    row += 1;
    // --- GROUP 3 ---

    // Self-Timer interval
    let timer_label = Label::new(Some("Self-Timer interval:"));
    timer_label.add_css_class("settings-group-title");
    timer_label.set_xalign(1.0);
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
    timer_interval_input.set_halign(Align::Start);
    grid.attach(&timer_label, 1, row, 1, 1);
    grid.attach(&timer_interval_input, 3, row, 1, 1);

    row += 1;
    // Cursor
    let cursor_label = Label::new(Some("Cursor:"));
    cursor_label.add_css_class("settings-group-title");
    cursor_label.set_xalign(1.0);
    let show_cursor_check = CheckButton::new();
    show_cursor_check.set_active(config.screenshot_show_cursor);
    let cursor_cell = GtkBox::new(Orientation::Horizontal, 0);
    cursor_cell.set_size_request(28, -1);
    cursor_cell.set_halign(Align::Start);
    cursor_cell.append(&show_cursor_check);
    let cursor_option_vbox = GtkBox::new(Orientation::Vertical, 4);
    let cursor_option = Label::new(Some("Show on screenshots"));
    cursor_option.set_xalign(0.0);
    let cursor_desc = Label::new(Some("This works in Fullscreen or Self-Timer modes only."));
    cursor_desc.set_xalign(0.0);
    cursor_desc.add_css_class("settings-description");
    cursor_option_vbox.append(&cursor_option);
    cursor_option_vbox.append(&cursor_desc);
    grid.attach(&cursor_label, 1, row, 1, 1);
    grid.attach(&cursor_cell, 2, row, 1, 1);
    grid.attach(&cursor_option_vbox, 3, row, 1, 1);

    section.append(&grid);

    ScreenshotsSettingsWidgets {
        section,
        export_location_entry,
        export_location_browse,
        format_input,
        retina_scale_check,
        frame_border_check,
        freeze_screen_check,
        crosshair_mode_input,
        show_magnifier_check,
        timer_interval_input,
        show_cursor_check,
    }
}
