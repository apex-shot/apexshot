use std::path::{Path, PathBuf};

use crate::config::{load_config, AppConfig};

use super::destination::Destination;

#[derive(Debug)]
pub struct UploadResult {
    pub share_url: String,
}

#[derive(Debug)]
pub enum UploadError {
    NotConfigured(String),
    FileRead(String),
    HttpRequest(String),
    Server(String),
    AuthExpired(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::NotConfigured(msg) => write!(f, "{msg}"),
            UploadError::FileRead(msg) => write!(f, "Failed to read file: {msg}"),
            UploadError::HttpRequest(msg) => write!(f, "Upload request failed: {msg}"),
            UploadError::Server(msg) => write!(f, "Server error: {msg}"),
            UploadError::AuthExpired(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UploadError {}

pub fn is_configured(config: &AppConfig) -> bool {
    Destination::from_config(config).is_configured(config)
}

/// True when Settings has auto-upload enabled and the selected destination is ready.
pub fn should_auto_upload_after_capture(config: &AppConfig) -> bool {
    config.cloud_auto_upload_after_capture && is_configured(config)
}

pub fn upload_file(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    Destination::from_config(config).upload(config, path)
}

pub fn not_configured_notification(config: &AppConfig) -> (&'static str, &'static str) {
    Destination::from_config(config).not_configured_notification(config)
}

/// Run an upload and surface the result with desktop notifications + optional
/// share-link clipboard copy. Logs start/success/failure so daemon debug
/// sessions show activity even without RUST_LOG instrumentation.
///
/// `replaces_notification_id` is the server id of a prior "Uploading…" toast
/// (if any) so GNOME replaces it with the final result instead of stacking.
pub fn upload_file_with_notifications(
    config: &AppConfig,
    path: &Path,
) -> Result<UploadResult, UploadError> {
    upload_file_with_notifications_replacing(config, path, 0)
}

fn upload_file_with_notifications_replacing(
    config: &AppConfig,
    path: &Path,
    replaces_notification_id: u32,
) -> Result<UploadResult, UploadError> {
    let dest = Destination::from_config(config);
    let dest_label = match dest {
        Destination::ApexShot => "ApexShot Cloud",
        Destination::XBackbone => "XBackBone",
    };
    eprintln!("[cloud] Uploading {} via {dest_label}…", path.display());

    match upload_file(config, path) {
        Ok(result) => {
            eprintln!("[cloud] Upload complete: {}", result.share_url);
            let mut body = result.share_url.to_string();
            if let Err(e) = crate::utils::clipboard::copy_text_to_clipboard(&result.share_url) {
                eprintln!("[cloud] Failed to copy share link to clipboard: {e}");
            } else {
                body = format!("Copied to clipboard\n{}", result.share_url);
            }
            // Always include the URL in the body so the toast is useful even if
            // the user misses the clipboard.
            let _ = crate::utils::notify::desktop_notification_replace(
                replaces_notification_id,
                "Upload complete",
                &body,
            );
            Ok(result)
        }
        Err(e) => {
            eprintln!("[cloud] Upload failed: {e}");
            let _ = crate::utils::notify::desktop_notification_replace(
                replaces_notification_id,
                "Upload failed",
                &e.to_string(),
            );
            Err(e)
        }
    }
}

/// Background auto-upload after a screenshot is saved.
///
/// Auto-upload off or destination not configured → silent no-op.
/// Destination ready → upload with success/failure notifications.
pub fn spawn_auto_upload_after_capture(path: PathBuf) {
    let config = load_config().sanitized();
    if !should_auto_upload_after_capture(&config) {
        return;
    }

    let dest = Destination::from_config(&config);
    let dest_label = match dest {
        Destination::ApexShot => "ApexShot Cloud",
        Destination::XBackbone => "XBackBone",
    };
    eprintln!(
        "[cloud] Auto-upload after capture: {} via {dest_label}",
        path.display()
    );
    std::thread::spawn(move || {
        let _ = upload_file_with_notifications_replacing(&config, &path, 0);
    });
}

/// True when post-capture auto-upload will place a share URL on the text
/// clipboard, so callers should avoid also writing a `file://` URI there.
pub fn should_defer_text_clipboard_to_share_url(config: &AppConfig) -> bool {
    should_auto_upload_after_capture(config)
}

pub(crate) fn guess_content_type(filename: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_auto_upload_requires_flag_and_destination_config() {
        let mut cfg = AppConfig {
            cloud_auto_upload_after_capture: true,
            cloud_destination: "xbackbone".to_string(),
            xbackbone_url: "https://xb.example".to_string(),
            xbackbone_api_token: "tok".to_string(),
            ..AppConfig::default()
        };
        assert!(should_auto_upload_after_capture(&cfg));

        cfg.cloud_auto_upload_after_capture = false;
        assert!(!should_auto_upload_after_capture(&cfg));

        cfg.cloud_auto_upload_after_capture = true;
        cfg.xbackbone_api_token.clear();
        assert!(!should_auto_upload_after_capture(&cfg));

        // ApexShot Cloud default: auto-upload on, empty token → silent no-op.
        let apex = AppConfig {
            cloud_auto_upload_after_capture: true,
            cloud_destination: "apexshot".to_string(),
            cloud_api_token: String::new(),
            ..AppConfig::default()
        };
        assert!(!should_auto_upload_after_capture(&apex));
    }

    #[test]
    fn guess_content_type_maps_common_image_formats() {
        assert_eq!(guess_content_type("shot.png"), "image/png");
        assert_eq!(guess_content_type("shot.jpg"), "image/jpeg");
        assert_eq!(guess_content_type("shot.jpeg"), "image/jpeg");
        assert_eq!(guess_content_type("shot.webp"), "image/webp");
        assert_eq!(guess_content_type("shot.gif"), "image/gif");
    }

    #[test]
    fn guess_content_type_maps_common_video_formats() {
        assert_eq!(guess_content_type("clip.mp4"), "video/mp4");
        assert_eq!(guess_content_type("clip.webm"), "video/webm");
        assert_eq!(guess_content_type("clip.mov"), "video/quicktime");
        assert_eq!(guess_content_type("clip.mkv"), "video/x-matroska");
    }

    #[test]
    fn guess_content_type_falls_back_to_octet_stream() {
        assert_eq!(guess_content_type("notes.txt"), "application/octet-stream");
        assert_eq!(guess_content_type("noext"), "application/octet-stream");
    }

    #[test]
    fn defer_text_clipboard_only_when_auto_upload_can_run() {
        let mut cfg = AppConfig {
            cloud_auto_upload_after_capture: true,
            cloud_destination: "apexshot".to_string(),
            cloud_api_token: "tok".to_string(),
            cloud_backend_url: "https://api.example".to_string(),
            ..AppConfig::default()
        };
        assert!(should_defer_text_clipboard_to_share_url(&cfg));

        cfg.cloud_api_token.clear();
        assert!(!should_defer_text_clipboard_to_share_url(&cfg));

        cfg.cloud_api_token = "tok".to_string();
        cfg.cloud_auto_upload_after_capture = false;
        assert!(!should_defer_text_clipboard_to_share_url(&cfg));
    }
}
