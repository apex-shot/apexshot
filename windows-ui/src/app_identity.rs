//! Windows counterpart of the Linux identity module.
//!
//! Only the handful of helpers the settings pages call is implemented; the
//! installed-path constants and portal helpers are deliberately absent.

use std::path::{Path, PathBuf};

pub const OFFICIAL_APP_ID: &str = "io.github.codegoddy.apexshot";
pub const DEV_APP_ID: &str = "io.github.codegoddy.apexshot.dev";

fn path_looks_like_dev(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "apexshot-dev")
        || path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|part| part == "apexshot-dev")
        })
}

pub fn is_dev() -> bool {
    if std::env::var("APEXSHOT_APP_FLAVOR")
        .map(|value| value.eq_ignore_ascii_case("dev"))
        .unwrap_or(false)
    {
        return true;
    }

    std::env::args_os()
        .next()
        .map(PathBuf::from)
        .as_deref()
        .is_some_and(path_looks_like_dev)
        || std::env::current_exe()
            .ok()
            .as_deref()
            .is_some_and(path_looks_like_dev)
}

pub fn app_id() -> &'static str {
    if is_dev() {
        DEV_APP_ID
    } else {
        OFFICIAL_APP_ID
    }
}

pub fn app_name() -> &'static str {
    if is_dev() {
        "ApexShot Dev"
    } else {
        "ApexShot"
    }
}

pub fn icon_name() -> &'static str {
    if is_dev() {
        "apexshot-dev"
    } else {
        "apexshot"
    }
}

pub fn daemon_name() -> &'static str {
    if is_dev() {
        "ApexShot Dev"
    } else {
        "ApexShot"
    }
}

/// Windows has no desktop entry to point the portal at.
pub fn desktop_file_for_portal() -> Option<PathBuf> {
    None
}

pub fn portal_only() -> bool {
    false
}

pub fn preferred_command_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot-settings.exe"))
}
