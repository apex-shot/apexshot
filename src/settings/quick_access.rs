use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Grid, Label, Orientation,
    PositionType, Scale, Separator,
};

#[allow(dead_code)]
pub struct QuickAccessSettingsWidgets {
    pub section: GtkBox,
    pub position_input: ComboBoxText,
    pub multi_display_check: CheckButton,
    pub overlay_size_input: Scale,
    pub auto_close_enabled_check: CheckButton,
    pub auto_close_action_input: ComboBoxText,
    pub auto_close_interval_input: ComboBoxText,
    pub close_after_dragging_check: CheckButton,
    pub close_after_uploading_check: CheckButton,
}

pub fn build_quick_access_section(config: &AppConfig) -> QuickAccessSettingsWidgets {
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

    // --- Overlay settings ---
    let overlay_label = Label::new(Some("Overlay:"));
    overlay_label.add_css_class("settings-group-title");
    overlay_label.set_xalign(1.0);
    overlay_label.set_size_request(165, -1);

    // Position
    let position_input = ComboBoxText::new();
    position_input.add_css_class("settings-select");
    for pos in ["Left", "Right"] {
        position_input.append(Some(pos), pos);
    }
    position_input.set_active_id(Some(&config.quick_access_position));
    position_input.set_halign(Align::Start);
    let pos_box = GtkBox::new(Orientation::Horizontal, 12);
    let pos_label = Label::new(Some("Position"));
    pos_label.add_css_class("settings-sub-option");
    pos_box.append(&pos_label);
    pos_box.append(&position_input);
    grid.attach(&overlay_label, 1, row, 1, 1);
    grid.attach(&pos_box, 3, row, 1, 1);

    row += 1;
    // Multi-display
    let multi_display_check = CheckButton::new();
    multi_display_check.set_active(config.quick_access_multi_display);
    let multi_cell = GtkBox::new(Orientation::Horizontal, 0);
    multi_cell.set_size_request(28, -1);
    multi_cell.set_halign(Align::Start);
    multi_cell.append(&multi_display_check);
    let multi_option = Label::new(Some("Show on all displays"));
    multi_option.set_xalign(0.0);
    grid.attach(&multi_cell, 2, row, 1, 1);
    grid.attach(&multi_option, 3, row, 1, 1);

    row += 1;
    // Overlay Size (Scale)
    let size_label = Label::new(Some("Overlay size"));
    size_label.add_css_class("settings-group-title");
    size_label.set_xalign(0.0);
    let overlay_size_input = Scale::with_range(Orientation::Horizontal, 0.5, 1.5, 0.1);
    overlay_size_input.set_value(config.quick_access_overlay_size);
    overlay_size_input.set_size_request(190, -1);
    overlay_size_input.set_digits(1);
    overlay_size_input.add_mark(0.5, PositionType::Bottom, None);
    overlay_size_input.add_mark(1.0, PositionType::Bottom, None);
    overlay_size_input.add_mark(1.5, PositionType::Bottom, None);
    let size_box = GtkBox::new(Orientation::Horizontal, 12);
    let size_control_box = GtkBox::new(Orientation::Vertical, 8);
    size_control_box.append(&size_label);
    size_control_box.append(&overlay_size_input);

    let size_caption_row = GtkBox::new(Orientation::Horizontal, 0);
    size_caption_row.set_hexpand(true);
    for (text, align) in [
        ("Smaller", Align::Start),
        ("Current", Align::Center),
        ("Larger", Align::End),
    ] {
        let caption = Label::new(Some(text));
        caption.add_css_class("settings-scale-caption");
        caption.set_hexpand(true);
        caption.set_halign(align);
        size_caption_row.append(&caption);
    }
    size_control_box.append(&size_caption_row);
    size_box.append(&size_control_box);
    grid.attach(&size_box, 3, row, 1, 1);

    row += 1;
    // SEPARATOR 1
    let sep1 = Separator::new(Orientation::Horizontal);
    sep1.set_margin_top(14);
    sep1.set_margin_bottom(14);
    sep1.set_hexpand(true);
    grid.attach(&sep1, 0, row, 5, 1);

    row += 1;
    // --- Auto-close behaviors ---
    let auto_close_label = Label::new(Some("Auto-close:"));
    auto_close_label.add_css_class("settings-group-title");
    auto_close_label.set_xalign(1.0);
    let auto_close_enabled_check = CheckButton::new();
    auto_close_enabled_check.set_active(config.quick_access_auto_close_enabled);
    let auto_close_cell = GtkBox::new(Orientation::Horizontal, 0);
    auto_close_cell.set_size_request(28, -1);
    auto_close_cell.set_halign(Align::Start);
    auto_close_cell.append(&auto_close_enabled_check);
    let auto_close_option = Label::new(Some("Automatically close window"));
    auto_close_option.set_xalign(0.0);
    grid.attach(&auto_close_label, 1, row, 1, 1);
    grid.attach(&auto_close_cell, 2, row, 1, 1);
    grid.attach(&auto_close_option, 3, row, 1, 1);

    row += 1;
    // Sub-option: Action
    let auto_close_action_input = ComboBoxText::new();
    auto_close_action_input.add_css_class("settings-select");
    for (id, lbl) in [("Close", "Close"), ("Hide", "Hide (stay in Tray)")] {
        auto_close_action_input.append(Some(id), lbl);
    }
    auto_close_action_input.set_active_id(Some(&config.quick_access_auto_close_action));
    auto_close_action_input.set_halign(Align::Start);
    let action_label = Label::new(Some("Action"));
    action_label.add_css_class("settings-sub-option");
    let action_box = GtkBox::new(Orientation::Horizontal, 12);
    action_box.append(&action_label);
    action_box.append(&auto_close_action_input);
    grid.attach(&action_box, 3, row, 1, 1);

    row += 1;
    // Sub-option: Interval
    let auto_close_interval_input = ComboBoxText::new();
    auto_close_interval_input.add_css_class("settings-select");
    for (id, lbl) in [
        ("5", "5 Seconds"),
        ("10", "10 Seconds"),
        ("30", "30 Seconds"),
        ("60", "1 Minute"),
    ] {
        auto_close_interval_input.append(Some(id), lbl);
    }
    auto_close_interval_input
        .set_active_id(Some(&config.quick_access_auto_close_interval.to_string()));
    auto_close_interval_input.set_halign(Align::Start);
    let interval_label = Label::new(Some("Interval"));
    interval_label.add_css_class("settings-sub-option");
    let interval_box = GtkBox::new(Orientation::Horizontal, 12);
    interval_box.append(&interval_label);
    interval_box.append(&auto_close_interval_input);
    grid.attach(&interval_box, 3, row, 1, 1);

    row += 1;
    // SEPARATOR 2
    let sep2 = Separator::new(Orientation::Horizontal);
    sep2.set_margin_top(14);
    sep2.set_margin_bottom(14);
    sep2.set_hexpand(true);
    grid.attach(&sep2, 0, row, 5, 1);

    row += 1;
    // --- Group 3: Specific close behaviors ---
    let behaviors_label = Label::new(Some("Behaviors:"));
    behaviors_label.add_css_class("settings-group-title");
    behaviors_label.set_xalign(1.0);

    // Drag & Drop
    let close_after_dragging_check = CheckButton::new();
    close_after_dragging_check.set_active(config.quick_access_close_after_dragging);
    let drag_cell = GtkBox::new(Orientation::Horizontal, 0);
    drag_cell.set_size_request(28, -1);
    drag_cell.set_halign(Align::Start);
    drag_cell.append(&close_after_dragging_check);
    let drag_option = Label::new(Some("Close window after dragging"));
    drag_option.set_xalign(0.0);
    grid.attach(&behaviors_label, 1, row, 1, 1);
    grid.attach(&drag_cell, 2, row, 1, 1);
    grid.attach(&drag_option, 3, row, 1, 1);

    row += 1;
    // Upload
    let close_after_uploading_check = CheckButton::new();
    close_after_uploading_check.set_active(config.quick_access_close_after_uploading);
    let upload_cell = GtkBox::new(Orientation::Horizontal, 0);
    upload_cell.set_size_request(28, -1);
    upload_cell.set_halign(Align::Start);
    upload_cell.append(&close_after_uploading_check);
    let upload_option = Label::new(Some("Close window after uploading"));
    upload_option.set_xalign(0.0);
    grid.attach(&upload_cell, 2, row, 1, 1);
    grid.attach(&upload_option, 3, row, 1, 1);

    section.append(&grid);

    QuickAccessSettingsWidgets {
        section,
        position_input,
        multi_display_check,
        overlay_size_input,
        auto_close_enabled_check,
        auto_close_action_input,
        auto_close_interval_input,
        close_after_dragging_check,
        close_after_uploading_check,
    }
}
