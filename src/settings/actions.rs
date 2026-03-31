use crate::{
    config::{load_config, save_config},
    daemon::{set_daemon_tray_visibility, start_daemon_subprocess, stop_daemon_via_dbus},
};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, ColorButton, ComboBoxText, Entry, Scale};

use super::windowing::{install_autostart_entry_for_current_exe, uninstall_autostart_entry};

#[allow(dead_code)]
pub struct SaveInputs {
    pub start_at_login: CheckButton,
    pub play_sounds: CheckButton,
    pub shutter_sound: ComboBoxText,
    pub show_menu_bar_icon: CheckButton,
    pub export_location: Entry,
    pub hide_desktop_icons: CheckButton,
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
    pub screenshot_capture_cursor: CheckButton,
    pub rec_controls: CheckButton,
    pub rec_display_time: CheckButton,
    pub rec_hidpi: CheckButton,
    pub rec_notifications: CheckButton,
    pub rec_cursor: CheckButton,
    pub rec_clicks: CheckButton,
    pub rec_keystrokes: CheckButton,
    pub rec_key_filter: ComboBoxText,
    pub wallpaper_mode_desktop: CheckButton,
    pub wallpaper_dont_change_on_space: CheckButton,
    pub wallpaper_mode_custom: CheckButton,
    pub wallpaper_custom_path_btn: Button,
    pub wallpaper_mode_color: CheckButton,
    pub wallpaper_color_btn: ColorButton,
    pub window_screenshot_mode_full: CheckButton,
    pub window_screenshot_mode_trans: CheckButton,
    pub window_screenshot_padding: Scale,
    pub window_screenshot_shadow: CheckButton,
    pub shortcut_toggle_desktop_icons: Button,
    pub shortcut_open_file: Button,
    pub shortcut_open_from_clipboard: Button,
    pub shortcut_pin_to_screen: Button,
    pub shortcut_restore_recently_closed: Button,
    pub shortcut_toggle_overlays: Button,
    pub shortcut_capture_area: Button,
    pub shortcut_capture_previous_area: Button,
    pub shortcut_capture_fullscreen: Button,
    pub shortcut_capture_window: Button,
    pub cloud_screenshot_quality: ComboBoxText,
    pub cloud_copy_to_clipboard: ComboBoxText,
    pub cloud_show_recently_uploaded: CheckButton,
    pub cloud_ask_name_tags: CheckButton,
    pub adv_ask_name_after_capture: CheckButton,
    pub adv_retina_suffix: CheckButton,
    pub adv_clipboard_mode: ComboBoxText,
    pub adv_pinned_rounded_corners: CheckButton,
    pub adv_pinned_shadow: CheckButton,
    pub adv_pinned_border: CheckButton,
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
    screenshot_crosshair_mode_input.connect_changed(move |combo| {
        let id = combo.active_id().unwrap_or_default();
        m1.set_sensitive(id == "On");
    });
}

pub fn save_settings(inputs: &SaveInputs) -> anyhow::Result<()> {
    let previous_config = load_config().sanitized();
    let mut config = previous_config.clone();
    config.start_at_login = inputs.start_at_login.is_active();
    config.play_sounds = inputs.play_sounds.is_active();
    config.shutter_sound = combo_value(&inputs.shutter_sound, "Default");
    config.show_menu_bar_icon = inputs.show_menu_bar_icon.is_active();
    config.export_location = inputs.export_location.text().to_string();
    config.hide_desktop_icons_while_capturing = inputs.hide_desktop_icons.is_active();

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

    config.screenshot_crosshair_mode = combo_value(&inputs.screenshot_crosshair_mode, "On");
    config.screenshot_show_magnifier = inputs.screenshot_show_magnifier.is_active();
    config.screenshot_freeze_screen = inputs.screenshot_freeze_screen.is_active();
    config.screenshot_show_cursor = inputs.screenshot_capture_cursor.is_active();

    config.rec_controls = inputs.rec_controls.is_active();
    config.rec_display_time = inputs.rec_display_time.is_active();
    config.rec_hidpi = inputs.rec_hidpi.is_active();
    config.rec_notifications = inputs.rec_notifications.is_active();
    config.rec_cursor = inputs.rec_cursor.is_active();
    config.rec_clicks = inputs.rec_clicks.is_active();
    config.rec_keystrokes = inputs.rec_keystrokes.is_active();
    config.rec_key_filter = combo_value(&inputs.rec_key_filter, "0")
        .parse::<u8>()
        .unwrap_or(0);

    config.wallpaper_dont_change_on_space = inputs.wallpaper_dont_change_on_space.is_active();
    config.window_screenshot_padding = inputs.window_screenshot_padding.value();
    config.window_screenshot_shadow = inputs.window_screenshot_shadow.is_active();

    config.cloud_screenshot_quality =
        combo_value(&inputs.cloud_screenshot_quality, "Optimized for sharing");
    config.cloud_copy_to_clipboard =
        combo_value(&inputs.cloud_copy_to_clipboard, "CleanShot Cloud link");
    config.cloud_show_recently_uploaded = inputs.cloud_show_recently_uploaded.is_active();
    config.cloud_ask_name_tags = inputs.cloud_ask_name_tags.is_active();

    config.adv_ask_name_after_capture = inputs.adv_ask_name_after_capture.is_active();
    config.adv_retina_suffix = inputs.adv_retina_suffix.is_active();
    config.adv_clipboard_mode = combo_value(&inputs.adv_clipboard_mode, "File & Image (default)");
    config.adv_pinned_rounded_corners = inputs.adv_pinned_rounded_corners.is_active();
    config.adv_pinned_shadow = inputs.adv_pinned_shadow.is_active();
    config.adv_pinned_border = inputs.adv_pinned_border.is_active();
    config.adv_ocr_language = combo_value(&inputs.adv_ocr_language, "English");
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

    save_config(&config)?;

    if config.start_at_login {
        install_autostart_entry_for_current_exe()?;
    } else {
        uninstall_autostart_entry()?;
    }

    let tray_visible = config.show_menu_bar_icon;
    std::thread::spawn(move || {
        if quick_access_runtime_changed && stop_daemon_via_dbus() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            let _ = start_daemon_subprocess();
            return;
        }

        if set_daemon_tray_visibility(tray_visible) {
            return;
        }
        if tray_visible {
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
