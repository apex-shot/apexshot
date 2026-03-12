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
}
