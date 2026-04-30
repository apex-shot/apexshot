//! Distribution-specific integrations
//!
//! This module provides distro-specific code paths that differ from
//! the standard Ubuntu/GNOME defaults. Each submodule corresponds
//! to a specific Linux distribution or family.

// Currently only Ubuntu/GNOME is fully supported.
// Other distributions are scaffolded for future implementation.

// TODO: Arch Linux support - see docs/ARCH_SUPPORT.md
// pub mod arch;

// TODO: Fedora/RHEL support  
// pub mod fedora;

// TODO: openSUSE support
// pub mod opensuse;

// TODO: NixOS support
// pub mod nixos;

use std::env;

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
        // TODO: Parse /etc/os-release
        // For now, return None to use default Ubuntu/GNOME behavior
        None
    }

    /// Check if this is an Arch-based distribution
    pub fn is_arch(&self) -> bool {
        self.id == "arch" || self.id_like.contains(&"arch".to_string())
    }

    /// Check if this is a Debian/Ubuntu-based distribution
    pub fn is_debian(&self) -> bool {
        self.id == "ubuntu" 
            || self.id == "debian"
            || self.id_like.contains(&"debian".to_string())
            || self.id_like.contains(&"ubuntu".to_string())
    }

    /// Check if this is a Fedora/RHEL-based distribution
    pub fn is_fedora(&self) -> bool {
        self.id == "fedora" 
            || self.id == "rhel"
            || self.id == "centos"
            || self.id == "almalinux"
            || self.id == "rocky"
            || self.id_like.contains(&"fedora".to_string())
            || self.id_like.contains(&"rhel".to_string())
    }

    /// Check if this is openSUSE
    pub fn is_opensuse(&self) -> bool {
        self.id == "opensuse-tumbleweed" 
            || self.id == "opensuse-leap"
            || self.id == "opensuse"
            || self.id_like.contains(&"suse".to_string())
    }

    /// Check if this is NixOS
    pub fn is_nixos(&self) -> bool {
        self.id == "nixos"
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
        match () {
            // Arch uses standard XDG paths
            _ if distro.is_arch() => Self {
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
            },
            // Default: Ubuntu/GNOME uses standard XDG
            _ => Self {
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
            },
        }
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
