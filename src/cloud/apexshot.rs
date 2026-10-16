use std::path::Path;

use serde::Deserialize;

use crate::config::{resolve_cloud_backend_url, save_config, AppConfig};

use super::upload::{guess_content_type, UploadError, UploadResult};

#[derive(Debug, Deserialize)]
struct CreateUploadResponse {
    id: String,
    #[serde(alias = "shareUrl")]
    share_url: String,
    #[serde(alias = "uploadUrl")]
    upload_url: String,
    #[serde(default)]
    upload_protocol_version: Option<u16>,
    #[serde(default)]
    protected_upload: Option<bool>,
    #[serde(default)]
    upload_mode: Option<String>,
    #[serde(default)]
    upload_headers: Option<UploadHeaders>,
}

#[derive(Debug, Deserialize)]
struct UploadHeaders {
    #[serde(rename = "If-Match")]
    if_match: String,
    #[serde(rename = "x-amz-meta-apexshot-upload-token")]
    upload_token: String,
}

#[derive(Debug, Deserialize)]
struct CompleteUploadResponse {
    success: bool,
    status: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: String,
    #[serde(default)]
    code: Option<String>,
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
        "upload_protocol_version": 2,
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

    let is_local_upload = require_protected_upload_session(&session, &backend_url)?;

    let mut put_request = ureq::put(&session.upload_url).set("Content-Type", &content_type);
    if is_local_upload {
        put_request = put_request.set("Authorization", &format!("Bearer {token}"));
    } else if let Some(headers) = session.upload_headers.as_ref() {
        put_request = put_request
            .set("If-Match", &headers.if_match)
            .set("x-amz-meta-apexshot-upload-token", &headers.upload_token);
    }
    match put_request.send_bytes(&file_bytes) {
        Ok(_) => match complete_upload_session(&backend_url, token, &session.id) {
            Ok(true) => {}
            Ok(false) => {
                let _ = delete_upload_session(&backend_url, token, &session.id);
                return Err(UploadError::Server(
                    "Cloud did not confirm that the upload completed".into(),
                ));
            }
            Err(error) => return Err(UploadError::Server(error)),
        },
        Err(upload_error) => match complete_upload_session(&backend_url, token, &session.id) {
            Ok(true) => {}
            Ok(false) => {
                let _ = delete_upload_session(&backend_url, token, &session.id);
                return Err(map_upload_http_error(upload_error));
            }
            Err(_) => return Err(map_upload_http_error(upload_error)),
        },
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
                .filter(|body| !body.error.trim().is_empty());

            match detail {
                Some(body) => {
                    let message = body.error.trim();
                    let detail = format!("HTTP {code}: {message}");
                    if body.code.as_deref() == Some("client_update_required") {
                        return UploadError::Server(message.to_string());
                    }
                    if code == 400
                        && (body.code.as_deref() == Some("upgrade_required")
                            || message.to_ascii_lowercase().contains("upgrade to pro"))
                    {
                        UploadError::UpgradeRequired(detail)
                    } else {
                        UploadError::HttpRequest(detail)
                    }
                }
                None => UploadError::HttpRequest(format!("HTTP {code}")),
            }
        }
        ureq::Error::Transport(transport) => UploadError::HttpRequest(transport.to_string()),
    }
}

fn require_protected_upload_session(
    session: &CreateUploadResponse,
    backend_url: &str,
) -> Result<bool, UploadError> {
    if session.upload_protocol_version != Some(2) || session.protected_upload != Some(true) {
        return Err(UploadError::Server(
            "This Cloud service does not confirm protected uploads. Update the service and app before retrying.".into(),
        ));
    }

    match session.upload_mode.as_deref() {
        Some("local") if is_backend_local_upload_url(&session.upload_url, backend_url) => {
            Ok(true)
        }
        Some("r2")
            if !is_backend_local_upload_url(&session.upload_url, backend_url)
                && session.upload_headers.as_ref().is_some_and(|headers| {
                    !headers.if_match.is_empty() && !headers.upload_token.is_empty()
                }) =>
        {
            Ok(false)
        }
        _ => Err(UploadError::Server(
            "The Cloud service returned an invalid protected upload session. Update the service and app before retrying.".into(),
        )),
    }
}

fn complete_upload_session(
    backend_url: &str,
    token: &str,
    upload_id: &str,
) -> Result<bool, String> {
    let response = match ureq::post(&format!("{backend_url}/v1/uploads/{upload_id}"))
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string("{}")
    {
        Ok(response) => response,
        Err(ureq::Error::Status(409, _)) => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let result: CompleteUploadResponse = response.into_json().map_err(|e| e.to_string())?;
    Ok(result.success && result.status == "ready")
}

fn delete_upload_session(backend_url: &str, token: &str, upload_id: &str) -> Result<(), String> {
    ureq::delete(&format!("{backend_url}/v1/uploads/{upload_id}"))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .map(|_| ())
        .map_err(|error| error.to_string())
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

fn is_backend_local_upload_url(upload_url: &str, backend_url: &str) -> bool {
    let (Ok(upload), Ok(backend)) = (url::Url::parse(upload_url), url::Url::parse(backend_url))
    else {
        return false;
    };

    if upload.scheme() != backend.scheme()
        || upload.host_str() != backend.host_str()
        || upload.port_or_known_default() != backend.port_or_known_default()
        || !upload.username().is_empty()
        || upload.password().is_some()
        || upload.query().is_some()
        || upload.fragment().is_some()
    {
        return false;
    }

    let backend_path = backend.path().trim_end_matches('/');
    let upload_prefix = format!("{backend_path}/v1/uploads/file/");
    let Some(storage_key) = upload.path().strip_prefix(&upload_prefix) else {
        return false;
    };
    let parts = storage_key.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && parts[0] == "users"
        && parts[1].len() == 36
        && parts[1]
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        && !parts[2].is_empty()
        && parts[2]
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
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
    fn only_authenticates_same_origin_backend_upload_routes() {
        assert!(is_backend_local_upload_url(
            "https://api.apexshot.org/v1/uploads/file/users/123e4567-e89b-12d3-a456-426614174000/ab12-file.png",
            "https://api.apexshot.org",
        ));
        assert!(!is_backend_local_upload_url(
            "https://storage.example/users/123e4567-e89b-12d3-a456-426614174000/ab12-file.png?X-Amz-Signature=opaque",
            "https://api.apexshot.org",
        ));
        assert!(!is_backend_local_upload_url(
            "https://api.apexshot.org/v1/uploads/file/users/123e4567-e89b-12d3-a456-426614174000/ab12-file.png",
            "https://api.apexshot.org/api",
        ));
        assert!(!is_backend_local_upload_url(
            "https://api.apexshot.org/v1/uploads/file/users/123e4567-e89b-12d3-a456-426614174000/ab12-file.png?next=https://elsewhere.example",
            "https://api.apexshot.org",
        ));
    }

    #[test]
    fn accepts_camel_case_create_response_fields() {
        let response: CreateUploadResponse = serde_json::from_str(
            r#"{
                "id": "123e4567-e89b-12d3-a456-426614174000",
                "shareUrl": "https://apexshot.org/s/7t1NE9mTWw9J",
                "uploadUrl": "https://storage.example/upload",
                "upload_protocol_version": 2,
                "protected_upload": true,
                "upload_mode": "r2",
                "upload_headers": {
                    "If-Match": "\"slot-etag\"",
                    "x-amz-meta-apexshot-upload-token": "session-token"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(response.upload_protocol_version, Some(2));
        assert_eq!(response.protected_upload, Some(true));
        assert_eq!(response.upload_mode.as_deref(), Some("r2"));
        assert_eq!(
            response.upload_headers.as_ref().unwrap().if_match,
            "\"slot-etag\""
        );
        assert_eq!(response.share_url, "https://apexshot.org/s/7t1NE9mTWw9J");
        assert_eq!(response.upload_url, "https://storage.example/upload");
    }

    #[test]
    fn refuses_unprotected_responses_from_older_cloud_services() {
        let old_response: CreateUploadResponse = serde_json::from_str(
            r#"{
                "id": "123e4567-e89b-12d3-a456-426614174000",
                "share_url": "https://apexshot.org/s/7t1NE9mTWw9J",
                "upload_url": "https://storage.example/upload"
            }"#,
        )
        .unwrap();

        let error = require_protected_upload_session(&old_response, "https://api.apexshot.org")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("does not confirm protected uploads"));
    }

    #[test]
    fn requires_conditional_headers_for_r2_and_local_backend_url_for_fallback() {
        let r2_without_headers = CreateUploadResponse {
            id: "id".into(),
            share_url: "https://apexshot.org/s/id".into(),
            upload_url: "https://storage.example/upload".into(),
            upload_protocol_version: Some(2),
            protected_upload: Some(true),
            upload_mode: Some("r2".into()),
            upload_headers: None,
        };
        assert!(
            require_protected_upload_session(&r2_without_headers, "https://api.apexshot.org")
                .is_err()
        );

        let local = CreateUploadResponse {
            id: "id".into(),
            share_url: "https://apexshot.org/s/id".into(),
            upload_url: "https://api.apexshot.org/v1/uploads/file/users/123e4567-e89b-12d3-a456-426614174000/42-file.png".into(),
            upload_protocol_version: Some(2),
            protected_upload: Some(true),
            upload_mode: Some("local".into()),
            upload_headers: None,
        };
        assert!(require_protected_upload_session(&local, "https://api.apexshot.org").unwrap());
    }

    #[test]
    fn client_update_error_is_not_mapped_to_a_pro_plan_upgrade() {
        let error = map_upload_http_error(make_status_error(
            426,
            r#"{"code":"client_update_required","error":"Update ApexShot to upload securely."}"#,
        ));

        assert!(
            matches!(error, UploadError::Server(message) if message.contains("Update ApexShot"))
        );
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
    fn identifies_structured_upgrade_response() {
        let error = map_upload_http_error(make_status_error(
            400,
            r#"{"error":"Monthly upload limit reached","code":"upgrade_required"}"#,
        ));

        assert!(matches!(error, UploadError::UpgradeRequired(_)));
    }

    #[test]
    fn identifies_legacy_free_storage_upgrade_response() {
        let error = map_upload_http_error(make_status_error(
            400,
            r#"{"error":"Free plan storage is limited to 2 GB total. Delete files or upgrade to Pro to upload more."}"#,
        ));

        assert!(matches!(error, UploadError::UpgradeRequired(_)));
    }

    #[test]
    fn does_not_offer_upgrade_for_paid_quota_or_provider_errors() {
        for (status, body) in [
            (
                400,
                r#"{"error":"Pro plan storage is limited to 100 GB total. Delete files before uploading more."}"#,
            ),
            (
                500,
                r#"{"error":"Upgrade to Pro","code":"upgrade_required"}"#,
            ),
        ] {
            let error = map_upload_http_error(make_status_error(status, body));
            assert!(matches!(error, UploadError::HttpRequest(_)));
        }
    }

    #[test]
    fn falls_back_to_status_for_invalid_error_response() {
        let error = map_upload_http_error(make_status_error(500, "upstream HTML response"));

        assert_eq!(error.to_string(), "Upload request failed: HTTP 500");
    }
}
