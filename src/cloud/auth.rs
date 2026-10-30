use serde::Deserialize;
use std::time::Duration;

use crate::config::{
    is_cloud_logged_in, load_config, resolve_cloud_backend_url, save_config, AppConfig,
};

use super::account::CloudAccount;

const POLL_INTERVAL: u64 = 5;
const MAX_POLL_SECONDS: u64 = 900;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Auth requests always run with finite timeouts so a stalled backend cannot
/// hold a device login (or the onboarding step waiting on it) forever.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i32,
    interval: i32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i32,
    device_id: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug)]
pub(crate) enum RefreshError {
    NoRefreshToken,
    Rejected(String),
    Network(String),
    Server(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRefreshToken => write!(f, "no refresh token stored"),
            Self::Rejected(msg) => write!(f, "refresh token rejected: {msg}"),
            Self::Network(msg) => write!(f, "refresh request failed: {msg}"),
            Self::Server(msg) => write!(f, "{msg}"),
        }
    }
}

/// Refresh a device session without overwriting a newer login or logout.
pub(crate) fn refresh_access_token(config: &mut AppConfig) -> Result<String, RefreshError> {
    if config.cloud_refresh_token.trim().is_empty() {
        return Err(RefreshError::NoRefreshToken);
    }
    let backend_url = resolve_cloud_backend_url(config);
    let body = serde_json::json!({ "refresh_token": config.cloud_refresh_token }).to_string();
    let response = agent()
        .post(&format!("{backend_url}/v1/auth/refresh"))
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|error| match error {
            ureq::Error::Status(code, _) if (400..500).contains(&code) => {
                RefreshError::Rejected(format!("HTTP {code}"))
            }
            ureq::Error::Status(code, _) => RefreshError::Server(format!("HTTP {code}")),
            ureq::Error::Transport(error) => RefreshError::Network(error.to_string()),
        })?;
    let tokens: RefreshResponse = response
        .into_json()
        .map_err(|e| RefreshError::Server(format!("Invalid refresh response: {e}")))?;
    if tokens.access_token.trim().is_empty() || tokens.refresh_token.trim().is_empty() {
        return Err(RefreshError::Server(
            "Invalid refresh response: missing tokens".to_string(),
        ));
    }
    let mut latest = load_config();
    if latest.cloud_refresh_token != config.cloud_refresh_token
        || latest.cloud_api_token != config.cloud_api_token
        || resolve_cloud_backend_url(&latest) != backend_url
    {
        return Err(RefreshError::Rejected("session changed".to_string()));
    }
    latest.cloud_api_token = tokens.access_token;
    latest.cloud_refresh_token = tokens.refresh_token;
    save_config(&latest)
        .map_err(|e| RefreshError::Server(format!("Failed to save config: {e}")))?;
    config.cloud_api_token = latest.cloud_api_token;
    config.cloud_refresh_token = latest.cloud_refresh_token;
    Ok(config.cloud_api_token.clone())
}

/// A started device-authorization login. Show [`DeviceLogin::user_code`] to the
/// user, send them to [`DeviceLogin::verification_uri`], then poll with
/// [`poll_device_login`] until it returns an account.
#[derive(Debug, Clone)]
pub struct DeviceLogin {
    pub device_code: String,
    /// Ready to display, grouped for readability (e.g. `ABCD-EFGH`).
    pub user_code: String,
    pub verification_uri: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

#[derive(Debug)]
pub enum LoginError {
    NotConfigured,
    HttpRequest(String),
    Expired,
    Denied,
    Server(String),
    /// The sign-in reached the server, but this device could not persist the
    /// result (a config write failure, not a server refusal).
    Local(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoginError::NotConfigured => write!(
                f,
                "Cloud backend URL not set. Configure it in Settings first."
            ),
            LoginError::HttpRequest(msg) => write!(f, "Request failed: {msg}"),
            LoginError::Expired => write!(f, "Device code expired. Run `apexshot login` again."),
            LoginError::Denied => write!(f, "Authorization was denied."),
            LoginError::Server(msg) => write!(f, "Server error: {msg}"),
            LoginError::Local(msg) => write!(f, "Could not finish sign-in: {msg}"),
        }
    }
}

impl std::error::Error for LoginError {}

#[derive(Debug)]
pub enum LogoutError {
    NotLoggedIn,
    HttpRequest(String),
    Server(String),
}

impl std::fmt::Display for LogoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogoutError::NotLoggedIn => write!(f, "You are not logged in."),
            LogoutError::HttpRequest(msg) => write!(f, "Logout request failed: {msg}"),
            LogoutError::Server(msg) => write!(f, "Server error: {msg}"),
        }
    }
}

impl std::error::Error for LogoutError {}

pub fn needs_backend_url(config: &AppConfig) -> bool {
    // Public installs always resolve to DEFAULT_CLOUD_BACKEND_URL when empty.
    resolve_cloud_backend_url(config).is_empty()
}

/// Ensure the config carries a resolved backend URL so later request paths
/// match, persisting it when it was empty.
fn ensure_backend_url(config: &mut AppConfig) -> Result<String, LoginError> {
    if needs_backend_url(config) {
        return Err(LoginError::NotConfigured);
    }
    let backend_url = resolve_cloud_backend_url(config);
    if config.cloud_backend_url.trim().is_empty() {
        config.cloud_backend_url = backend_url.clone();
        let _ = save_config(config);
    }
    Ok(backend_url)
}

fn ensure_install_id(config: &mut AppConfig) {
    if config.cloud_install_id.is_empty() {
        config.cloud_install_id = generate_install_id();
        let _ = save_config(config);
    }
}

/// Begin a device-authorization login. Returns the code the user must enter at
/// [`DeviceLogin::verification_uri`]. Follow with [`poll_device_login`].
pub fn begin_device_login() -> Result<DeviceLogin, LoginError> {
    let mut config = load_config();
    let backend_url = ensure_backend_url(&mut config)?;
    ensure_install_id(&mut config);

    let device_body = serde_json::json!({
        "client_id": "apexshot-desktop",
        "device_name": device_name(),
        "install_id": config.cloud_install_id,
    })
    .to_string();
    let device_resp: DeviceCodeResponse = agent()
        .post(&format!("{backend_url}/v1/auth/device"))
        .set("Content-Type", "application/json")
        .send_string(&device_body)
        .map_err(|e| LoginError::HttpRequest(e.to_string()))?
        .into_json()
        .map_err(|e| LoginError::Server(format!("Invalid response: {e}")))?;

    Ok(DeviceLogin {
        device_code: device_resp.device_code,
        user_code: format_user_code(&device_resp.user_code),
        verification_uri: device_resp.verification_uri,
        interval_secs: device_resp.interval.max(1) as u64,
        expires_in_secs: match device_resp.expires_in {
            seconds if seconds > 0 => seconds as u64,
            _ => MAX_POLL_SECONDS,
        },
    })
}

/// Poll once for a started login. `Ok(None)` means the user has not authorized
/// yet; `Ok(Some(account))` means the session was created, tokens and the
/// account were saved, and auto-upload-after-capture is on.
pub fn poll_device_login(device_code: &str) -> Result<Option<CloudAccount>, LoginError> {
    let config = load_config();
    let backend_url = resolve_cloud_backend_url(&config);
    if backend_url.is_empty() {
        return Err(LoginError::NotConfigured);
    }

    let poll_body = serde_json::json!({
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        "device_code": device_code,
    })
    .to_string();

    match agent()
        .post(&format!("{backend_url}/v1/auth/token"))
        .set("Content-Type", "application/json")
        .send_string(&poll_body)
    {
        Ok(resp) => {
            let token: TokenResponse = resp
                .into_json()
                .map_err(|e| LoginError::Server(format!("Invalid token response: {e}")))?;
            finish_login(token)
        }
        Err(ureq::Error::Status(400, resp)) => {
            let body: serde_json::Value = resp.into_json().unwrap_or(serde_json::Value::Null);
            device_poll_rejection(body["error"].as_str().unwrap_or(""))
        }
        Err(e) => Err(LoginError::HttpRequest(e.to_string())),
    }
}

/// Map the error code of a rejected device-token poll. `pending` keeps polling;
/// `access_denied` is the user refusing in the browser and must not look like a
/// server fault.
fn device_poll_rejection(error: &str) -> Result<Option<CloudAccount>, LoginError> {
    if error.contains("pending") {
        return Ok(None);
    }
    if error.contains("expired") {
        return Err(LoginError::Expired);
    }
    if error.contains("denied") {
        return Err(LoginError::Denied);
    }
    Err(LoginError::Server(error.to_string()))
}

/// Persist a completed device login: store tokens, cache the account (email +
/// tier), and turn on auto-upload after capture.
fn finish_login(token: TokenResponse) -> Result<Option<CloudAccount>, LoginError> {
    let mut config = load_config();
    let backend_url = resolve_cloud_backend_url(&config);
    config.cloud_api_token = token.access_token;
    config.cloud_refresh_token = token.refresh_token;

    let account: CloudAccount = agent()
        .get(&format!("{backend_url}/v1/account"))
        .set(
            "Authorization",
            &format!("Bearer {}", config.cloud_api_token),
        )
        .call()
        .map_err(|e| LoginError::HttpRequest(e.to_string()))?
        .into_json()
        .map_err(|e| LoginError::Server(format!("Invalid account response: {e}")))?;

    // Caches email + plan tier (and syncs the pro-plan flag) so the
    // entitlement is readable later without another request.
    account.apply_to_config(&mut config);
    config.cloud_auto_upload_after_capture = true;
    save_config(&config).map_err(|e| LoginError::Local(format!("{e:#}")))?;

    Ok(Some(account))
}

pub fn login() -> Result<(), LoginError> {
    let config = load_config();
    let was_logged_in = is_cloud_logged_in(&config);
    let previous_email = config.cloud_user_email.clone();

    let start = begin_device_login()?;
    println!("First copy your one-time code: {}", start.user_code);
    println!(
        "Press Enter to open {} in your browser...",
        start.verification_uri
    );

    let mut _input = String::new();
    let _ = std::io::stdin().read_line(&mut _input);

    open_browser(&start.verification_uri);

    let interval = start.interval_secs.max(1);
    let elapsed = std::time::Instant::now();

    loop {
        std::thread::sleep(Duration::from_secs(interval.max(POLL_INTERVAL)));

        if elapsed.elapsed().as_secs() > MAX_POLL_SECONDS {
            return Err(LoginError::Expired);
        }

        if poll_device_login(&start.device_code)?.is_none() {
            continue;
        }

        let config = load_config();
        println!("\n✓ Authentication complete.");
        println!("✓ Logged in as {}", config.cloud_user_email);
        println!("Your next screenshot will upload and copy a share link.");
        if was_logged_in && config.cloud_user_email == previous_email {
            println!("! You were already logged in to this account");
        }
        crate::utils::notify::desktop_notification_important(
            &crate::i18n::t("You're connected"),
            &crate::i18n::t("Your next screenshot gets a share link."),
        );
        return Ok(());
    }
}

pub fn logout() -> Result<(), LogoutError> {
    let mut config = load_config();

    if config.cloud_api_token.is_empty() {
        return Err(LogoutError::NotLoggedIn);
    }

    let backend_url = resolve_cloud_backend_url(&config);
    let revoke_body =
        serde_json::json!({ "token": config.cloud_api_token, "token_type_hint": "access_token" })
            .to_string();

    let revoke_result = agent()
        .post(&format!("{backend_url}/v1/auth/revoke"))
        .set("Content-Type", "application/json")
        .send_string(&revoke_body);

    match revoke_result {
        Ok(_) => {}
        Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
            // Token may already be expired/invalid — proceed with local cleanup.
        }
        Err(e) => return Err(LogoutError::HttpRequest(e.to_string())),
    }

    clear_session(&mut config);

    save_config(&config).map_err(|e| LogoutError::Server(format!("Failed to save config: {e}")))?;
    println!("✓ Logged out.");
    Ok(())
}

/// Forget the cached session in `config`: the tokens, the account, and the plan.
/// Revoking the token is the caller's job, so a sign-out can still clear this
/// device when the server cannot be reached.
pub fn clear_session(config: &mut AppConfig) {
    config.cloud_api_token.clear();
    config.cloud_refresh_token.clear();
    config.cloud_user_name.clear();
    config.cloud_user_email.clear();
    config.cloud_pro_plan = false;
    config.cloud_plan_tier.clear();
}

fn format_user_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() <= 4 {
        return chars.into_iter().collect();
    }
    let mid = chars.len() / 2;
    let (a, b) = chars.split_at(mid);
    format!(
        "{}-{}",
        a.iter().collect::<String>(),
        b.iter().collect::<String>()
    )
}

/// Hand a URL to the platform's default handler. Failure is not fatal: the
/// caller shows the link so the user can open it by hand.
fn open_browser(url: &str) {
    let _ = crate::utils::open::open_url(url);
}

#[cfg(target_os = "windows")]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "windows"))]
fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn platform_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        " (Windows)"
    } else {
        " (Linux)"
    }
}

fn device_name() -> String {
    const MAX_LEN: usize = 100;
    let suffix = platform_suffix();
    match hostname() {
        Some(host) => {
            let max_host = MAX_LEN.saturating_sub(suffix.len());
            let host = if host.chars().count() > max_host {
                host.chars().take(max_host).collect::<String>()
            } else {
                host
            };
            format!("{host}{suffix}")
        }
        None => format!("ApexShot{suffix}"),
    }
}

#[cfg(target_os = "windows")]
fn generate_install_id() -> String {
    use sha2::{Digest, Sha256};

    // Windows has no /dev/urandom to read. Hash the machine's boot-scoped
    // identifiers and the current time into a UUID-shaped install id; the
    // config persists the first one so later calls never need to match it.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = format!(
        "{}:{}:{}:{}",
        hostname().unwrap_or_default(),
        std::process::id(),
        stamp,
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    );
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(not(target_os = "windows"))]
fn generate_install_id() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok();
    if ok {
        buf[6] = (buf[6] & 0x0f) | 0x40;
        buf[8] = (buf[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]
        )
    } else {
        format!("install-{}", chrono::Utc::now().timestamp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_poll_keeps_waiting() {
        assert!(matches!(
            device_poll_rejection("authorization_pending"),
            Ok(None)
        ));
    }

    #[test]
    fn access_denied_maps_to_denied_not_server_error() {
        assert!(matches!(
            device_poll_rejection("access_denied"),
            Err(LoginError::Denied)
        ));
    }

    #[test]
    fn expired_device_code_maps_to_expired() {
        assert!(matches!(
            device_poll_rejection("expired_token"),
            Err(LoginError::Expired)
        ));
    }

    #[test]
    fn unknown_rejection_is_a_server_error() {
        assert!(matches!(
            device_poll_rejection("invalid_grant"),
            Err(LoginError::Server(code)) if code == "invalid_grant"
        ));
    }
}
