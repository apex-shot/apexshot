use std::path::Path;

use crate::config::AppConfig;

use super::upload::{UploadError, UploadResult};
use super::{apexshot, xbackbone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    ApexShot,
    XBackbone,
}

impl Destination {
    pub fn from_config(config: &AppConfig) -> Self {
        match config.cloud_destination.as_str() {
            "xbackbone" => Destination::XBackbone,
            _ => Destination::ApexShot,
        }
    }

    pub fn is_configured(self, config: &AppConfig) -> bool {
        match self {
            Destination::ApexShot => apexshot::is_configured(config),
            Destination::XBackbone => xbackbone::is_configured(config),
        }
    }

    pub fn upload(self, config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
        match self {
            Destination::ApexShot => apexshot::upload(config, path),
            Destination::XBackbone => xbackbone::upload(config, path),
        }
    }

    pub fn not_configured_notification(self, config: &AppConfig) -> (&'static str, &'static str) {
        match self {
            Destination::ApexShot => apexshot::not_configured_notification(config),
            Destination::XBackbone => xbackbone::not_configured_notification(config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_defaults_to_apexshot_for_unknown_values() {
        let cfg = AppConfig {
            cloud_destination: "something-else".to_string(),
            ..AppConfig::default()
        };
        assert_eq!(Destination::from_config(&cfg), Destination::ApexShot);
    }

    #[test]
    fn from_config_selects_xbackbone() {
        let cfg = AppConfig {
            cloud_destination: "xbackbone".to_string(),
            ..AppConfig::default()
        };
        assert_eq!(Destination::from_config(&cfg), Destination::XBackbone);
    }

    #[test]
    fn from_config_selects_apexshot() {
        let cfg = AppConfig {
            cloud_destination: "apexshot".to_string(),
            ..AppConfig::default()
        };
        assert_eq!(Destination::from_config(&cfg), Destination::ApexShot);
    }

    #[test]
    fn apexshot_is_configured_requires_token_and_backend_url() {
        let empty = AppConfig::default();
        assert!(!Destination::ApexShot.is_configured(&empty));

        let with_token = AppConfig {
            cloud_api_token: "tok".to_string(),
            cloud_backend_url: "https://api.example".to_string(),
            ..AppConfig::default()
        };
        assert!(Destination::ApexShot.is_configured(&with_token));
    }

    #[test]
    fn xbackbone_is_configured_requires_url_and_token() {
        let empty = AppConfig::default();
        assert!(!Destination::XBackbone.is_configured(&empty));

        let configured = AppConfig {
            xbackbone_url: "https://xb.example".to_string(),
            xbackbone_api_token: "tok".to_string(),
            ..AppConfig::default()
        };
        assert!(Destination::XBackbone.is_configured(&configured));
    }
}
