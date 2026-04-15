use crate::config::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation,
};

#[allow(dead_code)]
pub struct RecordingSettingsWidgets {
    pub section: GtkBox,
    pub video_export_location_entry: Entry,
    pub video_export_location_browse: Button,
    pub rec_notifications_check: CheckButton,
    pub rec_countdown_check: CheckButton,
    pub rec_remember_selection_check: CheckButton,
    pub rec_display_time_check: CheckButton,
}

pub fn build_recording_section(config: &AppConfig) -> RecordingSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 14);
    section.set_halign(Align::Fill);
    section.set_valign(Align::Start);
    section.set_hexpand(true);
    section.set_margin_top(20);
    section.set_margin_bottom(8);

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

    // --- Save Location Group ---
    let location_title = Label::new(Some("Save Location"));
    location_title.add_css_class("settings-group-title");
    location_title.set_xalign(0.0);
    location_title.set_halign(Align::Start);
    location_title.set_margin_bottom(8);
    section.append(&location_title);

    let location_frame = build_frame();

    let video_export_location_entry = Entry::new();
    video_export_location_entry.set_hexpand(true);
    video_export_location_entry.set_width_chars(28);
    video_export_location_entry.set_placeholder_text(Some("Choose a folder"));
    video_export_location_entry.set_text(&config.video_export_location);
    let video_export_location_browse = Button::with_label("Browse");

    let export_hbox = GtkBox::new(Orientation::Horizontal, 12);
    export_hbox.set_hexpand(true);
    let export_label = Label::new(Some("Recordings folder"));
    export_label.set_xalign(0.0);
    export_label.set_hexpand(true);
    let entry_row = GtkBox::new(Orientation::Horizontal, 8);
    entry_row.append(&video_export_location_entry);
    entry_row.append(&video_export_location_browse);
    export_hbox.append(&export_label);
    export_hbox.append(&entry_row);
    location_frame.append(&build_row!(&export_hbox, false));
    section.append(&location_frame);

    // --- Recording Behavior Group ---
    let behavior_title = Label::new(Some("Recording Behavior"));
    behavior_title.add_css_class("settings-group-title");
    behavior_title.set_xalign(0.0);
    behavior_title.set_halign(Align::Start);
    behavior_title.set_margin_bottom(8);
    section.append(&behavior_title);

    let behavior_frame = build_frame();

    let create_row = |frame: &GtkBox, label_text: &str, is_muted: bool| -> CheckButton {
        let hbox = GtkBox::new(Orientation::Horizontal, 12);
        hbox.set_hexpand(true);
        let label = Label::new(Some(label_text));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let check = CheckButton::new();
        hbox.append(&label);
        hbox.append(&check);
        frame.append(&build_row!(&hbox, is_muted));
        check
    };

    let rec_notifications_check = create_row(
        &behavior_frame,
        "Enable \"Do Not Disturb\" while recording",
        false,
    );
    rec_notifications_check.set_active(config.rec_notifications);

    let rec_countdown_check = create_row(&behavior_frame, "Show countdown before start", true);
    rec_countdown_check.set_active(config.rec_countdown);

    let rec_remember_selection_check =
        create_row(&behavior_frame, "Remember last selection area", false);
    rec_remember_selection_check.set_active(config.rec_remember_selection);

    let rec_display_time_check = create_row(
        &behavior_frame,
        "Display recording time in the top bar",
        true,
    );
    rec_display_time_check.set_active(config.rec_display_time);

    section.append(&behavior_frame);

    RecordingSettingsWidgets {
        section,
        video_export_location_entry,
        video_export_location_browse,
        rec_notifications_check,
        rec_countdown_check,
        rec_remember_selection_check,
        rec_display_time_check,
    }
}
