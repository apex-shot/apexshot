use std::path::Path;

use serde::Deserialize;

use crate::config::{resolve_cloud_backend_url, save_config, AppConfig};

use super::upload::{guess_content_type, UploadError, UploadResult};

#[derive(Debug, Deserialize)]
struct CreateUploadResponse {
    #[serde(alias = "shareUrl")]
    share_url: String,
    #[serde(alias = "uploadUrl")]
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: String,
}

/// Why an access-token refresh failed. Uploads discard this (they retry
/// regardless), while the read client maps it onto its own error surface.
#[derive(Debug)]
pub(crate) enum RefreshError {
    NoRefreshToken,
    /// The server refused the refresh token — re-login is the only way out.
    Rejected(String),
    Network(String),
    Server(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::NoRefreshToken => write!(f, "no refresh token stored"),
            RefreshError::Rejected(msg) => write!(f, "refresh token rejected: {msg}"),
            RefreshError::Network(msg) => write!(f, "refresh request failed: {msg}"),
            RefreshError::Server(msg) => write!(f, "{msg}"),
        }
    }
}

pub(crate) fn is_configured(config: &AppConfig) -> bool {
    // Token is the real session. Backend URL always resolves to the public
    // default when unset, so end-user installs work without a .env file.
    !config.cloud_api_token.is_empty() && !resolve_cloud_backend_url(config).is_empty()
}

pub(crate) fn not_configured_notification(_config: &AppConfig) -> (String, String) {
    (
        crate::i18n::t("Cloud upload not configured"),
        crate::i18n::t("Connect ApexShot Cloud in Settings → Cloud, or switch destination to XBackBone and configure it."),
    )
}

pub(crate) fn upload(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    if !is_configured(config) {
        return Err(UploadError::NotConfigured(
            "Cloud upload not configured. Run `apexshot login` to connect.".to_string(),
        ));
    }

    let mut config = config.clone();
    let result = upload_file_with_token(&config, path);
    if is_auth_error(&result) {
        if config.cloud_refresh_token.is_empty() {
            return Err(UploadError::AuthExpired(
                "Your ApexShot Cloud session has expired. Run `apexshot login` again.".to_string(),
            ));
        }
        let _ = refresh_access_token(&mut config);
        return upload_file_with_token(&config, path);
    }
    result
}

fn is_auth_error(result: &Result<UploadResult, UploadError>) -> bool {
    matches!(result, Err(UploadError::HttpRequest(msg)) if msg.contains("401") || msg.contains("403"))
}

/// Exchange the stored refresh token for a fresh access token and persist both.
///
/// Shared by the upload path and the read client so there is one refresh
/// implementation, not one per endpoint.
pub(crate) fn refresh_access_token(config: &mut AppConfig) -> Result<String, RefreshError> {
    if config.cloud_refresh_token.trim().is_empty() {
        return Err(RefreshError::NoRefreshToken);
    }

    let backend_url = resolve_cloud_backend_url(config);

    let refresh_body =
        serde_json::json!({ "refresh_token": config.cloud_refresh_token }).to_string();
    let resp = ureq::post(&format!("{backend_url}/v1/auth/refresh"))
        .set("Content-Type", "application/json")
        .send_string(&refresh_body)
        .map_err(map_refresh_http_error)?;

    let tokens: RefreshResponse = resp
        .into_json()
        .map_err(|e| RefreshError::Server(format!("Invalid refresh response: {e}")))?;

    config.cloud_api_token = tokens.access_token;
    config.cloud_refresh_token = tokens.refresh_token;
    save_config(config).map_err(|e| RefreshError::Server(format!("Failed to save config: {e}")))?;

    Ok(config.cloud_api_token.clone())
}

fn map_refresh_http_error(error: ureq::Error) -> RefreshError {
    match error {
        // Any 4xx on the refresh endpoint means this token pair is spent.
        ureq::Error::Status(code, _) if (400..500).contains(&code) => {
            RefreshError::Rejected(format!("HTTP {code}"))
        }
        ureq::Error::Status(code, _) => RefreshError::Server(format!("HTTP {code}")),
        ureq::Error::Transport(transport) => RefreshError::Network(transport.to_string()),
    }
}

fn upload_file_with_token(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    let backend_url = resolve_cloud_backend_url(config);
    let token = &config.cloud_api_token;

    let file_bytes = std::fs::read(path).map_err(|e| UploadError::FileRead(e.to_string()))?;
    let size_bytes = file_bytes.len() as i64;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();

    let content_type = guess_content_type(&filename);

    let create_body = serde_json::json!({
        "filename": filename,
        "size_bytes": size_bytes,
        "content_type": content_type,
        "visibility": "public",
        "source": "desktop",
    });

    let create_body_str = create_body.to_string();
    let create_resp = ureq::post(&format!("{backend_url}/v1/uploads"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&create_body_str)
        .map_err(map_upload_http_error)?;

    let session: CreateUploadResponse = create_resp
        .into_json()
        .map_err(|e| UploadError::Server(format!("Invalid response: {e}")))?;

    let put_resp = ureq::put(&session.upload_url)
        .set("Content-Type", &content_type)
        .send_bytes(&file_bytes)
        .map_err(|e| UploadError::HttpRequest(e.to_string()))?;

    if put_resp.status() >= 400 {
        return Err(UploadError::Server(format!(
            "Upload failed with status {}",
            put_resp.status()
        )));
    }

    Ok(UploadResult {
        share_url: normalize_share_url(&session.share_url, &backend_url)?,
    })
}

fn map_upload_http_error(error: ureq::Error) -> UploadError {
    match error {
        ureq::Error::Status(code, response) => {
            let detail = response
                .into_string()
                .ok()
                .and_then(|body| serde_json::from_str::<ApiErrorResponse>(&body).ok())
                .map(|body| body.error.trim().to_string())
                .filter(|message| !message.is_empty());

            match detail {
                Some(message) => UploadError::HttpRequest(format!("HTTP {code}: {message}")),
                None => UploadError::HttpRequest(format!("HTTP {code}")),
            }
        }
        ureq::Error::Transport(transport) => UploadError::HttpRequest(transport.to_string()),
    }
}

fn normalize_share_url(raw_share_url: &str, backend_url: &str) -> Result<String, UploadError> {
    let raw_share_url = raw_share_url.trim();
    if raw_share_url.is_empty() {
        return Err(UploadError::Server(
            "Upload response did not include a share URL".to_string(),
        ));
    }

    if let Ok(url) = url::Url::parse(raw_share_url) {
        return validate_web_share_url(url);
    }

    let mut origin = url::Url::parse(backend_url)
        .map_err(|e| UploadError::Server(format!("Invalid cloud backend URL: {e}")))?;
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);

    let path = if raw_share_url.starts_with('/') {
        raw_share_url.to_string()
    } else {
        format!("/{raw_share_url}")
    };
    let url = origin
        .join(&path)
        .map_err(|e| UploadError::Server(format!("Invalid share URL: {e}")))?;

    validate_web_share_url(url)
}

fn validate_web_share_url(url: url::Url) -> Result<String, UploadError> {
    match url.scheme() {
        "http" | "https" => Ok(url.to_string()),
        _ => Err(UploadError::Server(
            "Upload response returned a non-web share URL".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_status_error(code: u16, body: &str) -> ureq::Error {
        ureq::is_test(true);
        let response = ureq::Response::new(code, "Status", body).expect("test response");
        ureq::Error::Status(code, response)
    }

    #[test]
    fn keeps_absolute_web_share_url() {
        let url = normalize_share_url(
            "https://apexshot.org/s/7t1NE9mTWw9J",
            "https://apexshot.org/api",
        )
        .unwrap();

        assert_eq!(url, "https://apexshot.org/s/7t1NE9mTWw9J");
    }

    #[test]
    fn expands_absolute_path_share_url_from_backend_origin() {
        let url = normalize_share_url("/s/7t1NE9mTWw9J", "https://apexshot.org/api").unwrap();

        assert_eq!(url, "https://apexshot.org/s/7t1NE9mTWw9J");
    }

    #[test]
    fn expands_relative_path_share_url_from_backend_origin() {
        let url = normalize_share_url("s/7t1NE9mTWw9J", "https://apexshot.org/api").unwrap();

        assert_eq!(url, "https://apexshot.org/s/7t1NE9mTWw9J");
    }

    #[test]
    fn rejects_local_file_share_url() {
        let err = normalize_share_url(
            "file:///home/codegoddy/Pictures/ApexShot2026-06-28_17-39-42.png",
            "https://apexshot.org/api",
        )
        .unwrap_err();

        assert!(err.to_string().contains("non-web share URL"));
    }

    #[test]
    fn accepts_camel_case_create_response_fields() {
        let response: CreateUploadResponse = serde_json::from_str(
            r#"{
                "shareUrl": "https://apexshot.org/s/7t1NE9mTWw9J",
                "uploadUrl": "https://storage.example/upload"
            }"#,
        )
        .unwrap();

        assert_eq!(response.share_url, "https://apexshot.org/s/7t1NE9mTWw9J");
        assert_eq!(response.upload_url, "https://storage.example/upload");
    }

    #[test]
    fn surfaces_cloud_quota_error_message() {
        let error = map_upload_http_error(make_status_error(
            400,
            r#"{"error":"Free plan uploads are limited to 20 files per month. Upgrade to Pro to remove the monthly cap."}"#,
        ));

        assert_eq!(
            error.to_string(),
            "Upload request failed: HTTP 400: Free plan uploads are limited to 20 files per month. Upgrade to Pro to remove the monthly cap."
        );
    }

    #[test]
    fn keeps_auth_status_visible_for_token_refresh() {
        let result = Err(map_upload_http_error(make_status_error(
            401,
            r#"{"error":"Token expired"}"#,
        )));

        assert!(is_auth_error(&result));
    }

    #[test]
    fn falls_back_to_status_for_invalid_error_response() {
        let error = map_upload_http_error(make_status_error(500, "upstream HTML response"));

        assert_eq!(error.to_string(), "Upload request failed: HTTP 500");
    }
}
