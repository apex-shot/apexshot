use std::path::{Path, PathBuf};

pub const OFFICIAL_APP_ID: &str = "io.github.codegoddy.apexshot";
pub const DEV_APP_ID: &str = "io.github.codegoddy.apexshot.dev";

pub const OFFICIAL_BINARY: &str = "/usr/bin/apexshot";
pub const LEGACY_LOCAL_BINARY: &str = "/usr/local/bin/apexshot";
pub const DEV_WRAPPER: &str = "/usr/local/bin/apexshot-dev";

pub const OFFICIAL_DESKTOP_FILE: &str =
    "/usr/share/applications/io.github.codegoddy.apexshot.desktop";

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

    if std::env::args_os()
        .next()
        .map(PathBuf::from)
        .as_deref()
        .is_some_and(path_looks_like_dev)
    {
        return true;
    }

    std::env::current_exe()
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

pub fn daemon_name() -> &'static str {
    if is_dev() {
        "ApexShot Dev Daemon"
    } else {
        "ApexShot Daemon"
    }
}

pub fn icon_name() -> &'static str {
    if is_dev() {
        "apexshot"
    } else {
        OFFICIAL_APP_ID
    }
}

pub fn desktop_file_name() -> String {
    format!("{}.desktop", app_id())
}

pub fn local_desktop_file_path() -> Option<PathBuf> {
    let mut dir = dirs::data_dir()?;
    dir.push("applications");
    dir.push(desktop_file_name());
    Some(dir)
}

pub fn preferred_command_path() -> PathBuf {
    if is_dev() && Path::new(DEV_WRAPPER).exists() {
        return PathBuf::from(DEV_WRAPPER);
    }
    if !is_dev() && Path::new(OFFICIAL_BINARY).exists() {
        return PathBuf::from(OFFICIAL_BINARY);
    }
    if !is_dev() && Path::new(LEGACY_LOCAL_BINARY).exists() {
        return PathBuf::from(LEGACY_LOCAL_BINARY);
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"))
}

pub fn desktop_file_for_portal() -> Option<PathBuf> {
    if !is_dev() {
        let system_desktop = PathBuf::from(OFFICIAL_DESKTOP_FILE);
        if system_desktop.exists() {
            return Some(system_desktop);
        }
    }
    local_desktop_file_path().filter(|path| path.exists())
}
