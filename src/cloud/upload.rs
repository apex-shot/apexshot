use std::io::Read;
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
    UpgradeRequired(String),
    Server(String),
    AuthExpired(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::NotConfigured(msg) => write!(f, "{msg}"),
            UploadError::FileRead(msg) => write!(f, "Failed to read file: {msg}"),
            UploadError::HttpRequest(msg) => write!(f, "Upload request failed: {msg}"),
            UploadError::UpgradeRequired(msg) => write!(f, "Upload request failed: {msg}"),
            UploadError::Server(msg) => write!(f, "Server error: {msg}"),
            UploadError::AuthExpired(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UploadError {}

/// Called with the bytes sent so far and the total body size.
pub type UploadProgress<'a> = &'a dyn Fn(u64, u64);

/// A request body that reports how much of it has been read into the socket.
pub(crate) struct ProgressReader<'a> {
    bytes: &'a [u8],
    sent: usize,
    progress: UploadProgress<'a>,
}

impl<'a> ProgressReader<'a> {
    pub(crate) fn new(bytes: &'a [u8], progress: UploadProgress<'a>) -> Self {
        Self {
            bytes,
            sent: 0,
            progress,
        }
    }
}

impl Read for ProgressReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let remaining = &self.bytes[self.sent..];
        let count = remaining.len().min(buf.len()).min(64 * 1024);
        buf[..count].copy_from_slice(&remaining[..count]);
        self.sent += count;
        if count > 0 {
            (self.progress)(self.sent as u64, self.bytes.len() as u64);
        }
        Ok(count)
    }
}

pub fn is_configured(config: &AppConfig) -> bool {
    Destination::from_config(config).is_configured(config)
}

/// True when Settings has auto-upload enabled and the selected destination is ready.
pub fn should_auto_upload_after_capture(config: &AppConfig) -> bool {
    config.cloud_auto_upload_after_capture && is_configured(config)
}

pub fn upload_file(config: &AppConfig, path: &Path) -> Result<UploadResult, UploadError> {
    upload_file_with_progress(config, path, &|_, _| {})
}

/// Upload and report the request body's progress as it is sent.
pub fn upload_file_with_progress(
    config: &AppConfig,
    path: &Path,
    progress: UploadProgress<'_>,
) -> Result<UploadResult, UploadError> {
    Destination::from_config(config).upload(config, path, progress)
}

pub fn not_configured_notification(config: &AppConfig) -> (String, String) {
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
    upload_file_with_notifications_replacing(config, path, 0, true)
}

/// Run an upload and notify, leaving clipboard ownership to a GTK UI caller.
///
/// Quick Access and the editors receive the result on GTK's main thread and
/// copy there, avoiding command-line clipboard helper notifications.
pub fn upload_file_with_notifications_without_clipboard(
    config: &AppConfig,
    path: &Path,
) -> Result<UploadResult, UploadError> {
    upload_file_with_notifications_replacing(config, path, 0, false)
}

fn upload_file_with_notifications_replacing(
    config: &AppConfig,
    path: &Path,
    replaces_notification_id: u32,
    copy_share_link: bool,
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
            if copy_share_link {
                if let Err(e) = crate::utils::clipboard::copy_text_to_clipboard(&result.share_url) {
                    eprintln!("[cloud] Failed to copy share link to clipboard: {e}");
                } else {
                    body = crate::i18n::tfmt(
                        "Copied to clipboard\n{url}",
                        &[("url", &result.share_url)],
                    );
                }
            }
            // Always include the URL in the body so the toast is useful even if
            // the user misses the clipboard.
            let _ = crate::utils::notify::desktop_notification_replace(
                replaces_notification_id,
                &crate::i18n::t("Upload complete"),
                &body,
            );
            Ok(result)
        }
        Err(e) => {
            eprintln!("[cloud] Upload failed: {e}");
            let _ = crate::utils::notify::desktop_notification_replace(
                replaces_notification_id,
                &crate::i18n::t("Upload failed"),
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
        let _ = upload_file_with_notifications_replacing(&config, &path, 0, true);
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

    #[test]
    fn progress_reader_reports_every_byte_once() {
        use std::cell::RefCell;
        let bytes = vec![7u8; 150_000];
        let seen = RefCell::new(Vec::new());
        let report = |sent: u64, total: u64| seen.borrow_mut().push((sent, total));
        let mut reader = ProgressReader::new(&bytes, &report);
        let mut copied = Vec::new();
        reader.read_to_end(&mut copied).unwrap();
        assert_eq!(copied, bytes);
        let seen = seen.into_inner();
        assert_eq!(seen.last(), Some(&(150_000, 150_000)));
        assert!(seen.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
