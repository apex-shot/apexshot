/// Do Not Disturb toggle for recording sessions.
///
/// Uses `gsettings` on GNOME and `qdbus` on KDE to suppress
/// system notification banners. Falls back gracefully if neither
/// desktop environment is detected.
/// Guard that restores DND state when dropped.
pub struct DndGuard {
    desktop: DesktopEnv,
    previous_show_banners: Option<String>,
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

impl DndGuard {
    /// Enable "Do Not Disturb" mode. Returns a guard that restores
    /// the previous state when dropped. Returns `None` if the desktop
    /// environment is unsupported.
    pub fn enable() -> Option<Self> {
        let desktop = detect_desktop();
        match desktop {
            DesktopEnv::Gnome => {
                let prev = run_cmd(
                    "gsettings",
                    &["get", "org.gnome.desktop.notifications", "show-banners"],
                );
                run_cmd(
                    "gsettings",
                    &[
                        "set",
                        "org.gnome.desktop.notifications",
                        "show-banners",
                        "false",
                    ],
                );
                Some(Self {
                    desktop,
                    previous_show_banners: prev,
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
                let value = self.previous_show_banners.as_deref().unwrap_or("true");
                run_cmd(
                    "gsettings",
                    &[
                        "set",
                        "org.gnome.desktop.notifications",
                        "show-banners",
                        value,
                    ],
                );
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
