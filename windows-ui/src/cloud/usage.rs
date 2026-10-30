//! Authenticated Cloud usage snapshot from the account API.

use std::time::Duration;

use serde::Deserialize;

use crate::config::{is_cloud_logged_in, resolve_cloud_backend_url, AppConfig};

use super::auth::{refresh_access_token, RefreshError};

#[derive(Debug, Clone, Deserialize)]
pub struct UsageSnapshot {
    pub email: String,
    pub tier: String,
    pub usage: Usage,
    pub limits: Limits,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub uploads: i64,
    pub storage_bytes: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Limits {
    pub max_uploads: i64,
    pub max_storage_bytes: i64,
    pub retention_days: i32,
}

#[derive(Debug)]
pub enum UsageError {
    SignedOut,
    Expired,
    Unavailable,
}

/// Fetch a snapshot, refreshing an expired device token once.
pub fn fetch(config: &mut AppConfig) -> Result<UsageSnapshot, UsageError> {
    if !is_cloud_logged_in(config) {
        return Err(UsageError::SignedOut);
    }
    match request(config) {
        Err(UsageError::Expired) => {
            refresh_access_token(config).map_err(|error| match error {
                RefreshError::NoRefreshToken | RefreshError::Rejected(_) => UsageError::Expired,
                RefreshError::Network(_) | RefreshError::Server(_) => UsageError::Unavailable,
            })?;
            request(config)
        }
        result => result,
    }
}

fn request(config: &AppConfig) -> Result<UsageSnapshot, UsageError> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build();
    let response = agent
        .get(&format!("{}/v1/account", resolve_cloud_backend_url(config)))
        .set(
            "Authorization",
            &format!("Bearer {}", config.cloud_api_token),
        )
        .set("Accept", "application/json")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => UsageError::Expired,
            _ => UsageError::Unavailable,
        })?;
    let snapshot: UsageSnapshot = response.into_json().map_err(|_| UsageError::Unavailable)?;
    validate(snapshot)
}

fn validate(snapshot: UsageSnapshot) -> Result<UsageSnapshot, UsageError> {
    if snapshot.usage.uploads < 0
        || snapshot.usage.storage_bytes < 0
        || snapshot.limits.max_uploads < -1
        || snapshot.limits.max_storage_bytes <= 0
    {
        return Err(UsageError::Unavailable);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cloud_account_usage_and_unlimited_uploads() {
        let snapshot: UsageSnapshot = serde_json::from_str(
            r#"{"email":"a@example.com","tier":"pro","usage":{"uploads":7,"storage_bytes":100},"limits":{"max_uploads":-1,"max_storage_bytes":1000,"retention_days":90}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.usage.uploads, 7);
        assert_eq!(snapshot.usage.storage_bytes, 100);
        assert_eq!(snapshot.limits.max_uploads, -1);
        assert_eq!(snapshot.limits.max_storage_bytes, 1000);
    }

    #[test]
    fn signed_out_does_not_issue_a_request() {
        assert!(matches!(
            fetch(&mut AppConfig::default()),
            Err(UsageError::SignedOut)
        ));
    }

    #[test]
    fn rejects_invalid_or_incomplete_usage_instead_of_showing_zeroes() {
        let json = r#"{"email":"a@example.com","tier":"free","usage":{"uploads":-1,"storage_bytes":100},"limits":{"max_uploads":20,"max_storage_bytes":1000,"retention_days":7}}"#;
        let invalid: UsageSnapshot = serde_json::from_str(json).unwrap();
        assert!(matches!(validate(invalid), Err(UsageError::Unavailable)));
        assert!(serde_json::from_str::<UsageSnapshot>(
            r#"{"email":"a@example.com","tier":"free"}"#
        )
        .is_err());
    }
}
