use crate::config::AppConfig;
use crate::i18n::t;
use gtk4::{prelude::*, Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};

use super::select::SettingsSelect;

#[allow(dead_code)]
pub struct RecordingSettingsWidgets {
    pub section: GtkBox,
    pub video_export_location_entry: Entry,
    pub video_export_location_browse: Button,
    pub rec_filename_pattern_entry: Entry,
    pub rec_remember_export_folder: CheckButton,
    pub rec_controls: CheckButton,
    pub rec_hidpi: CheckButton,
    pub rec_notifications: CheckButton,
    pub rec_countdown: CheckButton,
    pub rec_video_max_res: SettingsSelect,
    pub rec_video_fps: SettingsSelect,
    pub rec_video_mono: CheckButton,
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
    video_export_location_entry.set_placeholder_text(Some(&t("Choose a folder")));
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
    rec_filename_pattern_entry.set_placeholder_text(Some(&t("Filename pattern")));
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

    let behavior_title = Label::new(Some(&t("Recording")));
    behavior_title.add_css_class("settings-group-title");
    behavior_title.set_xalign(0.0);
    behavior_title.set_halign(Align::Start);
    behavior_title.set_margin_bottom(8);
    section.append(&behavior_title);

    let behavior_frame = build_frame();
    let build_toggle = |title: &str, description: &str, active: bool| {
        let check = CheckButton::new();
        check.set_active(active);

        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.set_hexpand(true);
        let text = GtkBox::new(Orientation::Vertical, 4);
        text.set_hexpand(true);
        let title_label = Label::new(Some(&t(title)));
        title_label.set_xalign(0.0);
        let description_label = Label::new(Some(&t(description)));
        description_label.set_xalign(0.0);
        description_label.set_wrap(true);
        description_label.add_css_class("settings-sub-option-hint");
        text.append(&title_label);
        text.append(&description_label);
        row.append(&text);
        row.append(&check);
        (row, check)
    };

    let (controls_row, rec_controls) = build_toggle(
        "Show recording controls",
        "Display controls for stopping and managing an active recording.",
        config.rec_controls,
    );
    behavior_frame.append(&build_row!(&controls_row, false));

    let (hidpi_row, rec_hidpi) = build_toggle(
        "Record at display scale resolution",
        "Use the display's HiDPI resolution when available.",
        config.rec_hidpi,
    );
    behavior_frame.append(&build_row!(&hidpi_row, true));

    let (notifications_row, rec_notifications) = build_toggle(
        "Do Not Disturb while recording",
        "Suppress desktop notifications until recording ends.",
        config.rec_notifications,
    );
    behavior_frame.append(&build_row!(&notifications_row, false));

    let (countdown_row, rec_countdown) = build_toggle(
        "Show countdown",
        "Show a short countdown before recording starts.",
        config.rec_countdown,
    );
    behavior_frame.append(&build_row!(&countdown_row, true));
    section.append(&behavior_frame);

    let video_title = Label::new(Some(&t("Video")));
    video_title.add_css_class("settings-group-title");
    video_title.set_xalign(0.0);
    video_title.set_halign(Align::Start);
    video_title.set_margin_bottom(8);
    section.append(&video_title);

    let video_frame = build_frame();
    let rec_video_max_res = SettingsSelect::new(
        [("0", t("Original")), ("1", t("1080p")), ("2", t("720p"))],
        &config.rec_video_max_res.to_string(),
    );
    let resolution_row = GtkBox::new(Orientation::Horizontal, 12);
    resolution_row.set_hexpand(true);
    let resolution_label = Label::new(Some(&t("Maximum resolution")));
    resolution_label.set_xalign(0.0);
    resolution_label.set_hexpand(true);
    resolution_row.append(&resolution_label);
    resolution_row.append(rec_video_max_res.widget());
    video_frame.append(&build_row!(&resolution_row, false));

    let rec_video_fps = SettingsSelect::new(
        [
            ("0", t("24 FPS")),
            ("1", t("30 FPS")),
            ("2", t("50 FPS")),
            ("3", t("60 FPS")),
        ],
        &config.rec_video_fps.to_string(),
    );
    let frame_rate_row = GtkBox::new(Orientation::Horizontal, 12);
    frame_rate_row.set_hexpand(true);
    let frame_rate_label = Label::new(Some(&t("Frame rate")));
    frame_rate_label.set_xalign(0.0);
    frame_rate_label.set_hexpand(true);
    frame_rate_row.append(&frame_rate_label);
    frame_rate_row.append(rec_video_fps.widget());
    video_frame.append(&build_row!(&frame_rate_row, true));

    let (mono_row, rec_video_mono) = build_toggle(
        "Record audio in mono",
        "Combine recorded audio channels into a single channel.",
        config.rec_video_mono,
    );
    video_frame.append(&build_row!(&mono_row, false));

    section.append(&video_frame);

    RecordingSettingsWidgets {
        section,
        video_export_location_entry,
        video_export_location_browse,
        rec_filename_pattern_entry,
        rec_remember_export_folder,
        rec_controls,
        rec_hidpi,
        rec_notifications,
        rec_countdown,
        rec_video_max_res,
        rec_video_fps,
        rec_video_mono,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recording_settings_exclude_area_and_gif_options() {
        let source = include_str!("recording.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production_source.contains("Remember last area"));
        assert!(!production_source.contains("Dim screen"));
        assert!(!production_source.contains("GIF"));
    }
}
