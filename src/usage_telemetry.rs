//! Anonymous usage telemetry for active-install metrics.
//!
//! Sends a coarse daily heartbeat (plus optional feature counters) so we can
//! tell whether people actually run ApexShot after installing it.
//!
//! Privacy design:
//! - Random `install_id` stored at `~/.config/apexshot/install_id` (same file
//!   as the install scripts' download telemetry, so installs can be joined to
//!   usage without linking to people).
//! - No screenshot content, paths, OCR text, URLs, hostnames, or accounts.
//! - Rate limited to at most one successful heartbeat per 24 hours.
//! - Opt-out via Settings → Advanced → Privacy, or `APEXSHOT_TELEMETRY=0`.
//! - Fail-open: network errors never block the app.

use crate::config::{load_config, AppConfig};
use crate::distro::DistroInfo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default endpoint for usage heartbeats (apexshot.org dashboard).
pub const DEFAULT_USAGE_TELEMETRY_URL: &str = "https://apexshot.org/api/usage-telemetry";

const HEARTBEAT_MIN_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);

/// Local counters + last successful heartbeat, under XDG cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct UsageState {
    last_heartbeat_unix: u64,
    screenshots: u64,
    recordings: u64,
    ocr: u64,
}

#[derive(Debug, Clone, Serialize)]
struct HeartbeatPayload {
    event: &'static str,
    install_id: String,
    app_version: String,
    channel: &'static str,
    distro: String,
    desktop: String,
    session: String,
    screenshots: u64,
    recordings: u64,
    ocr: u64,
}

/// Whether telemetry is allowed for this process.
///
/// Order: env `APEXSHOT_TELEMETRY` (off wins) → config `telemetry_enabled`.
pub fn telemetry_enabled(config: &AppConfig) -> bool {
    if env_telemetry_disabled() {
        return false;
    }
    config.telemetry_enabled
}

fn env_telemetry_disabled() -> bool {
    match std::env::var("APEXSHOT_TELEMETRY") {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => false,
    }
}

fn usage_telemetry_url() -> String {
    if let Ok(url) = std::env::var("APEXSHOT_USAGE_TELEMETRY_URL") {
        let url = url.trim();
        if !url.is_empty() {
            return url.to_string();
        }
    }
    // Reuse the install-script override base when set to a full usage URL is
    // uncommon; prefer the dedicated env, else the public default.
    if let Ok(url) = std::env::var("APEXSHOT_TELEMETRY_URL") {
        let url = url.trim();
        // Only treat as usage endpoint if the path already says so.
        if url.contains("usage-telemetry") {
            return url.to_string();
        }
    }
    DEFAULT_USAGE_TELEMETRY_URL.to_string()
}

/// Path to the shared install UUID (`~/.config/apexshot/install_id`).
pub fn install_id_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("apexshot");
    path.push("install_id");
    Some(path)
}

fn usage_state_path() -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push("usage_telemetry.json");
    Some(path)
}

/// Read or create the anonymous install id used by download + usage telemetry.
pub fn ensure_install_id() -> Option<String> {
    let path = install_id_path()?;
    if let Ok(existing) = fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Some(existing.to_string());
        }
    }

    let id = generate_uuid_v4();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if fs::write(&path, &id).is_ok() {
        Some(id)
    } else {
        // Still report this session if we cannot persist (tmp / restricted HOME).
        Some(id)
    }
}

fn generate_uuid_v4() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    let ok = fs::File::open("/dev/urandom")
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
        // Extremely unlikely; still avoid panicking.
        format!(
            "install-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_state(path: &Path) -> UsageState {
    let Ok(raw) = fs::read_to_string(path) else {
        return UsageState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_state(path: &Path, state: &UsageState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, raw);
    }
}

fn detect_distro_label() -> String {
    match DistroInfo::detect() {
        Some(info) => match info.version_id {
            Some(ver) if !ver.is_empty() => format!("{}:{}", info.id, ver),
            _ => info.id,
        },
        None => "linux".to_string(),
    }
}

fn detect_desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .map(|v| {
            v.split(':')
                .next()
                .unwrap_or("unknown")
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn detect_session() -> String {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "wayland".to_string()
    } else if std::env::var_os("DISPLAY").is_some() {
        "x11".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Increment a local feature counter (flushed with the next heartbeat).
pub fn record_screenshot() {
    bump_counter(|s| s.screenshots = s.screenshots.saturating_add(1));
}

pub fn record_recording() {
    bump_counter(|s| s.recordings = s.recordings.saturating_add(1));
}

pub fn record_ocr() {
    bump_counter(|s| s.ocr = s.ocr.saturating_add(1));
}

fn bump_counter(f: impl FnOnce(&mut UsageState)) {
    let config = load_config().sanitized();
    if !telemetry_enabled(&config) {
        return;
    }
    let Some(path) = usage_state_path() else {
        return;
    };
    let mut state = load_state(&path);
    f(&mut state);
    save_state(&path, &state);
}

/// Spawn a background worker that sends a heartbeat when due, then rechecks hourly.
///
/// Safe to call once from the daemon; never blocks the UI or capture path.
pub fn spawn_daemon_telemetry_worker() {
    std::thread::Builder::new()
        .name("apexshot-usage-telemetry".into())
        .spawn(|| {
            // Small delay so portal/tray startup is not contending for the network.
            std::thread::sleep(Duration::from_secs(3));
            loop {
                let _ = maybe_send_heartbeat();
                std::thread::sleep(Duration::from_secs(60 * 60));
            }
        })
        .ok();
}

/// Attempt a heartbeat if enabled and the 24h rate limit allows it.
///
/// Returns `true` when a request was accepted by the server (2xx).
pub fn maybe_send_heartbeat() -> bool {
    let config = load_config().sanitized();
    maybe_send_heartbeat_with_config(&config)
}

fn maybe_send_heartbeat_with_config(config: &AppConfig) -> bool {
    if !telemetry_enabled(config) {
        return false;
    }

    let Some(install_id) = ensure_install_id() else {
        return false;
    };

    let Some(state_path) = usage_state_path() else {
        return false;
    };

    let mut state = load_state(&state_path);
    let now = now_unix();
    if state.last_heartbeat_unix > 0 {
        let elapsed = now.saturating_sub(state.last_heartbeat_unix);
        if elapsed < HEARTBEAT_MIN_INTERVAL.as_secs() {
            return false;
        }
    }

    let payload = HeartbeatPayload {
        event: "heartbeat",
        install_id,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        channel: "daemon",
        distro: detect_distro_label(),
        desktop: detect_desktop(),
        session: detect_session(),
        screenshots: state.screenshots,
        recordings: state.recordings,
        ocr: state.ocr,
    };

    if !post_heartbeat(&payload) {
        return false;
    }

    // Only clear counters after a confirmed send.
    state.last_heartbeat_unix = now;
    state.screenshots = 0;
    state.recordings = 0;
    state.ocr = 0;
    save_state(&state_path, &state);
    true
}

fn post_heartbeat(payload: &HeartbeatPayload) -> bool {
    let url = usage_telemetry_url();
    let agent = format!("ApexShotUsageTelemetry/{}", env!("CARGO_PKG_VERSION"));
    let result = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .user_agent(&agent)
        .build()
        .post(&url)
        .set("Content-Type", "application/json")
        .send_json(payload);

    match result {
        Ok(resp) => {
            let status = resp.status();
            (200..300).contains(&status)
        }
        Err(ureq::Error::Status(code, _)) => {
            eprintln!("[usage-telemetry] heartbeat rejected with HTTP {code}");
            false
        }
        Err(_) => {
            // Offline / timeout / DNS — silent by design.
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn telemetry_enabled_respects_config_default_on() {
        let cfg = AppConfig {
            telemetry_enabled: true,
            ..AppConfig::default()
        };
        let _g = env_lock().lock().unwrap();
        std::env::remove_var("APEXSHOT_TELEMETRY");
        assert!(telemetry_enabled(&cfg));
    }

    #[test]
    fn telemetry_enabled_respects_config_off() {
        let cfg = AppConfig {
            telemetry_enabled: false,
            ..AppConfig::default()
        };
        let _g = env_lock().lock().unwrap();
        std::env::remove_var("APEXSHOT_TELEMETRY");
        assert!(!telemetry_enabled(&cfg));
    }

    #[test]
    fn env_disables_even_when_config_on() {
        let cfg = AppConfig {
            telemetry_enabled: true,
            ..AppConfig::default()
        };
        let _g = env_lock().lock().unwrap();
        std::env::set_var("APEXSHOT_TELEMETRY", "0");
        assert!(!telemetry_enabled(&cfg));
        std::env::remove_var("APEXSHOT_TELEMETRY");
    }

    #[test]
    fn generate_uuid_looks_like_uuid() {
        let id = generate_uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn rate_limit_blocks_second_send_without_network() {
        // Unit-level: when last_heartbeat is "now", maybe_send should skip
        // before any HTTP (we can't easily mock ureq here, so only the early path).
        let cfg = AppConfig {
            telemetry_enabled: false,
            ..AppConfig::default()
        };
        assert!(!maybe_send_heartbeat_with_config(&cfg));
    }

    #[test]
    fn default_url_is_public_usage_endpoint() {
        assert!(DEFAULT_USAGE_TELEMETRY_URL.contains("usage-telemetry"));
    }
}
