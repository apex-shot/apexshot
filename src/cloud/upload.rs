use std::path::Path;

use serde::Deserialize;

use crate::config::{save_config, AppConfig};

#[derive(Debug)]
pub struct UploadResult {
    pub share_url: String,
}

#[derive(Debug, Deserialize)]
struct CreateUploadResponse {
    share_url: String,
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[allow(dead_code)]
    refresh_token: String,
}

#[derive(Debug)]
pub enum UploadError {
    NotConfigured,
    FileRead(String),
    HttpRequest(String),
    Server(String),
    AuthExpired,
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::NotConfigured => write!(f, "Cloud upload not configured. Run `apexshot login` to connect."),
            UploadError::FileRead(msg) => write!(f, "Failed to read file: {msg}"),
            UploadError::HttpRequest(msg) => write!(f, "Upload request failed: {msg}"),
            UploadError::Server(msg) => write!(f, "Server error: {msg}"),
            UploadError::AuthExpired => write!(f, "Your session has expired. Run `apexshot login` again."),
        }
    }
}

impl std::error::Error for UploadError {}

pub fn is_configured(config: &AppConfig) -> bool {
    !config.cloud_api_token.is_empty() && !config.cloud_backend_url.is_empty()
}

fn refresh_access_token(config: &mut AppConfig) -> Result<String, UploadError> {
    let backend_url = config.cloud_backend_url.trim_end_matches('/');

    let refresh_body = serde_json::json!({ "refresh_token": config.cloud_api_token }).to_string();
    let resp = ureq::post(&format!("{backend_url}/v1/auth/refresh"))
        .set("Content-Type", "application/json")
        .send_string(&refresh_body)
        .map_err(|e| UploadError::HttpRequest(e.to_string()))?;

    let tokens: RefreshResponse = resp
        .into_json()
        .map_err(|e| UploadError::Server(format!("Invalid refresh response: {e}")))?;

    config.cloud_api_token = tokens.access_token;
    save_config(config).map_err(|e| UploadError::Server(format!("Failed to save config: {e}")))?;

    Ok(config.cloud_api_token.clone())
}

pub fn upload_file(
    config: &AppConfig,
    path: &Path,
) -> Result<UploadResult, UploadError> {
    if !is_configured(config) {
        return Err(UploadError::NotConfigured);
    }

    let mut config = config.clone();
    let result = upload_file_with_token(&config, path);
    if is_auth_error(&result) {
        let _ = refresh_access_token(&mut config);
        return upload_file_with_token(&config, path);
    }
    result
}

fn is_auth_error(result: &Result<UploadResult, UploadError>) -> bool {
    matches!(result, Err(UploadError::HttpRequest(msg)) if msg.contains("401") || msg.contains("403"))
}

fn upload_file_with_token(
    config: &AppConfig,
    path: &Path,
) -> Result<UploadResult, UploadError> {
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
        .set("Authorization", &format!("Bearer {token}"))
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
        share_url: session.share_url,
    })
}

fn guess_content_type(filename: &str) -> String {
    let ext = filename.rsplit('.').next().map(|e| e.to_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("webp") => "image/webp".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("mp4") => "video/mp4".to_string(),
        Some("webm") => "video/webm".to_string(),
        Some("mov") => "video/quicktime".to_string(),
        Some("mkv") => "video/x-matroska".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}
