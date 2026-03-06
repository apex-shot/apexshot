use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 12;
pub const MIN_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 3;
pub const MAX_PREVIEW_AUTO_CLOSE_SECONDS: u32 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub preview_auto_close_seconds: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            preview_auto_close_seconds: DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS,
        }
    }
}

impl AppConfig {
    pub fn sanitized(mut self) -> Self {
        self.preview_auto_close_seconds = self.preview_auto_close_seconds.clamp(
            MIN_PREVIEW_AUTO_CLOSE_SECONDS,
            MAX_PREVIEW_AUTO_CLOSE_SECONDS,
        );
        self
    }
}

pub fn config_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("cleanshitx");
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
        }
        .sanitized();
        assert_eq!(
            low.preview_auto_close_seconds,
            MIN_PREVIEW_AUTO_CLOSE_SECONDS
        );

        let high = AppConfig {
            preview_auto_close_seconds: 999,
        }
        .sanitized();
        assert_eq!(
            high.preview_auto_close_seconds,
            MAX_PREVIEW_AUTO_CLOSE_SECONDS
        );
    }
}
