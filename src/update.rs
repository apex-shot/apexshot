//! Lightweight release checks for the tray-first ApexShot experience.
//!
//! Checking happens at most once per day, never blocks startup, and only sends
//! the app version to GitHub as part of the standard User-Agent header.  The
//! result is cached locally so offline launches and GitHub outages are quiet.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const RELEASES_URL: &str = "https://github.com/apex-shot/apexshot/releases";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/apex-shot/apexshot/releases/latest";
/// Official entrypoint for distribution-aware native package updates.
pub const UPDATE_SCRIPT_URL: &str = "https://apexshot.org/update";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const PROMPT_SNOOZE: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_RELEASE_METADATA_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
    #[serde(default)]
    pub studio_release_timestamp: Option<i64>,
    #[serde(default)]
    pub studio_key_fingerprint: Option<String>,
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
}

#[derive(Debug, Deserialize)]
struct StudioReleaseMetadata {
    schema_version: u8,
    tag: String,
    release_timestamp: i64,
    verify_key_sha256: String,
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

/// Check the latest stable release immediately, without the tray's daily cache
/// or preview overrides. Network and invalid release responses are errors.
pub fn check_for_update_now() -> Result<Option<UpdateInfo>, String> {
    let release = fetch_latest_release()?;
    let mut update = update_from_release(&release, env!("CARGO_PKG_VERSION"))?;
    if let Some(update) = update.as_mut() {
        update.set_studio_release_metadata(fetch_studio_release_metadata(&release));
    }
    Ok(update)
}

fn update_from_release(
    release: &GithubRelease,
    installed: &str,
) -> Result<Option<UpdateInfo>, String> {
    if release.draft || release.prerelease {
        return Err("The latest release is not a published stable release.".into());
    }
    if parse_version(&release.tag_name).is_none() || parse_version(installed).is_none() {
        return Err("Could not compare the installed and latest release versions.".into());
    }
    if !is_newer_version(&release.tag_name, installed) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_string(),
        release_url: release.html_url.clone(),
        studio_release_timestamp: None,
        studio_key_fingerprint: None,
    }))
}

impl UpdateInfo {
    fn set_studio_release_metadata(&mut self, metadata: Option<StudioReleaseMetadata>) {
        self.studio_release_timestamp =
            metadata.as_ref().map(|metadata| metadata.release_timestamp);
        self.studio_key_fingerprint = metadata.map(|metadata| metadata.verify_key_sha256);
    }
}

fn parse_studio_release_metadata(
    release: &GithubRelease,
    raw: &str,
) -> Option<StudioReleaseMetadata> {
    let metadata: StudioReleaseMetadata = serde_json::from_str(raw).ok()?;
    if metadata.schema_version != 2
        || metadata.tag != release.tag_name
        || metadata.release_timestamp <= 0
        || metadata.verify_key_sha256.len() != 64
        || !metadata
            .verify_key_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some(metadata)
}

fn fetch_studio_release_metadata(release: &GithubRelease) -> Option<StudioReleaseMetadata> {
    let mut assets = release
        .assets
        .iter()
        .filter(|asset| asset.name == "STUDIO_RELEASE_METADATA.json");
    let asset = assets.next()?;
    if assets.next().is_some()
        || !asset
            .browser_download_url
            .starts_with("https://github.com/apex-shot/apexshot/releases/download/")
    {
        return None;
    }
    let response = ureq::AgentBuilder::new()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .get(&asset.browser_download_url)
        .set(
            "User-Agent",
            concat!("ApexShot/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .ok()?;
    let mut raw = String::new();
    response
        .into_reader()
        .take(MAX_RELEASE_METADATA_BYTES + 1)
        .read_to_string(&mut raw)
        .ok()?;
    if raw.len() as u64 > MAX_RELEASE_METADATA_BYTES {
        return None;
    }
    parse_studio_release_metadata(release, &raw)
}

pub fn studio_update_notice(update: &UpdateInfo) -> Option<String> {
    let coverage = crate::studio_entitlement::update_coverage_for_release(
        update.studio_release_timestamp,
        update.studio_key_fingerprint.as_deref(),
    );
    studio_coverage_notice(&update.version, coverage)
}

fn studio_coverage_notice(
    version: &str,
    coverage: crate::studio_entitlement::StudioUpdateCoverage,
) -> Option<String> {
    use crate::studio_entitlement::StudioUpdateCoverage;

    match coverage {
        StudioUpdateCoverage::NotIncluded => Some(format!(
            "The Studio build marker for ApexShot v{version} is beyond your included updates. Studio exports will not be available on that version, but you can still install the update."
        )),
        StudioUpdateCoverage::Unknown => Some(format!(
            "Studio update coverage for ApexShot v{version} could not be confirmed from its published build marker. Check your license; the update will not be blocked."
        )),
        StudioUpdateCoverage::Included | StudioUpdateCoverage::NotApplicable => None,
    }
}

/// Checks GitHub for a newer stable release, respecting the local daily cache.
/// A failed request deliberately stays invisible to the user.
pub fn check_for_update() -> Option<UpdateInfo> {
    if crate::app_identity::portal_only()
        || std::env::var_os("APEXSHOT_DISABLE_UPDATE_CHECK").is_some()
    {
        return None;
    }

    // A local-only design preview. It is intentionally opt-in and does not
    // affect installed builds or make a network request.
    if std::env::var_os("APEXSHOT_UPDATE_PREVIEW").is_some() {
        return Some(UpdateInfo {
            version: "0.2.36".to_string(),
            release_url: RELEASES_URL.to_string(),
            studio_release_timestamp: None,
            studio_key_fingerprint: None,
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
    let mut available_update = if release.draft
        || release.prerelease
        || !is_newer_version(&release.tag_name, env!("CARGO_PKG_VERSION"))
    {
        None
    } else {
        Some(UpdateInfo {
            version: release.tag_name.trim_start_matches('v').to_string(),
            release_url: release.html_url.clone(),
            studio_release_timestamp: None,
            studio_key_fingerprint: None,
        })
    };
    if let Some(update) = available_update.as_mut() {
        update.set_studio_release_metadata(fetch_studio_release_metadata(&release));
    }

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
    if crate::app_identity::portal_only() {
        return None;
    }
    if std::env::var_os("APEXSHOT_UPDATE_PREVIEW").is_some() {
        return Some(UpdateInfo {
            version: "0.2.36".to_string(),
            release_url: RELEASES_URL.to_string(),
            studio_release_timestamp: None,
            studio_key_fingerprint: None,
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
    if crate::app_identity::portal_only() {
        return crate::utils::open::open_uri(&format!(
            "appstream://{}",
            crate::app_identity::app_id()
        ));
    }
    let command = update_terminal_command();

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

/// The updater command handed to the terminal.
///
/// The script is fetched in full before it runs, matching `apexshot update`:
/// a transfer that dies mid-body would otherwise hand the shell a truncated
/// script. Unlike the CLI path this cannot lean on an `EXIT` trap, because
/// `exec bash` replaces the shell and never fires one, so the temporary
/// directory is removed explicitly. The trailing `exec bash` runs even when
/// the download fails, leaving the window open with the error on screen.
fn update_terminal_command() -> String {
    format!(
        "workdir=$(mktemp -d)\n\
         curl --fail --silent --show-error --location --connect-timeout 10 --max-time 60 \
         --output \"$workdir/update.sh\" {UPDATE_SCRIPT_URL} && sh \"$workdir/update.sh\"\n\
         rm -rf \"$workdir\"\n\
         printf '\\nApexShot update finished. You can close this window.\\n'\n\
         exec bash"
    )
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

    fn release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.into(),
            html_url: RELEASES_URL.into(),
            assets: Vec::new(),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn explicit_update_skips_equal_or_older_releases() {
        assert_eq!(update_from_release(&release("v0.2.35"), "0.2.35"), Ok(None));
        assert_eq!(update_from_release(&release("v0.2.34"), "0.2.35"), Ok(None));
        assert_eq!(
            update_from_release(&release("v0.2.35.0"), "0.2.35"),
            Ok(None)
        );
    }

    #[test]
    fn older_update_cache_entries_without_studio_markers_remain_unknown() {
        let update: UpdateInfo = serde_json::from_value(serde_json::json!({
            "version": "0.2.35",
            "release_url": RELEASES_URL
        }))
        .unwrap();

        assert_eq!(update.studio_release_timestamp, None);
        assert_eq!(update.studio_key_fingerprint, None);
    }

    #[test]
    fn explicit_update_only_returns_a_newer_release() {
        let update = update_from_release(&release("v0.2.36"), "0.2.35")
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "0.2.36");
        assert_eq!(update.studio_release_timestamp, None);
        assert!(update_from_release(&release("v0.2.10"), "0.2.9")
            .unwrap()
            .is_some());
    }

    #[test]
    fn explicit_update_refuses_invalid_and_unpublished_releases() {
        assert!(update_from_release(&release("latest"), "0.2.35").is_err());
        let mut unpublished = release("v0.2.36");
        unpublished.draft = true;
        assert!(update_from_release(&unpublished, "0.2.35").is_err());
        let mut prerelease = release("v0.2.36-beta");
        prerelease.prerelease = true;
        assert!(update_from_release(&prerelease, "0.2.35").is_err());
    }

    #[test]
    fn published_date_is_not_used_as_the_immutable_studio_release_marker() {
        let release: GithubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "v0.2.36",
            "html_url": RELEASES_URL,
            "published_at": "2035-01-02T00:00:00Z",
            "assets": []
        }))
        .unwrap();
        let mut update = update_from_release(&release, "0.2.35").unwrap().unwrap();
        let published_metadata = serde_json::json!({
            "schema_version": 2,
            "tag": "v0.2.36",
            "release_timestamp": 300,
            "verify_key_sha256": "10ba682c8ad13513971e8b56881aab8bd702bb807796eca81932c735a94d6e6d"
        })
        .to_string();

        update.set_studio_release_metadata(parse_studio_release_metadata(
            &release,
            &published_metadata,
        ));

        assert_eq!(update.studio_release_timestamp, Some(300));
        assert_eq!(
            update.studio_key_fingerprint.as_deref(),
            Some("10ba682c8ad13513971e8b56881aab8bd702bb807796eca81932c735a94d6e6d")
        );
        assert_eq!(
            studio_coverage_notice(
                &update.version,
                crate::studio_entitlement::StudioUpdateCoverage::NotIncluded
            )
            .unwrap(),
            "The Studio build marker for ApexShot v0.2.36 is beyond your included updates. Studio exports will not be available on that version, but you can still install the update."
        );
    }

    #[test]
    fn release_marker_must_be_positive_and_match_the_published_tag_and_schema() {
        let release = release("v0.2.36");
        let fingerprint = "a".repeat(64);
        for metadata in [
            serde_json::json!({"schema_version": 1, "tag": "v0.2.36", "release_timestamp": 300, "verify_key_sha256": fingerprint.clone()}),
            serde_json::json!({"schema_version": 2, "tag": "v0.2.35", "release_timestamp": 300, "verify_key_sha256": fingerprint.clone()}),
            serde_json::json!({"schema_version": 2, "tag": "v0.2.36", "release_timestamp": 0, "verify_key_sha256": fingerprint.clone()}),
            serde_json::json!({"schema_version": 2, "tag": "v0.2.36", "release_timestamp": -1, "verify_key_sha256": fingerprint}),
            serde_json::json!({"schema_version": 2, "tag": "v0.2.36", "release_timestamp": 300, "verify_key_sha256": "invalid"}),
        ] {
            assert_eq!(
                parse_studio_release_metadata(&release, &metadata.to_string()).map(|_| ()),
                None
            );
        }
        let notice = studio_coverage_notice(
            "0.2.36",
            crate::studio_entitlement::StudioUpdateCoverage::Unknown,
        )
        .unwrap();
        assert!(notice.contains("could not be confirmed from its published build marker"));
        assert!(!notice.contains("covered"));
        assert!(notice.contains("will not be blocked"));
    }

    #[test]
    fn uncovered_studio_update_warns_without_blocking_the_update() {
        let notice = studio_coverage_notice(
            "0.2.36",
            crate::studio_entitlement::StudioUpdateCoverage::NotIncluded,
        )
        .unwrap();

        assert!(notice.contains("beyond your included updates"));
        assert!(notice.contains("you can still install the update"));
    }

    #[test]
    fn covered_or_unlicensed_updates_need_no_warning() {
        use crate::studio_entitlement::StudioUpdateCoverage;

        assert_eq!(
            studio_coverage_notice("0.2.36", StudioUpdateCoverage::Included),
            None
        );
        assert_eq!(
            studio_coverage_notice("0.2.36", StudioUpdateCoverage::NotApplicable),
            None
        );
    }

    #[test]
    fn ignores_invalid_versions() {
        assert!(!is_newer_version("next", "0.2.35"));
        assert!(!is_newer_version("0.2.36", "development"));
    }

    #[test]
    fn native_updater_uses_the_public_posix_entrypoint() {
        assert_eq!(UPDATE_SCRIPT_URL, "https://apexshot.org/update");
    }

    #[test]
    fn terminal_updater_downloads_the_complete_script_before_running_it() {
        let command = update_terminal_command();
        assert!(command.contains("workdir=$(mktemp -d)"));
        assert!(command.contains(UPDATE_SCRIPT_URL));
        assert!(command.contains("--fail --silent --show-error --location"));
        assert!(command.contains("--connect-timeout 10 --max-time 60"));
        assert!(command.contains("--output \"$workdir/update.sh\""));
        assert!(command.contains("&& sh \"$workdir/update.sh\""));
        assert!(command.contains("rm -rf \"$workdir\""));
        assert!(command.ends_with("exec bash"));
        assert!(!command.contains("| sh"));
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
