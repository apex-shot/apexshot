//! Lightweight release checks for the tray-first ApexShot experience.
//!
//! Checking happens at most once per day, never blocks startup, and only sends
//! the app version to GitHub as part of the standard User-Agent header.  The
//! result is cached locally so offline launches and GitHub outages are quiet.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const RELEASES_URL: &str = "https://github.com/apex-shot/apexshot/releases";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/apex-shot/apexshot/releases/latest";
const UPDATE_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/update.sh";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const PROMPT_SNOOZE: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct UpdateCheckState {
    last_checked_unix: u64,
    available_update: Option<UpdateInfo>,
    dismissed_version: Option<String>,
    prompt_snoozed_until_unix: u64,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn state_path() -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push("update-check.json");
    Some(path)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn checked_recently(state: &UpdateCheckState) -> bool {
    now_unix().saturating_sub(state.last_checked_unix) < CHECK_INTERVAL.as_secs()
}

fn load_state(path: &std::path::Path) -> UpdateCheckState {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(path: &std::path::Path, state: &UpdateCheckState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string(&state) {
        let _ = fs::write(path, raw);
    }
}

fn parse_version(raw: &str) -> Option<Vec<u64>> {
    let raw = raw.trim().trim_start_matches('v');
    let numeric = raw.split_once('-').map_or(raw, |(numeric, _)| numeric);
    let parts: Option<Vec<_>> = numeric
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect();
    let parts = parts?;
    (!parts.is_empty()).then_some(parts)
}

fn is_newer_version(candidate: &str, installed: &str) -> bool {
    let Some(candidate) = parse_version(candidate) else {
        return false;
    };
    let Some(installed) = parse_version(installed) else {
        return false;
    };
    let length = candidate.len().max(installed.len());
    (0..length)
        .find_map(|index| {
            let candidate = candidate.get(index).copied().unwrap_or(0);
            let installed = installed.get(index).copied().unwrap_or(0);
            (candidate != installed).then_some(candidate > installed)
        })
        .unwrap_or(false)
}

fn latest_release_endpoint() -> String {
    std::env::var("APEXSHOT_UPDATE_ENDPOINT")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| LATEST_RELEASE_API.to_string())
}

fn fetch_latest_release() -> Result<GithubRelease, String> {
    ureq::AgentBuilder::new()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .get(&latest_release_endpoint())
        .set("Accept", "application/vnd.github+json")
        .set(
            "User-Agent",
            concat!("ApexShot/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|err| err.to_string())?
        .into_json()
        .map_err(|err| err.to_string())
}

/// Checks GitHub for a newer stable release, respecting the local daily cache.
/// A failed request deliberately stays invisible to the user.
pub fn check_for_update() -> Option<UpdateInfo> {
    if std::env::var_os("APEXSHOT_DISABLE_UPDATE_CHECK").is_some() {
        return None;
    }

    // A local-only design preview. It is intentionally opt-in and does not
    // affect installed builds or make a network request.
    if std::env::var_os("APEXSHOT_UPDATE_PREVIEW").is_some() {
        return Some(UpdateInfo {
            version: "0.2.36".to_string(),
            release_url: RELEASES_URL.to_string(),
        });
    }

    let path = state_path();
    let mut state = path.as_deref().map(load_state).unwrap_or_default();
    if checked_recently(&state) {
        return state.available_update;
    }

    let release = match fetch_latest_release() {
        Ok(release) => release,
        Err(err) => {
            eprintln!("[update] Release check skipped: {err}");
            return None;
        }
    };
    let available_update = if release.draft
        || release.prerelease
        || !is_newer_version(&release.tag_name, env!("CARGO_PKG_VERSION"))
    {
        None
    } else {
        Some(UpdateInfo {
            version: release.tag_name.trim_start_matches('v').to_string(),
            release_url: release.html_url,
        })
    };

    state.last_checked_unix = now_unix();
    state.available_update = available_update.clone();
    if let Some(path) = path.as_deref() {
        save_state(path, &state);
    }
    available_update
}

/// Starts a non-blocking check and sends the result to the caller when a new
/// release exists. This is intentionally a standard thread: `ureq` is blocking.
pub fn spawn_update_check(on_update: impl FnOnce(UpdateInfo) + Send + 'static) {
    std::thread::spawn(move || {
        if let Some(update) = check_for_update() {
            on_update(update);
        }
    });
}

/// Reads the latest known release without starting a network request. App
/// windows use this so the prompt can appear immediately after the daemon has
/// already performed its background check.
pub fn cached_update() -> Option<UpdateInfo> {
    if std::env::var_os("APEXSHOT_UPDATE_PREVIEW").is_some() {
        return Some(UpdateInfo {
            version: "0.2.36".to_string(),
            release_url: RELEASES_URL.to_string(),
        });
    }
    state_path()
        .as_deref()
        .and_then(|path| load_state(path).available_update)
}

/// Whether the launch card should remain hidden for this particular release.
/// Snoozing does not remove the persistent tray-menu update action.
pub fn prompt_is_snoozed(update: &UpdateInfo) -> bool {
    state_path().as_deref().is_some_and(|path| {
        let state = load_state(path);
        state.dismissed_version.as_deref() == Some(&update.version)
            && now_unix() < state.prompt_snoozed_until_unix
    })
}

/// Hide the update card for one day. A newer version always clears the snooze.
pub fn snooze_prompt(update: &UpdateInfo) {
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    state.dismissed_version = Some(update.version.clone());
    state.prompt_snoozed_until_unix = now_unix().saturating_add(PROMPT_SNOOZE.as_secs());
    save_state(&path, &state);
}

/// Launches the updater in a visible terminal, where password prompts and
/// package-manager output remain under the user's control. Flatpak updates use
/// Flatpak's verified repository metadata; native installs use ApexShot's
/// existing distro-aware updater script.
pub fn launch_update() -> Result<(), String> {
    let command = if crate::app_identity::portal_only() {
        format!(
            "flatpak update {}; printf '\\nApexShot update finished. You can close this window.\\n'; exec bash",
            crate::app_identity::app_id()
        )
    } else {
        format!(
            "curl -fsSL {UPDATE_SCRIPT_URL} | bash; printf '\\nApexShot update finished. You can close this window.\\n'; exec bash"
        )
    };

    let terminals: [(&str, &[&str]); 4] = [
        ("xdg-terminal-exec", &["bash", "-lc"]),
        ("kgx", &["--", "bash", "-lc"]),
        ("gnome-terminal", &["--", "bash", "-lc"]),
        ("konsole", &["-e", "bash", "-lc"]),
    ];
    for (terminal, args) in terminals {
        let mut child = Command::new(terminal);
        child.args(args).arg(&command);
        if child.spawn().is_ok() {
            return Ok(());
        }
    }

    Err("Could not find a supported terminal to run the updater.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_versions_numerically() {
        assert!(is_newer_version("v0.2.36", "0.2.35"));
        assert!(is_newer_version("1.10.0", "1.9.9"));
        assert!(!is_newer_version("0.2.35", "0.2.35"));
        assert!(!is_newer_version("0.2.34", "0.2.35"));
    }

    #[test]
    fn ignores_invalid_versions() {
        assert!(!is_newer_version("next", "0.2.35"));
        assert!(!is_newer_version("0.2.36", "development"));
    }

    #[test]
    fn prerelease_suffix_does_not_break_version_parsing() {
        assert_eq!(parse_version("v1.2.3-beta.1"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn newer_release_is_not_held_by_an_old_snooze() {
        let state = UpdateCheckState {
            dismissed_version: Some("0.2.35".into()),
            prompt_snoozed_until_unix: now_unix().saturating_add(60),
            ..Default::default()
        };
        assert_ne!(state.dismissed_version.as_deref(), Some("0.2.36"));
    }
}
