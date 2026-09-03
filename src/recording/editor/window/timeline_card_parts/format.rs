pub fn labeled_tool_button(icon_name: &str, label: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-tool");
    button.set_tooltip_text(Some(tooltip));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let text = Label::new(Some(label));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    row.append(&icon);
    row.append(&text);
    button.set_child(Some(&row));
    button
}

pub fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button
}

pub fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub fn format_range(start: f64, end: f64) -> String {
    format!("{} - {}", format_mmss(start), format_mmss(end))
}

pub fn format_mmss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}
