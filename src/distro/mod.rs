//! Distribution and desktop support metadata.
//!
//! Capture support is intentionally distro-light: on Wayland ApexShot uses the
//! XDG ScreenCast portal plus PipeWire as the primary "share screen" capture
//! path, matching the broad approach used by Flameshot-compatible Linux
//! desktops. Distro-specific code here is for detection, dependency guidance,
//! packaging, and small integration differences.

pub mod arch;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Information about the current Linux distribution
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroInfo {
    pub id: String,
    pub id_like: Vec<String>,
    pub name: String,
    pub version_id: Option<String>,
}

impl DistroInfo {
    /// Detect the current Linux distribution from /etc/os-release
    pub fn detect() -> Option<Self> {
        Self::detect_from_path("/etc/os-release")
    }

    /// Detect a distribution from an os-release file.
    pub fn detect_from_path(path: impl AsRef<Path>) -> Option<Self> {
        let raw = fs::read_to_string(path).ok()?;
        Self::parse_os_release(&raw)
    }

    fn parse_os_release(raw: &str) -> Option<Self> {
        let fields = parse_os_release_fields(raw);
        let id = normalize_id(fields.get("ID")?)?;
        let id_like = fields
            .get("ID_LIKE")
            .map(|value| {
                value
                    .split_whitespace()
                    .filter_map(normalize_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let name = fields.get("NAME").cloned().unwrap_or_else(|| id.clone());
        let version_id = fields.get("VERSION_ID").cloned();

        Some(Self {
            id,
            id_like,
            name,
            version_id,
        })
    }

    /// Check if this is an Arch-based distribution
    pub fn is_arch(&self) -> bool {
        self.matches_any(&["arch"])
    }

    /// Check if this is a Debian/Ubuntu-based distribution
    pub fn is_debian(&self) -> bool {
        self.matches_any(&["ubuntu", "debian", "linuxmint", "pop", "elementary"])
    }

    /// Check if this is a Fedora/RHEL-based distribution
    pub fn is_fedora(&self) -> bool {
        self.matches_any(&["fedora", "rhel", "centos", "almalinux", "rocky"])
    }

    /// Check if this is openSUSE
    pub fn is_opensuse(&self) -> bool {
        self.matches_any(&["opensuse-tumbleweed", "opensuse-leap", "opensuse", "suse"])
    }

    /// Check if this is NixOS
    pub fn is_nixos(&self) -> bool {
        self.id == "nixos"
    }

    /// Check if this is Alpine Linux.
    pub fn is_alpine(&self) -> bool {
        self.matches_any(&["alpine"])
    }

    /// Check if this is Gentoo.
    pub fn is_gentoo(&self) -> bool {
        self.matches_any(&["gentoo"])
    }

    /// Check if this is Void Linux.
    pub fn is_void(&self) -> bool {
        self.matches_any(&["void"])
    }

    /// Group this distribution into a support family.
    pub fn family(&self) -> DistroFamily {
        if self.is_debian() {
            DistroFamily::Debian
        } else if self.is_arch() {
            DistroFamily::Arch
        } else if self.is_fedora() {
            DistroFamily::Fedora
        } else if self.is_opensuse() {
            DistroFamily::OpenSuse
        } else if self.is_nixos() {
            DistroFamily::Nixos
        } else if self.is_alpine() {
            DistroFamily::Alpine
        } else if self.is_gentoo() {
            DistroFamily::Gentoo
        } else if self.is_void() {
            DistroFamily::Void
        } else {
            DistroFamily::Unknown
        }
    }

    /// Runtime and packaging guidance for this distribution family.
    pub fn support_profile(&self) -> DistroSupport {
        DistroSupport::for_family(self.family())
    }

    /// Summary of how screenshots are captured on this distribution.
    pub fn screenshot_capture_summary(&self) -> &'static str {
        if self.is_debian() || self.is_arch() {
            // GNOME Wayland: GNOME Shell D-Bus screenshot API (via daemon),
            // falls back to XDG Screenshot portal, then Qt screen grab.
            // wlroots: zwlr_screencopy_manager_v1 direct protocol,
            // falls back to ScreenCast portal + PipeWire.
            // X11: direct X11 backend.
            "GNOME Shell D-Bus / Screenshot portal / wlr-screencopy (no external CLI tools)"
        } else {
            "XDG Screenshot portal + PipeWire fallback"
        }
    }

    fn matches_any(&self, ids: &[&str]) -> bool {
        ids.iter()
            .any(|candidate| self.id == *candidate || self.id_like.iter().any(|id| id == candidate))
    }
}

/// Supported distro families. These are package-manager families, not capture
/// backends; the Wayland capture backend remains ScreenCast portal + PipeWire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistroFamily {
    Debian,
    Arch,
    Fedora,
    OpenSuse,
    Nixos,
    Alpine,
    Gentoo,
    Void,
    Unknown,
}

/// How much confidence we have before distro-specific manual testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportTier {
    Tested,
    ImplementedNeedsTesting,
    CommunityPackaging,
    Unknown,
}

/// Distro-family support metadata used by installers, diagnostics, and docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroSupport {
    pub family: DistroFamily,
    pub tier: SupportTier,
    pub package_manager: &'static str,
    pub install_command: &'static str,
    pub wayland_capture_method: &'static str,
    pub required_runtime_packages: &'static [&'static str],
    pub recommended_portal_backends: &'static [&'static str],
}

impl DistroSupport {
    pub fn for_family(family: DistroFamily) -> Self {
        match family {
            DistroFamily::Debian => Self {
                family,
                tier: SupportTier::Tested,
                package_manager: "apt/dpkg",
                install_command: "scripts/ubuntu-install.sh",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "pipewire-pulse",
                    "gstreamer1.0-pipewire",
                    "wl-clipboard",
                    "tesseract-ocr",
                    "ffmpeg",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
            DistroFamily::Arch => Self {
                family,
                tier: SupportTier::Tested,
                package_manager: "pacman",
                install_command: "scripts/arch-install.sh",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "pipewire-pulse",
                    "gst-plugin-pipewire",
                    "wl-clipboard",
                    "tesseract",
                    "ffmpeg",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                    "xdg-desktop-portal-hyprland",
                ],
            },
            DistroFamily::Fedora => Self {
                family,
                tier: SupportTier::ImplementedNeedsTesting,
                package_manager: "dnf/rpm",
                install_command: "pending: fedora/rpm packaging",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "pipewire-pulseaudio",
                    "gstreamer1-plugin-pipewire",
                    "wl-clipboard",
                    "tesseract",
                    "ffmpeg-free",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
            DistroFamily::OpenSuse => Self {
                family,
                tier: SupportTier::ImplementedNeedsTesting,
                package_manager: "zypper/rpm",
                install_command: "pending: opensuse/rpm packaging",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "pipewire-pulseaudio",
                    "gstreamer-plugin-pipewire",
                    "wl-clipboard",
                    "tesseract-ocr",
                    "ffmpeg",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
            DistroFamily::Nixos => Self {
                family,
                tier: SupportTier::CommunityPackaging,
                package_manager: "nix",
                install_command: "pending: flake/package expression",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "gst_all_1.gst-plugins-rs",
                    "wl-clipboard",
                    "tesseract",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                    "xdg-desktop-portal-hyprland",
                ],
            },
            DistroFamily::Alpine => Self {
                family,
                tier: SupportTier::CommunityPackaging,
                package_manager: "apk",
                install_command: "pending: alpine packaging",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "gst-plugins-rs",
                    "wl-clipboard",
                    "tesseract-ocr",
                ],
                recommended_portal_backends: &["xdg-desktop-portal-wlr"],
            },
            DistroFamily::Gentoo => Self {
                family,
                tier: SupportTier::CommunityPackaging,
                package_manager: "portage",
                install_command: "pending: ebuild",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "gst-plugins-rs",
                    "wl-clipboard",
                    "tesseract",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
            DistroFamily::Void => Self {
                family,
                tier: SupportTier::CommunityPackaging,
                package_manager: "xbps",
                install_command: "pending: void template",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "gst-plugins-rs",
                    "wl-clipboard",
                    "tesseract-ocr",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
            DistroFamily::Unknown => Self {
                family,
                tier: SupportTier::Unknown,
                package_manager: "unknown",
                install_command: "manual/source build",
                wayland_capture_method: "XDG ScreenCast portal + PipeWire",
                required_runtime_packages: &[
                    "xdg-desktop-portal",
                    "pipewire",
                    "gstreamer pipewire plugin",
                    "wl-clipboard",
                    "tesseract",
                ],
                recommended_portal_backends: &[
                    "xdg-desktop-portal-gnome",
                    "xdg-desktop-portal-kde",
                    "xdg-desktop-portal-wlr",
                ],
            },
        }
    }
}

/// Platform-specific paths and commands
pub struct PlatformPaths {
    pub config_dir: String,
    pub data_dir: String,
    pub cache_dir: String,
    pub autostart_dir: String,
}

impl PlatformPaths {
    /// Get platform paths for the current distribution
    pub fn for_distro(distro: &DistroInfo) -> Self {
        match distro.family() {
            DistroFamily::Nixos => Self::xdg_defaults(),
            _ => Self::xdg_defaults(),
        }
    }

    fn xdg_defaults() -> Self {
        Self {
            config_dir: dirs::config_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "~/.config".to_string()),
            data_dir: dirs::data_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "~/.local/share".to_string()),
            cache_dir: dirs::cache_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "~/.cache".to_string()),
            autostart_dir: "~/.config/autostart".to_string(),
        }
    }
}

fn parse_os_release_fields(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((
                key.trim().to_string(),
                unquote_os_release_value(value.trim()),
            ))
        })
        .collect()
}

fn unquote_os_release_value(value: &str) -> String {
    let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return value.to_string();
    };
    if !value.ends_with(quote) || value.len() < 2 {
        return value.to_string();
    }

    let inner = &value[1..value.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

fn normalize_id(value: impl AsRef<str>) -> Option<String> {
    let value = value
        .as_ref()
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distro_detection_arch() {
        let arch = DistroInfo {
            id: "arch".to_string(),
            id_like: vec![],
            name: "Arch Linux".to_string(),
            version_id: None,
        };
        assert!(arch.is_arch());
        assert!(!arch.is_debian());
        assert!(!arch.is_fedora());
    }

    #[test]
    fn parses_os_release_with_quotes_and_id_like() {
        let distro = DistroInfo::parse_os_release(
            r#"
            NAME="Fedora Linux"
            ID=fedora
            VERSION_ID="40"
            ID_LIKE="rhel centos"
            "#,
        )
        .unwrap();

        assert_eq!(distro.id, "fedora");
        assert_eq!(distro.name, "Fedora Linux");
        assert_eq!(distro.version_id.as_deref(), Some("40"));
        assert!(distro.id_like.contains(&"rhel".to_string()));
        assert!(distro.is_fedora());
        assert_eq!(distro.family(), DistroFamily::Fedora);
    }

    #[test]
    fn maps_supported_distro_profiles_to_screencast() {
        for family in [
            DistroFamily::Debian,
            DistroFamily::Arch,
            DistroFamily::Fedora,
            DistroFamily::OpenSuse,
            DistroFamily::Nixos,
            DistroFamily::Alpine,
            DistroFamily::Gentoo,
            DistroFamily::Void,
        ] {
            let profile = DistroSupport::for_family(family);
            assert_eq!(
                profile.wayland_capture_method,
                "XDG ScreenCast portal + PipeWire"
            );
            assert!(profile
                .required_runtime_packages
                .iter()
                .any(|pkg| pkg.contains("pipewire")));
            assert!(profile
                .recommended_portal_backends
                .iter()
                .any(|backend| backend.contains("xdg-desktop-portal")));
        }
    }

    #[test]
    fn test_distro_detection_ubuntu() {
        let ubuntu = DistroInfo {
            id: "ubuntu".to_string(),
            id_like: vec!["debian".to_string()],
            name: "Ubuntu".to_string(),
            version_id: Some("24.04".to_string()),
        };
        assert!(!ubuntu.is_arch());
        assert!(ubuntu.is_debian());
        assert!(!ubuntu.is_fedora());
    }

    #[test]
    fn test_distro_detection_fedora() {
        let fedora = DistroInfo {
            id: "fedora".to_string(),
            id_like: vec![],
            name: "Fedora Linux".to_string(),
            version_id: Some("40".to_string()),
        };
        assert!(!fedora.is_arch());
        assert!(!fedora.is_debian());
        assert!(fedora.is_fedora());
    }
}
