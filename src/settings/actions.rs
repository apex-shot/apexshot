use crate::{
    config::{load_config, save_config},
    daemon::{set_daemon_tray_visibility, start_daemon_subprocess, stop_daemon_via_dbus},
};
use gtk4::prelude::*;
use gtk4::{Button, CheckButton, Entry, Scale};

use super::cloud::AutoUploadSync;
use super::select::SettingsSelect;
use super::theme_picker::ThemePicker;
use super::windowing::{install_autostart_entry_smart, uninstall_autostart_entry};

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
    pub rec_auto_zoom_on_import: CheckButton,
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
    /// Checkbox controlling whether image and Motion exports close the editor.
    pub annotate_close_after_export: CheckButton,
    /// Checkbox controlling whether still-image exports replace their source.
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
    pub cloud_auto_upload_sync: std::rc::Rc<AutoUploadSync>,
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

fn screenshot_or_general_shortcuts_configured(config: &crate::config::AppConfig) -> bool {
    !crate::hotkeys::hotkey_config_from_app_config(config)
        .bindings
        .is_empty()
}

pub struct SaveOutcome {
    pub language_changed: bool,
    pub theme_changed: bool,
}

pub(crate) struct SaveEffects {
    pub language_changed: bool,
    pub theme_changed: bool,
    pub quick_access_runtime_changed: bool,
    pub shortcuts_runtime_changed: bool,
    pub sync_hotkeys: bool,
    pub sync_autostart: bool,
    pub daemon_affected: bool,
}

fn shortcuts_differ(a: &crate::config::AppConfig, b: &crate::config::AppConfig) -> bool {
    a.shortcut_open_file != b.shortcut_open_file
        || a.shortcut_open_from_clipboard != b.shortcut_open_from_clipboard
        || a.shortcut_restore_recently_closed != b.shortcut_restore_recently_closed
        || a.shortcut_toggle_overlays != b.shortcut_toggle_overlays
        || a.shortcut_capture_menu != b.shortcut_capture_menu
        || a.shortcut_capture_area != b.shortcut_capture_area
        || a.shortcut_capture_crosshair != b.shortcut_capture_crosshair
        || a.shortcut_capture_previous_area != b.shortcut_capture_previous_area
        || a.shortcut_capture_fullscreen != b.shortcut_capture_fullscreen
        || a.shortcut_show_last_preview != b.shortcut_show_last_preview
        || a.shortcut_save_last_capture != b.shortcut_save_last_capture
        || a.shortcut_open_recording_ui != b.shortcut_open_recording_ui
        || a.shortcut_record_screen != b.shortcut_record_screen
        || a.shortcut_recording_stop_save != b.shortcut_recording_stop_save
}

fn daemon_needed(config: &crate::config::AppConfig) -> bool {
    config.show_menu_bar_icon || screenshot_or_general_shortcuts_configured(config)
}

#[derive(Debug, PartialEq, Eq)]
enum DaemonAction {
    Stop,
    VisibilityOnly,
    Restart,
    VisibilityThenStart,
}

fn daemon_action(allow_respawn: bool, needed: bool, restart: bool) -> DaemonAction {
    if !needed {
        return DaemonAction::Stop;
    }
    if !allow_respawn {
        return DaemonAction::VisibilityOnly;
    }
    if restart {
        return DaemonAction::Restart;
    }
    DaemonAction::VisibilityThenStart
}

static DAEMON_FOLLOW_UPS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct SaveState {
    pub applied: crate::config::AppConfig,
    pending_hotkeys: bool,
    pending_autostart: bool,
    follow_ups: Vec<std::thread::JoinHandle<()>>,
}

impl SaveState {
    pub(crate) fn new(config: crate::config::AppConfig) -> Self {
        Self {
            applied: config,
            pending_hotkeys: false,
            pending_autostart: false,
            follow_ups: Vec::new(),
        }
    }

    pub(crate) fn finish_runtime_updates(&mut self) {
        for handle in std::mem::take(&mut self.follow_ups) {
            if let Err(error) = handle.join() {
                eprintln!("[settings] daemon follow-up panicked: {error:?}");
            }
        }
    }
}

pub(crate) fn persist_and_apply(
    state: &mut SaveState,
    config: &crate::config::AppConfig,
    persist: impl FnOnce(&crate::config::AppConfig) -> anyhow::Result<()>,
    sync: impl FnOnce(&SaveEffects, &crate::config::AppConfig) -> anyhow::Result<()>,
) -> anyhow::Result<SaveEffects> {
    let mut effects = save_effects(&state.applied, config);
    effects.sync_hotkeys |= state.pending_hotkeys;
    effects.sync_autostart |= state.pending_autostart;
    persist(config)?;
    state.pending_hotkeys = effects.sync_hotkeys;
    state.pending_autostart = effects.sync_autostart;
    sync(&effects, config)?;
    state.applied = config.clone();
    state.pending_hotkeys = false;
    state.pending_autostart = false;
    Ok(effects)
}

pub(crate) fn save_effects(
    previous: &crate::config::AppConfig,
    config: &crate::config::AppConfig,
) -> SaveEffects {
    let language_changed = previous.ui_language != config.ui_language;
    let theme_changed = previous.ui_theme != config.ui_theme;
    let quick_access_runtime_changed = previous.quick_access_position
        != config.quick_access_position
        || (previous.quick_access_overlay_size - config.quick_access_overlay_size).abs()
            > f64::EPSILON
        || previous.quick_access_auto_close_enabled != config.quick_access_auto_close_enabled
        || previous.quick_access_auto_close_action != config.quick_access_auto_close_action
        || previous.quick_access_auto_close_interval != config.quick_access_auto_close_interval
        || previous.quick_access_close_after_dragging != config.quick_access_close_after_dragging
        || previous.quick_access_close_after_uploading != config.quick_access_close_after_uploading;
    let shortcuts_runtime_changed = shortcuts_differ(previous, config);
    let sync_autostart = previous.start_at_login != config.start_at_login;
    let daemon_affected = quick_access_runtime_changed
        || shortcuts_runtime_changed
        || language_changed
        || theme_changed
        || previous.show_menu_bar_icon != config.show_menu_bar_icon
        || daemon_needed(previous) != daemon_needed(config);
    SaveEffects {
        language_changed,
        theme_changed,
        quick_access_runtime_changed,
        shortcuts_runtime_changed,
        sync_hotkeys: shortcuts_runtime_changed,
        sync_autostart,
        daemon_affected,
    }
}

pub fn save_settings(inputs: &SaveInputs, state: &mut SaveState) -> anyhow::Result<SaveOutcome> {
    let mut config = load_config().sanitized();
    inputs.cloud_auto_upload_sync.sync(
        &inputs.cloud_auto_upload,
        config.cloud_auto_upload_after_capture,
    );
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
    config.rec_video_auto_zoom_on_import = inputs.rec_auto_zoom_on_import.is_active();
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
    // Interface capture options (freeze/crosshair/magnifier) are no longer
    // exposed in Settings; keep whatever is already in config (defaults for new users).
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
    let effects = persist_and_apply(
        state,
        &config,
        |config| save_config(config).map(|_| ()),
        |effects, config| {
            if effects.sync_hotkeys {
                crate::hotkeys::sync_hotkeys_from_app_config(config)?;
                let _ = crate::hotkeys::sync_gnome_hotkeys_for_current_desktop(None);
            }
            if effects.sync_autostart {
                if config.start_at_login {
                    install_autostart_entry_smart()?;
                } else {
                    uninstall_autostart_entry()?;
                }
            }
            Ok(())
        },
    )?;
    inputs
        .cloud_auto_upload_sync
        .note_saved(config.cloud_auto_upload_after_capture);

    if effects.language_changed {
        crate::i18n::init(&config.ui_language);
    }

    if effects.daemon_affected {
        let restart = effects.language_changed
            || effects.theme_changed
            || effects.quick_access_runtime_changed
            || effects.shortcuts_runtime_changed;
        state.follow_ups.retain(|handle| !handle.is_finished());
        let handle = std::thread::spawn(move || {
            let _serial = DAEMON_FOLLOW_UPS.lock().unwrap_or_else(|e| e.into_inner());
            let config = crate::config::load_config();
            match daemon_action(
                should_auto_respawn_daemon_for_save(),
                daemon_needed(&config),
                restart,
            ) {
                DaemonAction::Stop => {
                    let _ = stop_daemon_via_dbus();
                }
                DaemonAction::VisibilityOnly => {
                    let _ = set_daemon_tray_visibility(config.show_menu_bar_icon);
                }
                DaemonAction::Restart => {
                    if stop_daemon_via_dbus() {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    let _ = start_daemon_subprocess();
                }
                DaemonAction::VisibilityThenStart => {
                    if !set_daemon_tray_visibility(config.show_menu_bar_icon) {
                        let _ = start_daemon_subprocess();
                    }
                }
            }
        });
        state.follow_ups.push(handle);
    }

    Ok(SaveOutcome {
        language_changed: effects.language_changed,
        theme_changed: effects.theme_changed,
    })
}

fn combo_value(combo: &SettingsSelect, fallback: &str) -> String {
    combo.active_id().unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        screenshot_or_general_shortcuts_configured, shortcut_label_value,
        should_auto_respawn_daemon_for_save_with_env,
    };

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

    #[test]
    fn configured_screenshot_shortcuts_keep_daemon_needed_when_tray_hidden() {
        let config = crate::config::AppConfig {
            show_menu_bar_icon: false,
            shortcut_capture_area: "Ctrl+Alt+4".into(),
            ..Default::default()
        };
        assert!(screenshot_or_general_shortcuts_configured(&config));
    }

    #[test]
    fn daemon_not_needed_when_tray_hidden_and_all_shortcuts_empty() {
        let config = crate::config::AppConfig {
            show_menu_bar_icon: false,
            shortcut_open_file: String::new(),
            shortcut_open_from_clipboard: String::new(),
            shortcut_restore_recently_closed: String::new(),
            shortcut_toggle_overlays: String::new(),
            shortcut_capture_menu: String::new(),
            shortcut_capture_area: String::new(),
            shortcut_capture_crosshair: String::new(),
            shortcut_capture_fullscreen: String::new(),
            shortcut_show_last_preview: String::new(),
            shortcut_open_recording_ui: String::new(),
            shortcut_record_screen: String::new(),
            shortcut_recording_stop_save: String::new(),
            ..Default::default()
        };
        assert!(!screenshot_or_general_shortcuts_configured(&config));
    }

    #[test]
    fn unrelated_edits_trigger_no_side_effects() {
        let previous = crate::config::AppConfig::default();
        let mut next = previous.clone();
        next.play_sounds = !next.play_sounds;
        next.screenshot_format = "JPEG".into();
        next.rec_filename_pattern = "shot-%Y".into();
        let effects = super::save_effects(&previous, &next);
        assert!(!effects.sync_hotkeys);
        assert!(!effects.sync_autostart);
        assert!(!effects.daemon_affected);
    }

    #[test]
    fn shortcut_or_quick_access_edits_touch_hotkeys_and_daemon_once() {
        let previous = crate::config::AppConfig::default();
        let mut next = previous.clone();
        next.shortcut_capture_area = "Ctrl+Alt+4".into();
        let effects = super::save_effects(&previous, &next);
        assert!(effects.sync_hotkeys);
        assert!(effects.daemon_affected);
        assert!(!effects.sync_autostart);

        let mut next = previous.clone();
        next.quick_access_overlay_size += 0.1;
        let effects = super::save_effects(&previous, &next);
        assert!(!effects.sync_hotkeys);
        assert!(effects.daemon_affected);
        assert!(!effects.sync_autostart);
    }

    #[test]
    fn autostart_syncs_only_on_start_at_login_change() {
        let previous = crate::config::AppConfig::default();
        let mut next = previous.clone();
        next.start_at_login = !next.start_at_login;
        let effects = super::save_effects(&previous, &next);
        assert!(effects.sync_autostart);
        assert!(!effects.daemon_affected);

        let effects = super::save_effects(&previous, &previous.clone());
        assert!(!effects.sync_autostart);
    }

    #[test]
    fn tray_or_session_changes_reach_the_daemon_but_not_hotkeys() {
        let previous = crate::config::AppConfig::default();
        let mut next = previous.clone();
        next.show_menu_bar_icon = false;
        let effects = super::save_effects(&previous, &next);
        assert!(effects.daemon_affected);
        assert!(!effects.sync_hotkeys);

        let mut next = previous.clone();
        next.ui_theme = "light".into();
        let effects = super::save_effects(&previous, &next);
        assert!(effects.daemon_affected);
        assert!(effects.theme_changed);
        assert!(!effects.sync_hotkeys);
    }

    #[test]
    fn manual_daemon_sessions_never_restart_or_start() {
        for restart in [false, true] {
            assert_eq!(
                super::daemon_action(false, true, restart),
                super::DaemonAction::VisibilityOnly
            );
        }
        assert_eq!(
            super::daemon_action(false, false, true),
            super::DaemonAction::Stop
        );
    }

    #[test]
    fn desktop_managed_sessions_restart_once_then_fall_back() {
        assert_eq!(
            super::daemon_action(true, true, true),
            super::DaemonAction::Restart
        );
        assert_eq!(
            super::daemon_action(true, true, false),
            super::DaemonAction::VisibilityThenStart
        );
        assert_eq!(
            super::daemon_action(true, false, false),
            super::DaemonAction::Stop
        );
    }

    #[test]
    fn a_failed_effect_keeps_the_baseline_and_retries_it() {
        let mut state = super::SaveState::new(crate::config::AppConfig::default());
        let mut desired = crate::config::AppConfig::default();
        desired.start_at_login = true;
        let persists = std::cell::Cell::new(0u32);
        let syncs = std::cell::Cell::new(0u32);

        let first = super::persist_and_apply(
            &mut state,
            &desired,
            |_| {
                persists.set(persists.get() + 1);
                Ok(())
            },
            |effects, _| {
                syncs.set(syncs.get() + 1);
                assert!(effects.sync_autostart);
                Err(anyhow::anyhow!("autostart failed"))
            },
        );
        assert!(first.is_err());
        assert!(!state.applied.start_at_login);

        let second = super::persist_and_apply(
            &mut state,
            &desired,
            |_| {
                persists.set(persists.get() + 1);
                Ok(())
            },
            |effects, _| {
                syncs.set(syncs.get() + 1);
                assert!(effects.sync_autostart);
                Ok(())
            },
        );
        assert!(second.is_ok());
        assert!(state.applied.start_at_login);
        assert_eq!(persists.get(), 2);
        assert_eq!(syncs.get(), 2);
    }

    #[test]
    fn a_failed_persist_never_runs_effects() {
        let mut state = super::SaveState::new(crate::config::AppConfig::default());
        let mut desired = crate::config::AppConfig::default();
        desired.start_at_login = true;
        let sync_ran = std::cell::Cell::new(false);
        let result = super::persist_and_apply(
            &mut state,
            &desired,
            |_| Err(anyhow::anyhow!("disk full")),
            |_, _| {
                sync_ran.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!sync_ran.get());
        assert!(!state.applied.start_at_login);
    }

    #[test]
    fn unfinished_theme_effects_stay_required_after_a_sync_failure() {
        let mut state = super::SaveState::new(crate::config::AppConfig::default());
        let mut desired = crate::config::AppConfig::default();
        desired.ui_theme = "dark".into();
        assert!(super::persist_and_apply(
            &mut state,
            &desired,
            |_| Ok(()),
            |_, _| Err(anyhow::anyhow!("sync failed"))
        )
        .is_err());
        assert_ne!(state.applied.ui_theme, "dark");

        let theme_seen = std::cell::Cell::new(false);
        let result = super::persist_and_apply(
            &mut state,
            &desired,
            |_| Ok(()),
            |effects, _| {
                theme_seen.set(effects.theme_changed);
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert!(theme_seen.get());
        assert_eq!(state.applied.ui_theme, "dark");
    }

    #[test]
    fn a_partially_applied_sync_is_retried_even_after_reverting() {
        let mut desired = crate::config::AppConfig::default();
        desired.start_at_login = true;
        desired.shortcut_capture_area = "Ctrl+Alt+4".into();
        let mut state = super::SaveState::new(crate::config::AppConfig::default());
        let seen = std::cell::Cell::new(0u32);
        let fails = std::cell::Cell::new(1u32);
        let sync = |effects: &super::SaveEffects, _: &crate::config::AppConfig| {
            seen.set(seen.get() + 1);
            assert!(effects.sync_hotkeys);
            assert!(effects.sync_autostart);
            if fails.get() > 0 {
                fails.set(fails.get() - 1);
                return Err(anyhow::anyhow!("autostart failed after hotkeys ran"));
            }
            Ok(())
        };
        assert!(super::persist_and_apply(&mut state, &desired, |_| Ok(()), &sync).is_err());
        assert!(!state.applied.start_at_login);

        let reverted = crate::config::AppConfig::default();
        let result = super::persist_and_apply(&mut state, &reverted, |_| Ok(()), &sync);
        assert!(result.is_ok());
        assert_eq!(seen.get(), 2);
        assert!(!state.applied.start_at_login);

        let third = super::persist_and_apply(
            &mut state,
            &reverted,
            |_| Ok(()),
            |e, _| {
                assert!(!e.sync_hotkeys);
                assert!(!e.sync_autostart);
                Ok(())
            },
        );
        assert!(third.is_ok());
    }

    #[test]
    fn finish_runtime_updates_waits_for_scheduled_follow_ups() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut state = super::SaveState::new(crate::config::AppConfig::default());
        {
            let flag = std::sync::Arc::clone(&flag);
            state.follow_ups.push(std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(120));
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }));
        }
        state.finish_runtime_updates();
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    }
}
