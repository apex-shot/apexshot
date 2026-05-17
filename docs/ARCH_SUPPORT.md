# Arch Linux Support

This document outlines the Arch Linux support implementation for ApexShot.

## Status: Source, Package Build, and Runtime Metadata Implemented

The infrastructure for Arch Linux support has been created, the source-build dependency list has been tested on Arch, the PKGBUILD has completed a local package build, and the runtime support metadata is wired into `src/distro`. Installer and desktop-environment coverage still need more testing. This approach ensures:
- **Isolation**: Arch-specific code is separate from Ubuntu/GNOME implementation
- **Maintainability**: Ubuntu code remains unchanged and functional
- **Extensibility**: Pattern can be replicated for other distributions

## File Structure

```
/home/codegoddy/apexshot/
├── packaging/
│   ├── deb/              # Ubuntu/Debian packages (existing)
│   ├── debian/           # Debian maintainer scripts (existing)
│   └── arch/             # NEW: Arch Linux packaging
│       ├── PKGBUILD      # Package build script
│       ├── .INSTALL      # Pacman install hooks
│       └── .SRCINFO      # AUR metadata
├── scripts/
│   ├── install.sh        # Ubuntu installer (existing)
│   ├── update.sh         # Update script (existing)
│   └── install-arch.sh   # NEW: Arch Linux installer
├── src/
│   ├── gnome_integration/ # GNOME-specific code (existing, unchanged)
│   └── distro/            # NEW: Distribution-specific code
│       ├── mod.rs         # Core distro detection
│       └── arch/
│           └── mod.rs     # Arch-specific integrations
└── docs/
    └── ARCH_SUPPORT.md    # This file
```

## Components

### 1. Packaging (`packaging/arch/`)

- **PKGBUILD**: Package build script for pacman/AUR
- **.INSTALL**: Post-install/upgrade/remove hooks
- **.SRCINFO**: Metadata for AUR submission

#### To Build Locally (for testing):

```bash
cd packaging/arch
makepkg -si
```

#### To Submit to AUR:

```bash
# Create AUR git repository
git clone ssh://aur@aur.archlinux.org/apexshot.git
cp packaging/arch/PKGBUILD apexshot/
cp packaging/arch/.INSTALL apexshot/
cd apexshot
makepkg --printsrcinfo > .SRCINFO
git add .
git commit -m "Initial release v0.2.25"
git push
```

### 2. Install Script (`scripts/install-arch.sh`)

Three installation methods supported:
1. **AUR** (recommended): Uses `yay`, `paru`, or installs `yay` first
2. **GitHub Release**: Downloads pre-built binaries
3. **Source Build**: Compiles from source with pacman dependencies

#### Usage:

```bash
# Direct install from GitHub
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/install-arch.sh | bash

# Or manually
bash scripts/install-arch.sh
```

### 3. Distro Module (`src/distro/`)

Provides distribution detection and platform-specific code:

```rust
use apexshot::distro::DistroInfo;

let distro = DistroInfo::detect();
if distro.is_arch() {
    // Apply Arch-specific settings
}
```

#### Key Features:

- **`DistroInfo::detect()`**: Parses `/etc/os-release`
- **Helper methods**: `is_arch()`, `is_debian()`, `is_fedora()`, etc.
- **`PlatformPaths`**: Distribution-specific file paths
- **`DistroSupport`**: Runtime dependency and portal backend guidance for Debian/Ubuntu, Arch, Fedora/RHEL, openSUSE, NixOS, Alpine, Gentoo, Void, and unknown Linux families

### 4. Arch Module (`src/distro/arch/`)

Arch-specific functionality:

- **Desktop environment detection**: Hyprland, Sway, i3, etc.
- **Portal backend preference**: `xdg-desktop-portal-hyprland`, `xdg-desktop-portal-wlr`, `xdg-desktop-portal-kde`, or GNOME based on session detection
- **Dependency checking**: Verify pacman packages
- **Capture policy**: Hyprland/wlroots sessions try native `wlr-screencopy` first for non-interactive monitor frames, then fall back to the shared XDG ScreenCast portal + PipeWire path

## Implementation Roadmap

### Phase 1: Testing (Current)

- [x] Test PKGBUILD local package build
- [ ] Test `install-arch.sh` script
- [x] Verify source-build dependencies are correctly specified
- [ ] Test on major Arch-based distros:
  - [ ] Manjaro
  - [ ] EndeavourOS
  - [ ] Garuda Linux

### Phase 2: Core Integration

- [x] Implement `DistroInfo::detect()` in `src/distro/mod.rs`
- [x] Uncomment `pub mod arch;` in distro module
- [x] Add portal backend selection for wlroots/Hyprland/KDE/GNOME
- [ ] Add Arch-specific defaults to Config

### Phase 3: Desktop Environment Support

- [ ] Test with Hyprland
- [ ] Test with Sway
- [ ] Test with KDE Plasma on Arch
- [ ] Test with GNOME on Arch
- [ ] Test with XFCE/i3

### Phase 4: AUR Submission

- [ ] Create AUR account
- [ ] Submit `apexshot` package
- [ ] Set up automated AUR updates via CI/CD

## Key Differences from Ubuntu

| Aspect | Ubuntu | Arch |
|--------|--------|------|
| Package Manager | apt/dpkg | pacman |
| Desktop Portal | xdg-desktop-portal-gnome | xdg-desktop-portal-hyprland / xdg-desktop-portal-wlr |
| Clipboard | wl-clipboard | wl-clipboard (same) |
| OCR | tesseract-ocr | tesseract |
| Build Deps | build-essential | base-devel |
| Install Path | /usr/bin | /usr/bin (same) |
| Config Path | ~/.config/apexshot | ~/.config/apexshot (same) |

## Shared Wayland Capture Policy

For other Flameshot-compatible Linux distros, ApexShot should keep the same Wayland capture method instead of adding distro-specific screenshot backends:

1. On Hyprland, Sway, and wlroots-like sessions, try direct `zwlr_screencopy_manager_v1` capture for non-interactive monitor frames.
2. Fall back to `org.freedesktop.portal.ScreenCast` to request a shared monitor/window stream.
3. Read the selected PipeWire node through GStreamer.
4. Reuse restore tokens where the portal backend supports them.
5. Keep `org.freedesktop.portal.Screenshot` only for explicit interactive selector helpers.
6. Leave GNOME Shell extension features behind GNOME session gates so non-GNOME desktops use portal-backed capture without requiring the extension.

The distro work that remains is packaging, dependency naming, portal backend installation defaults, and manual testing on each distro/desktop combination.

## Testing Checklist

Before enabling Arch support:

```bash
# 1. Build on Arch
makepkg -si

# 2. Test basic functionality
apexshot --version
apexshot capture screen
apexshot capture area
apexshot record screen --duration 5

# 3. Test settings
apexshot settings

# 4. Test browser extension (if applicable)

# 5. Test on different DEs
# - Hyprland
# - Sway
# - KDE Plasma
# - GNOME
```

## Known Issues

None yet - this is a fresh scaffold.

## Contributing

When adding Arch support:

1. **Never modify Ubuntu code**: Keep implementations isolated
2. **Use feature flags**: Consider `#[cfg(feature = "arch-support")]` for conditional compilation
3. **Test thoroughly**: Arch users expect bleeding-edge but stable software
4. **Document in AUR**: Update PKGBUILD comments with any special instructions

## References

- [Arch Wiki: PKGBUILD](https://wiki.archlinux.org/title/PKGBUILD)
- [Arch Wiki: Creating Packages](https://wiki.archlinux.org/title/Creating_packages)
- [AUR Submission Guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
