//! Arch Linux-specific integrations
//!
//! This module contains code specific to Arch Linux and Arch-based
//! distributions (Manjaro, EndeavourOS, Garuda, etc.).
//!
//! Arch has first-class packaging support. Desktop coverage is still tested per
//! compositor, but capture follows the shared ScreenCast portal path used by
//! the rest of the Linux Wayland implementation.

use crate::config::AppConfig;

/// Arch-specific configuration adjustments
pub struct ArchConfig;

impl ArchConfig {
    /// Apply Arch-specific default settings
    ///
    /// Arch uses different package paths and may have different
    /// default desktop environments than Ubuntu.
    pub fn apply_defaults(config: &mut AppConfig) {
        // TODO: Implement Arch-specific defaults
        // - Different clipboard tool preference (wl-clipboard)
        // - Check for sway/hyprland/i3 as primary DEs on Arch
        // - Adjust portal backend preferences
        let _ = config;
    }

    /// Get the preferred screenshot portal for Arch
    ///
    /// On Arch, users commonly run wlroots-based compositors
    /// (sway, hyprland, dwl) where xdg-desktop-portal-wlr
    /// should be preferred over xdg-desktop-portal-gnome.
    pub fn preferred_portal_backend() -> &'static str {
        if is_hyprland() {
            "xdg-desktop-portal-hyprland"
        } else if is_wlroots() {
            "xdg-desktop-portal-wlr"
        } else if desktop_contains(["kde", "plasma"]) {
            "xdg-desktop-portal-kde"
        } else {
            "xdg-desktop-portal-gnome"
        }
    }
}

/// Check if running on a wlroots-based compositor
///
/// Common on Arch: sway, hyprland, dwl, river, cage
pub fn is_wlroots() -> bool {
    is_hyprland()
        || is_sway()
        || desktop_contains([
            "sway", "hyprland", "river", "dwl", "wayfire", "labwc", "niri",
        ])
}

/// Check if running under Hyprland
pub fn is_hyprland() -> bool {
    // Hyprland sets HYPRLAND_INSTANCE_SIGNATURE
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
}

/// Check if running under Sway
pub fn is_sway() -> bool {
    // Sway sets SWAYSOCK
    std::env::var("SWAYSOCK").is_ok()
}

fn desktop_contains<const N: usize>(needles: [&str; N]) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split([':', ';', ','])
        .map(|part| part.trim().to_ascii_lowercase())
        .any(|part| needles.iter().any(|needle| part.contains(needle)))
}

/// Arch-specific dependency checks
pub struct DependencyCheck;

impl DependencyCheck {
    /// Verify all required packages are installed
    pub fn verify_all() -> Vec<String> {
        let mut missing = Vec::new();

        // Core packages that should be present for the ScreenCast portal path.
        let required = [
            "wl-clipboard",
            "pipewire",
            "gst-plugin-pipewire",
            "xdg-desktop-portal",
        ];

        for pkg in &required {
            if !Self::is_installed(pkg) {
                missing.push(pkg.to_string());
            }
        }

        missing
    }

    fn is_installed(pkg: &str) -> bool {
        std::process::Command::new("pacman")
            .args(["-Qq", pkg])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wlroots_detection() {
        // These tests will only pass in actual environments
        // Marked as should_panic until implemented
    }

    #[test]
    fn test_hyprland_detection() {
        // HYPRRLAND_INSTANCE_SIGNATURE is set by Hyprland
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            assert!(is_hyprland());
        }
    }

    #[test]
    fn test_sway_detection() {
        // SWAYSOCK is set by Sway
        if std::env::var("SWAYSOCK").is_ok() {
            assert!(is_sway());
        }
    }
}
