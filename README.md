# ApexShot

A premium, open-source Linux screen capture tool with annotation, recording, and OCR.

> **Note:** Currently tested on GNOME Ubuntu (Wayland). Support for other distributions and desktop environments will be added as the project grows.

![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux-lightgrey.svg)
![Version](https://img.shields.io/badge/version-0.2.19-orange.svg)
![Status](https://img.shields.io/badge/status-Alpha-yellow.svg)

## Features

### Screenshots
- **Multiple Capture Modes** — Full screen, area selection, window capture, and crosshair mode
- **Image Editor** — Annotate with arrows, shapes, text, blur, pixelate, highlighter, and more
- **OCR** — Extract text from images using Tesseract and ocrs dual-engine OCR
- **QR Code Detection** — Automatically detect and copy QR codes from screenshots

### Screen Recording
- **Flexible Recording** — Area or full-screen recording with MP4/GIF output
- **Audio Monitoring** — Real-time mic and speaker level monitoring via PipeWire
- **Webcam PiP** — Picture-in-picture webcam overlay during recording
- **Recording Controls** — Pause, resume, and stop recording with on-screen controls
- **Runtime Overlays** — Display keystrokes and click events during recording (GNOME extension)

### Integration
- **Daemon Mode** — Background service with system tray and global hotkeys for instant capture
- **Dual Display Support** — Works on both X11 and Wayland (including GNOME)
- **Browser Integration** — Full-page scroll capture via Chrome/Chromium extension
- **GNOME Integration** — Always-on-top previews and shell-managed recording overlays
- **Smart Clipboard** — Automatic clipboard integration for quick sharing

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Core** | Rust 2021 Edition |
| **Native Overlay** | C++17 / Qt5 (region selection, drawing) |
| **GUI** | GTK4 + gtk4-layer-shell |
| **Display Servers** | X11 (x11rb + MIT-SHM), Wayland (ashpd + wlr-screencopy) |
| **Recording** | GStreamer (VP8, VP9, H.264, H.265, Theora, GIF) |
| **Audio** | PipeWire (mic/speaker level monitoring) |
| **OCR** | Tesseract + ocrs/rten |
| **System Tray** | ksni (KDE System Tray Integration) |
| **Webcam** | GStreamer + v4l2 |

## Download

### Debian/Ubuntu (.deb)

Download and install the latest `.deb` package from GitHub Releases:

```bash
# Download and install the latest release
curl -s https://api.github.com/repos/apex-shot/apexshot/releases/latest | grep "browser_download_url.*amd64.deb" | cut -d '"' -f 4 | xargs wget && sudo dpkg -i apexshot_*.deb && sudo apt install -f
```

Or manually download from [GitHub Releases](https://github.com/apex-shot/apexshot/releases):

```bash
# Install the downloaded package
sudo dpkg -i apexshot_*.deb
sudo apt install -f  # Install any missing dependencies
```

### Build from Source

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
cargo build --release
```

The C++ Qt5 overlay is automatically compiled via CMake during the Rust build.

## Installation

### System Dependencies (Ubuntu/Debian)

```bash
sudo apt install \
  build-essential cmake pkg-config \
  libx11-dev libxext6 libxtst-dev \
  qtbase5-dev libqt5widgets5 libqt5x11extras5-dev libqt5network5-dev libqt5dbus5-dev \
  libgstreamer1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
  libpipewire-0.3-dev \
  tesseract-ocr \
  libgtk-4-dev libadwaita-1-dev libgtk4-layer-shell-dev
```

### First-Time Setup

After installation, ApexShot will launch an onboarding wizard to help you:

1. **GNOME Extension** (required) — Install the GNOME Shell extension for full functionality
2. **Browser Extension** (optional) — Set up Chrome/Chromium extension for full-page capture
3. **Cloud Sync** (coming soon) — Configure cloud storage for automatic backup

### Manual Install

```bash
sudo apexshot install                          # Binary + autostart
sudo apexshot install --extension-id <id>      # + browser native messaging host
sudo apexshot install --force                  # Reinstall even if same version
```

### GNOME Extension (Required)

ApexShot requires the GNOME Shell extension for full functionality on GNOME:

```bash
# Download from GitHub releases
wget https://github.com/apex-shot/apexshot/releases/download/gnome-extension-v2/apexshot-gnome-integration.zip

# Install using gnome-extensions
gnome-extensions install apexshot-gnome-integration.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

The extension provides:
- Always-on-top preview windows during drag operations
- Shell-managed recording masks
- Runtime overlays (click display)

**Known Limitations:**
- Keystroke display is not currently functional on GNOME due to platform constraints. Only click display is supported during recording.
- Capture overlay is tied to the window where it was initiated. Moving to another application window will hide the overlay until you return to the original window.

**Note:** The onboarding wizard will automatically guide you through installing the GNOME extension.

## Usage

### Default Behavior (Deb Package)

The deb package installs ApexShot as a background daemon with system tray icon and global hotkeys by default. It starts automatically on login.

### Manual Daemon Mode

If you built from source or want to run the daemon manually:

```bash
apexshot daemon
```

### CLI Commands

```bash
# Screenshots
apexshot capture screen          # Full screen capture
apexshot capture area            # Area selection capture
apexshot capture window          # Window capture

# Recording
apexshot record screen           # Full screen recording
apexshot record area --gif       # Area recording as GIF

# OCR (requires image path)
apexshot ocr <image-path>        # Extract text from image

# Editor (requires image path)
apexshot edit <image-path>       # Open image in annotation editor

# Settings
apexshot settings                # Open settings window
```

### Keyboard Shortcuts

Configure global hotkeys in Settings > Shortcuts. The daemon supports:

- **Capture shortcuts** — Full screen, area, window, last capture
- **Recording shortcuts** — Start/stop/pause recording
- **Custom shortcuts** — Record and assign any key combination per action

## Project Structure

```
apexshot/
├── src/                    # Rust core (capture, editor, recording, settings, daemon)
│   ├── capture/            # Screen capture logic
│   ├── editor/             # Image annotation editor
│   ├── recording/          # Screen recording with GStreamer
│   ├── settings/           # Settings UI and management
│   ├── onboarding/         # First-time setup wizard
│   └── gnome_integration/  # GNOME Shell integration
├── capture-overlay/        # C++ Qt5 native overlay (region selection, drawing)
├── gnome-extension/        # GNOME Shell extension (preview windows, recording mask)
├── web-scroll-extension/   # Chrome/Chromium extension (full-page scroll capture)
├── native-host/            # Native messaging host for browser integration
├── packaging/              # Package assets (desktop files, icons, deb helper)
└── docs/                   # Architecture, data flow, and implementation docs
```

## Development

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

### Building Debian Package

```bash
cargo deb
```

The package will be created in `target/debian/`.

### Running from Source

```bash
cargo run -- daemon
```

### Code Style

- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Follow Rust best practices and idioms

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

GPL-3.0 — See [LICENSE](LICENSE) for details.
