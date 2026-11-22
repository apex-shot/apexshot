# Installing ApexShot

ApexShot runs on Windows and Linux. Pick your platform:

- [Windows](#windows)
- [Linux](#linux)

## Windows

### Requirements

- Windows 10 or Windows 11, 64-bit (x64). Windows 11 on ARM runs the x64
  build under emulation.
- No administrator rights: ApexShot installs for the current user only.

### Install

1. Download `ApexShot-<version>-setup.exe` from the
   [latest release](https://github.com/apex-shot/apexshot/releases/latest).
2. Run it. ApexShot installs to `%LOCALAPPDATA%\Programs\ApexShot`, adds a
   Start menu entry, and opens Settings for first-run onboarding.
3. After onboarding, ApexShot lives in the system tray. Use the tray icon or
   your capture shortcuts to take screenshots and recordings.

The installer is not yet code-signed, so Windows SmartScreen may show
**"Windows protected your PC"**. Choose **More info → Run anyway** to continue.

### Updating

ApexShot lets you know when a new release is available and installs it for
you. You can also download a newer `setup.exe` and run it over the current
installation; your settings, captures, and edits are kept.

### Uninstalling

Open **Settings → Apps → Installed apps**, find **ApexShot**, and choose
**Uninstall**. Your screenshots and recordings stay where they were saved.

### Building from source

Building the Windows installer needs the GTK 4 runtime, Qt 5, Visual Studio
2022, Inno Setup, and an FFmpeg payload. The full steps are in
[`packaging/windows/README.md`](../packaging/windows/README.md).

## Linux

### Supported desktops

Works best today on:

| Environment | Status |
|---|---|
| Ubuntu GNOME Wayland | Public beta, personally tested |
| Arch Linux GNOME Wayland | Public beta, personally tested |
| Hyprland Wayland | Public beta, personally tested |
| Ubuntu 22.04-based derivatives | Their stock GTK 4.6 is below the application's GTK 4.10 requirement; the current native `.deb` is not compatible with that base |
| GNOME Shell 46 (Wayland) | Supported by the extension; GNOME integration tested on Ubuntu 24.04, not an actual Zorin installation |
| Sway / wlroots-like compositors (Wayland) | Implementation exists through GTK4 layer-shell, `wlr-screencopy`, and `wf-recorder`, but needs more manual validation |
| KDE Plasma 6 / Niri / other Wayland desktops | ScreenCast portal + PipeWire path implemented, not yet personally tested |
| Fedora / RHEL (Wayland) | Screenshots supported (Fedora KDE validated). Video recording uses the same portal/PipeWire/GStreamer path as other distros; newly enabled on Fedora, runtime validation in progress |
| openSUSE Tumbleweed / Leap (Wayland) | Development stage: RPM spec + `scripts/build-opensuse-rpm.sh` exist; **no published binary** yet (generic installer will refuse) |
| NixOS (x86_64 Linux) | Source package provided through the repository flake; desktop capture, recording, and hotkeys still need runtime validation |
| Alpine / Gentoo / Void (Wayland) | Development stage: distro-family metadata only; native packaging/testing pending |
| SteamOS / Steam Deck (immutable Arch) | Not supported. The generic installer used to treat this as Arch because `pacman` exists; SteamOS is a read-only image, so that path fails on sudo, pacman keys, and PGP prompts, and any successful pacman install is wiped by the next SteamOS update. The installer now refuses instead. |
| X11 on any distro | Experimental |

If ApexShot breaks on your setup, please [open an issue](https://github.com/apex-shot/apexshot/issues/new/choose).
The issue templates ask for distro, desktop environment, and display server so
triage stays fast.

### Quick install (recommended)

Run the interactive installer. The production-tested paths are Ubuntu/Debian
and Arch Linux; Fedora supports screenshots and video recording like other
distros (recording on Fedora was newly enabled; runtime validation is in
progress). openSUSE remains development-stage for full runtime validation:

```bash
curl -fsSL https://apexshot.org/install | sh
```

### Ubuntu / Debian

The released `.deb` is built on Ubuntu 24.04. Distro-family detection selects
an installer, not a guarantee of binary compatibility: derivatives must satisfy
the package's actual library dependencies. ApexShot requires GTK 4.10 or newer;
the [Ubuntu 22.04 archive](https://packages.ubuntu.com/jammy/libgtk-4-1) provides
GTK 4.6. Don't force-install the package on an older base. The Nix source package
provides its own locked library dependencies on x86_64 Linux, but desktop
integration outside the tested environments still needs validation.

The generic installer above will select this automatically. Direct command:

```bash
curl -fsSL https://apexshot.org/install/ubuntu | bash
```

### Quick install on Arch Linux

The generic installer above will select this automatically. Direct command:

```bash
curl -fsSL https://apexshot.org/install/arch | bash
```

By default this installs the pre-built GitHub Release package. To choose a
different Arch install method explicitly:

```bash
curl -fsSL https://apexshot.org/install/arch | bash -s -- --aur
curl -fsSL https://apexshot.org/install/arch | bash -s -- --source
```

Or install manually from the AUR PKGBUILD:

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot/packaging/arch
makepkg -sf
sudo pacman -U *.pkg.tar.zst
```

AUR publishing notes for maintainers live in
[`AUR_PUBLISHING.md`](AUR_PUBLISHING.md).

> **Note:** The package installs the GNOME Shell extension system-wide.
> Restart GNOME Shell (log out and back in on Wayland) to activate it.

### Fedora Workstation / KDE Plasma / Spins

Fedora is supported for **screenshots** (area / screen / window), preview,
settings, tray, hotkeys, and **video recording**. Fedora KDE Plasma Wayland has
been runtime-validated for the capture path.

> **Video recording on Fedora** uses the same ScreenCast portal, PipeWire, and
> GStreamer path as other Linux distros, on both Fedora GNOME and KDE Plasma
> Wayland. Recording on Fedora was newly enabled; runtime validation is in
> progress. Fedora's `ffmpeg-free`/`openh264` codec packaging is handled by the
> existing encoder probe.

The generic installer selects the Fedora path automatically when `dnf` is
available. Direct command:

```bash
curl -fsSL https://apexshot.org/install/fedora | bash
```

The direct Fedora installer downloads the latest published GitHub Release RPM and
installs it with `dnf`, after ensuring the required runtime packages are present.

A local RPM build path is available for packaging work:

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
scripts/build-fedora-rpm.sh
sudo dnf install target/fedora-rpmbuild/RPMS/*/apexshot-*.rpm
```

For local development reinstalls on Fedora:

```bash
./scripts/fedora-reinstall.sh --start
```

### openSUSE Tumbleweed / Leap

openSUSE support is in development. **No binary openSUSE RPM is published on
GitHub Releases yet**, so the generic installer refuses openSUSE rather than
installing a Fedora RPM by mistake. Build locally for packaging work:

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
scripts/build-opensuse-rpm.sh
sudo zypper install target/opensuse-rpmbuild/RPMS/*/apexshot-*.rpm
```

The openSUSE RPM build path is intended for Tumbleweed / Leap packaging
validation and future OBS work. Runtime testing on KDE Plasma Wayland is still
in progress.

### NixOS

Build the native application and Qt capture helper from the pinned Nixpkgs
input and Cargo lockfile:

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
nix build .#apexshot
./result/bin/apexshot --help
```

This requires Nix's `nix-command` and `flakes` features. For declarative
installation, portal/PipeWire setup, GNOME integration, browser native messaging,
and updates, see [the Nix package guide](../packaging/nix/README.md). The package
supplies application dependencies; it doesn't configure the host's desktop
services. Desktop capture and recording on NixOS are not yet runtime-validated.
The generic installer points NixOS users to this package instead of selecting
an unrelated package manager.

### Updating

Update ApexShot from your terminal:

```bash
apexshot update
```

The command runs the official updater in your current terminal, detects the
distribution, and asks for sudo when needed. Ubuntu/Debian, Arch, and Fedora
use their existing package update paths. If the installed version is current
or newer, it exits without downloading or installing a package. Flatpak installs should use
`flatpak update` from a host terminal instead.
NixOS installs should update their ApexShot flake input and rebuild their
configuration; Nix profile installs should upgrade the corresponding profile
entry. The generic updater does not modify Nix-managed installations.

For older ApexShot versions without this command, the generic updater still works:

```bash
curl -fsSL https://apexshot.org/update | sh
```

Direct Ubuntu / Debian command:

```bash
curl -fsSL https://apexshot.org/update/ubuntu | bash
```

Direct Arch Linux command:

```bash
curl -fsSL https://apexshot.org/update/arch | bash
```

Direct Fedora command:

```bash
curl -fsSL https://apexshot.org/update/fedora | bash
```

Direct openSUSE command (the openSUSE scripts are not proxied through
`apexshot.org` yet, so this one still comes from GitHub):

```bash
curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-update.sh | bash
```

### Build from Source

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
cargo build --release
```

The C++ Qt5 overlay is automatically compiled via CMake during the Rust build.

### System Dependencies (Ubuntu/Debian)

```bash
sudo apt install \
  build-essential cmake pkg-config \
  libx11-dev libxext6 libxtst-dev \
  qtbase5-dev libqt5widgets5 libqt5x11extras5-dev \
  libgstreamer1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  libpipewire-0.3-dev \
  tesseract-ocr libtesseract-dev libleptonica-dev \
  libgtk-4-dev libadwaita-1-dev libgtk4-layer-shell-dev
```

### System Dependencies (Arch Linux)

```bash
sudo pacman -S --needed \
  base-devel rust cargo git cmake clang pkgconf \
  gtk4 libadwaita gtk4-layer-shell \
  gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav gst-plugin-pipewire \
  pipewire pipewire-pulse libpipewire \
  tesseract tesseract-data-eng leptonica \
  qt5-base qt5-x11extras libxtst \
  wl-clipboard xclip libnotify xdg-utils ffmpeg grim \
  xdg-desktop-portal xdg-desktop-portal-hyprland xdg-desktop-portal-wlr
```

### First-Time Setup

After installation, ApexShot will launch an onboarding wizard to help you:

1. **GNOME Extension** (required on GNOME): install the GNOME Shell extension for full functionality
2. **Browser Extension** (optional): set up Chrome/Chromium extension for full-page capture
3. **Cloud Upload** (optional): configure ApexShot Cloud or self-hosted XBackBone for share links

### Manual Install

The manual install command is for local builds. If a package-managed ApexShot
already exists in `/usr/bin`, the installer refuses to place another
`apexshot` in `/usr/local/bin` because that would shadow the `.deb` app.
Use `--dev` for a separate test install instead.

Because binaries need root but autostart lives in your home directory, run
the two parts separately:

```bash
# 1. Install binaries (requires root)
sudo apexshot install --no-autostart

# 2. Set up autostart for the CURRENT user (no sudo)
apexshot install --no-binary
```

For a local development build that can live beside the official package:

```bash
cargo build --release
sudo target/release/apexshot install --dev --no-autostart
apexshot-dev
```

Optional flags:

```bash
# Also install the browser native-messaging host (for the web-scroll extension)
sudo apexshot install --no-autostart --extension-id <chrome-extension-id>

# Force reinstall even if the same version is already present
sudo apexshot install --no-autostart --force

# Remove the separate development install
sudo apexshot-dev uninstall --dev
```

### GNOME Extension (Recommended)

The GNOME Shell extension improves ApexShot on GNOME Wayland. Without it,
preview windows may not stay on top, the recording mask will not appear, and the
window picker cannot list windows.

This is a GNOME platform limitation, not a sign that the app is unfinished:
normal desktop apps cannot freely draw above every window or enumerate windows on
GNOME Wayland. The extension gives ApexShot the shell-side hooks it needs.

Recording itself works without the extension. Pause, stop, and restart are always
available through global shortcuts, the tray icon, and the recording notification.

**Supported GNOME versions:** 46 and 48–50

#### Install from GitHub Release (Recommended)

```bash
# Download the GNOME extension (finds latest release that has the zip)
curl -fsSL -o apexshot-gnome-integration.zip \
  "$(curl -fsSL https://api.github.com/repos/apex-shot/apexshot/releases | grep -o '"browser_download_url": *"[^"]*apexshot-gnome-integration.zip"' | head -n 1 | cut -d '"' -f 4)"

# Install using gnome-extensions
gnome-extensions install apexshot-gnome-integration.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

#### Install from Source

If you cloned the repository and want to install the development version:

```bash
cd gnome-extension
zip apexshot-gnome-integration.zip \
  extension.js metadata.json \
  cursor-classifier.js press-tracker.js shell-overlay.js window-list.js preview-stacking.js daemon-ownership.js capture-menu-placement.js
gnome-extensions install apexshot-gnome-integration.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

#### Install via Extension Manager (GUI)

1. Install [Extension Manager](https://flathub.org/apps/com.mattjakeman.ExtensionManager) from Flathub
2. Click **Browse** and search for "ApexShot"
3. Install and enable the extension

> **Note:** The extension may not yet be published on extensions.gnome.org. Use the release zip or source methods above until it is available.

#### Verify Installation

```bash
# Check that the extension is installed and enabled
gnome-extensions list
gnome-extensions info apexshot-gnome-integration@apexshot.github.io

# Check GNOME Shell logs for ApexShot activity
journalctl /usr/bin/gnome-shell -f | grep apexshot
```

#### What the Extension Provides

- **Always-on-top preview windows**: screenshot previews and annotation editor windows stay above other applications during drag operations
- **Shell-managed recording mask**: a dimmed mask highlights the selected recording area
- **Window list for the window picker**: lets ApexShot enumerate and focus windows, which Wayland clients cannot do themselves

#### Troubleshooting

| Issue | Solution |
|---|---|
| Preview windows get hidden behind other apps | Verify extension is enabled: `gnome-extensions list` |
| Recording mask does not appear | Check logs: `journalctl /usr/bin/gnome-shell -f | grep apexshot` |
| Extension fails to enable | Confirm GNOME Shell version is 46 or 48–50 and matches `metadata.json` |
| D-Bus signals not working | Monitor session bus: `dbus-monitor --session | grep apexshot` |

**Known Limitations:**
- Capture overlay is tied to the window where it was initiated. Moving to another application window will hide the overlay until you return to the original window.

**Note:** The onboarding wizard will automatically guide you through installing the GNOME extension on first launch.

### Running ApexShot

#### Default Behavior (Deb Package)

The deb package installs ApexShot as a background daemon with system tray icon and global hotkeys by default. It starts automatically on login.

#### Manual Daemon Mode

If you built from source or want to run the daemon manually:

```bash
apexshot daemon
```

#### CLI Commands

```bash
# Screenshots
apexshot capture screen          # Full screen capture
apexshot capture area            # Area selection capture
apexshot capture crosshair       # Crosshair / precise point capture

# Recording
apexshot record screen           # Full screen recording

# OCR (requires image path)
apexshot ocr <image-path>        # Extract text from image

# Image Editor
apexshot edit <image-path>       # Open image in annotation editor
apexshot image-editor            # Open empty image editor (drag & drop or browse to load)

# Video Editor
apexshot video-editor            # Open video editor (with optional MP4 path)
apexshot video-editor <video>    # Open video editor with a specific video

# Settings and cloud
apexshot settings                # Open settings window
apexshot history                 # Open capture history window (captures, edits, cloud)
apexshot login                   # Sign in to ApexShot Cloud (device flow)
apexshot logout                  # Sign out of ApexShot Cloud

# Daemon / install helpers
apexshot daemon                  # Background tray + hotkeys
apexshot hotkeys install         # Install desktop keybindings
apexshot install                 # Local binary + autostart install
apexshot update                  # Update to the latest published release
apexshot --version               # Print version
```

#### Recording on non-GNOME systems (Hyprland, Sway, KDE, X11)

> **Fedora:** Video recording uses the same portal/PipeWire/GStreamer path as
> other distros. Recording on Fedora was newly enabled; runtime validation is
> in progress. See the [Fedora install section](#fedora-workstation--kde-plasma--spins).

ApexShot uses a fully self-contained Rust stack for recording on systems
without GNOME Shell. No Qt overlay, no shell extension:
the daemon, the GTK4 overlay, and native PipeWire handle everything.

**How it works:**
1. The daemon runs in the background with a system tray icon and global hotkeys.
2. Triggering a recording action (`apexshot record screen`, tray click, or hotkey)
   opens the Rust GTK4 quick-capture menu.
3. The menu picks what to capture and the mic/speaker toggles; format
   (MP4/WebM), countdown, resolution and quality come from Settings →
   Recording, shared with the GNOME path.
4. Once confirmed, recording begins. On wlroots compositors (Hyprland/Sway),
   `wf-recorder` is preferred when installed for native `wlr-screencopy`
   capture. On other Wayland compositors, native PipeWire capture
   (`src/pipewire_engine.rs`) grabs frames via the XDG ScreenCast portal,
   and ffmpeg encodes them to the chosen format. On X11, a GStreamer
   `ximagesrc` pipeline is used as fallback.
5. During recording, a floating GTK4 stop overlay shows pause/stop controls
   and elapsed time. Pause/resume/stop can also be triggered from the tray menu
   or global hotkeys.

**What you get without GNOME:**
- Full-screen recording (MP4, WebM)
- Mic and speaker audio capture with level monitoring
- Countdown timer before recording starts
- Pause/resume/restart/stop during recording
- Post-recording video editor (trim, resize, re-encode)
- System tray with quick-access actions
- Global hotkeys configurable in Settings

```bash
# Start the daemon (launches tray icon + hotkey listener)
apexshot daemon

# Record directly from CLI without the daemon
apexshot record screen              # Full screen recording

# Control an active recording (requires daemon)
apexshot record stop                # Stop and save
apexshot record toggle-pause        # Pause / resume
apexshot record restart             # Restart
apexshot record discard             # Discard without saving
```

#### Keyboard Shortcuts

Configure global hotkeys in Settings > Shortcuts. The daemon supports:

- **Capture shortcuts**: full screen, area, window, last capture
- **Recording shortcuts**: start/stop/pause recording
- **Custom shortcuts**: record and assign any key combination per action
