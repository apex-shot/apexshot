use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Label, Orientation,
    PositionType, Scale,
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

    // --- Overlay Group ---
    let overlay_title = Label::new(Some("Overlay"));
    overlay_title.add_css_class("settings-group-title");
    overlay_title.set_xalign(0.0);
    overlay_title.set_halign(Align::Start);
    overlay_title.set_margin_bottom(8);
    section.append(&overlay_title);

    let overlay_frame = build_frame();

    // Position
    let position_input = ComboBoxText::new();
    position_input.add_css_class("settings-select");
    for pos in ["Left", "Right"] {
        position_input.append(Some(pos), pos);
    }
    position_input.set_active_id(Some(&config.quick_access_position));
    let pos_hbox = GtkBox::new(Orientation::Horizontal, 12);
    pos_hbox.set_hexpand(true);
    let pos_label = Label::new(Some("Position"));
    pos_label.set_xalign(0.0);
    pos_label.set_hexpand(true);
    pos_hbox.append(&pos_label);
    pos_hbox.append(&position_input);
    overlay_frame.append(&build_row!(&pos_hbox, false));

    // Multi-display
    let multi_display_check = CheckButton::new();
    multi_display_check.set_active(config.quick_access_multi_display);
    let multi_hbox = GtkBox::new(Orientation::Horizontal, 12);
    multi_hbox.set_hexpand(true);
    let multi_option = Label::new(Some("Show on all displays"));
    multi_option.set_xalign(0.0);
    multi_option.set_hexpand(true);
    multi_hbox.append(&multi_option);
    multi_hbox.append(&multi_display_check);
    overlay_frame.append(&build_row!(&multi_hbox, true));

    // Overlay Size
    let overlay_size_input = Scale::with_range(Orientation::Horizontal, 0.5, 1.5, 0.1);
    overlay_size_input.set_value(config.quick_access_overlay_size);
    overlay_size_input.set_size_request(220, -1);
    overlay_size_input.set_digits(1);
    overlay_size_input.add_mark(0.5, PositionType::Bottom, None);
    overlay_size_input.add_mark(1.0, PositionType::Bottom, None);
    overlay_size_input.add_mark(1.5, PositionType::Bottom, None);
    
    let size_vbox = GtkBox::new(Orientation::Vertical, 4);
    size_vbox.append(&overlay_size_input);
    
    let size_caption_row = GtkBox::new(Orientation::Horizontal, 0);
    for (text, align) in [
        ("Min", Align::Start),
        ("Def", Align::Center),
        ("Max", Align::End),
    ] {
        let caption = Label::new(Some(text));
        caption.add_css_class("settings-scale-caption");
        caption.set_hexpand(true);
        caption.set_halign(align);
        size_caption_row.append(&caption);
    }
    size_vbox.append(&size_caption_row);
    
    let size_hbox = GtkBox::new(Orientation::Horizontal, 12);
    size_hbox.set_hexpand(true);
    let size_label = Label::new(Some("Overlay size"));
    size_label.set_xalign(0.0);
    size_label.set_hexpand(true);
    size_hbox.append(&size_label);
    size_hbox.append(&size_vbox);
    overlay_frame.append(&build_row!(&size_hbox, false));

    section.append(&overlay_frame);


    // --- Auto-close Group ---
    let auto_close_title = Label::new(Some("Auto-close"));
    auto_close_title.add_css_class("settings-group-title");
    auto_close_title.set_xalign(0.0);
    auto_close_title.set_halign(Align::Start);
    auto_close_title.set_margin_bottom(8);
    section.append(&auto_close_title);

    let auto_close_frame = build_frame();
    
    // Auto-close check
    let auto_close_enabled_check = CheckButton::new();
    auto_close_enabled_check.set_active(config.quick_access_auto_close_enabled);
    let auto_close_hbox = GtkBox::new(Orientation::Horizontal, 12);
    auto_close_hbox.set_hexpand(true);
    let auto_close_option = Label::new(Some("Automatically close window"));
    auto_close_option.set_xalign(0.0);
    auto_close_option.set_hexpand(true);
    auto_close_hbox.append(&auto_close_option);
    auto_close_hbox.append(&auto_close_enabled_check);
    auto_close_frame.append(&build_row!(&auto_close_hbox, false));

    // Action
    let auto_close_action_input = ComboBoxText::new();
    auto_close_action_input.add_css_class("settings-select");
    for (id, lbl) in [("Close", "Close"), ("Hide", "Hide (stay in Tray)")] {
        auto_close_action_input.append(Some(id), lbl);
    }
    auto_close_action_input.set_active_id(Some(&config.quick_access_auto_close_action));
    let action_hbox = GtkBox::new(Orientation::Horizontal, 12);
    action_hbox.set_hexpand(true);
    let action_label = Label::new(Some("Action"));
    action_label.add_css_class("settings-sub-option");
    action_label.set_xalign(0.0);
    action_label.set_hexpand(true);
    action_hbox.append(&action_label);
    action_hbox.append(&auto_close_action_input);
    auto_close_frame.append(&build_row!(&action_hbox, true));

    // Interval
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
    auto_close_interval_input.set_active_id(Some(&config.quick_access_auto_close_interval.to_string()));
    let interval_hbox = GtkBox::new(Orientation::Horizontal, 12);
    interval_hbox.set_hexpand(true);
    let interval_label = Label::new(Some("Interval"));
    interval_label.add_css_class("settings-sub-option");
    interval_label.set_xalign(0.0);
    interval_label.set_hexpand(true);
    interval_hbox.append(&interval_label);
    interval_hbox.append(&auto_close_interval_input);
    auto_close_frame.append(&build_row!(&interval_hbox, false));
    section.append(&auto_close_frame);


    // --- Behaviors Group ---
    let behaviors_title = Label::new(Some("Behaviors"));
    behaviors_title.add_css_class("settings-group-title");
    behaviors_title.set_xalign(0.0);
    behaviors_title.set_halign(Align::Start);
    behaviors_title.set_margin_bottom(8);
    section.append(&behaviors_title);

    let behaviors_frame = build_frame();
    
    // Drag
    let close_after_dragging_check = CheckButton::new();
    close_after_dragging_check.set_active(config.quick_access_close_after_dragging);
    let drag_hbox = GtkBox::new(Orientation::Horizontal, 12);
    drag_hbox.set_hexpand(true);
    let drag_option = Label::new(Some("Close window after dragging"));
    drag_option.set_xalign(0.0);
    drag_option.set_hexpand(true);
    drag_hbox.append(&drag_option);
    drag_hbox.append(&close_after_dragging_check);
    behaviors_frame.append(&build_row!(&drag_hbox, false));

    // Upload
    let close_after_uploading_check = CheckButton::new();
    close_after_uploading_check.set_active(config.quick_access_close_after_uploading);
    let upload_hbox = GtkBox::new(Orientation::Horizontal, 12);
    upload_hbox.set_hexpand(true);
    let upload_option = Label::new(Some("Close window after uploading"));
    upload_option.set_xalign(0.0);
    upload_option.set_hexpand(true);
    upload_hbox.append(&upload_option);
    upload_hbox.append(&close_after_uploading_check);
    behaviors_frame.append(&build_row!(&upload_hbox, true));

    section.append(&behaviors_frame);

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
