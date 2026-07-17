# Arch Linux Support

This document outlines Arch Linux packaging and install paths for ApexShot.

## Status

- PKGBUILD and AUR install hooks are in-tree and versioned with the project (`0.2.32` at time of writing; always trust `Cargo.toml` / the release tag).
- Runtime distro metadata is wired in `src/distro/`.
- Ubuntu GNOME Wayland, Arch GNOME Wayland, and Hyprland Wayland are the known-good personal-test targets (see root `README.md`).
- Broader desktop-environment coverage on Arch still benefits from community testing.

## File Structure

```
apexshot/
├── packaging/
│   ├── deb/                 # Debian helper binaries / assets
│   ├── debian/              # Debian maintainer scripts
│   ├── arch/                # Arch / AUR packaging
│   │   ├── PKGBUILD
│   │   ├── apexshot.install # Pacman install hooks (used by PKGBUILD `install=`)
│   │   ├── .INSTALL         # Legacy/alternate install hook copy (prefer apexshot.install)
│   │   └── .SRCINFO         # AUR metadata snapshot
│   ├── fedora/              # Fedora RPM spec
│   └── opensuse/            # openSUSE RPM spec
├── scripts/
│   ├── install.sh           # Distro-detecting installer entrypoint
│   ├── arch-install.sh      # Arch installer (GitHub release / AUR / source)
│   ├── install-arch.sh      # Backward-compatible wrapper → arch-install.sh
│   ├── arch-update.sh
│   └── aur-prepare.sh       # Tag-based AUR package prep helper
├── src/
│   └── distro/              # Distro detection + Arch helpers
│       ├── mod.rs
│       └── arch/
│           └── mod.rs
└── docs/
    ├── ARCH_SUPPORT.md      # This file
    └── AUR_PUBLISHING.md    # Maintainer AUR publish flow
```

## Components

### 1. Packaging (`packaging/arch/`)

- **PKGBUILD**: Package build script for pacman/AUR
- **apexshot.install**: Post-install/upgrade/remove hooks (`install=apexshot.install` in PKGBUILD)
- **.SRCINFO**: Metadata for AUR submission

#### Build locally

```bash
cd packaging/arch
makepkg -si
```

#### AUR publish

Prefer the automated flow in [`AUR_PUBLISHING.md`](AUR_PUBLISHING.md) (`scripts/aur-prepare.sh` + CI). Manual outline:

```bash
scripts/aur-prepare.sh vX.Y.Z
git clone ssh://aur@aur.archlinux.org/apexshot.git /tmp/apexshot-aur
cp packaging/arch/PKGBUILD packaging/arch/.SRCINFO packaging/arch/apexshot.install /tmp/apexshot-aur/
cd /tmp/apexshot-aur
git add PKGBUILD .SRCINFO apexshot.install
git commit -m "Update to X.Y.Z"
git push
```

### 2. Install scripts

Recommended user path (README):

```bash
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/install.sh | bash
# or directly:
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-install.sh | bash
```

`scripts/install-arch.sh` remains as a compatibility wrapper that execs `arch-install.sh`.

Methods supported by `arch-install.sh`:

1. **GitHub Release package** (default) — pre-built release asset
2. **AUR** — `yay` / `paru` when requested (`--aur`)
3. **Source build** — compile from tree (`--source`)

### 3. Distro module (`src/distro/`)

```rust
use apexshot::distro::DistroInfo;

let distro = DistroInfo::detect();
if distro.is_arch() {
    // Arch-family branch
}
```

- **`DistroInfo::detect()`**: Parses `/etc/os-release`
- Helpers: `is_arch()`, `is_debian()`, `is_fedora()`, etc.

## Runtime capture notes on Arch

ApexShot picks the best capture route at runtime (not at package build time):

| Environment | Still screenshots | Recording |
|---|---|---|
| GNOME Wayland | C++ overlay + Screenshot portal; shell extension for masks/previews | PipeWire + ffmpeg; shell extension controls when available |
| Hyprland / Sway | `wlr-screencopy` / grim fallback | `wf-recorder` preferred when installed; else PipeWire + ffmpeg |
| Other Wayland | Portal + PipeWire paths | ScreenCast portal + PipeWire + ffmpeg |
| X11 | Experimental `x11rb` path | GStreamer `ximagesrc` fallback |

Arch package dependencies intentionally include both Wayland and X11 clipboard helpers (`wl-clipboard`, `xclip`) so one package works across desktops.

## Fedora note

**Video recording is not supported on Fedora** (`DistroInfo::is_fedora()`). All
recording entry points refuse with a desktop notification; screenshots remain
supported. Details:
[`progress-fedora-kde-overlay-and-preview.md`](progress-fedora-kde-overlay-and-preview.md).

## Related docs

- [`README.md`](../README.md) — install matrix and user commands
- [`AUR_PUBLISHING.md`](AUR_PUBLISHING.md) — release → AUR automation
- [`DEVELOPER_GUIDE.md`](DEVELOPER_GUIDE.md) — build from source
- [`progress-fedora-kde-overlay-and-preview.md`](progress-fedora-kde-overlay-and-preview.md) — Fedora product limits
