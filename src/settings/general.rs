use crate::config::{AppConfig, DEFAULT_SHUTTER_SOUND};
use crate::i18n::t;
use gtk4::{prelude::*, Align, Box as GtkBox, CheckButton, Label, Orientation};

use super::select::{language_combo, SettingsSelect};
use super::theme_picker::ThemePicker;

pub struct GeneralSettingsWidgets {
    pub section: GtkBox,
    pub start_at_login_check: CheckButton,
    pub play_sounds_check: CheckButton,
    pub shutter_sound_input: SettingsSelect,
    pub show_icon_check: CheckButton,
    pub theme_input: ThemePicker,
    pub ui_language_input: SettingsSelect,
}

pub fn build_general_section(config: &AppConfig) -> GeneralSettingsWidgets {
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

    // --- Appearance Group ---
    let appearance_title = Label::new(Some(&t("Appearance")));
    appearance_title.add_css_class("settings-group-title");
    appearance_title.set_xalign(0.0);
    appearance_title.set_halign(Align::Start);
    appearance_title.set_margin_bottom(8);
    section.append(&appearance_title);

    let appearance_frame = build_frame();
    let theme_hbox = GtkBox::new(Orientation::Horizontal, 12);
    theme_hbox.set_hexpand(true);
    theme_hbox.set_valign(Align::Center);
    let theme_option = Label::new(Some(&t("Theme")));
    theme_option.set_xalign(0.0);
    theme_option.set_hexpand(true);
    let theme_input = ThemePicker::new(&config.ui_theme);
    theme_hbox.append(&theme_option);
    theme_hbox.append(theme_input.widget());
    appearance_frame.append(&build_row!(&theme_hbox, false));
    section.append(&appearance_frame);

    // --- Startup Group ---
    let language_title = Label::new(Some(&t("Language")));
    language_title.add_css_class("settings-group-title");
    language_title.set_xalign(0.0);
    language_title.set_halign(Align::Start);
    language_title.set_margin_bottom(8);
    section.append(&language_title);

    let language_frame = build_frame();
    let ui_language_input = language_combo(&config.ui_language);
    let language_hbox = GtkBox::new(Orientation::Horizontal, 12);
    language_hbox.set_hexpand(true);
    language_hbox.set_valign(Align::Center);
    let language_option = Label::new(Some(&t("Display language")));
    language_option.set_xalign(0.0);
    language_option.set_hexpand(true);
    let language_help = Label::new(Some(&t(
        "System default uses your desktop language. Changes apply after Save.",
    )));
    language_help.add_css_class("dim-label");
    language_help.set_wrap(true);
    language_help.set_xalign(0.0);
    let language_col = GtkBox::new(Orientation::Vertical, 4);
    language_col.set_hexpand(true);
    language_col.append(&language_option);
    language_col.append(&language_help);
    language_hbox.append(&language_col);
    language_hbox.append(ui_language_input.widget());
    language_frame.append(&build_row!(&language_hbox, false));
    section.append(&language_frame);

    let startup_title = Label::new(Some(&t("Startup")));
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
    let startup_option = Label::new(Some(&t("Start at login")));
    startup_option.set_xalign(0.0);
    startup_option.set_hexpand(true);
    startup_hbox.append(&startup_option);
    startup_hbox.append(&start_at_login_check);
    startup_frame.append(&build_row!(&startup_hbox, false));
    section.append(&startup_frame);

    // --- Sounds Group ---
    let sound_title = Label::new(Some(&t("Sounds")));
    sound_title.add_css_class("settings-group-title");
    sound_title.set_xalign(0.0);
    sound_title.set_halign(Align::Start);
    sound_title.set_margin_bottom(8);
    section.append(&sound_title);

    let sounds_frame = build_frame();

    let custom_shutter_sound = crate::capture_overlay::supports_custom_shutter_sound();

    let play_sounds_check = CheckButton::new();
    play_sounds_check.set_active(config.play_sounds);
    play_sounds_check.set_sensitive(custom_shutter_sound);
    let sounds_hbox = GtkBox::new(Orientation::Horizontal, 12);
    sounds_hbox.set_hexpand(true);
    let sound_option = Label::new(Some(&t("Play sounds")));
    sound_option.set_xalign(0.0);
    sound_option.set_hexpand(true);
    sounds_hbox.append(&sound_option);
    sounds_hbox.append(&play_sounds_check);
    sounds_frame.append(&build_row!(&sounds_hbox, false));

    let shutter_sound_input = SettingsSelect::new(
        ["Camera", "Classic", "Pop", "None"].map(|sound| (sound, t(sound))),
        &config.shutter_sound,
    );
    if shutter_sound_input.active_id().as_deref() != Some(config.shutter_sound.as_str()) {
        shutter_sound_input.set_active_id(DEFAULT_SHUTTER_SOUND);
    }
    shutter_sound_input.set_sensitive(custom_shutter_sound && config.play_sounds);

    let shutter_hbox = GtkBox::new(Orientation::Horizontal, 12);
    shutter_hbox.set_hexpand(true);
    let shutter_title = Label::new(Some(&t("Shutter sound")));
    shutter_title.add_css_class("settings-sub-option");
    shutter_title.set_xalign(0.0);
    shutter_title.set_hexpand(true);
    shutter_hbox.append(&shutter_title);
    shutter_hbox.append(shutter_sound_input.widget());
    sounds_frame.append(&build_row!(&shutter_hbox, true));

    let sound_help_text = if custom_shutter_sound {
        t("Played once, when the capture freezes. Saving or cropping stays silent.")
    } else {
        t("Your desktop controls the capture sound on this route, so ApexShot cannot replace or disable it here and will not play an additional sound.")
    };
    let sound_help = Label::new(Some(&sound_help_text));
    sound_help.add_css_class("dim-label");
    sound_help.set_wrap(true);
    sound_help.set_xalign(0.0);
    sound_help.set_hexpand(true);
    let sound_help_hbox = GtkBox::new(Orientation::Horizontal, 12);
    sound_help_hbox.set_hexpand(true);
    sound_help_hbox.append(&sound_help);
    sounds_frame.append(&build_row!(&sound_help_hbox, true));
    section.append(&sounds_frame);

    // --- System Tray Group ---
    let tray_title = Label::new(Some(&t("System tray")));
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
    let tray_option = Label::new(Some(&t("Show tray icon")));
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
        theme_input,
        ui_language_input,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn general_section_does_not_force_a_fixed_width() {
        let source = include_str!("general.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("set_size_request(450, -1);"),
            "general settings section still hardcodes a 450px width"
        );
    }
}
