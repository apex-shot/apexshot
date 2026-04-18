use crate::{
    config::{load_config, save_config},
    daemon::{set_daemon_tray_visibility, start_daemon_subprocess, stop_daemon_via_dbus},
};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, ComboBoxText, Entry, Scale};

use super::windowing::{install_autostart_entry_smart, uninstall_autostart_entry};

fn shortcut_label_value(label: Option<gtk4::glib::GString>) -> String {
    let label = label.unwrap_or_default();
    if label == "Record shortcut" {
        String::new()
    } else {
        label.to_string()
    }
}

fn button_label_value(button: &Button) -> String {
    shortcut_label_value(button.label())
}

fn should_auto_respawn_daemon_for_save_with_env(
    gio_launched_desktop_file_present: bool,
    daemon_desktop_relaunched_present: bool,
) -> bool {
    gio_launched_desktop_file_present || daemon_desktop_relaunched_present
}

fn should_auto_respawn_daemon_for_save() -> bool {
    should_auto_respawn_daemon_for_save_with_env(
        std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE").is_some(),
        std::env::var_os("APEXSHOT_DAEMON_DESKTOP_RELAUNCHED").is_some(),
    )
}

#[allow(dead_code)]
pub struct SaveInputs {
    pub start_at_login: CheckButton,
    pub play_sounds: CheckButton,
    pub shutter_sound: ComboBoxText,
    pub show_menu_bar_icon: CheckButton,
    pub screenshot_export_location: Entry,
    pub screenshot_format: ComboBoxText,
    pub video_export_location: Entry,
    pub screenshot_quick_access: CheckButton,
    pub screenshot_copy_to_clipboard: CheckButton,
    pub screenshot_save: CheckButton,
    pub screenshot_open_annotate: CheckButton,
    pub quick_access_position: ComboBoxText,
    pub quick_access_multi_display: CheckButton,
    pub quick_access_overlay_size: Scale,
    pub quick_access_auto_close_enabled: CheckButton,
    pub quick_access_auto_close_action: ComboBoxText,
    pub quick_access_auto_close_interval: ComboBoxText,
    pub quick_access_close_after_dragging: CheckButton,
    pub quick_access_close_after_uploading: CheckButton,
    pub screenshot_crosshair_mode: ComboBoxText,
    pub screenshot_show_magnifier: CheckButton,
    // pub screenshot_selection_color: ColorButton, // Removed missing mapping
    pub screenshot_freeze_screen: CheckButton,
    pub screenshot_timer_interval: ComboBoxText,
    pub screenshot_capture_cursor: CheckButton,
    pub annotate_inverse_arrow: CheckButton,
    pub annotate_smooth_drawing: CheckButton,
    pub annotate_draw_shadow: CheckButton,
    pub annotate_auto_expand: CheckButton,
    pub annotate_show_color_names: CheckButton,
    pub annotate_always_on_top: CheckButton,
    pub annotate_show_dock_icon: CheckButton,
    pub rec_notifications: CheckButton,
    pub rec_countdown: CheckButton,
    pub rec_remember_selection: CheckButton,
    pub rec_display_time: CheckButton,
    pub shortcut_open_file: Button,
    pub shortcut_open_from_clipboard: Button,
    pub shortcut_restore_recently_closed: Button,
    pub shortcut_toggle_overlays: Button,
    pub shortcut_capture_area: Button,
    pub shortcut_capture_crosshair: Button,
    pub shortcut_capture_previous_area: Button,
    pub shortcut_capture_fullscreen: Button,
    pub shortcut_capture_window: Button,
    pub shortcut_open_recording_ui: Button,
    pub shortcut_record_screen: Button,
    pub shortcut_recording_pause_resume: Button,
    pub shortcut_recording_stop_save: Button,
    pub shortcut_recording_restart: Button,
    pub shortcut_recording_discard: Button,
    pub cloud_screenshot_quality: ComboBoxText,
    pub cloud_copy_to_clipboard: ComboBoxText,
    pub cloud_show_recently_uploaded: CheckButton,
    pub cloud_ask_name_tags: CheckButton,
    pub adv_retina_suffix: CheckButton,
    pub adv_clipboard_mode: ComboBoxText,
    pub adv_ocr_language: ComboBoxText,
    pub adv_ocr_keep_line_breaks: CheckButton,
}

pub fn install_checkbox_behaviors(
    play_sounds_check: &CheckButton,
    shutter_sound_input: &ComboBoxText,
    screenshot_quick_access_check: &CheckButton,
    screenshot_copy_to_clipboard_check: &CheckButton,
    screenshot_save_check: &CheckButton,
    screenshot_open_annotate_check: &CheckButton,
    quick_access_auto_close_enabled_check: &CheckButton,
    quick_access_auto_close_action_input: &ComboBoxText,
    quick_access_auto_close_interval_input: &ComboBoxText,
    screenshot_crosshair_mode_input: &ComboBoxText,
    screenshot_show_magnifier_check: &CheckButton,
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

    let a1 = quick_access_auto_close_action_input.clone();
    let a2 = quick_access_auto_close_interval_input.clone();
    a1.set_sensitive(quick_access_auto_close_enabled_check.is_active());
    a2.set_sensitive(quick_access_auto_close_enabled_check.is_active());
    quick_access_auto_close_enabled_check.connect_toggled(move |check| {
        let active = check.is_active();
        a1.set_sensitive(active);
        a2.set_sensitive(active);
    });

    let m1 = screenshot_show_magnifier_check.clone();
    m1.set_sensitive(matches!(
        screenshot_crosshair_mode_input.active_id().as_deref(),
        Some("Crosshair") | Some("Magnifier")
    ));
    screenshot_crosshair_mode_input.connect_changed(move |combo| {
        let id = combo.active_id().unwrap_or_default();
        m1.set_sensitive(id == "Crosshair" || id == "Magnifier");
    });
}

pub fn save_settings(inputs: &SaveInputs) -> anyhow::Result<()> {
    let previous_config = load_config().sanitized();
    let mut config = previous_config.clone();
    config.start_at_login = inputs.start_at_login.is_active();
    config.play_sounds = inputs.play_sounds.is_active();
    config.shutter_sound = combo_value(&inputs.shutter_sound, "Default");
    config.show_menu_bar_icon = inputs.show_menu_bar_icon.is_active();
    config.export_location.clear();
    config.screenshot_export_location = inputs.screenshot_export_location.text().to_string();
    config.video_export_location = inputs.video_export_location.text().to_string();

    config.after_capture_show_quick_access = inputs.screenshot_quick_access.is_active();
    config.after_capture_copy_file_to_clipboard = inputs.screenshot_copy_to_clipboard.is_active();
    config.after_capture_save = inputs.screenshot_save.is_active();
    config.after_capture_open_annotate = inputs.screenshot_open_annotate.is_active();
    config.quick_access_position = combo_value(&inputs.quick_access_position, "Left");
    config.quick_access_multi_display = inputs.quick_access_multi_display.is_active();
    config.quick_access_overlay_size = inputs.quick_access_overlay_size.value();
    config.quick_access_auto_close_enabled = inputs.quick_access_auto_close_enabled.is_active();
    config.quick_access_auto_close_action =
        combo_value(&inputs.quick_access_auto_close_action, "Close");
    config.quick_access_auto_close_interval = inputs
        .quick_access_auto_close_interval
        .active_id()
        .or_else(|| {
            inputs
                .quick_access_auto_close_interval
                .active_text()
                .map(Into::into)
        })
        .unwrap_or_else(|| "30".into())
        .parse()
        .unwrap_or(30);
    config.quick_access_close_after_dragging = inputs.quick_access_close_after_dragging.is_active();
    config.quick_access_close_after_uploading =
        inputs.quick_access_close_after_uploading.is_active();

    config.screenshot_format = combo_value(&inputs.screenshot_format, "PNG");
    config.screenshot_crosshair_mode = combo_value(&inputs.screenshot_crosshair_mode, "Disabled");
    config.screenshot_show_magnifier = inputs.screenshot_show_magnifier.is_active();
    config.screenshot_freeze_screen = inputs.screenshot_freeze_screen.is_active();
    config.screenshot_timer_interval = inputs
        .screenshot_timer_interval
        .active_id()
        .or_else(|| {
            inputs
                .screenshot_timer_interval
                .active_text()
                .map(Into::into)
        })
        .unwrap_or_else(|| "5".into())
        .parse()
        .unwrap_or(5);
    config.screenshot_show_cursor = inputs.screenshot_capture_cursor.is_active();
    config.annotate_inverse_arrow = inputs.annotate_inverse_arrow.is_active();
    config.annotate_smooth_drawing = inputs.annotate_smooth_drawing.is_active();
    config.annotate_draw_shadow = inputs.annotate_draw_shadow.is_active();
    config.annotate_auto_expand = inputs.annotate_auto_expand.is_active();
    config.annotate_show_color_names = inputs.annotate_show_color_names.is_active();
    config.annotate_always_on_top = inputs.annotate_always_on_top.is_active();
    config.annotate_show_dock_icon = inputs.annotate_show_dock_icon.is_active();

    config.rec_notifications = inputs.rec_notifications.is_active();
    config.rec_countdown = inputs.rec_countdown.is_active();
    config.rec_remember_selection = inputs.rec_remember_selection.is_active();
    config.rec_display_time = inputs.rec_display_time.is_active();

    config.shortcut_open_file = button_label_value(&inputs.shortcut_open_file);
    config.shortcut_open_from_clipboard = button_label_value(&inputs.shortcut_open_from_clipboard);
    config.shortcut_restore_recently_closed =
        button_label_value(&inputs.shortcut_restore_recently_closed);
    config.shortcut_toggle_overlays = button_label_value(&inputs.shortcut_toggle_overlays);
    config.shortcut_capture_area = button_label_value(&inputs.shortcut_capture_area);
    config.shortcut_capture_crosshair = button_label_value(&inputs.shortcut_capture_crosshair);
    config.shortcut_capture_previous_area =
        button_label_value(&inputs.shortcut_capture_previous_area);
    config.shortcut_capture_fullscreen = button_label_value(&inputs.shortcut_capture_fullscreen);
    config.shortcut_capture_window = button_label_value(&inputs.shortcut_capture_window);
    config.shortcut_open_recording_ui = button_label_value(&inputs.shortcut_open_recording_ui);
    config.shortcut_record_screen = button_label_value(&inputs.shortcut_record_screen);
    config.shortcut_recording_pause_resume =
        button_label_value(&inputs.shortcut_recording_pause_resume);
    config.shortcut_recording_stop_save = button_label_value(&inputs.shortcut_recording_stop_save);
    config.shortcut_recording_restart = button_label_value(&inputs.shortcut_recording_restart);
    config.shortcut_recording_discard = button_label_value(&inputs.shortcut_recording_discard);

    config.cloud_screenshot_quality =
        combo_value(&inputs.cloud_screenshot_quality, "Optimized for sharing");
    config.cloud_copy_to_clipboard =
        combo_value(&inputs.cloud_copy_to_clipboard, "ApexShot Cloud link");
    config.cloud_show_recently_uploaded = inputs.cloud_show_recently_uploaded.is_active();
    config.cloud_ask_name_tags = inputs.cloud_ask_name_tags.is_active();

    config.adv_retina_suffix = inputs.adv_retina_suffix.is_active();
    config.adv_clipboard_mode = combo_value(&inputs.adv_clipboard_mode, "File & Image (default)");

    config.adv_ocr_language = combo_value(&inputs.adv_ocr_language, "eng");
    config.adv_ocr_keep_line_breaks = inputs.adv_ocr_keep_line_breaks.is_active();

    let config = config.sanitized();
    let quick_access_runtime_changed = previous_config.quick_access_position
        != config.quick_access_position
        || previous_config.quick_access_multi_display != config.quick_access_multi_display
        || (previous_config.quick_access_overlay_size - config.quick_access_overlay_size).abs()
            > f64::EPSILON
        || previous_config.quick_access_auto_close_enabled
            != config.quick_access_auto_close_enabled
        || previous_config.quick_access_auto_close_action != config.quick_access_auto_close_action
        || previous_config.quick_access_auto_close_interval
            != config.quick_access_auto_close_interval
        || previous_config.quick_access_close_after_dragging
            != config.quick_access_close_after_dragging
        || previous_config.quick_access_close_after_uploading
            != config.quick_access_close_after_uploading;

    let shortcuts_runtime_changed = previous_config.shortcut_open_file != config.shortcut_open_file
        || previous_config.shortcut_open_from_clipboard != config.shortcut_open_from_clipboard
        || previous_config.shortcut_restore_recently_closed
            != config.shortcut_restore_recently_closed
        || previous_config.shortcut_toggle_overlays != config.shortcut_toggle_overlays
        || previous_config.shortcut_capture_area != config.shortcut_capture_area
        || previous_config.shortcut_capture_crosshair != config.shortcut_capture_crosshair
        || previous_config.shortcut_capture_previous_area != config.shortcut_capture_previous_area
        || previous_config.shortcut_capture_fullscreen != config.shortcut_capture_fullscreen
        || previous_config.shortcut_capture_window != config.shortcut_capture_window
        || previous_config.shortcut_open_recording_ui != config.shortcut_open_recording_ui
        || previous_config.shortcut_record_screen != config.shortcut_record_screen
        || previous_config.shortcut_recording_pause_resume
            != config.shortcut_recording_pause_resume
        || previous_config.shortcut_recording_stop_save != config.shortcut_recording_stop_save
        || previous_config.shortcut_recording_restart != config.shortcut_recording_restart
        || previous_config.shortcut_recording_discard != config.shortcut_recording_discard;

    save_config(&config)?;
    crate::hotkeys::sync_hotkeys_from_app_config(&config)?;
    let _ = crate::hotkeys::sync_gnome_hotkeys_for_current_desktop(None);

    if config.start_at_login {
        install_autostart_entry_smart()?;
    } else {
        uninstall_autostart_entry()?;
    }

    // Start or stop daemon based on tray icon setting
    let tray_visible = config.show_menu_bar_icon;
    std::thread::spawn(move || {
        if tray_visible {
            let _ = start_daemon_subprocess();
        } else {
            let _ = stop_daemon_via_dbus();
        }
    });

    let allow_auto_respawn = should_auto_respawn_daemon_for_save();
    std::thread::spawn(move || {
        if quick_access_runtime_changed || shortcuts_runtime_changed {
            if allow_auto_respawn && stop_daemon_via_dbus() {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = start_daemon_subprocess();
                return;
            }

            if !allow_auto_respawn {
                let _ = set_daemon_tray_visibility(tray_visible);
                return;
            }
        }

        if set_daemon_tray_visibility(tray_visible) {
            return;
        }
        if tray_visible && allow_auto_respawn {
            let _ = start_daemon_subprocess();
        }
    });

    Ok(())
}

fn combo_value(combo: &ComboBoxText, fallback: &str) -> String {
    combo
        .active_id()
        .or_else(|| combo.active_text().map(Into::into))
        .map(|value| value.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

#[allow(dead_code)]
pub fn close_window(window: &gtk4::ApplicationWindow) {
    window.close();
}

#[cfg(test)]
mod tests {
    use super::{shortcut_label_value, should_auto_respawn_daemon_for_save_with_env};

    #[test]
    fn button_label_value_treats_placeholder_as_empty() {
        assert_eq!(shortcut_label_value(Some("Record shortcut".into())), "");
        assert_eq!(
            shortcut_label_value(Some("Ctrl+Alt+R".into())),
            "Ctrl+Alt+R"
        );
    }

    #[test]
    fn auto_respawn_is_disabled_for_manual_daemon_sessions() {
        assert!(!should_auto_respawn_daemon_for_save_with_env(false, false));
    }

    #[test]
    fn auto_respawn_is_enabled_for_desktop_managed_daemon_sessions() {
        assert!(should_auto_respawn_daemon_for_save_with_env(true, false));
        assert!(should_auto_respawn_daemon_for_save_with_env(false, true));
    }
}
