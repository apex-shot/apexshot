//! Local usage counters for the Windows heartbeat.
//!
//! Windows usage telemetry is opt-in (`windows_telemetry_enabled`). The
//! editor processes only keep counts in `windows-usage-telemetry.json`, next to
//! the shell's `settings.json`; the C++ capture shell reads that file and sends
//! the counts in its daily heartbeat. Counts only: no capture contents, paths,
//! or account identities.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Counter file shared with the C++ capture shell.
const STATE_FILE: &str = "windows-usage-telemetry.json";
/// The shell's file for the local Studio nudge counters.
const STUDIO_NUDGES_FILE: &str = "studio-nudges.json";
/// The dismissal counter the card increments in that file.
const CLOUD_NUDGE_DISMISSALS: &str = "cloud_nudge_dismissals";
/// The shell stores counters as signed ints, so stay well inside that range.
const MAX_COUNT: u64 = 999_999_999;
/// Keeps temporary names apart when two writes start in the same nanosecond.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn usage_state_path() -> Option<PathBuf> {
    crate::settings::actions::shell_settings_path().and_then(|path| {
        path.parent()
            .filter(|directory| !directory.as_os_str().is_empty())
            .map(|directory| directory.join(STATE_FILE))
    })
}

fn telemetry_enabled() -> bool {
    if env_telemetry_disabled() {
        return false;
    }
    crate::config::load_config().windows_telemetry_enabled
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

fn bump_counter(key: &str) {
    if !telemetry_enabled() {
        return;
    }
    let Some(path) = usage_state_path() else {
        return;
    };
    let _ = increment_counter(&path, key);
}

fn read_state(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

/// Increment one counter, preserving every other key the shell keeps there.
///
/// The shell and an editor process can write this file at nearly the same
/// moment; the rename below keeps either copy whole, and losing the odd
/// increment is acceptable for a coarse daily count.
fn increment_counter(path: &Path, key: &str) -> std::io::Result<u64> {
    let mut state = read_state(path);
    let current = state.get(key).and_then(|value| value.as_u64()).unwrap_or(0);
    let next = current.saturating_add(1).min(MAX_COUNT);
    state.insert(key.to_owned(), serde_json::Value::from(next));
    write_state(path, &state)?;
    Ok(next)
}

fn write_state(
    path: &Path,
    state: &serde_json::Map<String, serde_json::Value>,
) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    let temp = temp_path(path);
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(STATE_FILE);
    path.with_file_name(format!(
        ".{name}.{}.{stamp}.{sequence}.tmp",
        std::process::id()
    ))
}

/// An Upload button was pressed while signed out of ApexShot Cloud.
pub fn record_upload_click_signed_out() {
    bump_counter("upload_click_signed_out");
}

/// A device sign-in flow started.
pub fn record_login_started() {
    bump_counter("login_started");
}

/// A device sign-in flow finished and the session was saved.
pub fn record_login_completed() {
    bump_counter("login_completed");
}

/// The Cloud upgrade card was shown.
pub fn record_upgrade_card_shown() {
    bump_counter("upgrade_card_shown");
}

/// The upgrade card's Go Pro button was pressed.
pub fn record_upgrade_clicked() {
    bump_counter("upgrade_clicked");
}

/// An image editor window was opened.
pub fn record_editor_opened() {
    bump_counter("editor_opened");
}

/// An unlicensed document reached the Studio preview mark for the first time
/// in an editor window.
pub fn record_studio_tool_opened() {
    bump_counter("studio_tool_opened");
}

/// A finished Studio output consumed a free trial export.
pub fn record_studio_trial_export() {
    bump_counter("studio_trial_export");
}

/// The Studio card was raised over an editor to block an export.
pub fn record_studio_upsell_shown() {
    bump_counter("studio_upsell_shown");
}

/// The Studio card's upgrade button opened the purchase page.
pub fn record_studio_buy_clicked() {
    bump_counter("studio_buy_clicked");
}

/// The Quick Access Cloud nudge was revealed after a Copy. Recorded by the
/// Windows Quick Access nudge once it lands, matching the Linux counters.
#[allow(dead_code)]
pub fn record_cloud_nudge_shown() {
    bump_counter("cloud_nudge_shown");
}

/// The Quick Access Cloud nudge's Get link button was pressed.
pub fn record_cloud_nudge_clicked() {
    bump_counter("cloud_nudge_clicked");
}

/// The Quick Access Cloud nudge was dismissed.
pub fn record_cloud_nudge_dismissed() {
    bump_counter("cloud_nudge_dismissed");
}

/// One Cloud nudge dismissal, for the shell's own `studio-nudges.json`.
///
/// The shell counts the shows itself and owns that file; the nudge card runs
/// in this process, so this is a read-modify-write that keeps every other key
/// the shell stores there. It is not telemetry, so the telemetry switch does
/// not gate it.
pub fn persist_cloud_nudge_dismissal() {
    let Some(path) = studio_nudges_path() else {
        return;
    };
    let _ = increment_counter(&path, CLOUD_NUDGE_DISMISSALS);
}

/// Path of the shell's local Studio nudge counters, beside its settings mirror.
fn studio_nudges_path() -> Option<PathBuf> {
    Some(crate::settings::actions::shell_settings_path()?.with_file_name(STUDIO_NUDGES_FILE))
}

#[cfg(test)]
pub(crate) fn ensure_install_id() -> Option<String> {
    Some("12345678-1234-4123-8123-123456789abc".into())
}

#[cfg(not(test))]
pub(crate) fn ensure_install_id() -> Option<String> {
    let path = dirs::config_dir()?.join("apexshot").join("install_id");
    load_or_create(&path)
}

#[cfg_attr(test, allow(dead_code))]
fn load_or_create(path: &Path) -> Option<String> {
    if let Ok(id) = fs::read_to_string(path) {
        return valid_id(id.trim()).then(|| id.trim().to_owned());
    }
    fs::create_dir_all(path.parent()?).ok()?;
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let id = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    );
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(id.as_bytes()).ok()?;
            file.sync_all().ok()?;
            Some(id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = fs::read_to_string(path).ok()?;
            valid_id(existing.trim()).then(|| existing.trim().to_owned())
        }
        Err(_) => None,
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-usage-telemetry-test-{}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn generated_id_is_persistent_and_valid() {
        let dir = test_dir("install-id");
        let path = dir.join("install_id");
        let first = load_or_create(&path).unwrap();
        assert!(valid_id(&first));
        assert_eq!(load_or_create(&path).as_deref(), Some(first.as_str()));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn increments_one_counter_and_preserves_the_rest() {
        let dir = test_dir("increment");
        let path = dir.join(STATE_FILE);
        fs::write(
            &path,
            r#"{"install_id":"abc","last_heartbeat":42,"screenshots":3,"ocr":7}"#,
        )
        .unwrap();

        assert_eq!(increment_counter(&path, "studio_buy_clicked").unwrap(), 1);
        assert_eq!(increment_counter(&path, "studio_buy_clicked").unwrap(), 2);
        assert_eq!(increment_counter(&path, "editor_opened").unwrap(), 1);

        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["install_id"], "abc");
        assert_eq!(value["last_heartbeat"], 42);
        assert_eq!(value["screenshots"], 3);
        assert_eq!(value["ocr"], 7);
        assert_eq!(value["studio_buy_clicked"], 2);
        assert_eq!(value["editor_opened"], 1);
        assert!(!dir.read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_a_missing_state_file_and_saturates() {
        let dir = test_dir("saturate");
        let path = dir.join(STATE_FILE);

        assert_eq!(increment_counter(&path, "login_started").unwrap(), 1);
        fs::write(&path, format!(r#"{{"login_started":{MAX_COUNT}}}"#)).unwrap();
        assert_eq!(
            increment_counter(&path, "login_started").unwrap(),
            MAX_COUNT
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_state_starts_over_without_panicking() {
        let dir = test_dir("malformed");
        let path = dir.join(STATE_FILE);
        fs::write(&path, b"not json").unwrap();

        assert_eq!(increment_counter(&path, "upgrade_clicked").unwrap(), 1);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["upgrade_clicked"], 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
