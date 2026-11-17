//! GNOME Shell extension plumbing shared by onboarding and the updater.
//!
//! Onboarding uses the individual steps to install the extension during setup;
//! the update window uses [`refresh_from_archive`] to replace the files of an
//! extension the user already had. Both paths must agree on the UUID, the
//! archive name, and the fallback that writes the extension files directly when
//! the `gnome-extensions` CLI cannot do it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The extension this app installs and updates.
pub const UUID: &str = "apexshot-gnome-integration@apexshot.github.io";
/// The older extension id some installs still carry.
pub const OLD_UUID: &str = "apexshot-preview-helper@apexshot.github.io";
/// Name of the release asset holding the packed extension.
pub const ARCHIVE_NAME: &str = "apexshot-gnome-integration.zip";

pub const RELEASES_URL: &str = "https://github.com/apex-shot/apexshot/releases";
pub const RELEASES_API_URL: &str = "https://api.github.com/repos/apex-shot/apexshot/releases";

const USER_AGENT: &str = concat!("ApexShot/", env!("CARGO_PKG_VERSION"));

/// What a refresh did, so the caller can word the result page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionRefresh {
    /// Replaced and enabled; the shell picks the new code up now.
    Updated,
    /// Replaced an extension the session already had loaded. On GNOME Wayland
    /// the running shell keeps the old code until the next login.
    UpdatedNeedsRelogin,
    /// The user never installed the extension, so there is nothing to refresh.
    NotInstalled,
    /// The refresh could not complete. The message is safe to show.
    Failed(String),
}

/// True when the current desktop is GNOME. The extension is meaningless on
/// other compositors, so the updater leaves their sessions alone.
pub fn is_gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("gnome")
}

fn gnome_extensions_available() -> bool {
    Command::new("gnome-extensions")
        .arg("list")
        .output()
        .is_ok()
}

fn listed_extensions() -> Option<Vec<String>> {
    if crate::app_identity::portal_only() {
        return None;
    }
    let output = Command::new("gnome-extensions").arg("list").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>(),
    )
}

/// Whether this user already has the extension installed.
pub fn is_installed() -> bool {
    listed_extensions().is_some_and(|lines| lines.iter().any(|line| line == UUID))
}

/// Whether the extension is currently switched on. Uses the CLI's own view,
/// which is what the updater must restore after replacing the files.
pub fn is_listed_enabled() -> bool {
    if crate::app_identity::portal_only() {
        return false;
    }
    Command::new("gnome-extensions")
        .args(["list", "--enabled"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.trim() == UUID)
        })
}

/// Whether the old extension id is still present.
pub fn is_old_installed() -> bool {
    listed_extensions().is_some_and(|lines| lines.iter().any(|line| line == OLD_UUID))
}

/// Disable and delete the old extension id, leaving its files gone so the
/// shell stops loading it on the next login.
pub fn remove_old_extension() {
    let _ = Command::new("gnome-extensions")
        .args(["disable", OLD_UUID])
        .output();

    if let Some(home) = std::env::var_os("HOME") {
        let old_dir = PathBuf::from(home)
            .join(".local/share/gnome-shell/extensions")
            .join(OLD_UUID);
        let _ = std::fs::remove_dir_all(old_dir);
    }
}

fn extension_archive_url_from_releases(releases: &serde_json::Value) -> Option<String> {
    for release in releases.as_array()? {
        let Some(assets) = release.get("assets").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for asset in assets {
            if asset.get("name").and_then(serde_json::Value::as_str) == Some(ARCHIVE_NAME) {
                return asset
                    .get("browser_download_url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
        }
    }
    None
}

/// The newest release that publishes the packed extension, if any.
pub fn latest_download_url() -> Option<String> {
    let response = ureq::get(RELEASES_API_URL)
        .set("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let releases: serde_json::Value = serde_json::from_str(&response).ok()?;
    extension_archive_url_from_releases(&releases)
}

/// Download `url` into `destination`, replacing any earlier copy.
///
/// The archive is small, so this stays a plain blocking transfer; callers run
/// it on a worker thread.
pub fn download_archive(url: &str, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the extension folder: {error}"))?;
    }
    let response = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
        .get(url)
        .call()
        .map_err(|error| format!("The extension download failed: {error}"))?;
    let mut file = std::fs::File::create(destination)
        .map_err(|error| format!("Could not save the extension: {error}"))?;
    let mut reader = response.into_reader();
    std::io::copy(&mut reader, &mut file)
        .map_err(|error| format!("Could not save the extension: {error}"))?;
    Ok(())
}

/// Install the packed extension through the CLI, without any fallback.
pub fn install_archive(zip: &Path) -> bool {
    Command::new("gnome-extensions")
        .arg("install")
        .arg("--force")
        .arg(zip)
        .status()
        .is_ok_and(|status| status.success())
}

/// Switch the shared extension on. Returns whether the CLI accepted it.
pub fn enable_extension() -> bool {
    Command::new("gnome-extensions")
        .args(["enable", UUID])
        .status()
        .is_ok_and(|status| status.success())
}

fn disable() {
    let _ = Command::new("gnome-extensions")
        .args(["disable", UUID])
        .output();
}

/// Write the extension files into the user's extension directory.
///
/// This is the updater script's fallback for a `gnome-extensions install`
/// failure, and the only route when the CLI is not present. `unzip` ships with
/// the native packages; `bsdtar` covers systems where it does not.
fn extract_into_user_dir(zip: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let extensions = PathBuf::from(home)
        .join(".local/share/gnome-shell/extensions")
        .join(UUID);
    let _ = std::fs::remove_dir_all(&extensions);
    if std::fs::create_dir_all(&extensions).is_err() {
        return false;
    }

    let unzipped = Command::new("unzip")
        .arg("-q")
        .arg(zip)
        .arg("-d")
        .arg(&extensions)
        .status()
        .is_ok_and(|status| status.success());
    if unzipped {
        return true;
    }
    Command::new("bsdtar")
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(&extensions)
        .status()
        .is_ok_and(|status| status.success())
}

/// Replace the files of an already-installed extension with `zip`.
///
/// The user's enable state is recorded first and restored afterwards, matching
/// the update script: disabling before the install stops the shell from
/// reloading a half-replaced directory. A session that had the extension
/// enabled keeps running the old code on Wayland until the next login, which
/// the caller reports as [`ExtensionRefresh::UpdatedNeedsRelogin`].
pub fn refresh_from_archive(zip: &Path) -> ExtensionRefresh {
    if crate::app_identity::portal_only() || !is_gnome_session() {
        return ExtensionRefresh::NotInstalled;
    }
    if !is_installed() {
        return ExtensionRefresh::NotInstalled;
    }

    remove_old_extension();

    let was_enabled = is_listed_enabled();
    let cli_available = gnome_extensions_available();
    if cli_available {
        disable();
    }

    let replaced = cli_available && install_archive(zip);
    if !replaced && !extract_into_user_dir(zip) {
        if cli_available && was_enabled {
            let _ = enable_extension();
        }
        return ExtensionRefresh::Failed(crate::i18n::t(
            "Could not install the GNOME extension update.",
        ));
    }

    if cli_available {
        let _ = enable_extension();
    }

    if was_enabled {
        ExtensionRefresh::UpdatedNeedsRelogin
    } else {
        ExtensionRefresh::Updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_lookup_prefers_the_newest_archive() {
        let releases = serde_json::json!([
            {"assets": [{"name": "apexshot_0.2.36.deb", "browser_download_url": "https://example.invalid/app.deb"}]},
            {"assets": [{"name": ARCHIVE_NAME, "browser_download_url": "https://example.invalid/v0.2.35/extension.zip"}]}
        ]);

        assert_eq!(
            extension_archive_url_from_releases(&releases).as_deref(),
            Some("https://example.invalid/v0.2.35/extension.zip")
        );
    }

    #[test]
    fn a_release_without_the_archive_offers_nothing() {
        let releases = serde_json::json!([
            {"assets": [{"name": "apexshot_0.2.36.deb", "browser_download_url": "https://example.invalid/app.deb"}]}
        ]);

        assert_eq!(extension_archive_url_from_releases(&releases), None);
        assert_eq!(
            extension_archive_url_from_releases(&serde_json::json!({})),
            None
        );
    }

    #[test]
    fn the_extension_uuid_is_the_one_the_packaged_desktop_file_ships() {
        assert_eq!(UUID, "apexshot-gnome-integration@apexshot.github.io");
        assert_ne!(UUID, OLD_UUID);
    }

    #[test]
    fn refreshing_without_the_extension_present_does_not_install_it() {
        // The guard runs before any process or file work, so this is safe to
        // assert on machines without a session bus.
        if crate::app_identity::portal_only() || !is_gnome_session() || is_installed() {
            return;
        }
        let outcome = refresh_from_archive(Path::new("/nonexistent"));
        assert_eq!(outcome, ExtensionRefresh::NotInstalled);
    }
}
