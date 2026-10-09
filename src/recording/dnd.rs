/// Do Not Disturb toggle for recording sessions.
///
/// Uses `gsettings` on GNOME and `qdbus` on KDE to suppress
/// system notification banners. Falls back gracefully if neither
/// desktop environment is detected.
use std::path::PathBuf;

const DND_RECOVERY_FILE: &str = "recording-dnd-recovery";

/// Guard that restores DND state when dropped.
pub struct DndGuard {
    desktop: DesktopEnv,
    previous_show_banners: Option<String>,
    recovery_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum DesktopEnv {
    Gnome,
    Kde,
    Unknown,
}

fn detect_desktop() -> DesktopEnv {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase();
    if desktop.contains("GNOME") || desktop.contains("UBUNTU") || desktop.contains("PANTHEON") {
        DesktopEnv::Gnome
    } else if desktop.contains("KDE") || desktop.contains("PLASMA") {
        DesktopEnv::Kde
    } else {
        DesktopEnv::Unknown
    }
}

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn recovery_path() -> Option<PathBuf> {
    dirs::state_dir().map(|dir| dir.join("apexshot").join(DND_RECOVERY_FILE))
}

fn parse_recovery_marker(contents: &str) -> Option<(u32, &str)> {
    let mut lines = contents.lines();
    let pid = lines.next()?.strip_prefix("pid=")?.parse().ok()?;
    let show_banners = lines.next()?.strip_prefix("show_banners=")?;
    matches!(show_banners, "true" | "false").then_some((pid, show_banners))
}

fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Restore GNOME banners after a recording process was terminated before its
/// [`DndGuard`] could run. A live owner is left alone so concurrent ApexShot
/// commands cannot interrupt an active recording's DND state.
pub fn recover_stale_gnome_dnd() {
    if crate::app_identity::portal_only() || !matches!(detect_desktop(), DesktopEnv::Gnome) {
        return;
    }
    let Some(path) = recovery_path() else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let Some((owner_pid, previous_show_banners)) = parse_recovery_marker(&contents) else {
        let _ = std::fs::remove_file(path);
        return;
    };
    if process_is_alive(owner_pid) {
        return;
    }

    if run_cmd(
        "gsettings",
        &[
            "set",
            "org.gnome.desktop.notifications",
            "show-banners",
            previous_show_banners,
        ],
    )
    .is_some()
    {
        let _ = std::fs::remove_file(path);
    }
}

impl DndGuard {
    /// Enable "Do Not Disturb" mode. Returns a guard that restores
    /// the previous state when dropped. Returns `None` if the desktop
    /// environment is unsupported.
    pub fn enable() -> Option<Self> {
        if crate::app_identity::portal_only() {
            return None;
        }
        let desktop = detect_desktop();
        match desktop {
            DesktopEnv::Gnome => {
                recover_stale_gnome_dnd();
                let recovery_path = recovery_path();
                if recovery_path.as_ref().is_some_and(|path| path.exists()) {
                    return Some(Self {
                        desktop,
                        previous_show_banners: None,
                        recovery_path: None,
                    });
                }
                let prev = run_cmd(
                    "gsettings",
                    &["get", "org.gnome.desktop.notifications", "show-banners"],
                )?;
                let recovery_path = recovery_path.and_then(|path| {
                    let parent = path.parent()?;
                    std::fs::create_dir_all(parent).ok()?;
                    std::fs::write(
                        &path,
                        format!("pid={}\nshow_banners={prev}\n", std::process::id()),
                    )
                    .ok()?;
                    Some(path)
                });
                if run_cmd(
                    "gsettings",
                    &[
                        "set",
                        "org.gnome.desktop.notifications",
                        "show-banners",
                        "false",
                    ],
                )
                .is_none()
                {
                    if let Some(path) = recovery_path.as_ref() {
                        let _ = std::fs::remove_file(path);
                    }
                    return None;
                }
                Some(Self {
                    desktop,
                    previous_show_banners: Some(prev),
                    recovery_path,
                })
            }
            DesktopEnv::Kde => {
                // KDE uses a D-Bus call to toggle Do Not Disturb
                run_cmd(
                    "qdbus",
                    &[
                        "org.kde.kglobalaccel",
                        "/kglobalaccel",
                        "invokeShortcut",
                        "Toggle Do Not Disturb",
                    ],
                );
                Some(Self {
                    desktop,
                    previous_show_banners: None,
                    recovery_path: None,
                })
            }
            DesktopEnv::Unknown => None,
        }
    }
}

impl Drop for DndGuard {
    fn drop(&mut self) {
        match self.desktop {
            DesktopEnv::Gnome => {
                let restored = self.previous_show_banners.as_deref().is_some_and(|value| {
                    run_cmd(
                        "gsettings",
                        &[
                            "set",
                            "org.gnome.desktop.notifications",
                            "show-banners",
                            value,
                        ],
                    )
                    .is_some()
                });
                if restored {
                    if let Some(path) = self.recovery_path.take() {
                        let _ = std::fs::remove_file(path);
                    }
                } else if let (Some(value), Some(path)) = (
                    self.previous_show_banners.as_deref(),
                    self.recovery_path.as_ref(),
                ) {
                    // Mark the owner as dead so the next ApexShot command retries
                    // restoration even if the long-lived daemon itself is still up.
                    let _ = std::fs::write(path, format!("pid=0\nshow_banners={value}\n"));
                }
            }
            DesktopEnv::Kde => {
                // Toggle back
                run_cmd(
                    "qdbus",
                    &[
                        "org.kde.kglobalaccel",
                        "/kglobalaccel",
                        "invokeShortcut",
                        "Toggle Do Not Disturb",
                    ],
                );
            }
            DesktopEnv::Unknown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_recovery_marker;

    #[test]
    fn recovery_marker_requires_pid_and_boolean_banner_state() {
        assert_eq!(
            parse_recovery_marker("pid=42\nshow_banners=true\n"),
            Some((42, "true"))
        );
        assert_eq!(parse_recovery_marker("pid=42\nshow_banners=maybe\n"), None);
        assert_eq!(parse_recovery_marker("show_banners=true\n"), None);
    }
}
