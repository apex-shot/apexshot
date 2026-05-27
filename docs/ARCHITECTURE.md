# ApexShot Architecture

## Overview

ApexShot is a Linux screen capture tool written in Rust, featuring screenshot capture, screen recording, OCR, annotation, and browser integration capabilities. The application is designed primarily for GNOME (Wayland) and integrates with a GNOME Shell extension for enhanced functionality. It also provides a C++ Qt5 capture overlay, a Chrome/Chromium web scroll extension, and a native messaging host.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ApexShot                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Daemon     │  │   GUI App    │  │   CLI App    │  │  Library     │  │
│  │  (daemon/)   │  │ (settings/)  │  │   (main.rs)  │  │   (lib.rs)   │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘  │
│         │                 │                 │                 │            │
│         └─────────────────┴─────────────────┴─────────────────┘            │
│                                   │                                        │
│                            ┌──────▼──────┐                                 │
│                            │   Config    │                                 │
│                            │ (config.rs) │                                 │
│                            └─────────────┘                                 │
│                                   │                                        │
│  ┌────────────────────────────────┼────────────────────────────────┐      │
│  │                                │                                │      │
│  ▼                                ▼                                ▼      │
│ ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│ │  Capture    │  │  Recording  │  │  Settings   │  │   Backend   │  │   Overlay   │
│ │ (capture/)  │  │(recording/) │  │(settings/)  │  │ (backend/)  │  │(overlay.rs) │
│ │  editor/    │  │ control_    │  │ about.rs    │  │  x11.rs     │  │             │
│ │  preview_   │  │ session.rs  │  │ shortcuts.rs│  │  wayland.rs │  │             │
│ │  overlay.rs │  │ stop_       │  │ windowing.rs│  │  screencopy │  │             │
│ └─────────────┘  │ overlay.rs  │  │ after_      │  │  portal_    │  └─────────────┘
│                  │ countdown_  │  │ capture.rs  │  │  permissions│         │
│                  │ overlay.rs    │  │ storage.rs  │  └─────────────┘         │
│                  └─────────────┘  └─────────────┘                          │
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │
│  │ Annotations │  │     OCR     │  │   GNOME     │  │   Hotkeys   │      │
│  │(annotations/)│  │   (ocr/)    │  │(gnome_*)   │  │ (hotkeys/)  │      │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘      │
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │
│  │     QR      │  │    Tray     │  │  Onboarding │  │    Utils    │      │
│  │   (qr/)     │  │  (tray/)    │  │(onboarding/)│  │  (utils/)   │      │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘      │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                 C++ Qt5 Capture Overlay                              │   │
│  │              (capture-overlay/ — CMake)                              │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                        │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │              GNOME Shell Extension                                   │   │
│  │         (gnome-extension/ — JavaScript/GJS)                          │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │         Chrome Web Scroll Extension                                  │   │
│  │    (web-scroll-extension/ — Native Messaging Host)                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. Daemon (`src/daemon/`)
The daemon is a single long-running background process that:
- Shows a system tray icon via `ksni` (StatusNotifierItem protocol)
- Listens for global hotkeys via GNOME Shell `GrabAccelerators` or portal `GlobalShortcuts`
- Handles capture and recording operations in-process (no subprocess spawn, no GTK cold-start delay)
- Provides D-Bus IPC at `org.apexshot.Daemon` for single-instance coordination
- Manages recording timer state and preview overlay lifecycle
- Auto-starts `ydotoold` for scroll capture on Wayland
- Emits `TrackedWindowOpened`/`TrackedWindowClosed` signals for GNOME extension window stacking

**Key D-Bus interfaces:**
- `org.apexshot.Daemon` — `Trigger(action)`, `SetHotkeySuppressed(bool)`, `SetTrayVisible(bool)`, `ImportWebScrollCapture(...)`
- `org.apexshot.TrackedWindow` — window tracking signals
- `org.apexshot.RecordingControl` — recording pause/resume/stop/restart commands

### 2. Main Application (`src/main.rs`)
Entry point handling CLI arguments, mode selection, and subprocess delegation:
- `daemon` — background tray + hotkey listener
- `capture {screen|area|window|crosshair}` — screenshot capture
- `record {screen|area|ui}` — screen recording
- `ocr <path>` — text extraction
- `edit <path>` — open annotation editor
- `video-editor [path]` — open video editor (with optional MP4 path)
- `settings` — open settings window
- `hotkeys {install|uninstall|reset}` — hotkey management
- `show-last-preview`, `open-file`, `open-from-clipboard`, `restore-recently-closed`, `toggle-overlays` — daemon-triggered actions
- `recording-control {pause-resume|stop-save|restart|discard}` — recording control
- `edit-internal`, `settings-internal`, `preview` — GTK-only subprocess commands

### 3. Configuration (`src/config.rs`)
Centralized YAML configuration management:
- App settings (export locations, shortcuts, sounds, tray visibility)
- Recording settings (format, fps, quality, overlays, audio, countdown)
- Screenshot settings (format, cursor, timer, after-capture actions)
- Annotation settings (colors, tools, defaults)
- Shortcut settings (global hotkeys with custom bindings)
- Advanced settings (filename patterns, OCR, clipboard, desktop icon hiding)
- Storage settings (export location browse, hide desktop icons while capturing)
- After-capture settings (quick access, clipboard copy, save, open annotate)
- Cloud settings (waitlist placeholder)

**Storage:** `~/.config/apexshot/config.yml`

### 4. Capture Module (`src/capture/`)
Screen capture functionality:
- `mod.rs` — image saving, format conversion (PNG/JPEG/WebP), cursor compositing, filename generation
- `editor.rs` + `editor/` — full GTK4 annotation editor with drawing tools
- `preview_overlay.rs` — post-capture preview overlay with quick-access actions

**Editor submodules (`capture/editor/`):**
- `window/` — GTK4 editor window, toolbars, canvas, event handling
- `types.rs` — `AnnotationAction`, `ArrowStyle`, `DrawColor`, `ObfuscateMethod`, `Tool`, `Point`, `Rect`, `EditorError`
- `state.rs` — `EditorState`, undo/redo stack, action history
- `render.rs` — Cairo rendering for all annotation types (arrows, shapes, text, blur, pixelate, highlighter)
- `selection.rs` — selection hit-testing, resize handles, drag logic
- `color.rs` — color palette, hex/RGBA conversions
- `pen_weight.rs` — brush stroke weight definitions
- `numbering_style.rs` — numbered callout styles and sizing
- `text_detect.rs` — ML-based text detection using `ocrs`/`rten` for highlighter tool
- `preprocess.rs` — image preprocessing for OCR and detection
- `io_ops.rs` — clipboard URI operations
- `ui_support.rs` — shared GTK4 UI helpers, CSS, icon handling

**Supported annotation tools:** Pen, Line, Arrow (Standard/Fancy/Curved/Double), Rectangle, Circle, Highlighter, Text, Obfuscate (Pixelate/Blur/Blackout), Number callouts, Crop, Focus.

### 5. PipeWire Engine (`src/pipewire_engine.rs`)
OBS-style native `libpipewire` client that replaces GStreamer's `pipewiresrc`:
- `PipeWireCapture` — wraps `pw_thread_loop` → `pw_context` → `pw_core` → `pw_stream`
- Format negotiation via SPA pods (BGRx/BGRA/RGBx/RGBA, DMA-BUF first, SHM fallback)
- Frame extraction: DMA-BUF mmap or SHM memcpy → RGBA conversion, cursor compositing
- Cursor metadata via `SPA_META_Cursor` (raw spa_buffer FFI)
- Frame queue: `Arc<Mutex<VecDeque>>`, consumed by recording loop or screenshot path
- Color space reporting (BT.601/BT.709/RGB, full/limited range)

**Key exports:**
- `PipeWireCapture::connect(fd, node_id, max_frames, width_hint, height_hint)`
- `PipeWireCapture::wait_for_frame(timeout)` / `try_recv_frame()`
- `capture_single_frame(fd, node_id, timeout)` — convenience for screenshots

### 6. Recording Module (`src/recording/`)
Screen recording with native PipeWire + ffmpeg:
- `mod.rs` — Native PipeWire capture + ffmpeg pipe recording loop, X11 GStreamer fallback, codec selection, GIF encoding, portal session management
- `control_session.rs` — `RecordingControlCommand` (Pause/Resume/Restart/StopSave/StopDiscard), active session tracking via static `OnceLock`
- `stop_overlay.rs` — GTK4 floating control bar during recording (pause, stop, timer, position)
- `countdown_overlay.rs` — fullscreen 3-2-1 countdown with Escape cancellation
- `dim_overlay.rs` — fullscreen dim mask during countdown
- `editor/` — GTK4 video editor for trimming, dimension conversion, quality, and audio mode (MP4 only)
- `dnd.rs` — Do Not Disturb mode during recording

**Features:** MP4/WebM/OGV/GIF output, mic + speaker audio, webcam overlay, recording mask, pause/resume/restart, countdown timer, post-recording video editor.

### 7. X11/Wayland Area Selector (`src/overlay.rs`)
GTK4 overlay for interactive area selection:
- Full-screen transparent window with mouse drag selection
- Built with GTK4 + `gtk4-layer-shell`
- Used on X11; on Wayland, portal/`ashpd` handles area selection
- Supports `select_area_from_capture` and `select_area_from_image` for re-cropping

### 8. Settings Module (`src/settings/`)
GTK4-based settings window with custom chromeless styling and edge-drag resize:
- `mod.rs` — main settings window builder, single-instance check, tab navigation
- `general.rs` — general app settings (sounds, tray, startup, preview timeout)
- `screenshots.rs` — screenshot format, cursor, timer, naming
- `recording.rs` — recording format, fps, quality, overlays
- `annotate.rs` — annotation defaults, colors, tool preferences
- `quick_access.rs` — quick-access overlay configuration
- `advanced.rs` — filename patterns, OCR settings, clipboard options
- `shortcuts.rs` — global hotkey recording and binding editor
- `after_capture.rs` — per-action after-capture behavior matrix (screenshot vs recording)
- `storage.rs` — export location, hide desktop icons while capturing
- `cloud.rs` — cloud sync waitlist placeholder
- `about.rs` — app logo (Cairo-drawn procedural), version, update check, links
- `actions.rs` — `SaveInputs` struct, settings save/load logic, validation
- `ui_support.rs` — shared CSS styling, custom traffic-light buttons, form helpers
- `windowing.rs` — edge-drag resize, window drag, dark/light theme detection (`prefers_dark_glass_theme`), reduced-transparency support

### 9. Annotation Persistence (`src/annotations/`)
Non-destructive annotation storage:
- `mod.rs` — public API for save/load
- `schema.rs` — `AnnotationFile` schema with versioning, canvas size, action list
- `storage.rs` — SHA256-based image hashing, filesystem layout, original image preservation

**Storage locations:**
- Annotations: `~/.local/share/apexshot/annotations/`
- Originals: `~/.local/share/apexshot/originals/`

### 10. OCR Module (`src/ocr/`)
Text recognition using dual engines:
- Tesseract for traditional OCR with multi-language support
- QR code detection via `rqrr` (attempted first)
- `ocrs`/`rten` engine used in `capture/editor/text_detect.rs` for ML-based text detection
- Clipboard auto-copy option

### 11. GNOME Integration (`src/gnome_integration/`, `src/gnome_shell.rs`)
D-Bus communication with GNOME Shell extension:
- `gnome_shell.rs` — `org.apexshot.ShellOverlay` D-Bus proxy (ShowMask, HideMask, recording pause/resume/restart/end)
- `gnome_integration/` — extension installation, validation, metadata parsing

### 12. Hotkeys (`src/hotkeys/`)
Global hotkey management:
- GNOME: gsettings custom keybindings pointing at `apexshot daemon` subcommands
- Non-GNOME: portal `GlobalShortcuts` via `ashpd`
- Desktop entry generation (`ensure_desktop_entry_pub`)
- Config path: `~/.cache/apexshot/hotkey-daemon.log` (when not in terminal)

### 13. Tray (`src/tray/`)
System tray via `ksni` (StatusNotifierItem):
- Idle and recording states with elapsed timer
- Menu: Capture (screen/area/window), Record, Video Editor, Show Last Preview, Open Last Capture, Settings, Quit
- Procedural Cairo-drawn "A-Mark" icon at multiple resolutions

### 14. Onboarding (`src/onboarding/`)
First-time setup wizard:
- `welcome.rs` — welcome screen
- `extensions.rs` — GNOME extension and Chrome extension installation
- `cloud.rs` — cloud sync waitlist
- `complete.rs` — completion screen
- `mod.rs` — wizard flow controller, completion flag check

### 15. Utils (`src/utils/`)
Shared utilities:
- `clipboard.rs` — clipboard operations
- `desktop_env.rs` — desktop environment detection

### 16. Display Backend (`src/backend/`)
Abstraction over display servers:
- `mod.rs` — `DisplayBackend` trait, `CaptureData`, `PixelFormat`, `CursorData`
- `x11.rs` — X11 backend via `x11rb` + MIT-SHM
- `wayland.rs` — Wayland backend with a tiered capture strategy:
  - **GNOME:** XDG Screenshot portal via `APEXSHOT_WAYLAND_SCREENSHOT_PORTAL` env var (fast one-shot image)
  - **Hyprland/Sway:** `wlr-screencopy` native Wayland protocol
  - **Fallback:** XDG ScreenCast portal + PipeWire for cross-compositor compatibility
- `screencopy.rs` — `wlr-screencopy` protocol implementation for Hyprland/Sway
- `portal_permissions.rs` — persistent XDG portal permission setup

**Supported pixel formats:** RGB24, RGB32, RGBA32, BGR24, BGR32, BGRA32.

### 17. QR Code Detection (`src/qr/`)
Fast QR code decoding:
- `rqrr` with raw-byte API to avoid `image` crate version conflicts
- Integrated into OCR pipeline as primary detection path

### 18. C++ Overlay Launcher (`src/capture_overlay.rs`)
Rust wrapper for the C++ Qt5 overlay binary:
- `run_capture_overlay()` — spawns overlay process
- `capture_area_via_cpp()`, `capture_crosshair_via_cpp()`, `capture_screen_via_cpp()` — capture modes
- `open_recording_ui_via_cpp()` — recording UI overlay
- Binary location resolved via `option_env!("APEXSHOT_CAPTURE_BIN_DIR")` set by `build.rs`
- `CaptureOverlayGuard` / `LaunchBlockedReason` — concurrency control

### 19. Library Exports (`src/lib.rs`)
Public API surface for integration tests and downstream use:
- Re-exports from `backend`, `capture`, `config`, `ocr`, `overlay`, `recording`, `settings`, `onboarding`

## External Components

### C++ Qt5 Capture Overlay (`capture-overlay/`)
Native C++ overlay built with CMake and Qt5:
- Region selection with visual feedback
- Drawing tools (pen, shapes, text)
- Window picker overlay
- Crosshair capture mode
- Screen and area capture modes
- **Build:** triggered automatically by `build.rs` during Cargo build

**Key C++ files:**
- `src/main.cpp` — entry point
- `src/CaptureOverlay.cpp/h` — main overlay window
- `src/CaptureOverlay_Drawing.cpp` — drawing event handling
- `src/CaptureOverlay_Events.cpp` — mouse/keyboard events
- `src/CaptureOverlay_HitTest.cpp` — hit testing
- `src/WindowPickerOverlay.cpp/h` — window selection
- `src/ScreenCapture.cpp/h` — screen grab logic
- `src/request.cpp/h` — IPC request format

### GNOME Shell Extension (`gnome-extension/`)
JavaScript/GJS extension for GNOME Shell 45–49:
- `extension.js` — main extension logic, D-Bus service setup
- `controls-ui.js` — recording controls UI shell elements
- `controls-ui-layout.js` — layout positioning
- `runtime-overlays.js` — webcam/mic/speaker runtime overlay rendering
- `runtime-overlays-visibility.js` — overlay show/hide logic
- `mask-ui.js` — recording mask shell actor
- `session-state.js` — session tracking, window list
- `window-list.js` — window enumeration
- `screenshot-lock.js` — screenshot inhibition

**D-Bus services exposed:**
- `org.apexshot.TrackedWindow` — window stacking signals
- `org.apexshot.ShellOverlay` — mask and recording control

### Chrome Web Scroll Extension (`web-scroll-extension/`)
Browser extension for full-page webpage capture:
- `manifest.json` — extension manifest
- `background.js` — scroll-stitch capture logic, native messaging
- `popup.html/js` — extension popup UI
- Communicates with ApexShot daemon via native messaging host (`native-host/`)
- Captures are imported into the normal preview/editor flow

### Native Messaging Host (`native-host/`)
- `io.github.codegoddy.apexshot.json` — Chrome/Chromium native messaging manifest
- `apexshot-native-host` — symlink/script pointing to ApexShot binary
- Installed to `/etc/opt/chrome/NativeMessagingHosts/` and `/etc/chromium/NativeMessagingHosts/` by deb package

## Data Flow

### Screenshot Flow
1. User triggers capture via hotkey, tray, or CLI (`apexshot capture area`)
2. Daemon (or CLI) delegates to `DisplayBackend` (`WaylandBackend` or `X11Backend`)
3. For area capture: C++ Qt5 overlay (`capture-overlay`) or GTK4 overlay (`overlay.rs`) handles region selection
4. Raw pixel data returned as `CaptureData` with `PixelFormat`
5. `capture::save_capture()` converts to target format (PNG/JPEG/WebP) and composites cursor if enabled
6. File saved to configured export location with timestamped filename
7. After-capture actions executed per settings:
   - Show quick-access preview overlay (`preview_overlay.rs`)
   - Copy file URI to clipboard
   - Open in annotation editor (`capture/editor/`)
   - Save to disk

### Recording Flow
ApexShot supports two distinct recording paths: the **native Rust path** used on
non-GNOME compositors (Hyprland, Sway, KDE, X11) and the **Qt overlay + GNOME
extension path** used on GNOME Wayland.

#### Native Rust recording path (non-GNOME): daemon → portal → native PipeWire + ffmpeg

1. User triggers recording via hotkey, tray, or CLI (`apexshot record area`,
   `apexshot record screen`).
2. The daemon processes the action: for area recording, it launches the Rust GTK4
   layer-shell overlay (`src/overlay.rs`) for interactive area selection. For
   fullscreen recording, it skips this step and captures the full monitor bounds.
3. A `RecordingConfig` is built from user settings (format, fps, audio sources,
   webcam, countdown, cursor, etc.).
4. `recording::start_recording(config)` starts the recording. On wlroots
   compositors (Hyprland/Sway), `wf-recorder` is used when available for
   native `wlr-screencopy` capture with lower overhead.
5. On all other Wayland compositors, the flow is:
   - **Video source:** XDG ScreenCast portal → `get_wayland_source()` returns a
     `WaylandSource` with PipeWire fd + node_id (no GStreamer pipeline string).
   - **Frame capture:** `PipeWireCapture` (`src/pipewire_engine.rs`) opens a
     native libpipewire stream via `pw_stream_connect()`. Frames arrive in the
     process callback (DMA-BUF or SHM), are converted to RGBA, and placed in a
     `VecDeque` behind `Arc<Mutex<>>` for consumption on the application thread.
   - **Encoding:** Raw RGBA frames are written to ffmpeg's stdin (pipe), which
     handles encoding (VP9/libvpx-vp9, H.264/libx264, etc.) and muxing.
   - **Audio:** mic/speaker captured via ffmpeg's PulseAudio input (`-f pulse`).
     Multiple sources are mixed with ffmpeg's `amix` filter.
   - **Area cropping:** CPU-side crop applied to each frame before writing to
     ffmpeg (crop margins computed from portal stream position/size vs selection).
6. On X11, the GStreamer `ximagesrc` pipeline is used as fallback
   (`record_x11_with_gstreamer()`).
7. If countdown is enabled, `countdown_overlay.rs` shows a fullscreen 3-2-1
   countdown (with Escape to cancel). A `dim_overlay.rs` dim mask covers the
   screen during the countdown.
8. Recording starts; `control_session.rs` registers the active session on D-Bus
   at `/org/apexshot/RecordingControl`, enabling pause/resume/restart/stop
   commands from tray, hotkeys, or other processes.
9. The `stop_overlay.rs` GTK4 floating control bar appears with pause, stop, and
   elapsed-timer controls.
10. User stops recording via the overlay, hotkey, or tray; ffmpeg is signaled
    (stdin closed), finalizes, and the file is written.
11. After-capture actions are applied (copy to clipboard, show preview, open
    editor, upload, etc.) based on user settings.

#### Qt overlay + GNOME extension path (GNOME Wayland)

1. User triggers recording via hotkey, tray, or CLI.
2. The C++ Qt5 overlay (`capture-overlay/`) handles area selection and recording
   configuration UI (format picker, mic/speaker toggles, webcam PiP options).
3. The overlay outputs a JSON recording request; `RecordingConfig` is built from it.
4. `recording::start_recording()` uses the same native PipeWire + ffmpeg flow
   as the daemon path (no GStreamer for video).
5. The GNOME Shell extension is contacted via D-Bus to:
   - Show a recording mask (`org.apexshot.ShellOverlay.ShowMask`) that dims the
     screen outside the selected area.
   - Render recording controls (pause/stop/timer) directly on the GNOME Shell
     stage instead of a floating GTK4 overlay.
   - Display webcam PiP and audio level indicators as shell actors.
6. Countdown and recording proceed as in the native path, but mask/controls are
   shell-rendered instead of GTK4 overlays.
7. User stops; ffmpeg finalizes; after-capture actions applied.

### Web Scroll Capture Flow
1. User clicks browser extension button on a webpage
2. `background.js` scrolls page, stitches screenshots into single PNG
3. Image encoded as base64 and sent via native messaging to ApexShot daemon
4. Daemon receives `ImportWebScrollCapture` D-Bus call
5. Image decoded and saved to temp file
6. Normal preview/editor flow opened for the stitched image

### Annotation Editor Flow
1. Image opened via `open_image_editor()` (CLI, preview, or settings)
2. `capture/editor/window/` creates GTK4 chromeless window with custom toolbar
3. `EditorState` loads existing annotations from `annotations/` by SHA256 hash
4. User draws with tools; actions pushed to undo/redo stack
5. On save: rendered image exported + annotations serialized to `annotations/`
6. Original un-annotated image preserved in `originals/` for re-editing

### Settings Flow
1. User opens settings (`apexshot settings`)
2. `settings::show_settings_window()` spawns GTK4 subprocess (avoids tokio conflict)
3. `AppConfig` loaded from YAML, sanitized
4. UI built per tab; `SaveInputs` collects all widget references
5. On save: inputs validated, config written, daemon notified via D-Bus if running

## Communication

### D-Bus (Session Bus)
ApexShot uses D-Bus extensively for IPC:

**Daemon service:**
- Bus name: `org.apexshot.Daemon`
- Object path: `/org/apexshot/Daemon`
- Interface: `org.apexshot.Daemon`
- Methods: `Trigger(action: String)`, `SetHotkeySuppressed(suppressed: Bool)`, `SetTrayVisible(visible: Bool)`, `ImportWebScrollCapture(png_base64: String, page_url: String, page_title: String)`

**GNOME Extension services:**
- `org.apexshot.TrackedWindow` — signals for always-on-top window stacking
- `org.apexshot.ShellOverlay` — methods for recording mask and controls visibility

**Recording Control:**
- Object path: `/org/apexshot/RecordingControl`
- Commands: Pause, Resume, Restart, StopSave, StopDiscard

### Native Messaging
Chrome/Chromium extension communicates with ApexShot daemon via native messaging host. Messages are JSON-encoded and exchanged over stdin/stdout of the host process.

### GTK4 Channels
- Daemon uses `std::sync::mpsc` to send GTK work to the main OS thread
- Tray actions sent through channel to daemon main loop
- Recording stop actions use `tokio::sync::oneshot`

## Configuration

Configuration stored in `~/.config/apexshot/config.yml`:
- YAML format for easy manual editing
- Auto-saved on every settings change
- Loaded at application startup; sanitized via `AppConfig::sanitized()`
- Includes migration logic for legacy config keys
- Hotkey config stored separately and synced with GNOME gsettings

## Build System

- **Rust/Cargo** — main application (edition 2021)
- **CMake** — C++ capture overlay (`build.rs` triggers CMake automatically)
- **`build.rs`** — compiles C++ overlay, bundles Relm4 icons via `relm4-icons-build`
- **Cargo-deb** — Debian packaging (`cargo deb`)
- **`cargo fmt` + `cargo clippy`** — standard Rust toolchain linting

### Build-time Environment Variables
- `APEXSHOT_CAPTURE_BIN_DIR` — set by `build.rs`, consumed by `capture_overlay.rs` via `option_env!`
- `APEXSHOT_HOTKEY_DEBUG` — enable hotkey debug logging
- `APEXSHOT_HOTKEY_LOG` — redirect daemon logs to file
- `APEXSHOT_APP_ID` — override default portal app ID
- `APEXSHOT_REDUCED_TRANSPARENCY` — disable transparent effects

## Testing

- **Unit tests** — inline `#[cfg(test)]` in source modules (`backend/mod.rs`, `capture/editor.rs`, `settings/after_capture.rs`, etc.)
- **Integration tests** — `tests/` directory:
  - `desktop_identity.rs` — desktop environment detection
  - `package_metadata.rs` — deb package metadata validation
  - `wayland_backend_test.rs` — Wayland backend integration
  - `x11_backend_test.rs` — X11 backend integration
  - `window_picker_ui_contract.rs` — UI contract tests
  - `wayland_backend_mock_test.rs` — mock backend tests
- **Manual testing** — documented in `CONTRIBUTING.md`
- **Test crates:** `pretty_assertions`, `test-case`, `mockall`

## Platform Support

**Fully tested:**
- Ubuntu GNOME Wayland
- Arch Linux GNOME Wayland
- GNOME Shell versions 45–50

**Implemented but not thoroughly tested:**
- X11 (backend code in `src/backend/x11.rs`)
- KDE Plasma, Niri, and other non-wlroots Wayland compositors through the
  XDG ScreenCast portal + PipeWire path (implemented, not yet tested)
- Fedora/RHEL, openSUSE, NixOS, Alpine, Gentoo, and Void distro-family metadata

**Priority manual validation targets:**
- Fedora GNOME Wayland
- Fedora KDE Plasma Wayland
- openSUSE Tumbleweed or Leap KDE Plasma Wayland
- Arch Hyprland or Sway Wayland
- NixOS GNOME or KDE Wayland
