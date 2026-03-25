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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub preview_auto_close_seconds: u32,
    pub start_at_login: bool,
    pub play_sounds: bool,
    pub shutter_sound: String,
    pub show_menu_bar_icon: bool,
    pub export_location: String,
    pub hide_desktop_icons_while_capturing: bool,
    pub after_capture_show_quick_access: bool,
    pub after_capture_copy_file_to_clipboard: bool,
    pub after_capture_save: bool,
    pub after_capture_open_annotate: bool,
    // Recording General tab settings
    pub rec_controls: bool,
    pub rec_display_time: bool,
    pub rec_hidpi: bool,
    pub rec_notifications: bool,
    pub rec_cursor: bool,
    pub rec_clicks: bool,
    pub rec_keystrokes: bool,
    pub rec_remember_selection: bool,
    pub rec_dim_screen: bool,
    pub rec_countdown: bool,
    // Remember selection: last selection area
    pub last_selection_x: Option<i32>,
    pub last_selection_y: Option<i32>,
    pub last_selection_w: Option<i32>,
    pub last_selection_h: Option<i32>,
    // Recording Video tab settings
    pub rec_video_max_res: u8,
    pub rec_video_fps: u8,
    pub rec_video_mono: bool,
    pub rec_video_open_editor: bool,
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
            hide_desktop_icons_while_capturing: false,
            after_capture_show_quick_access: DEFAULT_AFTER_CAPTURE_SHOW_QUICK_ACCESS,
            after_capture_copy_file_to_clipboard: DEFAULT_AFTER_CAPTURE_COPY_FILE_TO_CLIPBOARD,
            after_capture_save: DEFAULT_AFTER_CAPTURE_SAVE,
            after_capture_open_annotate: DEFAULT_AFTER_CAPTURE_OPEN_ANNOTATE,
            rec_controls: true,
            rec_display_time: false,
            rec_hidpi: false,
            rec_notifications: true,
            rec_cursor: true,
            rec_clicks: false,
            rec_keystrokes: false,
            rec_remember_selection: false,
            rec_dim_screen: true,
            rec_countdown: true,
            last_selection_x: None,
            last_selection_y: None,
            last_selection_w: None,
            last_selection_h: None,
            // Video tab defaults
            rec_video_max_res: 0, // 0 = Original
            rec_video_fps: 1,     // 1 = 30fps
            rec_video_mono: false,
            rec_video_open_editor: false,
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
        self
    }
}

fn sanitize_shutter_sound(value: String) -> String {
    match value.trim() {
        "Camera" | "Classic" | "Pop" | "None" => value.trim().to_string(),
        _ => DEFAULT_SHUTTER_SOUND.to_string(),
    }
}

pub fn config_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("apexshot");
    path.push("config.yml");
    Some(path)
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return AppConfig::default();
    };

    serde_yml::from_str::<AppConfig>(&raw)
        .map(AppConfig::sanitized)
        .unwrap_or_default()
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
    fn recording_settings_have_correct_defaults() {
        let cfg = AppConfig::default();
        assert!(cfg.rec_controls);
        assert!(!cfg.rec_display_time);
        assert!(!cfg.rec_hidpi);
        assert!(cfg.rec_notifications);
        assert!(cfg.rec_cursor);
        assert!(!cfg.rec_clicks);
        assert!(!cfg.rec_keystrokes);
        assert!(!cfg.rec_remember_selection);
        assert!(cfg.rec_dim_screen);
        assert!(cfg.rec_countdown);
        assert!(cfg.last_selection_x.is_none());
    }

    #[test]
    fn config_without_recording_settings_uses_defaults() {
        let yaml = "preview_auto_close_seconds: 12\nstart_at_login: false\n";
        let cfg: AppConfig = serde_yml::from_str(yaml).unwrap();
        assert!(cfg.rec_controls);
        assert!(cfg.rec_cursor);
        assert!(!cfg.rec_hidpi);
    }
}
