use crate::{
    config::{load_config, save_config, AppConfig},
    daemon::set_daemon_tray_visibility,
};
use gtk4::{prelude::*, ApplicationWindow, CheckButton, ComboBoxText, Entry};

use super::windowing::{install_autostart_entry_for_current_exe, uninstall_autostart_entry};

pub struct SaveInputs {
    pub start_at_login: CheckButton,
    pub play_sounds: CheckButton,
    pub shutter_sound: ComboBoxText,
    pub show_tray_icon: CheckButton,
    pub export_location: Entry,
    pub hide_desktop_icons: CheckButton,
    pub screenshot_quick_access: CheckButton,
    pub screenshot_copy_to_clipboard: CheckButton,
    pub screenshot_save: CheckButton,
    pub screenshot_open_annotate: CheckButton,
}

pub fn install_checkbox_behaviors(
    play_sounds_check: &CheckButton,
    shutter_sound_input: &ComboBoxText,
    screenshot_quick_access_check: &CheckButton,
    screenshot_copy_to_clipboard_check: &CheckButton,
    screenshot_save_check: &CheckButton,
    screenshot_open_annotate_check: &CheckButton,
) {
    let shutter_sound_input_toggle = shutter_sound_input.clone();
    play_sounds_check.connect_toggled(move |check| {
        shutter_sound_input_toggle.set_sensitive(check.is_active());
    });

    let screenshot_open_annotate_toggle = screenshot_open_annotate_check.clone();
    screenshot_quick_access_check.connect_toggled(move |check| {
        if check.is_active() {
            screenshot_open_annotate_toggle.set_active(false);
        }
    });

    let screenshot_quick_access_toggle = screenshot_quick_access_check.clone();
    screenshot_open_annotate_check.connect_toggled(move |check| {
        if check.is_active() {
            screenshot_quick_access_toggle.set_active(false);
        }
    });

    screenshot_copy_to_clipboard_check.set_sensitive(screenshot_save_check.is_active());
    let screenshot_copy_to_clipboard_toggle_for_save = screenshot_copy_to_clipboard_check.clone();
    screenshot_save_check.connect_toggled(move |check| {
        let active = check.is_active();
        screenshot_copy_to_clipboard_toggle_for_save.set_sensitive(active);
        if !active {
            screenshot_copy_to_clipboard_toggle_for_save.set_active(false);
        }
    });

    screenshot_open_annotate_check.set_sensitive(screenshot_save_check.is_active());
    let screenshot_open_annotate_toggle_for_save = screenshot_open_annotate_check.clone();
    screenshot_save_check.connect_toggled(move |check| {
        let active = check.is_active();
        screenshot_open_annotate_toggle_for_save.set_sensitive(active);
        if !active {
            screenshot_open_annotate_toggle_for_save.set_active(false);
        }
    });
}

pub fn save_settings(inputs: &SaveInputs) -> anyhow::Result<AppConfig> {
    let mut config = load_config().sanitized();
    config.start_at_login = inputs.start_at_login.is_active();
    config.play_sounds = inputs.play_sounds.is_active();
    config.shutter_sound = inputs
        .shutter_sound
        .active_id()
        .map(|sound| sound.to_string())
        .unwrap_or_else(|| crate::config::DEFAULT_SHUTTER_SOUND.to_string());
    config.show_menu_bar_icon = inputs.show_tray_icon.is_active();
    config.export_location = inputs.export_location.text().to_string();
    config.hide_desktop_icons_while_capturing = inputs.hide_desktop_icons.is_active();
    config.after_capture_show_quick_access = inputs.screenshot_quick_access.is_active();
    config.after_capture_copy_file_to_clipboard = inputs.screenshot_copy_to_clipboard.is_active();
    config.after_capture_save = inputs.screenshot_save.is_active();
    config.after_capture_open_annotate = inputs.screenshot_open_annotate.is_active();
    let config = config.sanitized();

    save_config(&config)?;

    if config.start_at_login {
        install_autostart_entry_for_current_exe()?;
    } else {
        uninstall_autostart_entry()?;
    }

    let _ = set_daemon_tray_visibility(config.show_menu_bar_icon);

    Ok(config)
}

pub fn close_window(window: &ApplicationWindow) {
    window.close();
}
