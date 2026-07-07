use std::path::Path;

use serde::Deserialize;

use crate::config::{save_config, AppConfig};

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

pub(crate) fn is_configured(config: &AppConfig) -> bool {
    !config.cloud_api_token.is_empty() && !config.cloud_backend_url.is_empty()
}

pub(crate) fn not_configured_notification(_config: &AppConfig) -> (&'static str, &'static str) {
    (
        "Cloud upload not configured",
        "Run `apexshot login` to connect your account",
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

fn refresh_access_token(config: &mut AppConfig) -> Result<String, UploadError> {
    let backend_url = config.cloud_backend_url.trim_end_matches('/');

    let refresh_body =
        serde_json::json!({ "refresh_token": config.cloud_refresh_token }).to_string();
    let resp = ureq::post(&format!("{backend_url}/v1/auth/refresh"))
        .set("Content-Type", "application/json")
        .send_string(&refresh_body)
        .map_err(|e| UploadError::HttpRequest(e.to_string()))?;

    let tokens: RefreshResponse = resp
        .into_json()
        .map_err(|e| UploadError::Server(format!("Invalid refresh response: {e}")))?;

    config.cloud_api_token = tokens.access_token;
    config.cloud_refresh_token = tokens.refresh_token;
    save_config(config).map_err(|e| UploadError::Server(format!("Failed to save config: {e}")))?;

    Ok(config.cloud_api_token.clone())
}

fn upload_file_with_token(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    let backend_url = config.cloud_backend_url.trim_end_matches('/');
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
        .map_err(|e| UploadError::HttpRequest(e.to_string()))?;

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
        share_url: normalize_share_url(&session.share_url, backend_url)?,
    })
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
}
