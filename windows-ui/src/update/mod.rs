//! The Windows update surface: the prompt that replaces the update toast,
//! its download view, and the handoff to the downloaded installer.
//!
//! The window is a small, borderless surface built from the onboarding
//! stylesheet, so it reads as part of the same first-run flow: the brand mark
//! above a centred headline, a quiet fact card, and one primary action. It has
//! no entrance or page-transition animation.
//!
//! `--check` runs discovery in `check`, which mirrors the Linux app: one
//! stable-release lookup per day, cached locally, and a per-version 24-hour
//! snooze once the window has been shown. A caller that already knows the
//! release can pass it explicitly; the binary only presents it and downloads
//! the installer it was given.

mod check;
mod download;
mod window;

pub use check::check_and_show;

use std::ffi::OsString;

use gtk4::prelude::*;

/// What the update window knows about a release before it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The newer release without its `v` prefix, e.g. `0.2.37`.
    pub version: String,
    /// The installer asset's file name, e.g. `ApexShot-0.2.37-setup.exe`.
    pub asset_name: String,
    /// Where the installer is downloaded from.
    pub asset_url: String,
    /// The installer's size when the caller knows it from the release API.
    pub asset_size: Option<u64>,
    /// The release's page, for the Release notes link.
    pub release_url: Option<String>,
}

const USAGE: &str = "usage: apexshot-update --version <latest> --asset-url <url> \
[--asset-name <file>] [--asset-size <bytes>] [--release-url <url>]\n       \
apexshot-update --check";

/// What the command line asked the binary to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Present a release the caller already resolved.
    Show(Release),
    /// Look for a newer release and present it when there is one.
    Check,
    /// Print the usage and exit successfully.
    Help,
}

impl Release {
    /// Build a release from an already-resolved version and asset URL.
    pub fn new(version: &str, asset_url: &str) -> Result<Self, String> {
        let version = version.trim().trim_start_matches('v').to_string();
        if version.is_empty() {
            return Err("--version needs a version number".to_string());
        }
        if asset_url.trim().is_empty() {
            return Err("--asset-url needs a download URL".to_string());
        }
        Ok(Self {
            version,
            asset_name: download::file_name_from_url(asset_url),
            asset_url: asset_url.to_string(),
            asset_size: None,
            release_url: None,
        })
    }
}

/// The command line help text.
pub fn usage() -> &'static str {
    USAGE
}

/// Parse the `apexshot-update` command line.
///
/// `--check` is a standalone mode: it discovers a release itself. Otherwise
/// the flags describe a release the caller has already resolved.
pub fn command_from_args(args: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<OsString> = args.into_iter().collect();
    if args
        .iter()
        .any(|arg| arg.to_string_lossy().trim() == "--check")
    {
        if args.len() != 1 {
            return Err(format!(
                "--check cannot be combined with other arguments. {USAGE}"
            ));
        }
        return Ok(Command::Check);
    }
    match release_from_args(args)? {
        Some(release) => Ok(Command::Show(release)),
        None => Ok(Command::Help),
    }
}

/// Parse the `apexshot-update` command line.
///
/// `Ok(None)` means the caller asked for help; `Err` is a malformed command
/// line, which the binary reports on stderr and exits on.
pub fn release_from_args(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<Release>, String> {
    let mut version = None;
    let mut asset_url = None;
    let mut asset_name = None;
    let mut asset_size = None;
    let mut release_url = None;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.to_string_lossy().to_string();
        match arg.as_str() {
            "--version" => version = Some(next_value(&mut args, "--version")?),
            "--asset-url" => asset_url = Some(next_value(&mut args, "--asset-url")?),
            "--asset-name" => asset_name = Some(next_value(&mut args, "--asset-name")?),
            "--asset-size" => {
                let value = next_value(&mut args, "--asset-size")?;
                asset_size = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("--asset-size needs a byte count, got {value:?}"))?,
                );
            }
            "--release-url" => release_url = Some(next_value(&mut args, "--release-url")?),
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("unknown argument {other:?}. {USAGE}")),
        }
    }

    let (Some(version), Some(asset_url)) = (version, asset_url) else {
        return Err(USAGE.to_string());
    };
    let mut release = Release::new(&version, &asset_url)?;
    if let Some(name) = asset_name.filter(|name| !name.trim().is_empty()) {
        release.asset_name = name;
    }
    release.asset_size = asset_size;
    release.release_url = release_url.filter(|url| !url.trim().is_empty());
    Ok(Some(release))
}

fn next_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, String> {
    args.next()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Open the update window and block until it closes.
pub fn show(release: Release) -> anyhow::Result<()> {
    // Showing the prompt starts the snooze, so a check that runs again while
    // the window is still on screen cannot stack a second one.
    check::snooze(&release.version);

    let app = gtk4::Application::builder()
        .application_id(format!("{}.update", crate::app_identity::app_id()))
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |application| {
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        window::build_window(application, release.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_a_complete_release_command_line() {
        let release = release_from_args(args(&[
            "--version",
            "v0.2.37",
            "--asset-url",
            "https://example.test/ApexShot-0.2.37-setup.exe",
            "--asset-size",
            "191754859",
            "--release-url",
            "https://example.test/releases/v0.2.37",
        ]))
        .expect("valid arguments")
        .expect("a release");

        assert_eq!(release.version, "0.2.37");
        assert_eq!(release.asset_name, "ApexShot-0.2.37-setup.exe");
        assert_eq!(release.asset_size, Some(191_754_859));
        assert_eq!(
            release.release_url.as_deref(),
            Some("https://example.test/releases/v0.2.37")
        );
    }

    #[test]
    fn the_asset_name_defaults_to_the_url_and_can_be_overridden() {
        let derived = release_from_args(args(&[
            "--version",
            "v0.2.37",
            "--asset-url",
            "https://example.test/download/ApexShot-0.2.37-setup.exe?token=1",
        ]))
        .expect("valid arguments")
        .expect("a release");
        assert_eq!(derived.asset_name, "ApexShot-0.2.37-setup.exe");

        let named = release_from_args(args(&[
            "--version",
            "0.2.37",
            "--asset-url",
            "https://example.test/download?id=1",
            "--asset-name",
            "ApexShot-0.2.37-setup.exe",
        ]))
        .expect("valid arguments")
        .expect("a release");
        assert_eq!(named.asset_name, "ApexShot-0.2.37-setup.exe");
    }

    #[test]
    fn help_and_malformed_command_lines_are_distinct() {
        assert_eq!(release_from_args(args(&["--help"])), Ok(None));
        assert!(release_from_args(args(&["--version", "0.2.37"])).is_err());
        assert!(release_from_args(args(&["--asset-url", "https://example.test/a.exe"])).is_err());
        assert!(release_from_args(args(&[
            "--version",
            "0.2.37",
            "--asset-url",
            "https://example.test/a.exe",
            "--asset-size",
            "large",
        ]))
        .is_err());
        assert!(release_from_args(args(&["--unknown"])).is_err());
    }

    #[test]
    fn check_is_a_standalone_command() {
        assert_eq!(command_from_args(args(&["--check"])), Ok(Command::Check));
        assert_eq!(command_from_args(args(&["--help"])), Ok(Command::Help));
        assert!(command_from_args(args(&["--check", "--version", "0.2.37"])).is_err());
        assert_eq!(
            command_from_args(args(&[
                "--version",
                "0.2.37",
                "--asset-url",
                "https://example.test/a.exe",
            ]))
            .map(|command| matches!(command, Command::Show(_))),
            Ok(true)
        );
    }

    #[test]
    fn release_requires_a_version_and_download_url() {
        assert!(Release::new("", "https://example.test/a.exe").is_err());
        assert!(Release::new("0.2.37", "  ").is_err());
        assert_eq!(
            Release::new("v0.2.37", "https://example.test/a.exe")
                .expect("valid release")
                .version,
            "0.2.37"
        );
    }
}
