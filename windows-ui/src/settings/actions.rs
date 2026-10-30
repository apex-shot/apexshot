use crate::config::{load_config, save_config, AppConfig};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, Entry, Scale};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::select::SettingsSelect;
use super::theme_picker::ThemePicker;
use super::windowing::{
    autostart_entry_exists, install_autostart_entry_smart, uninstall_autostart_entry,
};

static MIRROR_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    pub webcam: super::webcam::WebcamInputs,
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

    let screenshot_checks = [
        screenshot_quick_access_check,
        screenshot_copy_to_clipboard_check,
        screenshot_save_check,
        screenshot_open_annotate_check,
    ];
    for check in screenshot_checks {
        let checks = screenshot_checks.map(|check| check.downgrade());
        check.connect_toggled(move |changed| {
            if !changed.is_active()
                && checks
                    .iter()
                    .filter_map(|check| check.upgrade())
                    .all(|check| !check.is_active())
            {
                changed.set_active(true);
            }
        });
    }

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
    if inputs.rec_notifications.is_sensitive() {
        config.rec_notifications = inputs.rec_notifications.is_active();
    }
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

    config.rec_webcam_enabled = inputs.webcam.enabled.is_active();
    config.rec_webcam_device = inputs.webcam.device.borrow().clone();
    config.rec_webcam_size = inputs
        .webcam
        .size
        .active_id()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    config.rec_webcam_shape = inputs
        .webcam
        .shape
        .active_id()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    config.rec_webcam_flip = inputs.webcam.flip.is_active();
    let (rel_x, rel_y) = inputs.webcam.position();
    config.rec_webcam_rel_x = rel_x;
    config.rec_webcam_rel_y = rel_y;

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
        .unwrap_or_else(|| "0".to_string())
        .parse()
        .unwrap_or(0);
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
    config.windows_telemetry_enabled = inputs.telemetry_enabled.is_active();

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

    let startup_before = if inputs.start_at_login.is_sensitive() {
        Some(autostart_entry_exists()?)
    } else {
        None
    };
    let startup_changed = startup_before.is_some_and(|enabled| enabled != config.start_at_login);
    if startup_changed {
        if config.start_at_login {
            install_autostart_entry_smart()?;
        } else {
            uninstall_autostart_entry()?;
        }
    }

    let save_result = (|| {
        if let Some(path) = shell_settings_path() {
            if let Some(directory) = path.parent() {
                sync_webcam_overrides(
                    directory,
                    &previous_config,
                    &config,
                    inputs.webcam.pending_edits(),
                )?;
            }
        }
        save_config(&config)
    })();
    if let Err(error) = save_result {
        if startup_changed {
            let rollback = if startup_before == Some(true) {
                install_autostart_entry_smart().map(|_| ())
            } else {
                uninstall_autostart_entry()
            };
            if let Err(rollback_error) = rollback {
                eprintln!("[settings] Could not restore login startup: {rollback_error:#}");
            }
        }
        return Err(error);
    }
    write_shell_settings(&config)?;
    inputs.webcam.clear_pending_edits();

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
pub(crate) fn write_shell_settings(config: &AppConfig) -> anyhow::Result<()> {
    let Some(path) = shell_settings_path() else {
        return Ok(());
    };
    write_shell_settings_to(&path, config)
}

/// Restore the shell's mirror on launch without replacing it with defaults
/// when a user's YAML exists but cannot be read or parsed.
pub(crate) fn sync_shell_settings_on_start() -> anyhow::Result<()> {
    let path = crate::config::config_path()
        .ok_or_else(|| anyhow::anyhow!("could not resolve the config directory"))?;
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            serde_yml::from_str::<AppConfig>(&raw)
                .map_err(|error| anyhow::anyhow!("could not parse {}: {error}", path.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    write_shell_settings(&load_config().sanitized())
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
        "shortcuts": {
            "quick_capture": config.shortcut_capture_menu,
            "capture_area": config.shortcut_capture_area,
            "capture_crosshair": config.shortcut_capture_crosshair,
            "capture_previous_area": config.shortcut_capture_previous_area,
            "capture_fullscreen": config.shortcut_capture_fullscreen,
            "show_last_preview": config.shortcut_show_last_preview,
            "save_last_capture": config.shortcut_save_last_capture,
            "record_screen": config.shortcut_record_screen,
            "recording_stop_save": config.shortcut_recording_stop_save,
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
            "show_controls": config.rec_controls,
            "high_dpi": config.rec_hidpi,
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
            "after_copy_to_clipboard": config.rec_after_capture_copy_to_clipboard,
            "after_save": config.rec_after_capture_save,
            "after_open_editor": config.rec_video_open_editor,
            "webcam": {
                "enabled": config.rec_webcam_enabled,
                "device": config.rec_webcam_device,
                "size_index": config.rec_webcam_size,
                "shape_index": config.rec_webcam_shape,
                "flip": config.rec_webcam_flip,
                "rel_x": config.rec_webcam_rel_x,
                "rel_y": config.rec_webcam_rel_y,
            },
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
        "telemetry": {
            "enabled": config.windows_telemetry_enabled,
        },
    });

    write_json_atomically(path, &payload)
}

fn write_json_atomically(path: &Path, payload: &serde_json::Value) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(payload)?;
    let sequence = MIRROR_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(".settings-{}-{sequence}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|error| anyhow::anyhow!("could not write {}: {error}", path.display()))?;
    Ok(())
}

/// Explicit settings edits take precedence over remembered pre-record choices,
/// while unrelated edits leave the shell's device and bubble state untouched.
fn sync_webcam_overrides(
    directory: &Path,
    previous: &AppConfig,
    current: &AppConfig,
    edits: u8,
) -> anyhow::Result<()> {
    use super::webcam::{
        EDIT_DEVICE, EDIT_ENABLED, EDIT_FLIP, EDIT_POSITION, EDIT_SHAPE, EDIT_SIZE,
    };
    let enabled =
        edits & EDIT_ENABLED != 0 || previous.rec_webcam_enabled != current.rec_webcam_enabled;
    let device =
        edits & EDIT_DEVICE != 0 || previous.rec_webcam_device != current.rec_webcam_device;
    let size = edits & EDIT_SIZE != 0 || previous.rec_webcam_size != current.rec_webcam_size;
    let shape = edits & EDIT_SHAPE != 0 || previous.rec_webcam_shape != current.rec_webcam_shape;
    let flip = edits & EDIT_FLIP != 0 || previous.rec_webcam_flip != current.rec_webcam_flip;
    let position = edits & EDIT_POSITION != 0
        || previous.rec_webcam_rel_x != current.rec_webcam_rel_x
        || previous.rec_webcam_rel_y != current.rec_webcam_rel_y;

    for (filename, changed) in [
        ("recording-devices.json", enabled || device),
        ("webcam-placement.json", size || shape || flip || position),
    ] {
        if !changed {
            continue;
        }
        let path = directory.join(filename);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            // The shell ignores invalid remembered state and uses Settings.
            continue;
        };
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        if filename == "recording-devices.json" {
            if enabled {
                object.insert("camera_enabled".into(), json!(current.rec_webcam_enabled));
            }
            if device {
                object.insert("camera_device".into(), json!(current.rec_webcam_device));
            }
        } else {
            if size {
                object.remove("size_fraction");
            }
            if shape {
                object.insert("shape".into(), json!(current.rec_webcam_shape));
            }
            if flip {
                object.insert("flip".into(), json!(current.rec_webcam_flip));
            }
            if position {
                object.insert("rel_x".into(), json!(current.rec_webcam_rel_x));
                object.insert("rel_y".into(), json!(current.rec_webcam_rel_y));
            }
        }
        write_json_atomically(&path, &value)?;
    }
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
        let config = AppConfig {
            screenshot_export_location: "C:\\shots".into(),
            screenshot_format: "WebP".into(),
            adv_clipboard_mode: "Image Only".into(),
            adv_retina_suffix: false,
            screenshot_timer_interval: 3,
            screenshot_show_cursor: false,
            after_capture_save: false,
            after_capture_copy_file_to_clipboard: true,
            video_export_location: "C:\\videos".into(),
            rec_filename_pattern: "Clip {Date} {Time}".into(),
            rec_video_fps: 3,
            rec_video_max_res: 6,
            rec_video_quality: 0,
            rec_video_mono: true,
            rec_countdown: false,
            rec_controls: false,
            rec_hidpi: false,
            rec_mic: true,
            rec_speaker: true,
            rec_cursor: false,
            rec_after_capture_copy_to_clipboard: true,
            rec_after_capture_save: false,
            rec_video_open_editor: true,
            rec_webcam_enabled: true,
            rec_webcam_device: "Integrated Camera".into(),
            rec_webcam_size: 2,
            rec_webcam_shape: 3,
            rec_webcam_flip: true,
            rec_webcam_rel_x: 0.0,
            rec_webcam_rel_y: 1.0,
            play_sounds: false,
            shutter_sound: "Pop".into(),
            start_at_login: true,
            show_menu_bar_icon: false,
            ui_theme: "dark".into(),
            ui_language: "fr".into(),
            shortcut_capture_menu: "Ctrl+Alt+M".into(),
            shortcut_capture_area: String::new(),
            shortcut_capture_crosshair: "Ctrl+Alt+X".into(),
            shortcut_capture_previous_area: "Ctrl+Alt+P".into(),
            shortcut_capture_fullscreen: "Ctrl+Alt+F".into(),
            shortcut_show_last_preview: "Ctrl+Alt+V".into(),
            shortcut_save_last_capture: "Ctrl+Alt+S".into(),
            shortcut_record_screen: "Ctrl+Alt+R".into(),
            shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".into(),
            quick_access_position: "Right".into(),
            quick_access_overlay_size: 1.2,
            quick_access_auto_close_enabled: true,
            quick_access_auto_close_interval: 45,
            adv_ocr_language: "deu".into(),
            adv_ocr_keep_line_breaks: false,
            windows_telemetry_enabled: true,
            ..Default::default()
        };

        write_shell_settings_to(&root.join("settings.json"), &config).unwrap();

        let path = root.join("settings.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        assert_eq!(value["screenshots"]["export_location"], "C:\\shots");
        assert_eq!(value["screenshots"]["format"], "WebP");
        assert_eq!(value["screenshots"]["clipboard_mode"], "Image Only");
        assert_eq!(value["screenshots"]["high_dpi_suffix"], false);
        assert_eq!(value["screenshots"]["timer_interval"], 3);
        assert_eq!(value["screenshots"]["show_cursor"], false);
        assert_eq!(value["screenshots"]["after_save"], false);
        assert_eq!(value["screenshots"]["after_copy_to_clipboard"], true);
        assert_eq!(value["recording"]["show_countdown"], false);
        assert_eq!(value["recording"]["show_controls"], false);
        assert_eq!(value["recording"]["high_dpi"], false);
        assert_eq!(value["recording"]["fps_index"], 3);
        assert_eq!(value["recording"]["max_resolution_index"], 6);
        assert_eq!(value["recording"]["quality_index"], 0);
        assert_eq!(value["recording"]["mono_audio"], true);
        assert_eq!(value["recording"]["include_microphone"], true);
        assert_eq!(value["recording"]["include_speaker"], true);
        assert_eq!(value["recording"]["show_cursor"], false);
        assert_eq!(value["recording"]["after_copy_to_clipboard"], true);
        assert_eq!(value["recording"]["after_save"], false);
        assert_eq!(value["recording"]["after_open_editor"], true);
        assert_eq!(value["recording"]["webcam"]["enabled"], true);
        assert_eq!(value["recording"]["webcam"]["device"], "Integrated Camera");
        assert_eq!(value["recording"]["webcam"]["size_index"], 2);
        assert_eq!(value["recording"]["webcam"]["shape_index"], 3);
        assert_eq!(value["recording"]["webcam"]["flip"], true);
        assert_eq!(value["recording"]["webcam"]["rel_x"], 0.0);
        assert_eq!(value["recording"]["webcam"]["rel_y"], 1.0);
        assert_eq!(value["quick_access"]["position"], "Right");
        assert_eq!(value["quick_access"]["overlay_scale"], 1.2);
        assert_eq!(value["quick_access"]["auto_close_enabled"], true);
        assert_eq!(value["quick_access"]["auto_close_seconds"], 45);
        assert_eq!(value["ocr"]["language"], "deu");
        assert_eq!(value["ocr"]["keep_line_breaks"], false);
        assert_eq!(value["telemetry"]["enabled"], true);
        assert_eq!(value["general"]["play_sounds"], false);
        assert_eq!(value["general"]["shutter_sound"], "Pop");
        assert_eq!(value["general"]["start_at_login"], true);
        assert_eq!(value["general"]["show_tray_icon"], false);
        assert_eq!(value["general"]["theme"], "dark");
        assert_eq!(value["general"]["language"], "fr");
        assert_eq!(value["shortcuts"]["quick_capture"], "Ctrl+Alt+M");
        assert_eq!(value["shortcuts"]["capture_area"], "");
        assert_eq!(value["shortcuts"]["capture_crosshair"], "Ctrl+Alt+X");
        assert_eq!(value["shortcuts"]["capture_previous_area"], "Ctrl+Alt+P");
        assert_eq!(value["shortcuts"]["capture_fullscreen"], "Ctrl+Alt+F");
        assert_eq!(value["shortcuts"]["show_last_preview"], "Ctrl+Alt+V");
        assert_eq!(value["shortcuts"]["save_last_capture"], "Ctrl+Alt+S");
        assert_eq!(value["shortcuts"]["record_screen"], "Ctrl+Alt+R");
        assert_eq!(
            value["shortcuts"]["recording_stop_save"],
            "Ctrl+Alt+Shift+S"
        );

        write_shell_settings_to(&path, &AppConfig::default()).unwrap();
        let updated: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(updated["recording"]["after_save"], true);
        assert_eq!(updated["general"]["shutter_sound"], "Camera");
        assert_eq!(updated["screenshots"]["high_dpi_suffix"], true);
        assert_eq!(updated["telemetry"]["enabled"], false);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn screenshot_timer_values_are_sanitized() {
        for seconds in [0, 1, 3, 5, 10] {
            let config = AppConfig {
                screenshot_timer_interval: seconds,
                ..Default::default()
            }
            .sanitized();
            assert_eq!(config.screenshot_timer_interval, seconds);
        }
        let config = AppConfig {
            screenshot_timer_interval: 7,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(config.screenshot_timer_interval, 0);
    }

    #[test]
    fn windows_telemetry_requires_an_explicit_opt_in() {
        let existing: AppConfig = serde_yml::from_str("telemetry_enabled: true\n").unwrap();
        assert!(!existing.windows_telemetry_enabled);
        let opted_in: AppConfig = serde_yml::from_str("windows_telemetry_enabled: true\n").unwrap();
        assert!(opted_in.windows_telemetry_enabled);
    }

    #[test]
    fn webcam_settings_replace_only_the_edited_remembered_choices() {
        let root = std::env::temp_dir().join(format!(
            "apexshot-webcam-overrides-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let devices = root.join("recording-devices.json");
        let placement = root.join("webcam-placement.json");
        std::fs::write(
            &devices,
            r#"{"microphone_enabled":true,"camera_enabled":false,"camera_device":"Old"}"#,
        )
        .unwrap();
        std::fs::write(
            &placement,
            r#"{"rel_x":0.3,"rel_y":0.4,"size_fraction":0.2,"shape":1,"flip":false}"#,
        )
        .unwrap();

        let previous = AppConfig::default();
        let current = AppConfig {
            rec_webcam_enabled: true,
            rec_webcam_device: "New".into(),
            rec_webcam_size: 3,
            rec_webcam_shape: 2,
            rec_webcam_rel_x: 0.0,
            rec_webcam_rel_y: 1.0,
            ..previous.clone()
        };
        sync_webcam_overrides(&root, &previous, &current, 0).unwrap();

        let device_state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&devices).unwrap()).unwrap();
        let mut placement_state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&placement).unwrap()).unwrap();
        assert_eq!(device_state["microphone_enabled"], true);
        assert_eq!(device_state["camera_enabled"], true);
        assert_eq!(device_state["camera_device"], "New");
        assert_eq!(placement_state["rel_x"], 0.0);
        assert_eq!(placement_state["rel_y"], 1.0);
        assert!(placement_state.get("size_fraction").is_none());
        assert_eq!(placement_state["shape"], 2);
        assert_eq!(placement_state["flip"], false);
        placement_state["flip"] = json!(true);
        std::fs::write(&placement, serde_json::to_vec(&placement_state).unwrap()).unwrap();
        sync_webcam_overrides(&root, &current, &current, super::super::webcam::EDIT_FLIP).unwrap();
        let placement_state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&placement).unwrap()).unwrap();
        assert_eq!(placement_state["flip"], false);
        std::fs::write(&devices, b"{invalid").unwrap();
        sync_webcam_overrides(
            &root,
            &current,
            &current,
            super::super::webcam::EDIT_ENABLED,
        )
        .unwrap();
        assert_eq!(std::fs::read(&devices).unwrap(), b"{invalid");
        std::fs::remove_dir_all(&root).ok();
    }
}
