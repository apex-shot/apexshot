//! Read side of the ApexShot Cloud API.
//!
//! Uploads only ever pushed bytes up; this module reads back what the signed-in
//! account already has: a cursor-paginated uploads listing, the account's plan
//! tier / entitlement, and an on-disk cache for remote thumbnails.
//!
//! Deliberately GUI-free — every entry point is safe to call from a background
//! thread, so a window can page and thumbnail without blocking its main loop.

use std::io::Read;
use std::path::PathBuf;

use serde::Deserialize;

use crate::config::{is_cloud_logged_in, resolve_cloud_backend_url, save_config, AppConfig};

use super::apexshot::{refresh_access_token, RefreshError};

/// Page size used when a caller has no opinion.
pub const DEFAULT_PAGE_SIZE: u32 = 50;
/// Clamp locally so a bad page size cannot turn into a server-side 400.
const MAX_PAGE_SIZE: u32 = 100;
/// Thumbnails are small — refuse to buffer a runaway response into memory.
const MAX_THUMBNAIL_BYTES: u64 = 16 * 1024 * 1024;
/// Server error bodies are shown to users verbatim; keep them toast-sized.
const MAX_SERVER_MESSAGE_CHARS: usize = 200;

// --- errors ---

/// Failure modes of the cloud read API, kept distinguishable so a caller can
/// offer sign-in, retry, or a plain error message as appropriate.
#[derive(Debug)]
pub enum CloudReadError {
    /// No ApexShot Cloud session on this install.
    NotLoggedIn,
    /// The request never reached the server (offline, DNS, TLS, timeout).
    Network(String),
    /// The server refused our credentials even after a token refresh.
    AuthRejected,
    /// The server answered with an error status, or with a body we cannot read.
    Server(String),
    /// The local thumbnail cache could not be read or written.
    Cache(String),
}

impl std::fmt::Display for CloudReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudReadError::NotLoggedIn => write!(
                f,
                "You are not signed in to ApexShot Cloud. Run `apexshot login` to connect."
            ),
            CloudReadError::Network(msg) => write!(f, "Could not reach ApexShot Cloud: {msg}"),
            CloudReadError::AuthRejected => write!(
                f,
                "Your ApexShot Cloud session has expired. Run `apexshot login` again."
            ),
            CloudReadError::Server(msg) => write!(f, "ApexShot Cloud error: {msg}"),
            CloudReadError::Cache(msg) => write!(f, "Thumbnail cache error: {msg}"),
        }
    }
}

impl std::error::Error for CloudReadError {}

// --- uploads listing ---

/// One remote upload, modelled on what a gallery card needs.
///
/// Every field is optional on the wire: a server that omits `content_type` (or
/// sends an item we only partly understand) must not cost us the whole page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CloudUpload {
    #[serde(default, alias = "uploadId", alias = "upload_id")]
    pub id: String,
    #[serde(default, alias = "fileName", alias = "name")]
    pub filename: String,
    #[serde(default, alias = "shareUrl", alias = "url")]
    pub share_url: Option<String>,
    #[serde(default, alias = "thumbnailUrl", alias = "thumb_url")]
    pub thumbnail_url: Option<String>,
    #[serde(default, alias = "sizeBytes", alias = "size")]
    pub size_bytes: Option<i64>,
    #[serde(default, alias = "contentType", alias = "mime_type")]
    pub content_type: Option<String>,
    #[serde(default, alias = "createdAt")]
    pub created_at: Option<String>,
}

impl CloudUpload {
    /// Filename for display, never empty.
    pub fn display_name(&self) -> String {
        let filename = self.filename.trim();
        if !filename.is_empty() {
            return filename.to_string();
        }
        let id = self.id.trim();
        if id.is_empty() {
            "Untitled upload".to_string()
        } else {
            id.to_string()
        }
    }

    /// Creation timestamp parsed from RFC 3339, or `None` when the server sent
    /// nothing usable. Never fails the item — the raw string stays available.
    pub fn created_at_utc(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let raw = self.created_at.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc))
    }
}

/// One page of uploads plus the server's pagination signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadsPage {
    pub items: Vec<CloudUpload>,
    /// Cursor to pass as `cursor` on the next request, when there is one.
    pub next_cursor: Option<String>,
    /// Whether the server says more items exist beyond this page.
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
struct UploadsPageWire {
    #[serde(default, alias = "uploads", alias = "data", alias = "results")]
    items: Vec<serde_json::Value>,
    #[serde(default, alias = "nextCursor")]
    next_cursor: Option<String>,
    #[serde(default, alias = "hasMore")]
    has_more: Option<bool>,
}

impl UploadsPageWire {
    fn into_page(self) -> UploadsPage {
        // Skip items we cannot read instead of discarding the whole page.
        let items = self
            .items
            .into_iter()
            .filter_map(|raw| match serde_json::from_value::<CloudUpload>(raw) {
                Ok(item) => Some(item),
                Err(e) => {
                    eprintln!("[cloud] Skipping unreadable upload in listing: {e}");
                    None
                }
            })
            .collect();

        let next_cursor = self
            .next_cursor
            .map(|cursor| cursor.trim().to_string())
            .filter(|cursor| !cursor.is_empty());
        // Older/leaner servers may only send a cursor: treat its presence as
        // "more to come" rather than silently stopping after the first page.
        let has_more = self.has_more.unwrap_or(next_cursor.is_some());

        UploadsPage {
            items,
            next_cursor,
            has_more,
        }
    }
}

/// Fetch one page of the signed-in account's uploads.
///
/// Pass `None` as `cursor` for the first page, then the previous page's
/// `next_cursor`. Fetching the first page also refreshes the cached plan tier
/// (best effort) so UI that renders from config stays current.
pub fn list_uploads(
    config: &AppConfig,
    page_size: u32,
    cursor: Option<&str>,
) -> Result<UploadsPage, CloudReadError> {
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE);
    let is_first_page = cursor
        .map(|cursor| cursor.trim().is_empty())
        .unwrap_or(true);

    let page = with_token_retry(config, |config| {
        request_uploads_page(config, page_size, cursor)
    })?;

    if is_first_page {
        // Non-fatal: a stale cached tier must not fail a listing that worked.
        if let Err(e) = fetch_account(config) {
            eprintln!("[cloud] Could not refresh cached plan tier: {e}");
        }
    }

    Ok(page)
}

fn request_uploads_page(
    config: &AppConfig,
    page_size: u32,
    cursor: Option<&str>,
) -> Result<UploadsPage, CloudReadError> {
    let endpoint = format!("{}/v1/uploads", backend_url(config)?);
    let mut request = ureq::get(&endpoint)
        .set(
            "Authorization",
            &format!("Bearer {}", config.cloud_api_token),
        )
        .set("Accept", "application/json")
        .query("limit", &page_size.to_string());
    if let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) {
        request = request.query("cursor", cursor);
    }

    let wire: UploadsPageWire = request
        .call()
        .map_err(map_http_error)?
        .into_json()
        .map_err(|e| CloudReadError::Server(format!("Invalid uploads response: {e}")))?;

    Ok(wire.into_page())
}

/// Walks the server's cursor pagination one page at a time.
///
/// Holds only the cursor, so a UI can keep one pager alive across "load more"
/// clicks and stop cleanly the moment the server reports no more items.
#[derive(Debug, Clone)]
pub struct UploadsPager {
    page_size: u32,
    cursor: Option<String>,
    exhausted: bool,
}

impl UploadsPager {
    pub fn new(page_size: u32) -> Self {
        Self {
            page_size: page_size.clamp(1, MAX_PAGE_SIZE),
            cursor: None,
            exhausted: false,
        }
    }

    /// True once the server has reported there is nothing left to fetch.
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Fetch the next page, or `Ok(None)` when the listing is complete.
    pub fn next_page(&mut self, config: &AppConfig) -> Result<Option<UploadsPage>, CloudReadError> {
        if self.exhausted {
            return Ok(None);
        }
        let page = list_uploads(config, self.page_size, self.cursor.as_deref())?;
        self.record(&page);
        Ok(Some(page))
    }

    fn record(&mut self, page: &UploadsPage) {
        let next = page
            .next_cursor
            .as_deref()
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty());
        match next {
            // A server echoing back the cursor we just sent would page forever.
            Some(cursor) if page.has_more && Some(cursor) != self.cursor.as_deref() => {
                self.cursor = Some(cursor.to_string());
            }
            _ => {
                self.exhausted = true;
                self.cursor = None;
            }
        }
    }
}

impl Default for UploadsPager {
    fn default() -> Self {
        Self::new(DEFAULT_PAGE_SIZE)
    }
}

// --- account and entitlement ---

pub use super::account::{is_subscribed_tier, CloudAccount};

/// The cached entitlement answer — no network, safe on the UI thread.
///
/// Falls back to the legacy pro flag for sessions established before the tier
/// was persisted, so those users are not downgraded until the next lookup.
pub fn cached_is_subscribed(config: &AppConfig) -> bool {
    if !is_cloud_logged_in(config) {
        return false;
    }
    let tier = config.cloud_plan_tier.trim();
    if tier.is_empty() {
        return config.cloud_pro_plan;
    }
    is_subscribed_tier(tier)
}

/// Look up the signed-in account's email and plan tier, and cache them in
/// config so later reads need no network round trip.
pub fn fetch_account(config: &AppConfig) -> Result<CloudAccount, CloudReadError> {
    let account = with_token_retry(config, request_account)?;

    let mut cached = config.clone();
    account.apply_to_config(&mut cached);
    if let Err(e) = save_config(&cached) {
        // The lookup succeeded; only the cache write failed.
        eprintln!("[cloud] Could not cache account tier: {e}");
    }

    Ok(account)
}

fn request_account(config: &AppConfig) -> Result<CloudAccount, CloudReadError> {
    let endpoint = format!("{}/v1/account", backend_url(config)?);
    ureq::get(&endpoint)
        .set(
            "Authorization",
            &format!("Bearer {}", config.cloud_api_token),
        )
        .set("Accept", "application/json")
        .call()
        .map_err(map_http_error)?
        .into_json()
        .map_err(|e| CloudReadError::Server(format!("Invalid account response: {e}")))
}

// --- thumbnail cache ---

/// Where remote thumbnails are cached, in its own subfolder of the app cache
/// dir (the recording editor uses a sibling folder for its FFmpeg frames).
pub fn thumbnail_cache_dir() -> PathBuf {
    let mut dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    dir.push("apexshot");
    dir.push("cloud-thumbnails");
    dir
}

/// Filename a thumbnail URL caches to: a stable hash plus a recognisable
/// extension, so the same URL always maps to the same file.
pub fn thumbnail_cache_key(url: &str) -> String {
    format!(
        "{}.{}",
        fnv1a_hex(url.trim()),
        thumbnail_extension(url.trim())
    )
}

/// Local path for a remote thumbnail, downloading it only when it is not
/// already cached. Safe to call from several background threads at once: the
/// bytes land in a per-call temp file and are renamed into place.
pub fn cached_thumbnail(url: &str) -> Result<PathBuf, CloudReadError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CloudReadError::Server(
            "Upload has no thumbnail URL".to_string(),
        ));
    }

    let dir = thumbnail_cache_dir();
    let path = dir.join(thumbnail_cache_key(url));
    if path.is_file() {
        return Ok(path);
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| CloudReadError::Cache(format!("Could not create {}: {e}", dir.display())))?;

    // No Authorization header: thumbnail URLs are pre-signed and may point at a
    // CDN, and our access token must not leak to a third-party host.
    let response = ureq::get(url).call().map_err(map_http_error)?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_THUMBNAIL_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| CloudReadError::Network(format!("Thumbnail download failed: {e}")))?;

    if bytes.is_empty() {
        return Err(CloudReadError::Server(
            "Thumbnail response was empty".to_string(),
        ));
    }

    let temp_path = dir.join(format!(
        ".{}.{}.{}.part",
        thumbnail_cache_key(url),
        std::process::id(),
        nanos_since_epoch()
    ));
    std::fs::write(&temp_path, &bytes)
        .map_err(|e| CloudReadError::Cache(format!("Could not write thumbnail: {e}")))?;

    if let Err(e) = std::fs::rename(&temp_path, &path) {
        let _ = std::fs::remove_file(&temp_path);
        // Another thread may have published the same thumbnail first.
        if !path.is_file() {
            return Err(CloudReadError::Cache(format!(
                "Could not store thumbnail: {e}"
            )));
        }
    }

    Ok(path)
}

/// FNV-1a, matching the recording editor's cache-key hashing.
fn fnv1a_hex(input: &str) -> String {
    let mut hash = 1469598103934665603_u64;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:x}")
}

/// Extension for a cached thumbnail, ignoring query strings and fragments so
/// the key stays stable for a given URL.
fn thumbnail_extension(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    let ext = last_segment
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "png",
        Some("jpg") | Some("jpeg") => "jpg",
        Some("webp") => "webp",
        Some("gif") => "gif",
        _ => "img",
    }
}

fn nanos_since_epoch() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

// --- shared request plumbing ---

fn backend_url(config: &AppConfig) -> Result<String, CloudReadError> {
    let url = resolve_cloud_backend_url(config);
    if url.is_empty() {
        return Err(CloudReadError::Server(
            "Cloud backend URL is not configured.".to_string(),
        ));
    }
    Ok(url)
}

/// Run an authenticated read, refreshing a stale access token once and retrying
/// so a merely expired token never bounces the user back to a login prompt.
fn with_token_retry<T>(
    config: &AppConfig,
    request: impl Fn(&AppConfig) -> Result<T, CloudReadError>,
) -> Result<T, CloudReadError> {
    if !is_cloud_logged_in(config) {
        return Err(CloudReadError::NotLoggedIn);
    }

    match request(config) {
        Err(CloudReadError::AuthRejected) => {
            let mut refreshed = config.clone();
            // Shared with uploads: persists the new token pair to config.
            if let Err(e) = refresh_access_token(&mut refreshed) {
                eprintln!("[cloud] Access token refresh failed: {e}");
                return Err(map_refresh_error(e));
            }
            request(&refreshed)
        }
        other => other,
    }
}

fn map_refresh_error(error: RefreshError) -> CloudReadError {
    match error {
        RefreshError::NoRefreshToken | RefreshError::Rejected(_) => CloudReadError::AuthRejected,
        RefreshError::Network(msg) => CloudReadError::Network(msg),
        RefreshError::Server(msg) => CloudReadError::Server(msg),
    }
}

fn map_http_error(error: ureq::Error) -> CloudReadError {
    match error {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => CloudReadError::AuthRejected,
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let message = truncate_message(body.trim());
            if message.is_empty() {
                CloudReadError::Server(format!("HTTP {code}"))
            } else {
                CloudReadError::Server(format!("HTTP {code}: {message}"))
            }
        }
        ureq::Error::Transport(transport) => CloudReadError::Network(transport.to_string()),
    }
}

fn truncate_message(message: &str) -> String {
    if message.chars().count() <= MAX_SERVER_MESSAGE_CHARS {
        return message.to_string();
    }
    let kept: String = message.chars().take(MAX_SERVER_MESSAGE_CHARS).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(json: &str) -> UploadsPage {
        serde_json::from_str::<UploadsPageWire>(json)
            .expect("listing payload parses")
            .into_page()
    }

    // --- listing response parsing ---

    #[test]
    fn parses_multi_item_page_with_cursor() {
        let parsed = page(
            r#"{
                "items": [
                    {
                        "id": "up_1",
                        "filename": "ApexShot 2026-06-28.png",
                        "share_url": "https://apexshot.org/s/7t1NE9mTWw9J",
                        "thumbnail_url": "https://cdn.apexshot.org/t/7t1NE9mTWw9J.webp",
                        "size_bytes": 184320,
                        "content_type": "image/png",
                        "created_at": "2026-06-28T17:39:42Z"
                    },
                    {
                        "id": "up_2",
                        "filename": "ApexShot Recording.mp4",
                        "share_url": "https://apexshot.org/s/abc123",
                        "size_bytes": 5242880,
                        "content_type": "video/mp4",
                        "created_at": "2026-06-27T09:02:11Z"
                    }
                ],
                "limit": 50,
                "next_cursor": "eyJpZCI6InVwXzIifQ",
                "has_more": true
            }"#,
        );

        assert_eq!(parsed.items.len(), 2);
        assert!(parsed.has_more);
        assert_eq!(parsed.next_cursor.as_deref(), Some("eyJpZCI6InVwXzIifQ"));

        let first = &parsed.items[0];
        assert_eq!(first.id, "up_1");
        assert_eq!(first.filename, "ApexShot 2026-06-28.png");
        assert_eq!(
            first.share_url.as_deref(),
            Some("https://apexshot.org/s/7t1NE9mTWw9J")
        );
        assert_eq!(
            first.thumbnail_url.as_deref(),
            Some("https://cdn.apexshot.org/t/7t1NE9mTWw9J.webp")
        );
        assert_eq!(first.size_bytes, Some(184320));
        assert_eq!(first.content_type.as_deref(), Some("image/png"));
        assert_eq!(
            first.created_at_utc().map(|dt| dt.to_rfc3339()),
            Some("2026-06-28T17:39:42+00:00".to_string())
        );

        // A video item without a thumbnail is still a valid item.
        assert!(parsed.items[1].thumbnail_url.is_none());
    }

    #[test]
    fn parses_final_page_without_next_cursor() {
        let parsed = page(
            r#"{
                "items": [{ "id": "up_9", "filename": "last.png" }],
                "has_more": false
            }"#,
        );

        assert_eq!(parsed.items.len(), 1);
        assert!(!parsed.has_more);
        assert!(parsed.next_cursor.is_none());
    }

    #[test]
    fn treats_empty_cursor_as_absent() {
        let parsed = page(r#"{ "items": [], "next_cursor": "   ", "has_more": true }"#);

        assert!(parsed.next_cursor.is_none());
        assert!(parsed.items.is_empty());
    }

    #[test]
    fn infers_has_more_from_cursor_when_server_omits_it() {
        let with_cursor = page(r#"{ "items": [], "next_cursor": "c1" }"#);
        assert!(with_cursor.has_more);

        let without_cursor = page(r#"{ "items": [] }"#);
        assert!(!without_cursor.has_more);
    }

    #[test]
    fn accepts_camel_case_listing_fields() {
        let parsed = page(
            r#"{
                "uploads": [
                    {
                        "id": "up_1",
                        "fileName": "shot.png",
                        "shareUrl": "https://apexshot.org/s/x",
                        "thumbnailUrl": "https://cdn.apexshot.org/t/x.png",
                        "sizeBytes": 42,
                        "contentType": "image/png",
                        "createdAt": "2026-06-28T17:39:42Z"
                    }
                ],
                "nextCursor": "c2",
                "hasMore": true
            }"#,
        );

        let item = &parsed.items[0];
        assert_eq!(item.filename, "shot.png");
        assert_eq!(item.share_url.as_deref(), Some("https://apexshot.org/s/x"));
        assert_eq!(item.size_bytes, Some(42));
        assert_eq!(parsed.next_cursor.as_deref(), Some("c2"));
    }

    #[test]
    fn partial_item_does_not_fail_the_page() {
        let parsed = page(
            r#"{
                "items": [
                    { "id": "up_1" },
                    { "id": "up_2", "size_bytes": "not-a-number" },
                    { "id": "up_3", "filename": "third.png" }
                ],
                "has_more": false
            }"#,
        );

        // The unreadable item is skipped; the readable ones survive.
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].id, "up_1");
        assert_eq!(parsed.items[1].id, "up_3");
        assert!(parsed.items[0].share_url.is_none());
        assert!(parsed.items[0].size_bytes.is_none());
        assert!(parsed.items[0].created_at_utc().is_none());
    }

    #[test]
    fn display_name_falls_back_when_filename_missing() {
        let named = CloudUpload {
            filename: " shot.png ".to_string(),
            ..bare_upload("up_1")
        };
        assert_eq!(named.display_name(), "shot.png");

        assert_eq!(bare_upload("up_2").display_name(), "up_2");

        let anonymous = bare_upload("");
        assert_eq!(anonymous.display_name(), "Untitled upload");
    }

    fn bare_upload(id: &str) -> CloudUpload {
        CloudUpload {
            id: id.to_string(),
            filename: String::new(),
            share_url: None,
            thumbnail_url: None,
            size_bytes: None,
            content_type: None,
            created_at: None,
        }
    }

    #[test]
    fn ignores_unparseable_created_at() {
        let item = CloudUpload {
            created_at: Some("last tuesday".to_string()),
            ..bare_upload("up_1")
        };
        assert!(item.created_at_utc().is_none());
    }

    // --- pagination ---

    #[test]
    fn pager_walks_pages_then_stops_when_server_reports_no_more() {
        let mut pager = UploadsPager::new(2);
        assert!(!pager.is_exhausted());

        pager.record(&UploadsPage {
            items: vec![bare_upload("up_1")],
            next_cursor: Some("c1".to_string()),
            has_more: true,
        });
        assert!(!pager.is_exhausted());
        assert_eq!(pager.cursor.as_deref(), Some("c1"));

        pager.record(&UploadsPage {
            items: vec![bare_upload("up_2")],
            next_cursor: Some("c2".to_string()),
            has_more: true,
        });
        assert!(!pager.is_exhausted());
        assert_eq!(pager.cursor.as_deref(), Some("c2"));

        pager.record(&UploadsPage {
            items: vec![bare_upload("up_3")],
            next_cursor: None,
            has_more: false,
        });
        assert!(pager.is_exhausted());
        assert!(pager.cursor.is_none());
    }

    #[test]
    fn pager_stops_when_server_claims_more_but_sends_no_cursor() {
        let mut pager = UploadsPager::new(2);
        pager.record(&UploadsPage {
            items: vec![bare_upload("up_1")],
            next_cursor: None,
            has_more: true,
        });

        assert!(pager.is_exhausted());
    }

    #[test]
    fn pager_stops_when_server_repeats_the_same_cursor() {
        let mut pager = UploadsPager::new(2);
        let repeated = UploadsPage {
            items: vec![bare_upload("up_1")],
            next_cursor: Some("c1".to_string()),
            has_more: true,
        };

        pager.record(&repeated);
        assert!(!pager.is_exhausted());

        pager.record(&repeated);
        assert!(pager.is_exhausted());
    }

    #[test]
    fn pager_clamps_page_size() {
        assert_eq!(UploadsPager::new(0).page_size, 1);
        assert_eq!(UploadsPager::new(5_000).page_size, MAX_PAGE_SIZE);
        assert_eq!(UploadsPager::default().page_size, DEFAULT_PAGE_SIZE);
    }

    // --- account and entitlement ---

    #[test]
    fn parses_account_response() {
        let account: CloudAccount =
            serde_json::from_str(r#"{ "email": "user@example.com", "tier": "pro" }"#).unwrap();

        assert_eq!(account.email, "user@example.com");
        assert_eq!(account.tier, "pro");
        assert!(account.is_subscribed());
    }

    #[test]
    fn parses_account_response_without_tier() {
        let account: CloudAccount =
            serde_json::from_str(r#"{ "email": "user@example.com" }"#).unwrap();

        assert!(account.tier.is_empty());
        assert!(!account.is_subscribed());
    }

    #[test]
    fn apply_to_config_caches_tier_and_syncs_pro_flag() {
        let mut config = AppConfig {
            cloud_api_token: "tok".to_string(),
            ..AppConfig::default()
        };

        CloudAccount {
            email: "user@example.com".to_string(),
            tier: "Team".to_string(),
        }
        .apply_to_config(&mut config);

        assert_eq!(config.cloud_user_email, "user@example.com");
        assert_eq!(config.cloud_plan_tier, "team");
        assert!(config.cloud_pro_plan);
        assert!(cached_is_subscribed(&config));

        CloudAccount {
            email: "user@example.com".to_string(),
            tier: "free".to_string(),
        }
        .apply_to_config(&mut config);

        assert_eq!(config.cloud_plan_tier, "free");
        assert!(!config.cloud_pro_plan);
        assert!(!cached_is_subscribed(&config));
    }

    #[test]
    fn apply_to_config_keeps_cached_values_the_server_did_not_send() {
        let mut config = AppConfig {
            cloud_api_token: "tok".to_string(),
            cloud_user_email: "user@example.com".to_string(),
            cloud_plan_tier: "pro".to_string(),
            cloud_pro_plan: true,
            ..AppConfig::default()
        };

        CloudAccount {
            email: String::new(),
            tier: String::new(),
        }
        .apply_to_config(&mut config);

        assert_eq!(config.cloud_user_email, "user@example.com");
        assert_eq!(config.cloud_plan_tier, "pro");
        assert!(config.cloud_pro_plan);
    }

    #[test]
    fn cached_subscription_requires_a_session() {
        let signed_out = AppConfig {
            cloud_plan_tier: "pro".to_string(),
            cloud_pro_plan: true,
            ..AppConfig::default()
        };
        assert!(!cached_is_subscribed(&signed_out));
    }

    #[test]
    fn cached_subscription_falls_back_to_legacy_pro_flag() {
        let legacy = AppConfig {
            cloud_api_token: "tok".to_string(),
            cloud_plan_tier: String::new(),
            cloud_pro_plan: true,
            ..AppConfig::default()
        };
        assert!(cached_is_subscribed(&legacy));

        let legacy_free = AppConfig {
            cloud_pro_plan: false,
            ..legacy
        };
        assert!(!cached_is_subscribed(&legacy_free));
    }

    // --- thumbnail cache ---

    #[test]
    fn thumbnail_cache_key_is_stable_for_the_same_url() {
        let url = "https://cdn.apexshot.org/t/7t1NE9mTWw9J.webp";
        assert_eq!(thumbnail_cache_key(url), thumbnail_cache_key(url));
        assert_eq!(
            thumbnail_cache_key(url),
            thumbnail_cache_key(&format!(" {url} "))
        );
    }

    #[test]
    fn thumbnail_cache_key_differs_across_urls() {
        let a = thumbnail_cache_key("https://cdn.apexshot.org/t/one.webp");
        let b = thumbnail_cache_key("https://cdn.apexshot.org/t/two.webp");
        assert_ne!(a, b);
    }

    #[test]
    fn thumbnail_cache_key_keeps_a_recognisable_extension() {
        assert!(thumbnail_cache_key("https://cdn.example/t/a.PNG").ends_with(".png"));
        assert!(thumbnail_cache_key("https://cdn.example/t/a.jpeg").ends_with(".jpg"));
        assert!(thumbnail_cache_key("https://cdn.example/t/a.webp?sig=abc").ends_with(".webp"));
        assert!(thumbnail_cache_key("https://cdn.example/t/a.gif#x").ends_with(".gif"));
        assert!(thumbnail_cache_key("https://cdn.example/t/opaque-id").ends_with(".img"));
    }

    #[test]
    fn thumbnail_cache_dir_lives_under_the_app_cache_dir() {
        let dir = thumbnail_cache_dir();
        assert!(dir.ends_with("apexshot/cloud-thumbnails"));
    }

    #[test]
    fn empty_thumbnail_url_is_rejected_without_a_request() {
        let err = cached_thumbnail("   ").unwrap_err();
        assert!(err.to_string().contains("no thumbnail URL"));
    }

    // --- error surface ---

    fn status_error(code: u16, body: &str) -> ureq::Error {
        ureq::is_test(true);
        ureq::Error::Status(
            code,
            ureq::Response::new(code, "Status", body).expect("test response"),
        )
    }

    #[test]
    fn unauthorized_maps_to_auth_rejected() {
        assert!(matches!(
            map_http_error(status_error(401, "Unauthorized")),
            CloudReadError::AuthRejected
        ));
        assert!(matches!(
            map_http_error(status_error(403, "Forbidden")),
            CloudReadError::AuthRejected
        ));
    }

    #[test]
    fn server_status_keeps_the_body_in_the_message() {
        let err = map_http_error(status_error(500, r#"{"error":"database unavailable"}"#));
        assert!(matches!(err, CloudReadError::Server(_)));
        assert!(err.to_string().contains("HTTP 500"));
        assert!(err.to_string().contains("database unavailable"));
    }

    #[test]
    fn server_status_without_a_body_still_reads_cleanly() {
        let err = map_http_error(status_error(502, ""));
        assert_eq!(err.to_string(), "ApexShot Cloud error: HTTP 502");
    }

    #[test]
    fn long_server_bodies_are_truncated() {
        let err = map_http_error(status_error(500, &"x".repeat(1_000)));
        assert!(err.to_string().chars().count() < 300);
        assert!(err.to_string().ends_with('…'));
    }

    #[test]
    fn every_failure_mode_has_its_own_readable_message() {
        assert!(CloudReadError::NotLoggedIn
            .to_string()
            .contains("not signed in"));
        assert!(CloudReadError::Network("dns error".to_string())
            .to_string()
            .contains("Could not reach ApexShot Cloud"));
        assert!(CloudReadError::AuthRejected
            .to_string()
            .contains("session has expired"));
        assert!(CloudReadError::Server("HTTP 500".to_string())
            .to_string()
            .contains("HTTP 500"));
        assert!(CloudReadError::Cache("disk full".to_string())
            .to_string()
            .contains("disk full"));
    }

    #[test]
    fn refresh_failures_map_to_matching_read_errors() {
        assert!(matches!(
            map_refresh_error(RefreshError::NoRefreshToken),
            CloudReadError::AuthRejected
        ));
        assert!(matches!(
            map_refresh_error(RefreshError::Rejected("HTTP 400".to_string())),
            CloudReadError::AuthRejected
        ));
        assert!(matches!(
            map_refresh_error(RefreshError::Network("offline".to_string())),
            CloudReadError::Network(_)
        ));
        assert!(matches!(
            map_refresh_error(RefreshError::Server("bad json".to_string())),
            CloudReadError::Server(_)
        ));
    }

    #[test]
    fn reads_without_a_session_fail_before_touching_the_network() {
        let signed_out = AppConfig {
            cloud_api_token: String::new(),
            ..AppConfig::default()
        };

        let err = list_uploads(&signed_out, DEFAULT_PAGE_SIZE, None).unwrap_err();
        assert!(matches!(err, CloudReadError::NotLoggedIn));

        let err = fetch_account(&signed_out).unwrap_err();
        assert!(matches!(err, CloudReadError::NotLoggedIn));
    }
}
