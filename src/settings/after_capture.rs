use crate::config::AppConfig;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Grid, Label, Orientation, Separator};

#[allow(dead_code)]
pub struct AfterCaptureWidgets {
    pub wrapper: GtkBox,
    pub screenshot_after_capture_checks: Vec<CheckButton>,
    pub rec_open_video_editor: CheckButton,
}

pub fn build_after_capture_section(config: &AppConfig) -> AfterCaptureWidgets {
    let after_capture_wrapper = GtkBox::new(Orientation::Vertical, 8);
    after_capture_wrapper.set_halign(Align::Fill);
    after_capture_wrapper.set_hexpand(true);

    let after_capture_section = Grid::new();
    after_capture_section.set_halign(Align::Fill);
    after_capture_section.set_hexpand(true);
    after_capture_section.set_row_spacing(8);
    after_capture_section.set_column_spacing(10);

    let after_capture_title = Label::new(Some("After capture:"));
    after_capture_title.add_css_class("settings-group-title");
    after_capture_title.set_size_request(165, -1);
    after_capture_title.set_xalign(1.0);

    let after_capture_description = Label::new(Some(
        "Here you can decide what should happen after taking a screenshot or recording your screen.",
    ));
    after_capture_description.add_css_class("dim-label");
    after_capture_description.set_wrap(true);
    after_capture_description.set_xalign(0.0);
    after_capture_description.set_max_width_chars(44);
    after_capture_description.set_halign(Align::Start);

    let after_capture_table = GtkBox::new(Orientation::Vertical, 0);
    after_capture_table.set_halign(Align::Center);
    after_capture_table.set_hexpand(true);

    let after_capture_header_row = Grid::new();
    after_capture_header_row.add_css_class("settings-table-row");
    after_capture_header_row.set_column_spacing(18);
    after_capture_header_row.set_hexpand(true);

    let screenshot_header = Label::new(Some("Screenshot"));
    screenshot_header.add_css_class("settings-table-header");
    screenshot_header.set_halign(Align::Center);
    let screenshot_header_cell = GtkBox::new(Orientation::Horizontal, 0);
    screenshot_header_cell.set_size_request(108, -1);
    screenshot_header_cell.set_halign(Align::Center);
    screenshot_header_cell.append(&screenshot_header);

    let recording_header = Label::new(Some("Recording"));
    recording_header.add_css_class("settings-table-header");
    recording_header.set_halign(Align::Center);
    let recording_header_cell = GtkBox::new(Orientation::Horizontal, 0);
    recording_header_cell.set_size_request(80, -1);
    recording_header_cell.set_halign(Align::Center);
    recording_header_cell.append(&recording_header);

    let action_header = Label::new(Some("Action"));
    action_header.add_css_class("settings-table-header");
    action_header.set_halign(Align::Start);
    action_header.set_xalign(0.0);

    after_capture_header_row.attach(&screenshot_header_cell, 0, 0, 1, 1);
    after_capture_header_row.attach(&recording_header_cell, 1, 0, 1, 1);
    after_capture_header_row.attach(&action_header, 2, 0, 1, 1);
    after_capture_table.append(&after_capture_header_row);

    let table_header_separator = Separator::new(Orientation::Horizontal);
    table_header_separator.set_hexpand(true);
    after_capture_table.append(&table_header_separator);

    let screenshot_after_capture_rows = [
        (
            "Show Quick Access overlay",
            config.after_capture_show_quick_access,
        ),
        (
            "Copy to clipboard",
            config.after_capture_copy_file_to_clipboard,
        ),
        ("Save", config.after_capture_save),
        ("Open Annotate tool", config.after_capture_open_annotate),
        ("Open Video Editor", config.rec_video_open_editor),
    ];
    let mut screenshot_after_capture_checks = Vec::new();
    let mut rec_open_video_editor = CheckButton::new();

    for (index, (action, active)) in screenshot_after_capture_rows.into_iter().enumerate() {
        let row = Grid::new();
        row.add_css_class("settings-table-row");
        if index % 2 == 1 {
            row.add_css_class("settings-table-row-muted");
        }
        row.set_column_spacing(18);
        row.set_hexpand(true);

        let screenshot_cell = GtkBox::new(Orientation::Horizontal, 0);
        screenshot_cell.set_size_request(108, -1);
        screenshot_cell.set_halign(Align::Center);
        if action != "Open Video Editor" {
            let screenshot_check = CheckButton::new();
            screenshot_check.set_active(active);
            screenshot_after_capture_checks.push(screenshot_check.clone());
            screenshot_cell.append(&screenshot_check);
        }

        let recording_cell = GtkBox::new(Orientation::Horizontal, 0);
        recording_cell.set_size_request(80, -1);
        recording_cell.set_halign(Align::Center);
        if action != "Open Annotate tool" {
            let recording_check = CheckButton::new();
            if action == "Open Video Editor" {
                recording_check.set_active(active);
                rec_open_video_editor = recording_check.clone();
            }
            recording_cell.append(&recording_check);
        }

        let action_label = Label::new(Some(action));
        action_label.set_xalign(0.0);
        action_label.set_halign(Align::Start);

        row.attach(&screenshot_cell, 0, 0, 1, 1);
        row.attach(&recording_cell, 1, 0, 1, 1);
        row.attach(&action_label, 2, 0, 1, 1);
        after_capture_table.append(&row);
    }

    let after_capture_table_frame = GtkBox::new(Orientation::Vertical, 0);
    after_capture_table_frame.add_css_class("settings-table-frame");
    after_capture_table_frame.set_halign(Align::Center);
    after_capture_table_frame.set_margin_start(4);
    after_capture_table_frame.append(&after_capture_table);

    after_capture_section.attach(&after_capture_title, 0, 0, 1, 1);
    after_capture_section.attach(&after_capture_description, 1, 0, 1, 1);

    let after_capture_table_row = GtkBox::new(Orientation::Horizontal, 0);
    after_capture_table_row.set_halign(Align::Fill);
    after_capture_table_row.set_hexpand(true);
    after_capture_table_row.append(&after_capture_table_frame);

    after_capture_wrapper.append(&after_capture_section);
    after_capture_wrapper.append(&after_capture_table_row);

    AfterCaptureWidgets {
        wrapper: after_capture_wrapper,
        screenshot_after_capture_checks,
        rec_open_video_editor,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn after_capture_wrapper_does_not_force_a_fixed_width() {
        let source = include_str!("after_capture.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("set_size_request(450, -1);"),
            "after capture settings wrapper still hardcodes a 450px width"
        );
    }
}
