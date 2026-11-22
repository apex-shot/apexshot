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
    /// When the monthly upload allowance returns. Older servers omit it.
    #[serde(default)]
    pub month_resets_at: Option<String>,
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

/// A short, best-effort account read for the post-upload heads-up.
///
/// The upload notification has already been shown, so a slow or failing
/// account endpoint only loses the extra usage line. No token refresh here:
/// the upload that just succeeded proved the token works.
pub fn fetch_for_heads_up(config: &AppConfig) -> Option<UsageSnapshot> {
    if !is_cloud_logged_in(config) {
        return None;
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout(Duration::from_secs(2))
        .build();
    let snapshot: UsageSnapshot = agent
        .get(&format!("{}/v1/account", resolve_cloud_backend_url(config)))
        .set(
            "Authorization",
            &format!("Bearer {}", config.cloud_api_token),
        )
        .set("Accept", "application/json")
        .call()
        .ok()?
        .into_json()
        .ok()?;
    if snapshot.usage.uploads < 0
        || snapshot.usage.storage_bytes < 0
        || snapshot.limits.max_uploads < -1
        || snapshot.limits.max_storage_bytes < 0
    {
        return None;
    }
    Some(snapshot)
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
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    struct Fixture {
        url: String,
        request: std::sync::mpsc::Receiver<String>,
    }

    fn serve_once(status: &str, body: &str) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let port = listener.local_addr().unwrap().port();
        let (tx, request) = std::sync::mpsc::channel();
        let status = status.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok() {
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                head.push_str(&line);
                line.clear();
            }
            let mut stream = reader.into_inner();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = tx.send(head);
        });
        Fixture {
            url: format!("http://127.0.0.1:{port}"),
            request,
        }
    }

    fn signed_in_config(backend_url: &str) -> AppConfig {
        AppConfig {
            cloud_backend_url: backend_url.to_string(),
            cloud_api_token: "fixture-token".to_string(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn parses_cloud_account_usage_and_unlimited_uploads() {
        let snapshot: UsageSnapshot = serde_json::from_str(
            r#"{"email":"a@example.com","tier":"pro","usage":{"uploads":7,"storage_bytes":100,"month_resets_at":"2026-12-01T00:00:00Z"},"limits":{"max_uploads":-1,"max_storage_bytes":1000,"retention_days":90}}"#,
        )
        .unwrap();
        assert_eq!(snapshot.usage.uploads, 7);
        assert_eq!(snapshot.usage.storage_bytes, 100);
        assert_eq!(
            snapshot.usage.month_resets_at.as_deref(),
            Some("2026-12-01T00:00:00Z")
        );
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

    #[test]
    fn successful_get_uses_the_account_endpoint_and_auth_headers() {
        let fixture = serve_once(
            "200 OK",
            r#"{"email":"a@example.com","tier":"pro","usage":{"uploads":3,"storage_bytes":2048},"limits":{"max_uploads":20,"max_storage_bytes":4096,"retention_days":30}}"#,
        );
        let mut config = signed_in_config(&fixture.url);
        let snapshot = fetch(&mut config).expect("fixture snapshot");
        assert_eq!(snapshot.email, "a@example.com");
        assert_eq!(snapshot.usage.uploads, 3);
        assert_eq!(snapshot.usage.storage_bytes, 2048);
        assert_eq!(snapshot.limits.retention_days, 30);

        let head = fixture.request.recv().expect("request recorded");
        let mut lines = head.lines();
        assert_eq!(lines.next(), Some("GET /v1/account HTTP/1.1"));
        assert!(lines
            .clone()
            .any(|line| line.starts_with("Authorization: Bearer ")));
        assert!(lines.any(|line| line == "Accept: application/json"));
    }

    #[test]
    fn expired_token_without_refresh_token_maps_to_expired() {
        let fixture = serve_once("401 Unauthorized", "{}");
        let mut config = signed_in_config(&fixture.url);
        assert!(matches!(fetch(&mut config), Err(UsageError::Expired)));
    }

    #[test]
    fn server_errors_and_unreachable_backends_map_to_unavailable() {
        let fixture = serve_once("500 Internal Server Error", "{}");
        let config = signed_in_config(&fixture.url);
        assert!(matches!(request(&config), Err(UsageError::Unavailable)));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut config = signed_in_config(&format!("http://127.0.0.1:{port}"));
        assert!(matches!(fetch(&mut config), Err(UsageError::Unavailable)));
    }

    #[test]
    fn malformed_json_maps_to_unavailable() {
        let fixture = serve_once("200 OK", "not json");
        let config = signed_in_config(&fixture.url);
        assert!(matches!(request(&config), Err(UsageError::Unavailable)));
    }
}
