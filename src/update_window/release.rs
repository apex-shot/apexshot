//! Release assets for the update window.
//!
//! The cached update check only knows the release tag, so the window resolves
//! the tag's assets itself before it offers an install. Everything that picks a
//! file, parses checksums, or decides a package format is pure and tested; the
//! network and command-line probes stay thin wrappers around it.

use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
#[cfg(test)]
use sha2::{Digest, Sha256};

use super::install::PackageFormat;

const RELEASE_API: &str = "https://api.github.com/repos/apex-shot/apexshot/releases/tags/v";
const USER_AGENT: &str = concat!("ApexShot/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Name of the published checksum manifest.
pub const CHECKSUMS_NAME: &str = "SHA256SUMS";
/// Name of the published GNOME extension archive.
pub const EXTENSION_NAME: &str = crate::gnome_extension::ARCHIVE_NAME;

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

/// The files this system needs from a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedAssets {
    pub package: ReleaseAsset,
    pub checksums: ReleaseAsset,
    pub extension: Option<ReleaseAsset>,
}

/// Why a release cannot be installed natively here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    /// No asset matched this distro's package format.
    NoPackage,
    /// The release has no checksum manifest.
    NoChecksums,
    /// The manifest does not list a file it must cover.
    NoChecksumEntry(String),
    /// A downloaded file's hash differs from the manifest.
    ChecksumMismatch(String),
}

impl AssetError {
    /// The message the window shows for this failure.
    pub fn message(&self) -> String {
        match self {
            Self::NoPackage => {
                crate::i18n::t("This release does not include a package for this system.")
            }
            Self::NoChecksums => crate::i18n::t("The release did not publish checksums."),
            Self::NoChecksumEntry(name) => crate::i18n::tfmt(
                "The release did not publish a checksum for {name}.",
                &[("name", name)],
            ),
            Self::ChecksumMismatch(_) => {
                crate::i18n::t("The download did not match the published checksum.")
            }
        }
    }
}

/// Fetch the assets of the release tagged `v{version}`.
pub fn fetch_assets(version: &str) -> Result<Vec<ReleaseAsset>, String> {
    let version = version.trim().trim_start_matches('v');
    let endpoint = format!("{RELEASE_API}{version}");
    let response = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .get(&endpoint)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(describe_request_error)?
        .into_json::<GithubRelease>()
        .map_err(|error| error.to_string())?;
    Ok(response.assets)
}

/// One line for the window: `ureq`'s text names the stage already, so only the
/// URL is dropped.
pub(super) fn describe_request_error(error: ureq::Error) -> String {
    let text = error.to_string().replace('\n', " ");
    match text.split_once(": https://") {
        Some((head, _)) => format!("{head}."),
        None => text,
    }
}

/// A release asset name is `apexshot_0.2.36_amd64.deb`.
fn is_deb(name: &str) -> bool {
    name.ends_with("_amd64.deb") && !name.ends_with(".ddeb")
}

/// The Arch package is `apexshot-0.2.36-1-x86_64.pkg.tar.zst`; debug packages
/// share the suffix and must never be offered.
fn is_arch(name: &str) -> bool {
    name.ends_with("-x86_64.pkg.tar.zst") && !name.contains("-debug-")
}

/// The build number and Fedora tag of an RPM for exactly `version`.
///
/// `apexshot-0.2.36-1.fc42.x86_64.rpm` gives `(1, Some(42))` and the
/// distribution-neutral `apexshot-0.2.36-0.x86_64.rpm` gives `(0, None)`.
fn rpm_build(name: &str, version: &str) -> Option<(u32, Option<u32>)> {
    let stem = name.strip_suffix(".x86_64.rpm")?;
    let rest = stem.strip_prefix(&format!("apexshot-{version}-"))?;
    match rest.split_once(".fc") {
        Some((release, fedora)) => Some((release.parse().ok()?, Some(fedora.parse().ok()?))),
        None => Some((rest.parse().ok()?, None)),
    }
}

/// Pick the RPM built for this Fedora release, then the newest build that is
/// not newer than it, then the newest published build, and only when the
/// release has no Fedora builds at all the distribution-neutral package.
///
/// A build for a newer Fedora needs a newer glibc than the host has, so the
/// order mirrors the install script rather than always taking the highest tag.
fn select_rpm<'a>(
    assets: &'a [ReleaseAsset],
    version: &str,
    host_fedora: Option<u32>,
) -> Option<&'a ReleaseAsset> {
    let candidates: Vec<(u32, Option<u32>, &ReleaseAsset)> = assets
        .iter()
        .filter_map(|asset| {
            rpm_build(&asset.name, version).map(|(release, fc)| (release, fc, asset))
        })
        .collect();

    if let Some(host) = host_fedora {
        if let Some(exact) = candidates.iter().find(|entry| entry.1 == Some(host)) {
            return Some(exact.2);
        }
        let mut installable: Vec<&(u32, Option<u32>, &ReleaseAsset)> = candidates
            .iter()
            .filter(|entry| entry.1.is_none_or(|fc| fc <= host))
            .collect();
        installable.sort_by_key(|entry| (entry.1.unwrap_or(0), entry.0));
        if let Some(best) = installable.last() {
            return Some(best.2);
        }
    }

    let mut fedora: Vec<&(u32, Option<u32>, &ReleaseAsset)> = candidates
        .iter()
        .filter(|entry| entry.1.is_some())
        .collect();
    fedora.sort_by_key(|entry| (entry.1.unwrap_or(0), entry.0));
    if let Some(best) = fedora.last() {
        return Some(best.2);
    }

    candidates
        .iter()
        .find(|entry| entry.1.is_none() && entry.0 == 0)
        .map(|entry| entry.2)
}

/// Which of the release's files this system needs.
pub fn select_assets(
    assets: &[ReleaseAsset],
    format: PackageFormat,
    version: &str,
    host_fedora: Option<u32>,
) -> Result<SelectedAssets, AssetError> {
    let package = match format {
        PackageFormat::Deb => assets.iter().find(|asset| is_deb(&asset.name)),
        PackageFormat::Rpm => select_rpm(assets, version, host_fedora),
        PackageFormat::Arch => assets.iter().find(|asset| is_arch(&asset.name)),
    }
    .cloned()
    .ok_or(AssetError::NoPackage)?;

    let checksums = assets
        .iter()
        .find(|asset| asset.name == CHECKSUMS_NAME)
        .cloned()
        .ok_or(AssetError::NoChecksums)?;

    let extension = assets
        .iter()
        .find(|asset| asset.name == EXTENSION_NAME)
        .cloned();

    Ok(SelectedAssets {
        package,
        checksums,
        extension,
    })
}

/// Parse `/etc/os-release` into the Fedora release number, when the host is
/// Fedora at all.
pub fn fedora_version_from_os_release(raw: &str) -> Option<u32> {
    let mut id = None;
    let mut version = None;
    for line in raw.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key.trim() {
            "ID" => id = Some(value.to_ascii_lowercase()),
            "VERSION_ID" => version = Some(value.to_owned()),
            _ => {}
        }
    }
    if id.as_deref() != Some("fedora") {
        return None;
    }
    let numeric = version?.split('.').next()?.parse::<u32>().ok()?;
    Some(numeric)
}

/// The host's Fedora release when `/etc/os-release` says it is Fedora.
pub fn host_fedora_version() -> Option<u32> {
    let raw = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok()?;
    fedora_version_from_os_release(&raw)
}

/// The published checksum of every file in a `SHA256SUMS` manifest.
///
/// Lines are `<hex>  <name>`, with the binary-mode `*` prefix tolerated. Blank
/// lines and lines that do not have a checksum shape are skipped.
pub fn parse_sha256sums(raw: &str) -> HashMap<String, String> {
    let mut sums = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(checksum) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        let name = name.trim();
        let name = name.strip_prefix('*').unwrap_or(name).trim();
        if checksum.len() == 64
            && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !name.is_empty()
        {
            sums.insert(name.to_string(), checksum.to_ascii_lowercase());
        }
    }
    sums
}

/// Hex SHA-256 of a byte slice, for the tests that pin the published vectors.
#[cfg(test)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Hex SHA-256 of a file, streamed so a package is never held in memory.
#[cfg(test)]
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("Could not read the download: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read the download: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compare a digest computed while downloading against the manifest entry
/// for `name`, so verification does not read the file a second time.
pub fn verify_digest(
    actual: &str,
    name: &str,
    sums: &HashMap<String, String>,
) -> Result<(), AssetError> {
    let expected = sums
        .get(name)
        .ok_or_else(|| AssetError::NoChecksumEntry(name.to_string()))?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AssetError::ChecksumMismatch(name.to_string()))
    }
}

/// Compare a file on disk against the manifest entry for `name`.
#[cfg(test)]
pub fn verify_file(
    path: &Path,
    name: &str,
    sums: &HashMap<String, String>,
) -> Result<(), AssetError> {
    let actual = sha256_file(path).map_err(|_| AssetError::ChecksumMismatch(name.to_string()))?;
    verify_digest(&actual, name, sums)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: 10,
        }
    }

    fn assets(names: &[&str]) -> Vec<ReleaseAsset> {
        names.iter().map(|name| asset(name)).collect()
    }

    #[test]
    fn deb_selection_takes_the_amd64_package() {
        let release = assets(&[
            "apexshot_0.2.36_arm64.deb",
            "apexshot_0.2.36_amd64.deb",
            "apexshot-0.2.36-x86_64.AppImage",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Deb, "0.2.36", None).unwrap();
        assert_eq!(selected.package.name, "apexshot_0.2.36_amd64.deb");
        assert_eq!(selected.checksums.name, CHECKSUMS_NAME);
        assert_eq!(selected.extension, None);
    }

    #[test]
    fn a_release_without_the_package_is_an_error() {
        let release = assets(&[CHECKSUMS_NAME, EXTENSION_NAME]);
        assert_eq!(
            select_assets(&release, PackageFormat::Deb, "0.2.36", None),
            Err(AssetError::NoPackage)
        );
        assert_eq!(
            select_assets(&release, PackageFormat::Arch, "0.2.36", None),
            Err(AssetError::NoPackage)
        );
    }

    #[test]
    fn a_release_without_checksums_is_an_error() {
        let release = assets(&["apexshot_0.2.36_amd64.deb"]);
        assert_eq!(
            select_assets(&release, PackageFormat::Deb, "0.2.36", None),
            Err(AssetError::NoChecksums)
        );
    }

    #[test]
    fn rpm_selection_prefers_the_host_fedora_build() {
        let release = assets(&[
            "apexshot-0.2.36-1.fc42.x86_64.rpm",
            "apexshot-0.2.36-1.fc44.x86_64.rpm",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Rpm, "0.2.36", Some(44)).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-1.fc44.x86_64.rpm");
    }

    #[test]
    fn rpm_selection_never_takes_a_newer_host_build() {
        let release = assets(&[
            "apexshot-0.2.36-1.fc42.x86_64.rpm",
            "apexshot-0.2.36-1.fc44.x86_64.rpm",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Rpm, "0.2.36", Some(43)).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-1.fc42.x86_64.rpm");
    }

    #[test]
    fn rpm_selection_falls_back_to_the_highest_fedora_build() {
        let release = assets(&[
            "apexshot-0.2.36-1.fc42.x86_64.rpm",
            "apexshot-0.2.36-1.fc44.x86_64.rpm",
            "apexshot-0.2.36-1.fc40.x86_64.rpm",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Rpm, "0.2.36", None).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-1.fc44.x86_64.rpm");
    }

    #[test]
    fn rpm_selection_ignores_debug_and_source_packages() {
        let release = assets(&[
            "apexshot-debuginfo-0.2.36-1.fc44.x86_64.rpm",
            "apexshot-debugsource-0.2.36-1.fc44.x86_64.rpm",
            "apexshot-0.2.36-1.fc44.src.rpm",
            "apexshot-0.2.36-1.fc42.x86_64.rpm",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Rpm, "0.2.36", Some(44)).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-1.fc42.x86_64.rpm");
    }

    #[test]
    fn rpm_selection_uses_the_neutral_build_only_without_fedora_assets() {
        let release = assets(&[
            "apexshot-0.2.36-0.x86_64.rpm",
            "apexshot-0.2.36-1.fc44.x86_64.rpm",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Rpm, "0.2.36", Some(44)).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-1.fc44.x86_64.rpm");

        let neutral = assets(&["apexshot-0.2.36-0.x86_64.rpm", CHECKSUMS_NAME]);
        let selected = select_assets(&neutral, PackageFormat::Rpm, "0.2.36", Some(44)).unwrap();
        assert_eq!(selected.package.name, "apexshot-0.2.36-0.x86_64.rpm");
    }

    #[test]
    fn rpm_selection_ignores_another_versions_package() {
        let release = assets(&["apexshot-0.2.35-1.fc44.x86_64.rpm", CHECKSUMS_NAME]);
        assert_eq!(
            select_assets(&release, PackageFormat::Rpm, "0.2.36", Some(44)),
            Err(AssetError::NoPackage)
        );
    }

    #[test]
    fn arch_selection_skips_debug_packages() {
        let release = assets(&[
            "apexshot-debug-0.2.36-1-x86_64.pkg.tar.zst",
            "apexshot-0.2.36-1-x86_64.pkg.tar.zst",
            CHECKSUMS_NAME,
        ]);
        let selected = select_assets(&release, PackageFormat::Arch, "0.2.36", None).unwrap();
        assert_eq!(
            selected.package.name,
            "apexshot-0.2.36-1-x86_64.pkg.tar.zst"
        );
    }

    #[test]
    fn the_extension_and_checksums_are_picked_up_when_present() {
        let release = assets(&["apexshot_0.2.36_amd64.deb", CHECKSUMS_NAME, EXTENSION_NAME]);
        let selected = select_assets(&release, PackageFormat::Deb, "0.2.36", None).unwrap();
        assert_eq!(
            selected.extension.map(|asset| asset.name),
            Some(EXTENSION_NAME.to_string())
        );
    }

    #[test]
    fn os_release_only_reports_a_fedora_number() {
        assert_eq!(
            fedora_version_from_os_release("ID=fedora\nVERSION_ID=44\n"),
            Some(44)
        );
        assert_eq!(
            fedora_version_from_os_release("ID=fedora\nVERSION_ID=\"43\"\n"),
            Some(43)
        );
        assert_eq!(
            fedora_version_from_os_release("ID=ubuntu\nVERSION_ID=\"24.04\"\n"),
            None
        );
        assert_eq!(
            fedora_version_from_os_release("NAME=Fedora Linux\nVERSION_ID=44\n"),
            None
        );
        assert_eq!(fedora_version_from_os_release(""), None);
    }

    #[test]
    fn checksum_lines_parse_with_text_and_binary_marks() {
        let checksum_a = "a".repeat(64);
        let checksum_b = "b".repeat(64);
        let raw = format!(
            "# generated\n{checksum_a}  apexshot_0.2.36_amd64.deb\n{checksum_b} *{EXTENSION_NAME}\nnot-a-sum  file\n\n"
        );
        let sums = parse_sha256sums(&raw);
        assert_eq!(
            sums.get("apexshot_0.2.36_amd64.deb").map(String::as_str),
            Some(checksum_a.as_str())
        );
        assert_eq!(
            sums.get(EXTENSION_NAME).map(String::as_str),
            Some(checksum_b.as_str())
        );
        assert_eq!(sums.get("file"), None);
        assert_eq!(sums.len(), 2);
    }

    #[test]
    fn checksums_verify_against_a_file_and_its_manifest_entry() {
        let expected = sha256_hex(b"abc");
        assert_eq!(
            expected,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let path = std::env::temp_dir().join(format!(
            "apexshot-checksum-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"abc").unwrap();
        let mut sums = HashMap::new();
        sums.insert("pkg.deb".to_string(), expected.clone());
        assert_eq!(verify_file(&path, "pkg.deb", &sums), Ok(()));

        sums.insert("pkg.deb".to_string(), sha256_hex(b"other"));
        assert_eq!(
            verify_file(&path, "pkg.deb", &sums),
            Err(AssetError::ChecksumMismatch("pkg.deb".to_string()))
        );
        assert_eq!(
            verify_file(&path, "missing.deb", &sums),
            Err(AssetError::NoChecksumEntry("missing.deb".to_string()))
        );

        let _ = std::fs::remove_file(path);
    }
}
