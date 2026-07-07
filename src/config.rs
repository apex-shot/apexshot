use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 12;
pub const MIN_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 3;
pub const MAX_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 120;
pub const DEFAULT_SHUTTER_SOUND: &str = "Camera";

pub const DEFAULT_AFTER_CAPTURE_SHOW_QUICK_ACCESS: bool = true;
pub const DEFAULT_AFTER_CAPTURE_COPY_FILE_TO_CLIPBOARD: bool = false;
pub const DEFAULT_AFTER_CAPTURE_SAVE: bool = true;
pub const DEFAULT_AFTER_CAPTURE_OPEN_ANNOTATE: bool = false;
pub const DEFAULT_REC_AFTER_CAPTURE_COPY_TO_CLIPBOARD: bool = false;
pub const DEFAULT_REC_AFTER_CAPTURE_SAVE: bool = true;
pub const QUICK_ACCESS_OVERLAY_SCALE_MIN: f64 = 0.5;
pub const QUICK_ACCESS_OVERLAY_SCALE_BASELINE: f64 = 1.0;
pub const QUICK_ACCESS_OVERLAY_SCALE_MAX: f64 = 1.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub preview_auto_close_seconds: u32,
    pub start_at_login: bool,
    pub play_sounds: bool,
    pub shutter_sound: String,
    pub show_menu_bar_icon: bool,
    pub export_location: String,
    pub screenshot_export_location: String,
    pub video_export_location: String,
    pub rec_filename_pattern: String,
    pub hide_desktop_icons_while_capturing: bool,
    pub after_capture_show_quick_access: bool,
    pub after_capture_copy_file_to_clipboard: bool,
    pub after_capture_save: bool,
    pub after_capture_open_annotate: bool,
    pub rec_after_capture_copy_to_clipboard: bool,
    pub rec_after_capture_save: bool,
    // Recording General tab settings
    pub rec_controls: bool,
    pub rec_display_time: bool,
    pub rec_hidpi: bool,
    pub rec_notifications: bool,
    pub rec_cursor: bool,
    pub rec_remember_selection: bool,
    pub rec_dim_screen: bool,
    pub rec_countdown: bool,
    // Remember selection: last selection area
    pub last_selection_x: Option<i32>,
    pub last_selection_y: Option<i32>,
    pub last_selection_w: Option<i32>,
    pub last_selection_h: Option<i32>,
    // Recording Video tab settings
    pub rec_video_format: u8,
    pub rec_video_max_res: u8,
    pub rec_video_fps: u8,
    pub rec_video_mono: bool,
    pub rec_video_open_editor: bool,
    // Recording GIF tab settings
    pub rec_gif_fps: u8,
    pub rec_gif_quality: f64,
    pub rec_gif_size_idx: u8,
    pub rec_gif_optimize: bool,
    pub rec_mic: bool,
    pub rec_speaker: bool,
    // Quick Access settings
    pub quick_access_position: String,
    pub quick_access_multi_display: bool,
    pub quick_access_overlay_size: f64,
    pub quick_access_auto_close_enabled: bool,
    pub quick_access_auto_close_action: String,
    pub quick_access_auto_close_interval: u32,
    pub quick_access_close_after_dragging: bool,
    pub quick_access_close_after_uploading: bool,
    // Screenshots tab settings
    pub screenshot_format: String,
    pub screenshot_retina_scale: bool,
    pub screenshot_frame_border: bool,
    pub screenshot_freeze_screen: bool,
    pub screenshot_crosshair_mode: String,
    pub screenshot_show_magnifier: bool,
    pub screenshot_timer_interval: u32,
    pub screenshot_show_cursor: bool,
    // Annotate tab settings
    pub annotate_inverse_arrow: bool,
    pub annotate_smooth_drawing: bool,
    pub annotate_draw_shadow: bool,
    pub annotate_auto_expand: bool,
    pub annotate_show_color_names: bool,
    pub annotate_always_on_top: bool,
    // Wallpaper settings
    pub wallpaper_mode: String,
    pub wallpaper_dont_change_on_space: bool,
    pub wallpaper_custom_path: String,
    pub wallpaper_plain_color: String,
    pub window_screenshot_mode: String,
    pub window_screenshot_padding: f64,
    pub window_screenshot_shadow: bool,
    // Shortcut settings
    pub shortcut_toggle_desktop_icons: String,
    pub shortcut_open_file: String,
    pub shortcut_open_from_clipboard: String,
    pub shortcut_pin_to_screen: String,
    pub shortcut_restore_recently_closed: String,
    pub shortcut_toggle_overlays: String,
    pub shortcut_capture_area: String,
    pub shortcut_capture_crosshair: String,
    pub shortcut_capture_previous_area: String,
    pub shortcut_capture_fullscreen: String,
    pub shortcut_capture_window: String,
    pub shortcut_capture_menu: String,
    pub shortcut_show_last_preview: String,
    pub shortcut_open_recording_ui: String,
    pub shortcut_record_screen: String,
    pub shortcut_recording_pause_resume: String,
    pub shortcut_recording_stop_save: String,
    pub shortcut_recording_restart: String,
    pub shortcut_recording_discard: String,
    // Cloud settings
    pub cloud_screenshot_quality: String,
    pub cloud_copy_to_clipboard: String,
    pub cloud_show_recently_uploaded: bool,
    pub cloud_ask_name_tags: bool,
    pub cloud_user_name: String,
    pub cloud_user_email: String,
    pub cloud_pro_plan: bool,
    pub cloud_backend_url: String,
    pub cloud_api_token: String,
    pub cloud_refresh_token: String,
    pub cloud_install_id: String,
    // Upload destination selection + self-hosted XBackBone settings
    pub cloud_destination: String,
    pub xbackbone_url: String,
    pub xbackbone_api_token: String,
    // Advanced settings
    pub adv_filename_pattern: String,
    pub adv_ask_name_after_capture: bool,
    pub adv_retina_suffix: bool,
    pub adv_clipboard_mode: String,
    pub adv_pinned_rounded_corners: bool,
    pub adv_pinned_shadow: bool,
    pub adv_pinned_border: bool,
    pub adv_ocr_language: String,
    pub adv_ocr_keep_line_breaks: bool,
    pub adv_filename_use_utc: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            preview_auto_close_seconds: DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS,
            start_at_login: false,
            play_sounds: true,
            shutter_sound: DEFAULT_SHUTTER_SOUND.to_string(),
            show_menu_bar_icon: true,
            export_location: String::new(),
            screenshot_export_location: String::new(),
            video_export_location: String::new(),
            rec_filename_pattern: "ApexShot Recording {Date} at {Time}".to_string(),
            hide_desktop_icons_while_capturing: false,
            after_capture_show_quick_access: DEFAULT_AFTER_CAPTURE_SHOW_QUICK_ACCESS,
            after_capture_copy_file_to_clipboard: DEFAULT_AFTER_CAPTURE_COPY_FILE_TO_CLIPBOARD,
            after_capture_save: DEFAULT_AFTER_CAPTURE_SAVE,
            after_capture_open_annotate: DEFAULT_AFTER_CAPTURE_OPEN_ANNOTATE,
            rec_after_capture_copy_to_clipboard: DEFAULT_REC_AFTER_CAPTURE_COPY_TO_CLIPBOARD,
            rec_after_capture_save: DEFAULT_REC_AFTER_CAPTURE_SAVE,
            rec_controls: true,
            rec_display_time: false,
            rec_hidpi: true,
            rec_notifications: true,
            rec_cursor: true,
            rec_remember_selection: false,
            rec_dim_screen: true,
            rec_countdown: true,
            last_selection_x: None,
            last_selection_y: None,
            last_selection_w: None,
            last_selection_h: None,
            // Video tab defaults
            rec_video_format: 0,  // 0 = MP4
            rec_video_max_res: 0, // 0 = Original
            rec_video_fps: 1,     // 1 = 30fps
            rec_video_mono: false,
            rec_video_open_editor: false,
            rec_gif_fps: 50,
            rec_gif_quality: 0.75,
            rec_gif_size_idx: 0,
            rec_gif_optimize: true,
            rec_mic: false,
            rec_speaker: false,
            quick_access_position: "Left".to_string(),
            quick_access_multi_display: true,
            quick_access_overlay_size: QUICK_ACCESS_OVERLAY_SCALE_BASELINE,
            quick_access_auto_close_enabled: false,
            quick_access_auto_close_action: "Close".to_string(),
            quick_access_auto_close_interval: 30,
            quick_access_close_after_dragging: true,
            quick_access_close_after_uploading: true,
            screenshot_format: "PNG".to_string(),
            screenshot_retina_scale: false,
            screenshot_frame_border: false,
            screenshot_freeze_screen: true,
            screenshot_crosshair_mode: "Disabled".to_string(),
            screenshot_show_magnifier: false,
            screenshot_timer_interval: 0,
            screenshot_show_cursor: true,
            annotate_inverse_arrow: false,
            annotate_smooth_drawing: true,
            annotate_draw_shadow: true,
            annotate_auto_expand: false,
            annotate_show_color_names: false,
            annotate_always_on_top: false,
            wallpaper_mode: "Desktop".to_string(),
            wallpaper_dont_change_on_space: false,
            wallpaper_custom_path: String::new(),
            wallpaper_plain_color: "#b0c4de".to_string(), // LightSteelBlue from image
            window_screenshot_mode: "Wallpaper".to_string(),
            window_screenshot_padding: 0.5,
            window_screenshot_shadow: true,
            shortcut_toggle_desktop_icons: "Ctrl+Super+H".to_string(),
            shortcut_open_file: String::new(),
            shortcut_open_from_clipboard: String::new(),
            shortcut_pin_to_screen: String::new(),
            shortcut_restore_recently_closed: String::new(),
            shortcut_toggle_overlays: String::new(),
            shortcut_capture_area: "Shift+Super+4".to_string(),
            shortcut_capture_crosshair: "Ctrl+Alt+X".to_string(),
            shortcut_capture_previous_area: String::new(),
            shortcut_capture_fullscreen: "Shift+Super+3".to_string(),
            shortcut_capture_window: "Shift+Super+5".to_string(),
            shortcut_capture_menu: String::new(),
            shortcut_show_last_preview: "Ctrl+Alt+P".to_string(),
            shortcut_open_recording_ui: "Ctrl+Alt+R".to_string(),
            shortcut_record_screen: String::new(),
            shortcut_recording_pause_resume: "Ctrl+Alt+Shift+P".to_string(),
            shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".to_string(),
            shortcut_recording_restart: "Ctrl+Alt+Shift+N".to_string(),
            shortcut_recording_discard: "Ctrl+Alt+Shift+BackSpace".to_string(),
            cloud_screenshot_quality: "Optimized for sharing".to_string(),
            cloud_copy_to_clipboard: "ApexShot Cloud link".to_string(),
            cloud_show_recently_uploaded: true,
            cloud_ask_name_tags: false,
            cloud_user_name: String::new(),
            cloud_user_email: String::new(),
            cloud_pro_plan: false,
            cloud_backend_url: String::new(),
            cloud_api_token: String::new(),
            cloud_refresh_token: String::new(),
            cloud_install_id: String::new(),
            cloud_destination: "apexshot".to_string(),
            xbackbone_url: String::new(),
            xbackbone_api_token: String::new(),
            adv_filename_pattern: "ApexShot {Date} at {Time}".to_string(),
            adv_ask_name_after_capture: false,
            adv_retina_suffix: true,
            adv_clipboard_mode: "File & Image (default)".to_string(),
            adv_pinned_rounded_corners: true,
            adv_pinned_shadow: true,
            adv_pinned_border: true,
            adv_ocr_language: "eng".to_string(),
            adv_ocr_keep_line_breaks: true,
            adv_filename_use_utc: false,
        }
    }
}

impl AppConfig {
    pub fn sanitized(mut self) -> Self {
        self.preview_auto_close_seconds = self.preview_auto_close_seconds.clamp(
            MIN_PREVIEW_AUTO_CLOSE_SECONDS,
            MAX_PREVIEW_AUTO_CLOSE_SECONDS,
        );
        self.shutter_sound = sanitize_shutter_sound(self.shutter_sound);
        self.export_location = self.export_location.trim().to_string();
        self.screenshot_export_location = self.screenshot_export_location.trim().to_string();
        self.video_export_location = self.video_export_location.trim().to_string();
        if self.screenshot_export_location.is_empty() && !self.export_location.is_empty() {
            self.screenshot_export_location = self.export_location.clone();
        }
        if self.video_export_location.is_empty() && !self.export_location.is_empty() {
            self.video_export_location = self.export_location.clone();
        }
        if self.after_capture_open_annotate {
            self.after_capture_show_quick_access = false;
        }
        if !self.after_capture_show_quick_access
            && !self.after_capture_copy_file_to_clipboard
            && !self.after_capture_save
            && !self.after_capture_open_annotate
        {
            self.after_capture_show_quick_access = DEFAULT_AFTER_CAPTURE_SHOW_QUICK_ACCESS;
        }
        self.rec_video_format = self.rec_video_format.min(1);
        self.rec_gif_fps = self.rec_gif_fps.clamp(5, 60);
        self.rec_gif_quality = self.rec_gif_quality.clamp(0.0, 1.0);
        self.rec_gif_size_idx = self.rec_gif_size_idx.min(3);
        self.quick_access_overlay_size =
            sanitize_quick_access_overlay_size(self.quick_access_overlay_size);
        self.quick_access_position = match self.quick_access_position.as_str() {
            "Left" | "Right" => self.quick_access_position,
            _ => "Left".to_string(),
        };
        self.quick_access_auto_close_action = match self.quick_access_auto_close_action.as_str() {
            "Close" | "Hide" => self.quick_access_auto_close_action,
            _ => "Close".to_string(),
        };
        self.screenshot_format = match self.screenshot_format.as_str() {
            "PNG" | "JPEG" | "WebP" => self.screenshot_format,
            _ => "PNG".to_string(),
        };
        self.screenshot_crosshair_mode = match self.screenshot_crosshair_mode.as_str() {
            "Disabled" | "Crosshair" | "Magnifier" => self.screenshot_crosshair_mode,
            _ => "Disabled".to_string(),
        };
        self.wallpaper_mode = match self.wallpaper_mode.as_str() {
            "Desktop" | "Custom" | "Color" => self.wallpaper_mode,
            _ => "Desktop".to_string(),
        };
        self.window_screenshot_mode = match self.window_screenshot_mode.as_str() {
            "Wallpaper" | "Transparent" => self.window_screenshot_mode,
            _ => "Wallpaper".to_string(),
        };
        self.window_screenshot_padding = self.window_screenshot_padding.clamp(0.0, 1.0);
        self.cloud_destination = match self.cloud_destination.as_str() {
            "apexshot" | "xbackbone" => self.cloud_destination,
            _ => "apexshot".to_string(),
        };
        self.xbackbone_url = self.xbackbone_url.trim().to_string();
        self.xbackbone_api_token = self.xbackbone_api_token.trim().to_string();
        self
    }
}

fn sanitize_shutter_sound(value: String) -> String {
    match value.trim() {
        "Camera" | "Classic" | "Pop" | "None" => value.trim().to_string(),
        _ => DEFAULT_SHUTTER_SOUND.to_string(),
    }
}

fn sanitize_quick_access_overlay_size(value: f64) -> f64 {
    value.clamp(
        QUICK_ACCESS_OVERLAY_SCALE_MIN,
        QUICK_ACCESS_OVERLAY_SCALE_MAX,
    )
}

fn should_migrate_legacy_quick_access_overlay_size(raw: &str, config: &AppConfig) -> bool {
    !raw.contains("quick_access_position:")
        && raw.contains("quick_access_overlay_size:")
        && (config.quick_access_overlay_size - QUICK_ACCESS_OVERLAY_SCALE_MIN).abs() < f64::EPSILON
}

pub fn config_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("apexshot");
    path.push("config.yml");
    Some(path)
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return apply_cloud_env_overrides(AppConfig::default());
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return apply_cloud_env_overrides(AppConfig::default());
    };

    serde_yml::from_str::<AppConfig>(&raw)
        .map(|config| {
            let mut sanitized = config.sanitized();
            if should_migrate_legacy_quick_access_overlay_size(&raw, &sanitized) {
                sanitized.quick_access_overlay_size = QUICK_ACCESS_OVERLAY_SCALE_BASELINE;
            }
            if sanitized.cloud_backend_url.is_empty() {
                if let Ok(url) = std::env::var("APEXSHOT_CLOUD_BACKEND_URL") {
                    sanitized.cloud_backend_url = url;
                }
            }
            apply_xbackbone_overrides(&mut sanitized);
            sanitized
        })
        .unwrap_or_else(|_| {
            let mut default = AppConfig::default();
            if let Ok(url) = std::env::var("APEXSHOT_CLOUD_BACKEND_URL") {
                default.cloud_backend_url = url;
            }
            apply_cloud_env_overrides(default)
        })
}

fn apply_cloud_env_overrides(mut config: AppConfig) -> AppConfig {
    if config.cloud_backend_url.is_empty() {
        if let Ok(url) = std::env::var("APEXSHOT_CLOUD_BACKEND_URL") {
            config.cloud_backend_url = url;
        }
    }
    apply_xbackbone_overrides(&mut config);
    config
}

fn apply_xbackbone_overrides(config: &mut AppConfig) {
    if config.xbackbone_url.is_empty() {
        if let Ok(url) = std::env::var("APEXSHOT_XBACKBONE_URL") {
            config.xbackbone_url = url.trim().to_string();
        }
    }
    if config.xbackbone_api_token.is_empty() {
        if let Ok(token) = std::env::var("APEXSHOT_XBACKBONE_TOKEN") {
            config.xbackbone_api_token = token.trim().to_string();
        }
    }
    // Auto-detect an existing XBackBone CLI / KDE plugin config so users who
    // already have `xbb` set up can go zero-config. Only used as a default —
    // explicit Settings values (or env vars above) always win.
    if config.xbackbone_url.is_empty() && config.xbackbone_api_token.is_empty() {
        if let Some((url, token)) = read_xbackbone_config_file() {
            config.xbackbone_url = url;
            config.xbackbone_api_token = token;
        }
    }
}

fn read_xbackbone_config_file() -> Option<(String, String)> {
    let mut path = dirs::config_dir()?;
    path.push("xbackbone");
    path.push("config");
    let content = std::fs::read_to_string(path).ok()?;
    parse_xbackbone_config_file(&content)
}

fn parse_xbackbone_config_file(content: &str) -> Option<(String, String)> {
    let mut url = None;
    let mut token = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "XBB_URL" if !value.is_empty() => url = Some(value.to_string()),
            "XBB_TOKEN" if !value.is_empty() => token = Some(value.to_string()),
            _ => {}
        }
    }
    match (url, token) {
        (Some(u), Some(t)) => Some((u, t)),
        _ => None,
    }
}

pub fn save_config(config: &AppConfig) -> anyhow::Result<PathBuf> {
    let path = config_path().context("Failed to resolve config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let serialized = serde_yml::to_string(&config.clone().sanitized())
        .context("Failed to serialize configuration")?;

    std::fs::write(&path, serialized)
        .with_context(|| format!("Failed to write config file {}", path.display()))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn default_config_has_expected_auto_close_seconds() {
        let cfg = AppConfig::default();
        assert_eq!(
            cfg.preview_auto_close_seconds,
            DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS
        );
    }

    #[test]
    fn default_screenshot_timer_is_off() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.screenshot_timer_interval, 0);
    }

    #[test]
    fn sanitize_clamps_auto_close_seconds() {
        let low = AppConfig {
            preview_auto_close_seconds: 0,
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(
            low.preview_auto_close_seconds,
            MIN_PREVIEW_AUTO_CLOSE_SECONDS
        );

        let high = AppConfig {
            preview_auto_close_seconds: 999,
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(
            high.preview_auto_close_seconds,
            MAX_PREVIEW_AUTO_CLOSE_SECONDS
        );
    }

    #[test]
    fn sanitize_normalizes_sound_and_export_location() {
        let cfg = AppConfig {
            shutter_sound: "Unknown".into(),
            export_location: "  /tmp/apexshot  ".into(),
            ..AppConfig::default()
        }
        .sanitized();

        assert_eq!(cfg.shutter_sound, DEFAULT_SHUTTER_SOUND);
        assert_eq!(cfg.export_location, "/tmp/apexshot");
    }

    #[test]
    fn sanitize_migrates_legacy_shared_export_location() {
        let cfg = AppConfig {
            export_location: "  /tmp/shared  ".into(),
            ..AppConfig::default()
        }
        .sanitized();

        assert_eq!(cfg.screenshot_export_location, "/tmp/shared");
        assert_eq!(cfg.video_export_location, "/tmp/shared");
    }

    #[test]
    fn sanitize_preserves_explicit_per_feature_export_locations() {
        let cfg = AppConfig {
            export_location: "/tmp/shared".into(),
            screenshot_export_location: " /tmp/screens ".into(),
            video_export_location: " /tmp/video ".into(),
            ..AppConfig::default()
        }
        .sanitized();

        assert_eq!(cfg.screenshot_export_location, "/tmp/screens");
        assert_eq!(cfg.video_export_location, "/tmp/video");
    }

    #[test]
    fn sanitize_keeps_at_least_one_screenshot_after_capture_action_enabled() {
        let cfg = AppConfig {
            after_capture_show_quick_access: false,
            after_capture_copy_file_to_clipboard: false,
            after_capture_save: false,
            after_capture_open_annotate: false,
            ..AppConfig::default()
        }
        .sanitized();

        assert!(cfg.after_capture_show_quick_access);
    }

    #[test]
    fn sanitize_makes_open_annotate_disable_quick_access() {
        let cfg = AppConfig {
            after_capture_show_quick_access: true,
            after_capture_open_annotate: true,
            ..AppConfig::default()
        }
        .sanitized();

        assert!(cfg.after_capture_open_annotate);
        assert!(!cfg.after_capture_show_quick_access);
    }

    #[test]
    fn screenshot_settings_round_trip_preserves_retained_fields() {
        let original = AppConfig {
            screenshot_export_location: "/tmp/screens".into(),
            screenshot_format: "JPEG".into(),
            screenshot_freeze_screen: false,
            screenshot_crosshair_mode: "Crosshair".into(),
            screenshot_show_magnifier: true,
            screenshot_timer_interval: 3,
            screenshot_show_cursor: false,
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).expect("config should serialize");
        let loaded: AppConfig = serde_yml::from_str(&yaml).expect("config should deserialize");

        assert_eq!(
            loaded.screenshot_export_location,
            original.screenshot_export_location
        );
        assert_eq!(loaded.screenshot_format, original.screenshot_format);
        assert_eq!(
            loaded.screenshot_freeze_screen,
            original.screenshot_freeze_screen
        );
        assert_eq!(
            loaded.screenshot_crosshair_mode,
            original.screenshot_crosshair_mode
        );
        assert_eq!(
            loaded.screenshot_show_magnifier,
            original.screenshot_show_magnifier
        );
        assert_eq!(
            loaded.screenshot_timer_interval,
            original.screenshot_timer_interval
        );
        assert_eq!(
            loaded.screenshot_show_cursor,
            original.screenshot_show_cursor
        );
    }

    #[test]
    fn annotate_settings_round_trip_through_yaml() {
        let original = AppConfig {
            annotate_inverse_arrow: true,
            annotate_smooth_drawing: false,
            annotate_draw_shadow: false,
            annotate_auto_expand: true,
            annotate_show_color_names: true,
            annotate_always_on_top: true,
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).unwrap();
        let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(
            loaded.annotate_inverse_arrow,
            original.annotate_inverse_arrow
        );
        assert_eq!(
            loaded.annotate_smooth_drawing,
            original.annotate_smooth_drawing
        );
        assert_eq!(loaded.annotate_draw_shadow, original.annotate_draw_shadow);
        assert_eq!(loaded.annotate_auto_expand, original.annotate_auto_expand);
        assert_eq!(
            loaded.annotate_show_color_names,
            original.annotate_show_color_names
        );
        assert_eq!(
            loaded.annotate_always_on_top,
            original.annotate_always_on_top
        );
    }

    #[test]
    fn legacy_annotate_show_dock_icon_key_is_ignored_on_load() {
        let loaded: AppConfig =
            serde_yml::from_str("annotate_always_on_top: true\nannotate_show_dock_icon: false\n")
                .unwrap();

        assert!(loaded.annotate_always_on_top);
    }

    #[test]
    fn recording_settings_have_correct_defaults() {
        let cfg = AppConfig::default();
        assert!(cfg.rec_controls);
        assert!(!cfg.rec_display_time);
        assert!(cfg.rec_hidpi);
        assert!(cfg.rec_notifications);
        assert!(cfg.rec_cursor);
        assert!(!cfg.rec_remember_selection);
        assert!(cfg.rec_dim_screen);
        assert!(cfg.rec_countdown);
        assert!(cfg.last_selection_x.is_none());
        assert_eq!(cfg.rec_video_format, 0);
    }

    #[test]
    fn recording_overlay_settings_have_expected_defaults() {
        let cfg = AppConfig::default();
        assert!(!cfg.rec_mic);
        assert!(!cfg.rec_speaker);
    }

    #[test]
    fn recording_overlay_settings_round_trip_through_yaml() {
        let original = AppConfig {
            rec_mic: true,
            rec_speaker: true,
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).unwrap();
        let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(loaded.rec_mic, original.rec_mic);
        assert_eq!(loaded.rec_speaker, original.rec_speaker);
    }

    #[test]
    fn config_without_recording_settings_uses_defaults() {
        let yaml = "preview_auto_close_seconds: 12\nstart_at_login: false\n";
        let cfg: AppConfig = serde_yml::from_str(yaml).unwrap();
        assert!(!cfg.rec_mic);
        assert!(!cfg.rec_speaker);
        assert!(cfg.rec_controls);
        assert!(cfg.rec_cursor);
        assert!(cfg.rec_hidpi);
        assert_eq!(cfg.rec_video_format, 0);
    }

    #[test]
    fn sanitize_quick_access_position_rejects_unsupported_values() {
        let top = AppConfig {
            quick_access_position: "Top".into(),
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(top.quick_access_position, "Left");

        let right = AppConfig {
            quick_access_position: "Right".into(),
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(right.quick_access_position, "Right");
    }

    #[test]
    fn sanitize_preserves_smallest_quick_access_overlay_size() {
        let cfg = AppConfig {
            quick_access_overlay_size: 0.5,
            ..AppConfig::default()
        }
        .sanitized();

        assert_eq!(cfg.quick_access_overlay_size, 0.5);
    }

    #[test]
    fn legacy_quick_access_overlay_size_migrates_only_for_old_schema() {
        let legacy_raw = r#"
preview_auto_close_seconds: 12
quick_access_overlay_size: 0.5
"#;
        let legacy_cfg = serde_yml::from_str::<AppConfig>(legacy_raw)
            .unwrap()
            .sanitized();
        assert!(should_migrate_legacy_quick_access_overlay_size(
            legacy_raw,
            &legacy_cfg
        ));

        let current_raw = r#"
preview_auto_close_seconds: 12
quick_access_position: Right
quick_access_overlay_size: 0.5
"#;
        let current_cfg = serde_yml::from_str::<AppConfig>(current_raw)
            .unwrap()
            .sanitized();
        assert!(!should_migrate_legacy_quick_access_overlay_size(
            current_raw,
            &current_cfg
        ));
    }

    #[test]
    fn sanitize_clamps_quick_access_overlay_size() {
        let low = AppConfig {
            quick_access_overlay_size: -10.0,
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(low.quick_access_overlay_size, 0.5);

        let high = AppConfig {
            quick_access_overlay_size: 10.0,
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(high.quick_access_overlay_size, 1.5);
    }

    #[test]
    fn shortcut_defaults_include_crosshair_capture() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.shortcut_capture_crosshair, "Ctrl+Alt+X");
    }

    #[test]
    fn config_yaml_round_trip_preserves_crosshair_shortcut() {
        let original = AppConfig {
            shortcut_capture_crosshair: "Alt+Print".into(),
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).unwrap();
        let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(
            loaded.shortcut_capture_crosshair,
            original.shortcut_capture_crosshair
        );
    }

    #[test]
    fn shortcut_defaults_include_open_recording_ui_and_controls() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.shortcut_open_recording_ui, "Ctrl+Alt+R");
        assert_eq!(cfg.shortcut_record_screen, "");
        assert_eq!(cfg.shortcut_recording_pause_resume, "Ctrl+Alt+Shift+P");
        assert_eq!(cfg.shortcut_recording_stop_save, "Ctrl+Alt+Shift+S");
        assert_eq!(cfg.shortcut_recording_restart, "Ctrl+Alt+Shift+N");
        assert_eq!(cfg.shortcut_recording_discard, "Ctrl+Alt+Shift+BackSpace");
    }

    #[test]
    fn config_yaml_round_trip_preserves_recording_shortcuts() {
        let original = AppConfig {
            shortcut_open_recording_ui: "Alt+R".into(),
            shortcut_record_screen: "Ctrl+Shift+R".into(),
            shortcut_recording_pause_resume: "Alt+P".into(),
            shortcut_recording_stop_save: "Alt+S".into(),
            shortcut_recording_restart: "Alt+N".into(),
            shortcut_recording_discard: "Alt+BackSpace".into(),
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).unwrap();
        let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(
            loaded.shortcut_open_recording_ui,
            original.shortcut_open_recording_ui
        );
        assert_eq!(
            loaded.shortcut_record_screen,
            original.shortcut_record_screen
        );
        assert_eq!(
            loaded.shortcut_recording_pause_resume,
            original.shortcut_recording_pause_resume
        );
        assert_eq!(
            loaded.shortcut_recording_stop_save,
            original.shortcut_recording_stop_save
        );
        assert_eq!(
            loaded.shortcut_recording_restart,
            original.shortcut_recording_restart
        );
        assert_eq!(
            loaded.shortcut_recording_discard,
            original.shortcut_recording_discard
        );
    }

    #[test]
    fn sanitize_clamps_cloud_destination_to_known_values() {
        let unknown = AppConfig {
            cloud_destination: "imgur".into(),
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(unknown.cloud_destination, "apexshot");

        let apexshot = AppConfig {
            cloud_destination: "apexshot".into(),
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(apexshot.cloud_destination, "apexshot");

        let xbackbone = AppConfig {
            cloud_destination: "xbackbone".into(),
            ..AppConfig::default()
        }
        .sanitized();
        assert_eq!(xbackbone.cloud_destination, "xbackbone");
    }

    #[test]
    fn sanitize_trims_xbackbone_fields() {
        let cfg = AppConfig {
            xbackbone_url: "  https://xb.example  ".into(),
            xbackbone_api_token: "  tok123  ".into(),
            ..AppConfig::default()
        }
        .sanitized();

        assert_eq!(cfg.xbackbone_url, "https://xb.example");
        assert_eq!(cfg.xbackbone_api_token, "tok123");
    }

    #[test]
    fn xbackbone_fields_round_trip_through_yaml() {
        let original = AppConfig {
            cloud_destination: "xbackbone".into(),
            xbackbone_url: "https://xb.example".into(),
            xbackbone_api_token: "secret-token".into(),
            ..AppConfig::default()
        };

        let yaml = serde_yml::to_string(&original).unwrap();
        let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(loaded.cloud_destination, original.cloud_destination);
        assert_eq!(loaded.xbackbone_url, original.xbackbone_url);
        assert_eq!(loaded.xbackbone_api_token, original.xbackbone_api_token);
    }

    #[test]
    fn default_cloud_destination_is_apexshot() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.cloud_destination, "apexshot");
        assert!(cfg.xbackbone_url.is_empty());
        assert!(cfg.xbackbone_api_token.is_empty());
    }

    #[test]
    fn parse_xbackbone_config_file_reads_url_and_token() {
        let content =
            "# XBackBone CLI config\nXBB_URL=https://files.example.com\nXBB_TOKEN=abc-123\n";
        let (url, token) = parse_xbackbone_config_file(content).expect("parsed");
        assert_eq!(url, "https://files.example.com");
        assert_eq!(token, "abc-123");
    }

    #[test]
    fn parse_xbackbone_config_file_strips_surrounding_quotes() {
        let content = "XBB_URL=\"https://files.example.com\"\nXBB_TOKEN=\"abc-123\"\n";
        let (url, token) = parse_xbackbone_config_file(content).expect("parsed");
        assert_eq!(url, "https://files.example.com");
        assert_eq!(token, "abc-123");
    }

    #[test]
    fn parse_xbackbone_config_file_ignores_comments_and_blank_lines() {
        let content =
            "\n# a comment\n\nXBB_URL=https://files.example.com\n# mid comment\nXBB_TOKEN=tok\n";
        let (url, token) = parse_xbackbone_config_file(content).expect("parsed");
        assert_eq!(url, "https://files.example.com");
        assert_eq!(token, "tok");
    }

    #[test]
    fn parse_xbackbone_config_file_returns_none_when_a_value_is_missing() {
        assert!(parse_xbackbone_config_file("XBB_URL=https://files.example.com\n").is_none());
        assert!(parse_xbackbone_config_file("XBB_TOKEN=tok\n").is_none());
        assert!(parse_xbackbone_config_file("").is_none());
    }

    #[test]
    fn parse_xbackbone_config_file_ignores_empty_values() {
        let content = "XBB_URL=\nXBB_TOKEN=\n";
        assert!(parse_xbackbone_config_file(content).is_none());
    }
}
