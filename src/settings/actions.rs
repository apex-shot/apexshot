use crate::config::{save_config, AppConfig};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, ColorButton, ComboBoxText, Entry, Scale};

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
    pub quick_access_auto_close_enabled: CheckButton,
    pub quick_access_auto_close_action: ComboBoxText,
    pub quick_access_auto_close_interval: ComboBoxText,
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

pub fn save_settings(inputs: &SaveInputs, mut config: AppConfig) -> anyhow::Result<()> {
    config.start_at_login = inputs.start_at_login.is_active();
    config.play_sounds = inputs.play_sounds.is_active();
    config.shutter_sound = inputs
        .shutter_sound
        .active_id()
        .unwrap_or_else(|| "Default".into())
        .to_string();
    config.show_menu_bar_icon = inputs.show_menu_bar_icon.is_active();
    config.export_location = inputs.export_location.text().to_string();
    config.hide_desktop_icons_while_capturing = inputs.hide_desktop_icons.is_active();

    config.after_capture_show_quick_access = inputs.screenshot_quick_access.is_active();
    config.after_capture_copy_file_to_clipboard = inputs.screenshot_copy_to_clipboard.is_active();
    config.after_capture_save = inputs.screenshot_save.is_active();
    config.after_capture_open_annotate = inputs.screenshot_open_annotate.is_active();

    config.quick_access_auto_close_interval = if inputs.quick_access_auto_close_enabled.is_active()
    {
        inputs
            .quick_access_auto_close_interval
            .active_id()
            .unwrap_or_else(|| "5".into())
            .parse()
            .unwrap_or(5)
    } else {
        0
    };

    config.screenshot_crosshair_mode = inputs
        .screenshot_crosshair_mode
        .active_id()
        .unwrap_or_else(|| "On".into())
        .to_string();
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
    config.rec_key_filter = inputs
        .rec_key_filter
        .active_id()
        .unwrap_or_else(|| "0".into())
        .parse::<u8>()
        .unwrap_or(0);

    config.wallpaper_dont_change_on_space = inputs.wallpaper_dont_change_on_space.is_active();
    config.window_screenshot_padding = inputs.window_screenshot_padding.value();
    config.window_screenshot_shadow = inputs.window_screenshot_shadow.is_active();

    config.cloud_screenshot_quality = inputs
        .cloud_screenshot_quality
        .active_id()
        .unwrap_or_else(|| "Optimized for sharing".into())
        .to_string();
    config.cloud_copy_to_clipboard = inputs
        .cloud_copy_to_clipboard
        .active_id()
        .unwrap_or_else(|| "CleanShot Cloud link".into())
        .to_string();
    config.cloud_show_recently_uploaded = inputs.cloud_show_recently_uploaded.is_active();
    config.cloud_ask_name_tags = inputs.cloud_ask_name_tags.is_active();

    config.adv_ask_name_after_capture = inputs.adv_ask_name_after_capture.is_active();
    config.adv_retina_suffix = inputs.adv_retina_suffix.is_active();
    config.adv_clipboard_mode = inputs
        .adv_clipboard_mode
        .active_id()
        .unwrap_or_else(|| "File & Image (default)".into())
        .to_string();
    config.adv_pinned_rounded_corners = inputs.adv_pinned_rounded_corners.is_active();
    config.adv_pinned_shadow = inputs.adv_pinned_shadow.is_active();
    config.adv_pinned_border = inputs.adv_pinned_border.is_active();
    config.adv_ocr_language = inputs
        .adv_ocr_language
        .active_id()
        .unwrap_or_else(|| "English".into())
        .to_string();
    config.adv_ocr_keep_line_breaks = inputs.adv_ocr_keep_line_breaks.is_active();

    let config = config.sanitized();

    save_config(&config)?;

    Ok(())
}

#[allow(dead_code)]
pub fn close_window(window: &gtk4::ApplicationWindow) {
    window.close();
}
