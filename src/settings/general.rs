use crate::config::{AppConfig, DEFAULT_SHUTTER_SOUND};
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, ComboBoxText, Grid, Label, Orientation};

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

    let settings_grid = Grid::new();
    settings_grid.set_halign(Align::Center);
    settings_grid.set_row_spacing(8);
    settings_grid.set_column_spacing(10);

    let startup_title = Label::new(Some("Startup:"));
    startup_title.add_css_class("settings-group-title");
    startup_title.set_size_request(165, -1);
    startup_title.set_xalign(1.0);
    let start_at_login_check = CheckButton::new();
    start_at_login_check.set_active(config.start_at_login);
    let start_at_login_cell = GtkBox::new(Orientation::Horizontal, 0);
    start_at_login_cell.set_size_request(28, -1);
    start_at_login_cell.set_halign(Align::Start);
    start_at_login_cell.append(&start_at_login_check);
    let startup_option = Label::new(Some("Start at login"));
    startup_option.set_xalign(0.0);
    settings_grid.attach(&startup_title, 0, 0, 1, 1);
    settings_grid.attach(&start_at_login_cell, 1, 0, 1, 1);
    settings_grid.attach(&startup_option, 2, 0, 1, 1);

    let sound_title = Label::new(Some("Sounds:"));
    sound_title.add_css_class("settings-group-title");
    sound_title.set_size_request(165, -1);
    sound_title.set_xalign(1.0);
    let play_sounds_check = CheckButton::new();
    play_sounds_check.set_active(config.play_sounds);
    let play_sounds_cell = GtkBox::new(Orientation::Horizontal, 0);
    play_sounds_cell.set_size_request(28, -1);
    play_sounds_cell.set_halign(Align::Start);
    play_sounds_cell.append(&play_sounds_check);
    let sound_option = Label::new(Some("Play sounds"));
    sound_option.set_xalign(0.0);
    settings_grid.attach(&sound_title, 0, 1, 1, 1);
    settings_grid.attach(&play_sounds_cell, 1, 1, 1, 1);
    settings_grid.attach(&sound_option, 2, 1, 1, 1);

    let shutter_title = Label::new(Some("Shutter sound"));
    shutter_title.add_css_class("settings-sub-option");
    shutter_title.set_xalign(0.0);
    let shutter_sound_input = ComboBoxText::new();
    shutter_sound_input.add_css_class("settings-select");
    for sound in ["Camera", "Classic", "Pop", "None"] {
        shutter_sound_input.append(Some(sound), sound);
    }
    if !shutter_sound_input.set_active_id(Some(&config.shutter_sound)) {
        shutter_sound_input.set_active_id(Some(DEFAULT_SHUTTER_SOUND));
    }
    shutter_sound_input.set_sensitive(config.play_sounds);
    shutter_sound_input.set_halign(Align::Start);
    let shutter_row = GtkBox::new(Orientation::Horizontal, 10);
    shutter_row.set_halign(Align::Start);
    shutter_row.append(&shutter_title);
    shutter_row.append(&shutter_sound_input);
    settings_grid.attach(&shutter_row, 2, 2, 1, 1);

    let tray_title = Label::new(Some("System tray:"));
    tray_title.add_css_class("settings-group-title");
    tray_title.set_size_request(165, -1);
    tray_title.set_xalign(1.0);
    let show_icon_check = CheckButton::new();
    show_icon_check.set_active(config.show_menu_bar_icon);
    let show_icon_cell = GtkBox::new(Orientation::Horizontal, 0);
    show_icon_cell.set_size_request(28, -1);
    show_icon_cell.set_halign(Align::Start);
    show_icon_cell.append(&show_icon_check);
    let tray_option = Label::new(Some("Show tray icon"));
    tray_option.set_xalign(0.0);
    settings_grid.attach(&tray_title, 0, 3, 1, 1);
    settings_grid.attach(&show_icon_cell, 1, 3, 1, 1);
    settings_grid.attach(&tray_option, 2, 3, 1, 1);

    section.append(&settings_grid);

    GeneralSettingsWidgets {
        section,
        start_at_login_check,
        play_sounds_check,
        shutter_sound_input,
        show_icon_check,
    }
}
