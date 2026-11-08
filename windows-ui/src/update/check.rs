//! Release discovery for the update window.
//!
//! The policy matches the Linux app: one stable-release lookup per day, cached
//! in the user's cache folder, and a per-version 24-hour snooze once the prompt
//! has been shown. A failed lookup is quiet and keeps whatever release was
//! found before, because that one is still newer than the installed build.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::Release;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/apex-shot/apexshot/releases/latest";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const PROMPT_SNOOZE: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
/// The Windows package publishes its installer as `ApexShot-<version>-setup.exe`.
const INSTALLER_SUFFIX: &str = "setup.exe";

/// The release the check found, kept so an offline launch still prompts.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Available {
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    #[serde(default)]
    pub asset_size: Option<u64>,
    #[serde(default)]
    pub release_url: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct CheckState {
    #[serde(default)]
    last_checked_unix: u64,
    #[serde(default)]
    available_update: Option<Available>,
    #[serde(default)]
    dismissed_version: Option<String>,
    #[serde(default)]
    prompt_snoozed_until_unix: u64,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubReleaseAsset>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Look for a newer stable release. Returns nothing while disabled, during the
/// daily cache window when no release was found, or while the prompt is
/// snoozed. A failed lookup reuses the last known release and does not count
/// as the day's check.
pub fn check() -> Option<Release> {
    if std::env::var_os("APEXSHOT_DISABLE_UPDATE_CHECK").is_some() {
        return None;
    }

    let path = state_path();
    let mut state = path.as_deref().map(load_state).unwrap_or_default();
    if !checked_recently(&state) {
        match fetch_latest_release() {
            Ok(release) => {
                state.last_checked_unix = now_unix();
                state.available_update =
                    available_from_release(&release, env!("CARGO_PKG_VERSION"));
                if let Some(path) = path.as_deref() {
                    save_state(path, &state);
                }
            }
            // A failed lookup does not count as the day's check: the next one
            // tries again instead of hiding the release for 24 hours.
            Err(error) => eprintln!("[update] release check skipped: {error}"),
        }
    }

    let available = state
        .available_update
        .as_ref()
        .filter(|available| is_newer_version(&available.version, env!("CARGO_PKG_VERSION")))?;
    if prompt_snoozed(&state, &available.version, now_unix()) {
        return None;
    }
    Some(available.clone().into())
}

/// Run the check and open the window when it finds a release to offer.
pub fn check_and_show() -> anyhow::Result<()> {
    match check() {
        Some(release) => super::show(release),
        None => Ok(()),
    }
}

/// Hide this version's prompt for a day. A newer release names a different
/// version, so it clears the snooze.
pub fn snooze(version: &str) {
    let Some(path) = state_path() else {
        return;
    };
    let mut state = load_state(&path);
    state.dismissed_version = Some(version.to_string());
    state.prompt_snoozed_until_unix = now_unix().saturating_add(PROMPT_SNOOZE.as_secs());
    save_state(&path, &state);
}

impl From<Available> for Release {
    fn from(available: Available) -> Self {
        Self {
            version: available.version,
            asset_name: available.asset_name,
            asset_url: available.asset_url,
            asset_size: available.asset_size,
            release_url: available.release_url,
        }
    }
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

fn checked_recently(state: &CheckState) -> bool {
    now_unix().saturating_sub(state.last_checked_unix) < CHECK_INTERVAL.as_secs()
}

fn prompt_snoozed(state: &CheckState, version: &str, now: u64) -> bool {
    state.dismissed_version.as_deref() == Some(version) && now < state.prompt_snoozed_until_unix
}

fn load_state(path: &std::path::Path) -> CheckState {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(path: &std::path::Path, state: &CheckState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string(state) {
        let _ = fs::write(path, raw);
    }
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
        .user_agent(concat!("ApexShot/", env!("CARGO_PKG_VERSION")))
        .build()
        .get(&latest_release_endpoint())
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?
        .into_json()
        .map_err(|error| error.to_string())
}

fn available_from_release(release: &GithubRelease, installed: &str) -> Option<Available> {
    if release.draft || release.prerelease || !is_newer_version(&release.tag_name, installed) {
        return None;
    }
    let asset = release
        .assets
        .iter()
        .find(|asset| is_installer(&asset.name))?;
    Some(Available {
        version: release.tag_name.trim().trim_start_matches('v').to_string(),
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: (asset.size > 0).then_some(asset.size),
        release_url: (!release.html_url.trim().is_empty()).then(|| release.html_url.clone()),
    })
}

fn is_installer(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(INSTALLER_SUFFIX)
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

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "tag_name": "v0.2.37",
        "html_url": "https://example.test/releases/tag/v0.2.37",
        "draft": false,
        "prerelease": false,
        "assets": [
            {
                "name": "apexshot_0.2.37-1_amd64.deb",
                "browser_download_url": "https://example.test/apexshot_0.2.37-1_amd64.deb",
                "size": 120000000
            },
            {
                "name": "ApexShot-0.2.37-setup.exe",
                "browser_download_url": "https://example.test/ApexShot-0.2.37-setup.exe",
                "size": 191754859
            }
        ]
    }"#;

    fn release(raw: &str) -> GithubRelease {
        serde_json::from_str(raw).expect("a release payload")
    }

    #[test]
    fn the_installer_is_chosen_among_the_other_assets() {
        let available = available_from_release(&release(RELEASE), "0.2.36").expect("an update");

        assert_eq!(available.version, "0.2.37");
        assert_eq!(available.asset_name, "ApexShot-0.2.37-setup.exe");
        assert_eq!(
            available.asset_url,
            "https://example.test/ApexShot-0.2.37-setup.exe"
        );
        assert_eq!(available.asset_size, Some(191_754_859));
        assert_eq!(
            available.release_url.as_deref(),
            Some("https://example.test/releases/tag/v0.2.37")
        );
    }

    #[test]
    fn a_release_without_an_installer_offers_nothing() {
        let linux_only = RELEASE.replace("ApexShot-0.2.37-setup.exe", "apexshot-0.2.37.rpm");
        assert!(available_from_release(&release(&linux_only), "0.2.36").is_none());

        let draft = RELEASE.replace("\"draft\": false", "\"draft\": true");
        assert!(available_from_release(&release(&draft), "0.2.36").is_none());

        let prerelease = RELEASE.replace("\"prerelease\": false", "\"prerelease\": true");
        assert!(available_from_release(&release(&prerelease), "0.2.36").is_none());
    }

    #[test]
    fn equal_or_older_releases_are_not_offered() {
        assert!(available_from_release(&release(RELEASE), "0.2.37").is_none());
        assert!(available_from_release(&release(RELEASE), "0.2.38").is_none());
        assert!(available_from_release(&release(RELEASE), "v0.2.36-alpha").is_some());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer_version("v0.2.37", "0.2.36"));
        assert!(is_newer_version("0.10.0", "0.9.9"));
        assert!(is_newer_version("0.2.36.1", "0.2.36"));
        assert!(!is_newer_version("0.2.36", "0.2.36"));
        assert!(!is_newer_version("0.2.35", "0.2.36"));
        assert!(!is_newer_version("latest", "0.2.36"));
    }

    #[test]
    fn a_snooze_hides_only_the_version_it_named() {
        let state = CheckState {
            dismissed_version: Some("0.2.37".to_string()),
            prompt_snoozed_until_unix: 1_000,
            ..CheckState::default()
        };

        assert!(prompt_snoozed(&state, "0.2.37", 999));
        assert!(!prompt_snoozed(&state, "0.2.37", 1_000));
        assert!(!prompt_snoozed(&state, "0.2.38", 999));
    }

    #[test]
    fn the_cached_release_round_trips_through_the_state_file() {
        let available = available_from_release(&release(RELEASE), "0.2.36").expect("an update");
        let state = CheckState {
            last_checked_unix: 42,
            available_update: Some(available.clone()),
            ..CheckState::default()
        };
        let raw = serde_json::to_string(&state).expect("state JSON");
        let loaded: CheckState = serde_json::from_str(&raw).expect("state parses");

        assert_eq!(loaded.last_checked_unix, 42);
        assert_eq!(loaded.available_update, Some(available));
    }
}
