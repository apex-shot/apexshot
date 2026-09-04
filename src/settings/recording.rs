use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};

#[allow(dead_code)]
pub struct RecordingSettingsWidgets {
    pub section: GtkBox,
    pub video_export_location_entry: Entry,
    pub video_export_location_browse: Button,
    pub rec_filename_pattern_entry: Entry,
    pub rec_remember_export_folder: CheckButton,
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
    let location_title = Label::new(Some(&t("Save Location")));
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
    let video_export_location_browse = Button::with_label(&t("Browse"));

    let export_hbox = GtkBox::new(Orientation::Horizontal, 12);
    export_hbox.set_hexpand(true);
    let export_label = Label::new(Some(&t("Recordings folder")));
    export_label.set_xalign(0.0);
    export_label.set_hexpand(true);
    let entry_row = GtkBox::new(Orientation::Horizontal, 8);
    entry_row.append(&video_export_location_entry);
    entry_row.append(&video_export_location_browse);
    export_hbox.append(&export_label);
    export_hbox.append(&entry_row);
    location_frame.append(&build_row!(&export_hbox, false));

    // Filename pattern
    let rec_filename_pattern_entry = Entry::new();
    rec_filename_pattern_entry.set_hexpand(true);
    rec_filename_pattern_entry.set_width_chars(28);
    rec_filename_pattern_entry.set_placeholder_text(Some("Filename pattern"));
    rec_filename_pattern_entry.set_text(&config.rec_filename_pattern);
    let pattern_hbox = GtkBox::new(Orientation::Horizontal, 12);
    pattern_hbox.set_hexpand(true);
    let pattern_label = Label::new(Some(&t("Filename pattern")));
    pattern_label.set_xalign(0.0);
    pattern_label.set_hexpand(true);
    let pattern_hint = Label::new(Some(&t("Use {Date} and {Time} placeholders")));
    pattern_hint.add_css_class("settings-sub-option");
    pattern_hint.set_xalign(0.0);
    pattern_hbox.append(&pattern_label);
    let pattern_vbox = GtkBox::new(Orientation::Vertical, 4);
    pattern_vbox.set_hexpand(true);
    pattern_vbox.append(&rec_filename_pattern_entry);
    pattern_vbox.append(&pattern_hint);
    pattern_hbox.append(&pattern_vbox);
    location_frame.append(&build_row!(&pattern_hbox, false));

    let rec_remember_export_folder = CheckButton::new();
    rec_remember_export_folder.set_active(config.rec_remember_export_folder);
    let remember_hbox = GtkBox::new(Orientation::Horizontal, 12);
    remember_hbox.set_hexpand(true);
    let remember_label = Label::new(Some(&t("Remember last export folder")));
    remember_label.set_xalign(0.0);
    remember_label.set_hexpand(true);
    remember_hbox.append(&remember_label);
    remember_hbox.append(&rec_remember_export_folder);
    location_frame.append(&build_row!(&remember_hbox, false));

    section.append(&location_frame);

    RecordingSettingsWidgets {
        section,
        video_export_location_entry,
        video_export_location_browse,
        rec_filename_pattern_entry,
        rec_remember_export_folder,
    }
}
