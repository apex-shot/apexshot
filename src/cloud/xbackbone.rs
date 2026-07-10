use std::path::Path;

use serde::Deserialize;

use crate::config::AppConfig;

use super::upload::{guess_content_type, UploadError, UploadResult};

// --- 4.x (upcoming, Laravel + Sanctum) ---

#[derive(Debug, Deserialize)]
struct XbV4Response {
    data: XbV4Resource,
}

#[derive(Debug, Deserialize)]
struct XbV4Resource {
    preview_ext_url: Option<String>,
    raw_url: Option<String>,
}

// --- 3.x (current stable 3.8.2, Slim PHP) ---

#[derive(Debug, Deserialize)]
struct XbV3Response {
    message: Option<String>,
    url: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    raw_url: Option<String>,
}

pub(crate) fn is_configured(config: &AppConfig) -> bool {
    !config.xbackbone_url.is_empty() && !config.xbackbone_api_token.is_empty()
}

pub(crate) fn not_configured_notification(_config: &AppConfig) -> (&'static str, &'static str) {
    (
        "XBackBone upload not configured",
        "Set the instance URL and API token in Settings \u{2192} Cloud",
    )
}

pub(crate) fn upload(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    if !is_configured(config) {
        return Err(UploadError::NotConfigured(
            "XBackBone upload not configured. Set the instance URL and API token in Settings."
                .to_string(),
        ));
    }

    // Try the 4.x API first (Bearer auth, /api/v1/upload). On 404 (endpoint
    // doesn't exist), fall back to the 3.x API (token form field, /upload).
    // This makes ApexShot work with both the current stable 3.x and the
    // upcoming 4.x without requiring the user to pick a version.
    match upload_v4(config, path) {
        Ok(result) => Ok(result),
        Err(UploadError::HttpRequest(msg)) if is_not_found(&msg) => upload_v3(config, path),
        Err(e) => Err(e),
    }
}

pub fn test_connection(config: &AppConfig) -> Result<(), String> {
    if config.xbackbone_url.is_empty() || config.xbackbone_api_token.is_empty() {
        return Err("Instance URL and API token are required.".into());
    }

    // Probe the 4.x endpoint. A 404 means the instance is 3.x (or the route
    // doesn't exist), so we fall back to probing the 3.x endpoint.
    match test_connection_v4(config) {
        Ok(()) => Ok(()),
        Err(V4Probe::NotFound) => test_connection_v3(config),
        Err(V4Probe::Error(msg)) => Err(msg),
    }
}

// --- 4.x implementation ---

fn upload_v4(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    let url = config.xbackbone_url.trim_end_matches('/');
    let token = &config.xbackbone_api_token;

    let file_bytes = std::fs::read(path).map_err(|e| UploadError::FileRead(e.to_string()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();
    let content_type = guess_content_type(&filename);

    let boundary = boundary();
    let body = build_multipart(&boundary, "file", &filename, &content_type, &file_bytes);

    let endpoint = format!("{url}/api/v1/upload");
    let send_result = ureq::post(&endpoint)
        .set("Authorization", &format!("Bearer {token}"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body);

    // A 404 on the 4.x route means the instance is 3.x (the route does not
    // exist). Surface it as a marker the dispatcher recognises so it can fall
    // back to the 3.x upload path. Any other failure is a real error.
    let resp = match send_result {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            return Err(UploadError::HttpRequest(
                "404 Not Found: instance has no 4.x upload API".to_string(),
            ));
        }
        Err(e) => return Err(map_http_error_v4(e)),
    };

    let parsed: XbV4Response = resp
        .into_json()
        .map_err(|e| UploadError::Server(format!("Invalid response: {e}")))?;

    let share_url = pick_v4_share_url(&parsed).ok_or_else(|| {
        UploadError::Server("Upload response did not include a share URL".to_string())
    })?;

    if share_url.trim().is_empty() {
        return Err(UploadError::Server(
            "Upload response returned an empty share URL".to_string(),
        ));
    }

    Ok(UploadResult { share_url })
}

fn pick_v4_share_url(parsed: &XbV4Response) -> Option<String> {
    let preview = parsed
        .data
        .preview_ext_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(url) = preview {
        return Some(url.to_string());
    }
    parsed
        .data
        .raw_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

enum V4Probe {
    NotFound,
    Error(String),
}

fn test_connection_v4(config: &AppConfig) -> Result<(), V4Probe> {
    let url = config.xbackbone_url.trim_end_matches('/');
    let token = &config.xbackbone_api_token;

    let boundary = boundary();
    let body = format!("--{boundary}--\r\n").into_bytes();
    let result = ureq::post(&format!("{url}/api/v1/upload"))
        .set("Authorization", &format!("Bearer {token}"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body);

    match result {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(404, _)) => Err(V4Probe::NotFound),
        Err(ureq::Error::Status(400, _)) | Err(ureq::Error::Status(422, _)) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err(V4Probe::Error(
            "Token rejected. Check the API token.".into(),
        )),
        Err(ureq::Error::Status(403, _)) => Err(V4Probe::Error(
            "Token lacks the resource:upload ability.".into(),
        )),
        Err(ureq::Error::Status(code, _)) => Err(V4Probe::Error(format!(
            "Unexpected HTTP {code} from instance."
        ))),
        Err(e) => Err(V4Probe::Error(format!("Could not reach instance: {e}"))),
    }
}

fn map_http_error_v4(e: ureq::Error) -> UploadError {
    match e {
        ureq::Error::Status(401, _) => UploadError::AuthExpired(
            "Your XBackBone token was rejected. Update it in Settings \u{2192} Cloud.".to_string(),
        ),
        ureq::Error::Status(413, _) => {
            UploadError::Server("Quota exceeded on the XBackBone instance.".to_string())
        }
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let trimmed = body.trim();
            if trimmed.is_empty() {
                UploadError::Server(format!("HTTP {code}"))
            } else {
                UploadError::Server(format!("HTTP {code}: {trimmed}"))
            }
        }
        e => UploadError::HttpRequest(e.to_string()),
    }
}

// --- 3.x implementation ---

fn upload_v3(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    let url = config.xbackbone_url.trim_end_matches('/');
    let token = &config.xbackbone_api_token;

    let file_bytes = std::fs::read(path).map_err(|e| UploadError::FileRead(e.to_string()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();
    let content_type = guess_content_type(&filename);

    let boundary = boundary();
    let body = build_multipart_v3(
        &boundary,
        "upload",
        &filename,
        &content_type,
        &file_bytes,
        token,
    );

    let endpoint = format!("{url}/upload");
    let resp = ureq::post(&endpoint)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body)
        .map_err(map_http_error_v3)?;

    let parsed: XbV3Response = resp
        .into_json()
        .map_err(|e| UploadError::Server(format!("Invalid response: {e}")))?;

    let share_url = match parsed
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(u) => u.to_string(),
        None => {
            return Err(UploadError::Server(format!(
                "Upload response did not include a share URL: {}",
                parsed.message.unwrap_or_default()
            )));
        }
    };

    Ok(UploadResult { share_url })
}

fn test_connection_v3(config: &AppConfig) -> Result<(), String> {
    let url = config.xbackbone_url.trim_end_matches('/');
    let token = &config.xbackbone_api_token;

    // Send an empty-file probe. A healthy 3.x instance with a valid token
    // returns 400 "Request without file attached." — that confirms the URL
    // is correct and the token is valid. A 404 means the URL is wrong.
    let boundary = boundary();
    let body = build_multipart_v3(
        &boundary,
        "file",
        "probe",
        "application/octet-stream",
        &[],
        token,
    );

    let result = ureq::post(&format!("{url}/upload"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body);

    match result {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(400, _)) => Ok(()), // "Request without file attached."
        Err(ureq::Error::Status(404, resp)) => {
            // XBackBone 3.x returns 404 both when the token is rejected
            // (`{message: "Token not found."}`) and when the route is absent.
            // Distinguish them so the user gets an actionable message.
            match parse_v3_message(resp).as_deref() {
                Some(m) if m.to_lowercase().contains("token") => {
                    Err("Token rejected. Check the API token.".into())
                }
                _ => Err("Upload endpoint not found. Check the instance URL.".into()),
            }
        }
        Err(ureq::Error::Status(401, _)) => Err("Account disabled on the instance.".into()),
        Err(ureq::Error::Status(503, _)) => Err("Instance is under maintenance.".into()),
        Err(ureq::Error::Status(507, _)) => Err("Disk quota exceeded on the instance.".into()),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = body.trim();
            if msg.is_empty() {
                Err(format!("Unexpected HTTP {code} from instance."))
            } else {
                Err(format!("Unexpected HTTP {code}: {msg}"))
            }
        }
        Err(e) => Err(format!("Could not reach instance: {e}")),
    }
}

fn map_http_error_v3(e: ureq::Error) -> UploadError {
    match e {
        ureq::Error::Status(404, resp) => {
            let msg = parse_v3_message(resp);
            UploadError::AuthExpired(format!(
                "XBackBone rejected the token.{}",
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            ))
        }
        ureq::Error::Status(401, resp) => {
            let msg = parse_v3_message(resp);
            UploadError::Server(format!(
                "Account disabled on the XBackBone instance.{}",
                msg.map(|m| format!(" ({m})")).unwrap_or_default()
            ))
        }
        ureq::Error::Status(507, _) => {
            UploadError::Server("Disk quota exceeded on the XBackBone instance.".to_string())
        }
        ureq::Error::Status(503, _) => {
            UploadError::Server("XBackBone instance is under maintenance.".to_string())
        }
        ureq::Error::Status(code, resp) => {
            let msg = parse_v3_message(resp);
            match msg {
                Some(m) => UploadError::Server(format!("HTTP {code}: {m}")),
                None => UploadError::Server(format!("HTTP {code}")),
            }
        }
        e => UploadError::HttpRequest(e.to_string()),
    }
}

fn parse_v3_message(resp: ureq::Response) -> Option<String> {
    let parsed: XbV3Response = resp.into_json().ok()?;
    parsed.message.filter(|m| !m.is_empty())
}

// --- shared helpers ---

fn is_not_found(http_error_message: &str) -> bool {
    http_error_message.contains("404")
}

fn boundary() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("apexshot-xb-{nanos:x}")
}

/// Build a multipart body with a single file part (4.x).
fn build_multipart(
    boundary: &str,
    field_name: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"");
    body.extend_from_slice(field_name.as_bytes());
    body.extend_from_slice(b"\"; filename=\"");
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\n");
    body.extend_from_slice(b"Content-Type: ");
    body.extend_from_slice(content_type.as_bytes());
    body.extend_from_slice(b"\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    body
}

/// Build a multipart body with a file part plus a `token` form field (3.x).
fn build_multipart_v3(
    boundary: &str,
    field_name: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
    token: &str,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + token.len() + 640);

    // token form field
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"token\"\r\n\r\n");
    body.extend_from_slice(token.as_bytes());
    body.extend_from_slice(b"\r\n");

    // file field
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"");
    body.extend_from_slice(field_name.as_bytes());
    body.extend_from_slice(b"\"; filename=\"");
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\n");
    body.extend_from_slice(b"Content-Type: ");
    body.extend_from_slice(content_type.as_bytes());
    body.extend_from_slice(b"\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- multipart construction ---

    #[test]
    fn build_multipart_wraps_file_part_with_boundary() {
        let boundary = "apexshot-xb-test";
        let body = build_multipart(boundary, "file", "shot.png", "image/png", &[1, 2, 3]);

        let s = String::from_utf8_lossy(&body);
        assert!(s.starts_with("--apexshot-xb-test\r\n"));
        assert!(s.contains("Content-Disposition: form-data; name=\"file\"; filename=\"shot.png\""));
        assert!(s.contains("Content-Type: image/png\r\n\r\n"));
        assert!(s.contains("\u{1}\u{2}\u{3}"));
        assert!(s.ends_with("--apexshot-xb-test--\r\n"));
    }

    #[test]
    fn build_multipart_preserves_binary_bytes() {
        let bytes: Vec<u8> = vec![0u8, 255, 10, 13, 128, 0, 1];
        let body = build_multipart("b", "f.bin", "f", "application/octet-stream", &bytes);
        let header_end = body
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("header terminator present");
        let payload_start = header_end + 4;
        let payload_end = payload_start + bytes.len();
        assert_eq!(&body[payload_start..payload_end], bytes.as_slice());
    }

    #[test]
    fn build_multipart_v3_includes_token_field() {
        let boundary = "apexshot-xb-test";
        let body = build_multipart_v3(
            boundary,
            "file",
            "shot.png",
            "image/png",
            &[1, 2, 3],
            "my-secret-token",
        );

        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("name=\"token\""));
        assert!(s.contains("my-secret-token"));
        assert!(s.contains("name=\"file\"; filename=\"shot.png\""));
        assert!(s.contains("Content-Type: image/png"));
        assert!(s.ends_with("--apexshot-xb-test--\r\n"));
    }

    // --- 4.x response parsing ---

    #[test]
    fn pick_v4_share_url_prefers_preview_ext_url() {
        let parsed = XbV4Response {
            data: XbV4Resource {
                preview_ext_url: Some("https://xb.example/p/abc".to_string()),
                raw_url: Some("https://xb.example/r/abc.png".to_string()),
            },
        };
        assert_eq!(
            pick_v4_share_url(&parsed).unwrap(),
            "https://xb.example/p/abc"
        );
    }

    #[test]
    fn pick_v4_share_url_falls_back_to_raw_url() {
        let parsed = XbV4Response {
            data: XbV4Resource {
                preview_ext_url: None,
                raw_url: Some("https://xb.example/r/abc.png".to_string()),
            },
        };
        assert_eq!(
            pick_v4_share_url(&parsed).unwrap(),
            "https://xb.example/r/abc.png"
        );
    }

    #[test]
    fn pick_v4_share_url_ignores_empty_preview_ext_url() {
        let parsed = XbV4Response {
            data: XbV4Resource {
                preview_ext_url: Some("   ".to_string()),
                raw_url: Some("https://xb.example/r/abc.png".to_string()),
            },
        };
        assert_eq!(
            pick_v4_share_url(&parsed).unwrap(),
            "https://xb.example/r/abc.png"
        );
    }

    #[test]
    fn pick_v4_share_url_returns_none_when_both_missing() {
        let parsed = XbV4Response {
            data: XbV4Resource {
                preview_ext_url: None,
                raw_url: None,
            },
        };
        assert!(pick_v4_share_url(&parsed).is_none());
    }

    #[test]
    fn parse_v4_response_with_preview_and_raw() {
        let raw = r#"{
            "data": {
                "id": 1,
                "preview_ext_url": "https://xb.example/p/abc",
                "raw_url": "https://xb.example/r/abc.png",
                "deletion_url": "https://xb.example/d/abc?token=x"
            }
        }"#;
        let parsed: XbV4Response = serde_json::from_str(raw).unwrap();
        assert_eq!(
            parsed.data.preview_ext_url.unwrap(),
            "https://xb.example/p/abc"
        );
        assert_eq!(parsed.data.raw_url.unwrap(), "https://xb.example/r/abc.png");
    }

    // --- 3.x response parsing ---

    #[test]
    fn parse_v3_response_success() {
        let raw = r#"{
            "message": "OK",
            "version": "3.8.2",
            "url": "https://xb.example/user/abc.png",
            "raw_url": "https://xb.example/user/abc/raw.png"
        }"#;
        let parsed: XbV3Response = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.message.unwrap(), "OK");
        assert_eq!(parsed.url.unwrap(), "https://xb.example/user/abc.png");
        assert_eq!(
            parsed.raw_url.unwrap(),
            "https://xb.example/user/abc/raw.png"
        );
    }

    #[test]
    fn parse_v3_response_error() {
        let raw = r#"{
            "message": "Token not found.",
            "version": "3.8.2"
        }"#;
        let parsed: XbV3Response = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.message.unwrap(), "Token not found.");
        assert!(parsed.url.is_none());
        assert!(parsed.raw_url.is_none());
    }

    // --- error mapping ---

    fn make_status_error(code: u16, body: &str) -> ureq::Error {
        ureq::is_test(true);
        let resp = ureq::Response::new(code, "Status", body).expect("test response");
        ureq::Error::Status(code, resp)
    }

    #[test]
    fn map_v4_error_401_is_auth_expired() {
        let err = map_http_error_v4(make_status_error(401, "Unauthorized"));
        assert!(matches!(err, UploadError::AuthExpired(_)));
        assert!(err.to_string().contains("token was rejected"));
    }

    #[test]
    fn map_v4_error_413_is_quota() {
        let err = map_http_error_v4(make_status_error(413, "Too Large"));
        assert!(matches!(err, UploadError::Server(_)));
        assert!(err.to_string().contains("Quota exceeded"));
    }

    #[test]
    fn map_v4_error_422_includes_body() {
        let err = map_http_error_v4(make_status_error(
            422,
            r#"{"message":"The file field is required."}"#,
        ));
        assert!(matches!(err, UploadError::Server(_)));
        assert!(err.to_string().contains("The file field is required."));
    }

    #[test]
    fn map_v3_error_404_is_auth_expired() {
        let err = map_http_error_v3(make_status_error(
            404,
            r#"{"message":"Token not found.","version":"3.8.2"}"#,
        ));
        assert!(matches!(err, UploadError::AuthExpired(_)));
        assert!(err.to_string().contains("rejected the token"));
        assert!(err.to_string().contains("Token not found."));
    }

    #[test]
    fn map_v3_error_507_is_quota() {
        let err = map_http_error_v3(make_status_error(
            507,
            r#"{"message":"User disk quota exceeded.","version":"3.8.2"}"#,
        ));
        assert!(matches!(err, UploadError::Server(_)));
        assert!(err.to_string().contains("quota exceeded"));
    }

    // --- is_not_found ---

    #[test]
    fn is_not_found_detects_404_in_message() {
        assert!(is_not_found(
            "Request to /api/v1/upload failed with status 404 Not Found"
        ));
        assert!(!is_not_found("Request failed with status 500"));
    }
}
