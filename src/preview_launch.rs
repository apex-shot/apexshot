use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{backend::kde_screenshot, capture::show_capture_preview_overlay};

const PREVIEW_TIMING_ENV: &str = "APEXSHOT_PREVIEW_TIMING";
const PREVIEW_PARENT_START_ENV: &str = "APEXSHOT_PREVIEW_PARENT_START_MS";

pub fn should_use_direct_preview_launch() -> bool {
    kde_screenshot::is_kde_wayland_session()
}

pub fn should_use_direct_editor_launch() -> bool {
    false
}

fn unix_epoch_millis_now() -> Option<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

/// Launch the floating preview in a separate process so it keeps an isolated
/// GTK application context. This preserves GNOME extension tracking and the
/// existing single-instance preview management in the daemon.
pub fn spawn_preview_subprocess(path: &Path) -> std::io::Result<Child> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
    let mut command = Command::new(&exe);
    command.arg("preview").arg(path);

    if std::env::var_os(PREVIEW_TIMING_ENV).is_some() {
        if let Some(start_ms) = unix_epoch_millis_now() {
            command.env(PREVIEW_PARENT_START_ENV, start_ms.to_string());
        }
    }

    command.spawn()
}

/// Best-effort fallback if the preview subprocess cannot be started.
pub fn show_preview_direct(path: PathBuf) {
    if let Err(e) = show_capture_preview_overlay(path) {
        eprintln!("Warning: Failed to show capture preview overlay: {}", e);
    }
}

pub fn launch_preview(path: &Path) -> std::io::Result<()> {
    if should_use_direct_preview_launch() {
        show_preview_direct(path.to_path_buf());
        Ok(())
    } else {
        spawn_preview_subprocess(path).map(|_| ())
    }
}
