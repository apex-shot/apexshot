# ApexShot

A premium, open-source Linux screen capture tool with annotation, recording, and OCR.

![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)
![Platform](https://img.shields.io/badge/platform-Linux-lightgrey.svg)
![Version](https://img.shields.io/badge/version-0.1.0-orange.svg)

## Features

- **Screenshot Capture** — Full screen, area selection, window capture, and crosshair mode
- **Image Editor** — Annotate with arrows, shapes, text, blur, pixelate, highlighter, and more
- **Screen Recording** — Area or full-screen recording with MP4/GIF output, audio monitoring, and webcam PiP
- **OCR** — Extract text from images using Tesseract and ocrs dual-engine OCR
- **Daemon Mode** — Background service with system tray and global hotkeys for instant capture
- **Dual Display Support** — Works on both X11 and Wayland (including GNOME)
- **Browser Integration** — Full-page scroll capture via Chrome/Chromium extension
- **GNOME Integration** — Always-on-top previews and shell-managed recording overlays

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

Download the latest `.deb` package from [GitHub Releases](https://github.com/apex-shot/apexshot/releases).

```bash
# Install the downloaded package
sudo apt install ./apexshot_*.deb
```

Dependencies are automatically installed from your system's package manager.

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

### Build from Source

```bash
git clone https://github.com/apex-shot/apexshot.git
cd apexshot
cargo build --release
```

The C++ Qt5 overlay is automatically compiled via CMake during the Rust build.

### Install

```bash
sudo apexshot install                          # Binary + autostart
sudo apexshot install --extension-id <id>      # + browser native messaging host
```

## Usage

### Daemon Mode (Recommended)

Runs as a background service with system tray icon and global hotkeys:

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

# OCR
apexshot ocr <image-path>        # Extract text from image

# Editor
apexshot edit <image-path>       # Open image in annotation editor

# Settings
apexshot settings                # Open settings window
```

### Keyboard Shortcuts

Configure global hotkeys in Settings > Shortcuts. The daemon supports:

- **Capture shortcuts** — Full screen, area, window, last region
- **Recording shortcuts** — Start/stop/pause recording
- **Custom shortcuts** — Record and assign any key combination per action

## Project Structure

```
apexshot/
├── src/                    # Rust core (capture, editor, recording, settings, daemon)
├── capture-overlay/        # C++ Qt5 native overlay (region selection, drawing)
├── gnome-extension/        # GNOME Shell extension (preview windows, recording mask)
├── web-scroll-extension/   # Chrome/Chromium extension (full-page scroll capture)
├── native-host/            # Native messaging host for browser integration
└── docs/                   # Architecture, data flow, and implementation docs
```

## Documentation

- [Architecture](docs/Architecture.md) — Module map, threading model, FFI bridge
- [Data Flow](docs/Data_Flow.md) — Capture pipeline, IPC protocol, memory management
- [Implementation Details](docs/Implementation_Details.md) — Editor pipeline, debugging guide
- [Linux Interactions](docs/Linux_Interactions.md) — X11/Wayland support, permissions, dependencies

## License

GPL-3.0 — See [LICENSE](LICENSE) for details.
