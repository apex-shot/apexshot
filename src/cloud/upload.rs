use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::{load_config, AppConfig};

use super::destination::Destination;

#[derive(Debug)]
pub struct UploadResult {
    pub share_url: String,
}

/// Which free-tier limit stopped an upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudLimitKind {
    MonthlyUploads,
    Storage,
    Other,
}

impl CloudLimitKind {
    /// The spelling the `cloud-limit` command line takes.
    pub fn as_arg(self) -> &'static str {
        match self {
            CloudLimitKind::MonthlyUploads => "monthly_uploads",
            CloudLimitKind::Storage => "storage",
            CloudLimitKind::Other => "other",
        }
    }

    /// Parse the spelling `cloud-limit` accepts, including the short aliases.
    pub fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "monthly_uploads" | "monthly" => Some(CloudLimitKind::MonthlyUploads),
            "storage" | "storage_full" => Some(CloudLimitKind::Storage),
            "other" => Some(CloudLimitKind::Other),
            _ => None,
        }
    }
}

/// A structured Cloud limit failure, as the account server describes it.
///
/// Older servers only send a message, so `kind` is inferred from the words in
/// it and the numbers stay absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudLimit {
    pub kind: CloudLimitKind,
    pub message: String,
    pub used: Option<u64>,
    pub max: Option<u64>,
    pub resets_at: Option<String>,
}

impl std::fmt::Display for CloudLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// The outcome of an upload that may also carry a near-limit heads-up.
///
/// UI callers use `heads_up` to raise the card beside the editor window after
/// the shared notification has already shown the usage lines.
pub struct UploadOutcome {
    pub result: Result<UploadResult, UploadError>,
    pub heads_up: Option<super::limit_prompt::UploadHeadsUp>,
}

#[derive(Debug)]
pub enum UploadError {
    NotConfigured(String),
    FileRead(String),
    HttpRequest(String),
    UpgradeRequired(CloudLimit),
    Server(String),
    AuthExpired(String),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::NotConfigured(msg) => write!(f, "{msg}"),
            UploadError::FileRead(msg) => write!(f, "Failed to read file: {msg}"),
            UploadError::HttpRequest(msg) => write!(f, "Upload request failed: {msg}"),
            UploadError::UpgradeRequired(limit) => write!(f, "Upload request failed: {limit}"),
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

/// Whether this upload's success toast should mention that free links expire.
///
/// Free ApexShot Cloud links last seven days, so the first successful upload
/// says so once and the config remembers that it has.
fn should_show_link_expiry_notice(config: &AppConfig) -> bool {
    Destination::from_config(config) == Destination::ApexShot
        && !config.cloud_link_expiry_notice_shown
        && !config.cloud_pro_plan
        && !super::account::is_subscribed_tier(&config.cloud_plan_tier)
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
    upload_and_notify(config, path, 0, true, true).result
}

/// Run an upload and notify, leaving clipboard ownership to a GTK UI caller.
///
/// Quick Access and the editors receive the result on GTK's main thread and
/// copy there, avoiding command-line clipboard helper notifications. The
/// returned heads-up lets that caller raise the card over its own window.
pub fn upload_file_with_notifications_without_clipboard(
    config: &AppConfig,
    path: &Path,
) -> UploadOutcome {
    upload_and_notify(config, path, 0, false, false)
}

/// Upload and compute the near-limit heads-up, without sending notifications.
///
/// The editors that own their own progress screen and result card use this so
/// the usage lines land in their copy and the card can sit on their window.
pub fn upload_file_with_progress_and_heads_up(
    config: &AppConfig,
    path: &Path,
    progress: UploadProgress<'_>,
) -> UploadOutcome {
    let result = upload_file_with_progress(config, path, progress);
    let heads_up = match &result {
        Ok(_) => super::limit_prompt::usage_heads_up(config),
        Err(_) => None,
    };
    UploadOutcome { result, heads_up }
}

fn upload_and_notify(
    config: &AppConfig,
    path: &Path,
    replaces_notification_id: u32,
    copy_share_link: bool,
    standalone_limit_card: bool,
) -> UploadOutcome {
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
            let mut copied = false;
            if copy_share_link {
                if let Err(e) = crate::utils::clipboard::copy_text_to_clipboard(&result.share_url) {
                    eprintln!("[cloud] Failed to copy share link to clipboard: {e}");
                } else {
                    copied = true;
                    body = crate::i18n::tfmt(
                        "Copied to clipboard\n{url}",
                        &[("url", &result.share_url)],
                    );
                }
            }
            if should_show_link_expiry_notice(config) {
                body = if copied {
                    crate::i18n::tfmt(
                        "Link copied. Free links stay up for 7 days.\n{url}",
                        &[("url", &result.share_url)],
                    )
                } else {
                    crate::i18n::tfmt(
                        "Free links stay up for 7 days.\n{url}",
                        &[("url", &result.share_url)],
                    )
                };
                let mut updated = load_config().sanitized();
                updated.cloud_link_expiry_notice_shown = true;
                if let Err(error) = crate::config::save_config(&updated) {
                    eprintln!("[cloud] Could not remember the link expiry notice: {error}");
                }
            }
            // Always include the URL in the body so the toast is useful even if
            // the user misses the clipboard.
            let notification_id = crate::utils::notify::desktop_notification_replace(
                replaces_notification_id,
                &crate::i18n::t("Upload complete"),
                &body,
            );

            // The notification is already up; this fetch is capped at two
            // seconds and a slow account endpoint only skips the extra line.
            let heads_up = super::limit_prompt::usage_heads_up(config);
            if let Some(heads_up) = &heads_up {
                if !heads_up.lines.is_empty() {
                    let _ = crate::utils::notify::desktop_notification_replace(
                        notification_id,
                        &crate::i18n::t("Upload complete"),
                        &heads_up.append_to(&body),
                    );
                }
                if standalone_limit_card {
                    for limit in &heads_up.cards {
                        super::limit_prompt::spawn_card(limit, true);
                    }
                }
            }

            UploadOutcome {
                result: Ok(result),
                heads_up,
            }
        }
        Err(error) => {
            eprintln!("[cloud] Upload failed: {error}");
            match &error {
                UploadError::UpgradeRequired(limit) => {
                    // A limit is expected account state, not a transport fault:
                    // the toast says what happened in the card's own words and
                    // the card carries the numbers and the way forward.
                    let _ = crate::utils::notify::desktop_notification_replace(
                        replaces_notification_id,
                        &crate::i18n::t("Not uploaded"),
                        &super::upgrade::headline(limit),
                    );
                    if standalone_limit_card
                        && super::limit_prompt::allow_upgrade_prompt(limit.kind)
                    {
                        super::limit_prompt::spawn_card(limit, false);
                    }
                }
                _ => {
                    let _ = crate::utils::notify::desktop_notification_replace(
                        replaces_notification_id,
                        &crate::i18n::t("Upload failed"),
                        &error.to_string(),
                    );
                }
            }
            UploadOutcome {
                result: Err(error),
                heads_up: None,
            }
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
        let _ = upload_and_notify(&config, &path, 0, true, true);
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
    fn link_expiry_notice_shows_once_for_a_free_apexshot_account() {
        let config = AppConfig {
            cloud_destination: "apexshot".to_string(),
            cloud_api_token: "tok".to_string(),
            cloud_plan_tier: "free".to_string(),
            ..AppConfig::default()
        };
        assert!(should_show_link_expiry_notice(&config));
    }

    #[test]
    fn link_expiry_notice_stays_hidden_after_it_was_shown() {
        let config = AppConfig {
            cloud_link_expiry_notice_shown: true,
            ..AppConfig::default()
        };
        assert!(!should_show_link_expiry_notice(&config));
    }

    #[test]
    fn link_expiry_notice_skips_paid_plans() {
        let by_flag = AppConfig {
            cloud_pro_plan: true,
            ..AppConfig::default()
        };
        assert!(!should_show_link_expiry_notice(&by_flag));

        let by_tier = AppConfig {
            cloud_plan_tier: "pro".to_string(),
            ..AppConfig::default()
        };
        assert!(!should_show_link_expiry_notice(&by_tier));
    }

    #[test]
    fn link_expiry_notice_skips_xbackbone() {
        let config = AppConfig {
            cloud_destination: "xbackbone".to_string(),
            ..AppConfig::default()
        };
        assert!(!should_show_link_expiry_notice(&config));
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
