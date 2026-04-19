# ApexShot Architecture

## Overview

ApexShot is a Linux screen capture tool written in Rust, featuring screenshot capture, screen recording, OCR, and annotation capabilities. The application is designed primarily for GNOME (Wayland) and integrates with the GNOME Shell extension for enhanced functionality.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         ApexShot                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐         │
│  │   Daemon     │  │   GUI App    │  │   CLI App    │         │
│  │  (daemon/)   │  │   (main.rs)  │  │   (main.rs)  │         │
│  └──────────────┘  └──────────────┘  └──────────────┘         │
│         │                 │                 │                 │
│         └─────────────────┴─────────────────┘                 │
│                           │                                   │
│                    ┌──────▼──────┐                            │
│                    │   Config    │                            │
│                    │  (config.rs) │                            │
│                    └─────────────┘                            │
│                           │                                   │
│         ┌─────────────────┼─────────────────┐                 │
│         │                 │                 │                 │
│  ┌──────▼──────┐  ┌──────▼──────┐  ┌──────▼──────┐          │
│  │  Capture    │  │  Recording  │  │  Settings   │          │
│  │ (capture/)  │  │(recording/) │  │(settings/)  │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│         │                 │                 │                 │
│         └─────────────────┼─────────────────┘                 │
│                           │                                   │
│                    ┌──────▼──────┐                            │
│                    │   Overlay    │                            │
│                    │(overlay.rs)  │                            │
│                    └─────────────┘                            │
│                           │                                   │
│         ┌─────────────────┼─────────────────┐                 │
│         │                 │                 │                 │
│  ┌──────▼──────┐  ┌──────▼──────┐  ┌──────▼──────┐          │
│  │ Annotations │  │     OCR     │  │   GNOME     │          │
│  │(annotations/)│  │   (ocr/)    │  │  (gnome_*)  │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
                           │
                    ┌──────▼──────┐
                    │  C++ Qt5    │
                    │  Overlay    │
                    │(capture-    │
                    │  overlay/)  │
                    └─────────────┘
                           │
                    ┌──────▼──────┐
                    │  GNOME      │
                    │  Extension  │
                    │(gnome-      │
                    │ extension/) │
                    └─────────────┘
```

## Core Components

### 1. Daemon (`src/daemon/`)
The daemon runs in the background with system tray icon and global hotkeys. It handles:
- Global hotkey registration and handling
- System tray icon and menu
- Background recording and capture operations
- D-Bus communication for inter-process coordination

### 2. Main Application (`src/main.rs`)
Entry point for both GUI and CLI modes. Handles:
- Command-line argument parsing
- Mode selection (daemon, GUI, CLI)
- Application initialization

### 3. Configuration (`src/config.rs`)
Centralized configuration management using YAML serialization:
- App settings (export locations, shortcuts, etc.)
- Recording settings (format, fps, quality, etc.)
- Screenshot settings (format, cursor, etc.)
- Advanced settings (filename patterns, OCR, etc.)

### 4. Capture Module (`src/capture/`)
Screen capture functionality:
- Full screen, area, and window capture
- Support for both X11 and Wayland
- Integration with capture-overlay for region selection
- Screenshot saving with configurable naming patterns

### 5. Recording Module (`src/recording/`)
Screen recording with GStreamer:
- Video recording (MP4) with various codecs
- GIF recording with quality/size optimization
- Runtime overlays (clicks, keystrokes, webcam)
- Recording controls (pause, resume, stop, restart)
- GNOME Shell integration for recording masks

### 6. Overlay (`src/overlay.rs`)
Native overlay window for:
- Region selection during capture
- Recording controls UI
- Runtime overlay display (clicks, keystrokes, time)
- Built with GTK4

### 7. Settings (`src/settings/`)
GTK4-based settings UI:
- General settings
- Screenshot settings
- Recording settings (General, Video, GIF, Overlay)
- Annotation settings
- Quick access settings
- Advanced settings

### 8. Annotations (`src/annotations/`)
Image annotation editor:
- Drawing tools (pen, arrow, text, number, blur, crop)
- Color management
- Serialization/deserialization of annotations
- Undo/redo support

### 9. OCR (`src/ocr/`)
Text recognition using Tesseract:
- Multi-language support
- Line break preservation
- Integration with annotation editor

### 10. GNOME Integration (`src/gnome_integration/`, `src/gnome_shell.rs`)
D-Bus communication with GNOME Shell extension:
- Window stacking management
- Recording mask support
- Runtime overlay support
- Extension installation and management

### 11. Hotkeys (`src/hotkeys/`)
Global hotkey management:
- Hotkey registration
- Hotkey handling
- Integration with daemon

### 12. Tray (`src/tray/`)
System tray icon:
- Tray menu
- Quick actions
- Status indicators

### 13. Onboarding (`src/onboarding/`)
First-time setup wizard:
- Welcome screen
- GNOME extension installation
- Chrome extension setup
- Cloud sync setup (future)
- Completion screen

### 14. Utils (`src/utils/`)
Utility functions:
- Common helper functions
- File operations
- System information

## External Components

### C++ Qt5 Capture Overlay (`capture-overlay/`)
Native C++ overlay for:
- Region selection
- Drawing tools
- Cross-platform window management

### GNOME Shell Extension (`gnome-extension/`)
GNOME Shell extension providing:
- Always-on-top preview windows
- Recording mask support
- Runtime overlay support
- D-Bus communication with main app

## Data Flow

### Screenshot Flow
1. User triggers capture (hotkey, GUI, or CLI)
2. Capture module initiates screen capture
3. Capture-overlay shown for region selection (if area capture)
4. Screenshot captured and saved to configured location
5. Optional: Open in annotation editor
6. Optional: Copy to clipboard
7. Optional: Show quick access overlay

### Recording Flow
1. User triggers recording (hotkey, GUI, or CLI)
2. Recording module prepares GStreamer pipeline
3. GNOME extension provides recording mask (if enabled)
4. Recording starts with runtime overlays (if enabled)
5. User can pause, resume, or stop recording
6. Recording saved to configured location with filename pattern

### Settings Flow
1. User opens settings window
2. Settings loaded from config file
3. User modifies settings
4. Settings validated and saved to config file
5. Daemon and other components reload configuration

## Communication

### D-Bus
ApexShot uses D-Bus for:
- Daemon communication
- GNOME Shell extension communication
- Inter-process coordination

### D-Bus Interfaces
- `org.apexshot.TrackedWindow` - Window tracking
- GNOME extension D-Bus interface for recording masks and overlays

## Configuration

Configuration stored in `~/.config/apexshot/config.yml`:
- YAML format for easy editing
- Auto-saved on settings changes
- Loaded at application startup
- Includes migration logic for legacy configurations

## Build System

- Rust/Cargo for main application
- CMake for C++ capture overlay
- Cargo-deb for Debian packaging
- Standard Rust toolchain (fmt, clippy, tests)

## Testing

- Unit tests in `tests/` directory
- Integration tests for recording module
- Manual testing documented in CONTRIBUTING.md

## Platform Support

Currently tested on:
- GNOME Ubuntu (Wayland)

Future support planned for:
- Other Linux distributions
- X11
- Other desktop environments (KDE Plasma, XFCE, etc.)
