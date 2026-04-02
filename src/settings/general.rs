use crate::config::{AppConfig, DEFAULT_SHUTTER_SOUND};
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Label, Orientation};

pub struct GeneralSettingsWidgets {
    pub section: GtkBox,
    pub start_at_login_check: CheckButton,
    pub play_sounds_check: CheckButton,
    pub shutter_sound_input: ComboBoxText,
    pub show_icon_check: CheckButton,
}

pub fn build_general_section(config: &AppConfig) -> GeneralSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 14);
    section.set_halign(Align::Center);
    section.set_valign(Align::Start);
    section.set_margin_top(20);
    section.set_margin_bottom(8);
    section.set_size_request(450, -1);

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

    // --- Startup Group ---
    let startup_title = Label::new(Some("Startup"));
    startup_title.add_css_class("settings-group-title");
    startup_title.set_xalign(0.0);
    startup_title.set_halign(Align::Start);
    startup_title.set_margin_bottom(8);
    section.append(&startup_title);

    let startup_frame = build_frame();
    
    let start_at_login_check = CheckButton::new();
    start_at_login_check.set_active(config.start_at_login);
    let startup_hbox = GtkBox::new(Orientation::Horizontal, 12);
    startup_hbox.set_hexpand(true);
    let startup_option = Label::new(Some("Start at login"));
    startup_option.set_xalign(0.0);
    startup_option.set_hexpand(true);
    startup_hbox.append(&startup_option);
    startup_hbox.append(&start_at_login_check);
    startup_frame.append(&build_row!(&startup_hbox, false));
    section.append(&startup_frame);

    // --- Sounds Group ---
    let sound_title = Label::new(Some("Sounds"));
    sound_title.add_css_class("settings-group-title");
    sound_title.set_xalign(0.0);
    sound_title.set_halign(Align::Start);
    sound_title.set_margin_bottom(8);
    section.append(&sound_title);

    let sounds_frame = build_frame();
    
    let play_sounds_check = CheckButton::new();
    play_sounds_check.set_active(config.play_sounds);
    let sounds_hbox = GtkBox::new(Orientation::Horizontal, 12);
    sounds_hbox.set_hexpand(true);
    let sound_option = Label::new(Some("Play sounds"));
    sound_option.set_xalign(0.0);
    sound_option.set_hexpand(true);
    sounds_hbox.append(&sound_option);
    sounds_hbox.append(&play_sounds_check);
    sounds_frame.append(&build_row!(&sounds_hbox, false));

    let shutter_sound_input = ComboBoxText::new();
    shutter_sound_input.add_css_class("settings-select");
    for sound in ["Camera", "Classic", "Pop", "None"] {
        shutter_sound_input.append(Some(sound), sound);
    }
    if !shutter_sound_input.set_active_id(Some(&config.shutter_sound)) {
        shutter_sound_input.set_active_id(Some(DEFAULT_SHUTTER_SOUND));
    }
    shutter_sound_input.set_sensitive(config.play_sounds);
    
    let shutter_hbox = GtkBox::new(Orientation::Horizontal, 12);
    shutter_hbox.set_hexpand(true);
    let shutter_title = Label::new(Some("Shutter sound"));
    shutter_title.add_css_class("settings-sub-option");
    shutter_title.set_xalign(0.0);
    shutter_title.set_hexpand(true);
    shutter_hbox.append(&shutter_title);
    shutter_hbox.append(&shutter_sound_input);
    sounds_frame.append(&build_row!(&shutter_hbox, true));
    section.append(&sounds_frame);

    // --- System Tray Group ---
    let tray_title = Label::new(Some("System tray"));
    tray_title.add_css_class("settings-group-title");
    tray_title.set_xalign(0.0);
    tray_title.set_halign(Align::Start);
    tray_title.set_margin_bottom(8);
    section.append(&tray_title);

    let tray_frame = build_frame();
    let show_icon_check = CheckButton::new();
    show_icon_check.set_active(config.show_menu_bar_icon);
    let tray_hbox = GtkBox::new(Orientation::Horizontal, 12);
    tray_hbox.set_hexpand(true);
    let tray_option = Label::new(Some("Show tray icon"));
    tray_option.set_xalign(0.0);
    tray_option.set_hexpand(true);
    tray_hbox.append(&tray_option);
    tray_hbox.append(&show_icon_check);
    tray_frame.append(&build_row!(&tray_hbox, false));
    section.append(&tray_frame);

    GeneralSettingsWidgets {
        section,
        start_at_login_check,
        play_sounds_check,
        shutter_sound_input,
        show_icon_check,
    }
}
