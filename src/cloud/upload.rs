use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::{load_config, AppConfig};

use super::destination::Destination;

/// Avoid spamming "not configured" toasts on every capture while auto-upload
/// stays enabled after logout / missing destination setup.
const NOT_CONFIGURED_NOTIFY_COOLDOWN: Duration = Duration::from_secs(120);
static LAST_NOT_CONFIGURED_NOTIFY: Mutex<Option<Instant>> = Mutex::new(None);

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

/// True when auto-upload is on but the selected cloud destination has no credentials.
pub fn auto_upload_enabled_but_not_configured(config: &AppConfig) -> bool {
    config.cloud_auto_upload_after_capture && !is_configured(config)
}

fn should_emit_not_configured_notification(now: Instant) -> bool {
    let Ok(mut guard) = LAST_NOT_CONFIGURED_NOTIFY.lock() else {
        return true;
    };
    if let Some(prev) = *guard {
        if now.duration_since(prev) < NOT_CONFIGURED_NOTIFY_COOLDOWN {
            return false;
        }
    }
    *guard = Some(now);
    true
}

/// Notify that auto-upload cannot run until the user connects a destination.
/// Rate-limited so rapid captures do not flood the desktop.
pub fn notify_auto_upload_not_configured(config: &AppConfig) {
    if !should_emit_not_configured_notification(Instant::now()) {
        eprintln!(
            "[cloud] Auto-upload enabled but destination not configured (notification suppressed by cooldown)"
        );
        return;
    }
    let (title, body) = not_configured_notification(config);
    eprintln!("[cloud] Auto-upload enabled but destination not configured — notifying user");
    crate::utils::notify::desktop_notification(title, body);
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
pub fn upload_file_with_notifications(
    config: &AppConfig,
    path: &Path,
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
            if let Err(e) = crate::utils::clipboard::copy_text_to_clipboard(&result.share_url) {
                eprintln!("[cloud] Failed to copy share link to clipboard: {e}");
                crate::utils::notify::desktop_notification(
                    "Upload complete",
                    &format!("Share link: {}", result.share_url),
                );
            } else {
                crate::utils::notify::desktop_notification(
                    "Upload complete",
                    "Share link copied to clipboard",
                );
            }
            Ok(result)
        }
        Err(e) => {
            eprintln!("[cloud] Upload failed: {e}");
            crate::utils::notify::desktop_notification("Upload failed", &e.to_string());
            Err(e)
        }
    }
}

/// Background auto-upload after a screenshot is saved.
///
/// - Auto-upload off → silent no-op
/// - Auto-upload on, destination not configured → desktop notification (rate-limited)
/// - Auto-upload on, destination ready → upload with success/failure notifications
pub fn spawn_auto_upload_after_capture(path: PathBuf) {
    let config = load_config().sanitized();
    if !config.cloud_auto_upload_after_capture {
        return;
    }
    if !is_configured(&config) {
        notify_auto_upload_not_configured(&config);
        return;
    }
    std::thread::spawn(move || {
        let _ = upload_file_with_notifications(&config, &path);
    });
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
        assert!(!auto_upload_enabled_but_not_configured(&cfg));

        cfg.cloud_auto_upload_after_capture = false;
        assert!(!should_auto_upload_after_capture(&cfg));
        assert!(!auto_upload_enabled_but_not_configured(&cfg));

        cfg.cloud_auto_upload_after_capture = true;
        cfg.xbackbone_api_token.clear();
        assert!(!should_auto_upload_after_capture(&cfg));
        assert!(auto_upload_enabled_but_not_configured(&cfg));
    }

    #[test]
    fn not_configured_notify_cooldown_suppresses_rapid_repeats() {
        if let Ok(mut guard) = LAST_NOT_CONFIGURED_NOTIFY.lock() {
            *guard = None;
        }
        let t0 = Instant::now();
        assert!(should_emit_not_configured_notification(t0));
        assert!(!should_emit_not_configured_notification(
            t0 + Duration::from_secs(30)
        ));
        assert!(should_emit_not_configured_notification(
            t0 + NOT_CONFIGURED_NOTIFY_COOLDOWN + Duration::from_secs(1)
        ));
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
}
