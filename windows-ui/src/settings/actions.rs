use crate::config::{load_config, save_config, AppConfig};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, Entry, Scale};
use serde_json::json;
use std::path::{Path, PathBuf};

use super::select::SettingsSelect;
use super::theme_picker::ThemePicker;
use super::windowing::{install_autostart_entry_smart, uninstall_autostart_entry};

#[allow(dead_code)]
pub struct SaveInputs {
    pub start_at_login: CheckButton,
    pub play_sounds: CheckButton,
    pub shutter_sound: SettingsSelect,
    pub show_menu_bar_icon: CheckButton,
    pub screenshot_export_location: Entry,
    pub screenshot_format: SettingsSelect,
    pub video_export_location: Entry,
    pub rec_filename_pattern: Entry,
    pub rec_remember_export_folder: CheckButton,
    pub rec_controls: CheckButton,
    pub rec_hidpi: CheckButton,
    pub rec_notifications: CheckButton,
    pub rec_countdown: CheckButton,
    pub rec_video_max_res: SettingsSelect,
    pub rec_video_fps: SettingsSelect,
    pub rec_video_quality: SettingsSelect,
    pub rec_video_mono: CheckButton,
    pub screenshot_quick_access: CheckButton,
    pub screenshot_copy_to_clipboard: CheckButton,
    pub screenshot_save: CheckButton,
    pub screenshot_open_annotate: CheckButton,
    pub rec_copy_to_clipboard: CheckButton,
    pub rec_save: CheckButton,
    pub rec_open_video_editor: CheckButton,
    pub quick_access_position: SettingsSelect,
    pub quick_access_overlay_size: Scale,
    pub quick_access_auto_close_enabled: CheckButton,
    pub quick_access_auto_close_action: SettingsSelect,
    pub quick_access_auto_close_interval: SettingsSelect,
    pub quick_access_close_after_dragging: CheckButton,
    pub quick_access_close_after_uploading: CheckButton,
    pub screenshot_timer_interval: SettingsSelect,
    pub screenshot_capture_cursor: CheckButton,
    pub annotate_inverse_arrow: CheckButton,
    pub annotate_smooth_drawing: CheckButton,
    pub annotate_draw_shadow: CheckButton,
    pub annotate_auto_expand: CheckButton,
    pub annotate_show_color_names: CheckButton,
    pub annotate_always_on_top: CheckButton,
    pub annotate_close_after_export: CheckButton,
    pub annotate_overwrite_original_image: CheckButton,
    pub shortcut_open_file: Button,
    pub shortcut_open_from_clipboard: Button,
    pub shortcut_restore_recently_closed: Button,
    pub shortcut_toggle_overlays: Button,
    pub shortcut_capture_menu: Button,
    pub shortcut_capture_area: Button,
    pub shortcut_capture_crosshair: Button,
    pub shortcut_capture_previous_area: Button,
    pub shortcut_capture_fullscreen: Button,
    pub shortcut_capture_window: Button,
    pub shortcut_show_last_preview: Button,
    pub shortcut_save_last_capture: Button,
    pub shortcut_record_screen: Button,
    pub shortcut_recording_stop_save: Button,
    pub adv_retina_suffix: CheckButton,
    pub adv_clipboard_mode: SettingsSelect,
    pub adv_ocr_language: SettingsSelect,
    pub adv_ocr_keep_line_breaks: CheckButton,
    pub telemetry_enabled: CheckButton,
    pub cloud_apexshot: CheckButton,
    pub cloud_xbackbone: CheckButton,
    pub cloud_auto_upload: CheckButton,
    pub xbackbone_url: Entry,
    pub xbackbone_api_token: Entry,
    pub ui_theme: ThemePicker,
    pub ui_language: SettingsSelect,
}

pub fn install_checkbox_behaviors(
    play_sounds_check: &CheckButton,
    shutter_sound_input: &SettingsSelect,
    screenshot_quick_access_check: &CheckButton,
    screenshot_copy_to_clipboard_check: &CheckButton,
    screenshot_save_check: &CheckButton,
    screenshot_open_annotate_check: &CheckButton,
    quick_access_auto_close_enabled_check: &CheckButton,
    quick_access_auto_close_action_input: &SettingsSelect,
    quick_access_auto_close_interval_input: &SettingsSelect,
) {
    let shutter_sound_supported = crate::capture_overlay::supports_custom_shutter_sound();
    let shutter_sound_input_toggle = shutter_sound_input.clone();
    play_sounds_check.connect_toggled(move |check| {
        shutter_sound_input_toggle.set_sensitive(check.is_active() && shutter_sound_supported);
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

    let _ = (screenshot_copy_to_clipboard_check, screenshot_save_check);

    let a1 = quick_access_auto_close_action_input.clone();
    let a2 = quick_access_auto_close_interval_input.clone();
    a1.set_sensitive(quick_access_auto_close_enabled_check.is_active());
    a2.set_sensitive(quick_access_auto_close_enabled_check.is_active());
    quick_access_auto_close_enabled_check.connect_toggled(move |check| {
        let active = check.is_active();
        a1.set_sensitive(active);
        a2.set_sensitive(active);
    });
}

pub struct SaveOutcome {
    pub language_changed: bool,
    pub theme_changed: bool,
}

fn shortcut_label_value(label: Option<gtk4::glib::GString>) -> String {
    let label = label.unwrap_or_default();
    if crate::i18n::is_empty_shortcut_label(&label) {
        String::new()
    } else {
        label.to_string()
    }
}

fn button_label_value(button: &Button) -> String {
    shortcut_label_value(button.label())
}

pub fn save_settings(inputs: &SaveInputs) -> anyhow::Result<SaveOutcome> {
    let previous_config = load_config().sanitized();
    let mut config = previous_config.clone();

    config.start_at_login = inputs.start_at_login.is_active();
    config.ui_theme = inputs
        .ui_theme
        .active_id()
        .unwrap_or_else(|| crate::config::DEFAULT_UI_THEME.to_string());
    config.ui_language = crate::i18n::sanitize_ui_language(
        &inputs
            .ui_language
            .active_id()
            .unwrap_or_else(|| crate::i18n::SYSTEM_LANGUAGE.to_string()),
    );
    config.play_sounds = inputs.play_sounds.is_active();
    config.shutter_sound = combo_value(&inputs.shutter_sound, crate::config::DEFAULT_SHUTTER_SOUND);
    config.show_menu_bar_icon = inputs.show_menu_bar_icon.is_active();

    config.export_location.clear();
    config.screenshot_export_location = inputs.screenshot_export_location.text().to_string();
    config.video_export_location = inputs.video_export_location.text().to_string();
    config.rec_filename_pattern = inputs.rec_filename_pattern.text().to_string();
    config.rec_remember_export_folder = inputs.rec_remember_export_folder.is_active();
    config.rec_controls = inputs.rec_controls.is_active();
    config.rec_hidpi = inputs.rec_hidpi.is_active();
    config.rec_notifications = inputs.rec_notifications.is_active();
    config.rec_countdown = inputs.rec_countdown.is_active();

    config.rec_video_max_res = inputs
        .rec_video_max_res
        .active_id()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    config.rec_video_fps = inputs
        .rec_video_fps
        .active_id()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    config.rec_video_quality = inputs
        .rec_video_quality
        .active_id()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    config.rec_video_mono = inputs.rec_video_mono.is_active();

    config.after_capture_show_quick_access = inputs.screenshot_quick_access.is_active();
    config.after_capture_copy_file_to_clipboard = inputs.screenshot_copy_to_clipboard.is_active();
    config.after_capture_save = inputs.screenshot_save.is_active();
    config.after_capture_open_annotate = inputs.screenshot_open_annotate.is_active();
    config.rec_after_capture_copy_to_clipboard = inputs.rec_copy_to_clipboard.is_active();
    config.rec_after_capture_save = inputs.rec_save.is_active();
    config.rec_video_open_editor = inputs.rec_open_video_editor.is_active();

    config.quick_access_position = combo_value(&inputs.quick_access_position, "Left");
    config.quick_access_overlay_size = inputs.quick_access_overlay_size.value();
    config.quick_access_auto_close_enabled = inputs.quick_access_auto_close_enabled.is_active();
    config.quick_access_auto_close_action =
        combo_value(&inputs.quick_access_auto_close_action, "Close");
    config.quick_access_auto_close_interval = inputs
        .quick_access_auto_close_interval
        .active_id()
        .unwrap_or_else(|| "30".to_string())
        .parse()
        .unwrap_or(30);
    config.quick_access_close_after_dragging = inputs.quick_access_close_after_dragging.is_active();
    config.quick_access_close_after_uploading =
        inputs.quick_access_close_after_uploading.is_active();

    config.screenshot_format = combo_value(&inputs.screenshot_format, "PNG");
    config.screenshot_timer_interval = inputs
        .screenshot_timer_interval
        .active_id()
        .unwrap_or_else(|| "5".to_string())
        .parse()
        .unwrap_or(5);
    config.screenshot_show_cursor = inputs.screenshot_capture_cursor.is_active();

    config.annotate_inverse_arrow = inputs.annotate_inverse_arrow.is_active();
    config.annotate_smooth_drawing = inputs.annotate_smooth_drawing.is_active();
    config.annotate_draw_shadow = inputs.annotate_draw_shadow.is_active();
    config.annotate_auto_expand = inputs.annotate_auto_expand.is_active();
    config.annotate_show_color_names = inputs.annotate_show_color_names.is_active();
    config.annotate_always_on_top = inputs.annotate_always_on_top.is_active();
    config.annotate_close_after_export = inputs.annotate_close_after_export.is_active();
    config.annotate_overwrite_original_image = inputs.annotate_overwrite_original_image.is_active();

    config.shortcut_open_file = button_label_value(&inputs.shortcut_open_file);
    config.shortcut_open_from_clipboard = button_label_value(&inputs.shortcut_open_from_clipboard);
    config.shortcut_restore_recently_closed =
        button_label_value(&inputs.shortcut_restore_recently_closed);
    config.shortcut_toggle_overlays = button_label_value(&inputs.shortcut_toggle_overlays);
    config.shortcut_capture_menu = button_label_value(&inputs.shortcut_capture_menu);
    config.shortcut_capture_area = button_label_value(&inputs.shortcut_capture_area);
    config.shortcut_capture_crosshair = button_label_value(&inputs.shortcut_capture_crosshair);
    config.shortcut_capture_previous_area =
        button_label_value(&inputs.shortcut_capture_previous_area);
    config.shortcut_capture_fullscreen = button_label_value(&inputs.shortcut_capture_fullscreen);
    config.shortcut_capture_window.clear();
    config.shortcut_show_last_preview = button_label_value(&inputs.shortcut_show_last_preview);
    config.shortcut_save_last_capture = button_label_value(&inputs.shortcut_save_last_capture);
    config.shortcut_open_recording_ui.clear();
    config.shortcut_record_screen = button_label_value(&inputs.shortcut_record_screen);
    config.shortcut_recording_stop_save = button_label_value(&inputs.shortcut_recording_stop_save);

    config.adv_retina_suffix = inputs.adv_retina_suffix.is_active();
    config.adv_clipboard_mode = combo_value(&inputs.adv_clipboard_mode, "File & Image (default)");
    config.adv_ocr_language = combo_value(&inputs.adv_ocr_language, "eng");
    config.adv_ocr_keep_line_breaks = inputs.adv_ocr_keep_line_breaks.is_active();
    config.telemetry_enabled = inputs.telemetry_enabled.is_active();

    config.cloud_destination = if inputs.cloud_xbackbone.is_active() {
        "xbackbone".to_string()
    } else {
        "apexshot".to_string()
    };
    config.cloud_auto_upload_after_capture = inputs.cloud_auto_upload.is_active();
    config.xbackbone_url = inputs.xbackbone_url.text().to_string();
    config.xbackbone_api_token = inputs.xbackbone_api_token.text().to_string();

    let config = config.sanitized();
    let language_changed = previous_config.ui_language != config.ui_language;
    let theme_changed = previous_config.ui_theme != config.ui_theme;
    if language_changed {
        crate::i18n::init(&config.ui_language);
    }

    save_config(&config)?;
    write_shell_settings(&config)?;

    if config.start_at_login != previous_config.start_at_login {
        if config.start_at_login {
            install_autostart_entry_smart()?;
        } else {
            uninstall_autostart_entry()?;
        }
    }

    Ok(SaveOutcome {
        language_changed,
        theme_changed,
    })
}

/// Path of the JSON mirror the C++ capture shell reads.
pub fn shell_settings_path() -> Option<PathBuf> {
    crate::config::config_path()
        .and_then(|path| path.parent().map(|parent| parent.join("settings.json")))
}

/// Write the subset of settings the Qt shell applies at runtime.
///
/// The Linux config stays YAML because the rest of the Rust app reads it. The
/// Windows shell is C++, so it gets a small JSON view instead of a YAML parser.
fn write_shell_settings(config: &AppConfig) -> anyhow::Result<()> {
    let Some(path) = shell_settings_path() else {
        return Ok(());
    };
    write_shell_settings_to(&path, config)
}

fn write_shell_settings_to(path: &Path, config: &AppConfig) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("could not create {}: {error}", parent.display()))?;
    }

    let payload = json!({
        "general": {
            "start_at_login": config.start_at_login,
            "show_tray_icon": config.show_menu_bar_icon,
            "theme": config.ui_theme,
            "language": config.ui_language,
            "play_sounds": config.play_sounds,
            "shutter_sound": config.shutter_sound,
        },
        "screenshots": {
            "export_location": config.screenshot_export_location,
            "format": config.screenshot_format,
            "clipboard_mode": config.adv_clipboard_mode,
            "timer_interval": config.screenshot_timer_interval,
            "show_cursor": config.screenshot_show_cursor,
            "after_show_result": config.after_capture_show_quick_access,
            "after_copy_to_clipboard": config.after_capture_copy_file_to_clipboard,
            "after_save": config.after_capture_save,
            "high_dpi_suffix": config.adv_retina_suffix,
        },
        "recording": {
            "show_countdown": config.rec_countdown,
            "export_location": config.video_export_location,
            "filename_pattern": config.rec_filename_pattern,
            "fps_index": config.rec_video_fps,
            "max_resolution_index": config.rec_video_max_res,
            "quality_index": config.rec_video_quality,
            "mono_audio": config.rec_video_mono,
            "include_microphone": config.rec_mic,
            "include_speaker": config.rec_speaker,
            "show_cursor": config.rec_cursor,
            "after_show_result": true,
        },
        "quick_access": {
            "position": config.quick_access_position,
            "overlay_scale": config.quick_access_overlay_size,
            "auto_close_enabled": config.quick_access_auto_close_enabled,
            "auto_close_seconds": config.quick_access_auto_close_interval,
        },
        "ocr": {
            "language": config.adv_ocr_language,
            "keep_line_breaks": config.adv_ocr_keep_line_breaks,
        },
    });

    let bytes = serde_json::to_vec_pretty(&payload)?;
    std::fs::write(&path, bytes)
        .map_err(|error| anyhow::anyhow!("could not write {}: {error}", path.display()))?;
    Ok(())
}

fn combo_value(combo: &SettingsSelect, fallback: &str) -> String {
    combo.active_id().unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_mirror_carries_the_runtime_fields() {
        let root = std::env::temp_dir().join(format!(
            "apexshot-settings-mirror-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("APPDATA", &root);

        let config = AppConfig {
            screenshot_export_location: "C:\\shots".into(),
            screenshot_format: "WebP".into(),
            adv_clipboard_mode: "Image Only".into(),
            screenshot_show_cursor: false,
            after_capture_save: false,
            after_capture_copy_file_to_clipboard: true,
            video_export_location: "C:\\videos".into(),
            rec_filename_pattern: "Clip {Date} {Time}".into(),
            rec_video_fps: 3,
            rec_video_max_res: 2,
            rec_video_quality: 0,
            rec_video_mono: true,
            rec_countdown: false,
            rec_mic: true,
            rec_speaker: true,
            rec_cursor: false,
            play_sounds: false,
            shutter_sound: "Pop".into(),
            quick_access_position: "Right".into(),
            quick_access_overlay_size: 1.2,
            quick_access_auto_close_enabled: true,
            quick_access_auto_close_interval: 45,
            adv_ocr_language: "deu".into(),
            adv_ocr_keep_line_breaks: false,
            ..Default::default()
        };

        write_shell_settings_to(&root.join("settings.json"), &config).unwrap();

        let path = root.join("settings.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        assert_eq!(value["screenshots"]["export_location"], "C:\\shots");
        assert_eq!(value["screenshots"]["format"], "WebP");
        assert_eq!(value["screenshots"]["clipboard_mode"], "Image Only");
        assert_eq!(value["screenshots"]["show_cursor"], false);
        assert_eq!(value["screenshots"]["after_save"], false);
        assert_eq!(value["screenshots"]["after_copy_to_clipboard"], true);
        assert_eq!(value["recording"]["show_countdown"], false);
        assert_eq!(value["recording"]["fps_index"], 3);
        assert_eq!(value["recording"]["max_resolution_index"], 2);
        assert_eq!(value["recording"]["quality_index"], 0);
        assert_eq!(value["recording"]["mono_audio"], true);
        assert_eq!(value["recording"]["include_microphone"], true);
        assert_eq!(value["recording"]["include_speaker"], true);
        assert_eq!(value["recording"]["show_cursor"], false);
        assert_eq!(value["quick_access"]["position"], "Right");
        assert_eq!(value["quick_access"]["overlay_scale"], 1.2);
        assert_eq!(value["quick_access"]["auto_close_enabled"], true);
        assert_eq!(value["quick_access"]["auto_close_seconds"], 45);
        assert_eq!(value["ocr"]["language"], "deu");
        assert_eq!(value["ocr"]["keep_line_breaks"], false);
        assert_eq!(value["general"]["play_sounds"], false);
        assert_eq!(value["general"]["shutter_sound"], "Pop");

        std::fs::remove_dir_all(&root).ok();
    }
}
