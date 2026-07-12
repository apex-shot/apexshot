# Module Documentation

This document provides detailed information about every module and submodule in the ApexShot codebase.

## Core Modules

### Config Module (`src/config.rs`)

**Purpose:** Centralized YAML configuration management for all application settings.

**Key Functions:**
- `config_path()` - Returns the path to `~/.config/apexshot/config.yml`
- `load_config()` - Loads, parses, and migrates the config file
- `save_config()` - Saves config to disk atomically
- `sanitized()` - Validates and sanitizes config values, applying defaults where needed

**Key Types:**
- `AppConfig` — Root configuration struct containing all settings categories
- `DEFAULT_PREVIEW_AUTO_CLOSE_SECONDS`, `MIN_PREVIEW_AUTO_CLOSE_SECONDS`, `MAX_PREVIEW_AUTO_CLOSE_SECONDS` — Preview timeout constants

**Config Categories:**
- General: `play_sounds`, `start_at_login`, `show_menu_bar_icon`, `preview_auto_close_seconds`
- Storage: `export_location`, `screenshot_export_location`, `video_export_location`, `hide_desktop_icons_while_capturing`
- Screenshots: format, cursor, timer, after-capture actions (`after_capture_save`, `after_capture_copy_file_to_clipboard`, `after_capture_show_quick_access`, `after_capture_open_annotate`)
- Recording: format, fps, quality, overlays, audio (mic/speaker), countdown
- Annotation: default colors, tool preferences (`annotate_inverse_arrow`, `annotate_smooth_drawing`, `annotate_draw_shadow`, `annotate_auto_expand`, `annotate_show_color_names`)
- Shortcuts: global hotkey bindings per action
- Advanced: filename patterns, OCR language, clipboard behavior
- Cloud: `cloud_destination` (`apexshot` | `xbackbone`), ApexShot auth tokens, XBackBone URL/token, upload preferences

**File Format:** YAML stored at `~/.config/apexshot/config.yml`

---

### Capture Module (`src/capture/`)

**Purpose:** Screen capture image saving, format conversion, cursor compositing, and post-capture UI.

**Submodules:**
- `mod.rs` — Image saving, format conversion, cursor compositing
- `editor.rs` + `editor/` — Full GTK4 annotation editor
- `preview_overlay.rs` — Post-capture preview overlay with quick-access actions

**Key Types (`mod.rs`):**
- `ImageFormat` — `Png`, `Jpeg { quality: u8 }`, `WebP`
- `SaveConfig` — Output directory, format, cursor inclusion, filename prefix, timestamp format
- `SaveError` — `InvalidPixelFormat`, `FilenameError`, `IoError`, `ImageError`
- `SaveResult<T>` — Type alias for `Result<T, SaveError>`

**Key Functions (`mod.rs`):**
- `save_capture()` — Converts `CaptureData` to target format and saves to disk
- `quick_save()` — Quick save helper for daemon hot path
- `copy_capture_uri_to_clipboard()` — Copies `file://` URI to clipboard

**Public Re-exports:**
- `editor::types::{AnnotationAction, ArrowStyle, DrawColor, FontSettings, ObfuscateMethod, Point, Rect}`
- `editor::{open_image_editor, EditorError}`
- `preview_overlay::{show_capture_preview_overlay, CapturePreviewError}`

#### Capture Editor (`capture/editor/`)

**Purpose:** Full GTK4 image annotation editor with non-destructive drawing.

**Submodules:**
- `window/` — Editor window, toolbars, canvas, event handling, background panel
- `types.rs` — Core types: `Tool`, `AnnotationAction`, `ArrowStyle`, `ObfuscateMethod`, `DrawColor`, `Point`, `Rect`, `BackgroundStyle`, `BackgroundAlignment`, `EditorError`
- `state.rs` — `EditorState`, undo/redo stacks, action list, zoom/pan transform
- `render.rs` — Cairo rendering for all annotation types, checkerboard background, surface utilities
- `selection.rs` — Hit-testing, resize handles, drag constraints, selection outlines
- `color.rs` — Color palette definitions, hex/RGBA conversions, named colors
- `pen_weight.rs` — `PenWeight` enum and display names
- `numbering_style.rs` — `NumberingStyle`, `NumberSize`, callout rendering metrics
- `text_detect.rs` — ML-based text detection using `ocrs`/`rten` engine for highlighter tool
- `preprocess.rs` — Image preprocessing for OCR and text detection
- `io_ops.rs` — Clipboard URI copy operations
- `ui_support.rs` — Shared GTK4 helpers, CSS loading, icon name constants, toolbar builders

**Key Functions:**
- `open_image_editor(image_path)` — Opens the annotation editor for a given image path
- `copy_file_uri_to_clipboard(path)` — Copies file URI to system clipboard

**Supported Tools:**
- Select (0), Crop (x), Pen (P), Box/Rectangle (r), Circle (o), Line (l), Arrow (a), Highlighter (h), Obfuscate (c/b), Number (n), Text (t), Focus (f)

#### Preview Overlay (`capture/preview_overlay.rs`)

**Purpose:** Post-capture floating preview with quick-access action buttons.

**Key Types:**
- `CapturePreviewError` — Error type for preview overlay failures

**Key Functions:**
- `show_capture_preview_overlay(path)` — Displays the preview window for a capture

---

### Recording Module (`src/recording/`)

**Purpose:** Screen recording with native PipeWire frame capture, ffmpeg
encoding/muxing, codec auto-detection, audio mixing, and runtime overlays. This
module is the authoritative recorder on supported distros (Ubuntu, Arch, etc.),
whether the user interacts through the Qt overlay (GNOME) or the daemon/CLI
(non-GNOME).

> **Fedora:** Video recording is **not supported**. All recording entry points
> call `refuse_fedora_recording()` and show a desktop notification. Screenshots
> and the rest of the app remain available. See
> [`progress-fedora-kde-overlay-and-preview.md`](progress-fedora-kde-overlay-and-preview.md).

**Architecture overview for non-GNOME users:**

- **Area selection** uses the Rust GTK4 layer-shell overlay (`src/overlay.rs`)
  invoked by the daemon or CLI.
- **Recording start** happens directly from the daemon (`src/daemon/mod.rs`)
  which calls `prepare_overlay_recording_request` and `start_recording`.
- **Recording backend** is chosen automatically: `wf-recorder` on wlroots
  compositors (Hyprland/Sway) for native `wlr-screencopy` capture;
  native PipeWire (`src/pipewire_engine.rs`) + ffmpeg on other Wayland
  compositors; GStreamer `ximagesrc` fallback on X11.
- **During recording:** a GTK4 `stop_overlay.rs` floating bar shows pause/stop
  controls and elapsed time. No shell extension or Qt process is involved.
- **After recording:** the GTK4 video editor, preview overlay, and after-capture
  actions all work identically to the GNOME path.

**Submodules:**
- `mod.rs` — Native PipeWire capture + ffmpeg pipe recording loop, X11 GStreamer
  fallback, codec selection, GIF encoding, portal session management
- `editor/` — GTK4 video editor for trimming, conversion, and export
- `control_session.rs` — Active recording session tracking and D-Bus control commands
- `stop_overlay.rs` — GTK4 floating control bar (pause, stop, timer) during recording
- `countdown_overlay.rs` — Fullscreen 3-2-1 countdown with Escape cancellation
- `dim_overlay.rs` — Fullscreen dim mask shown during countdown
- `dnd.rs` — Do Not Disturb inhibition during recording

**Key Types (`mod.rs`):**
- `RecordError` — `InitError`, `GStreamerError`, `PortalError`, `IoError`, `UnsupportedBackend`, `Cancelled`, `NoEncoderFound`, `GifError`
- `RecordResult<T>` — `Result<T, RecordError>`
- `RecordingConfig` — `output_path`, `width`/`height`, `x`/`y`, `cursor`, `fps`, audio sources, overlay options

**Key Functions (`mod.rs`):**
- `start_recording(config)` — Main recording entry point; selects backend (wf-recorder / native PipeWire + ffmpeg / GStreamer X11) and runs recording loop
- `record_wayland_with_ffmpeg_sync()` — Wayland recording: native PipeWire capture → RGBA frames → ffmpeg stdin pipe
- `record_gif_wayland_native()` — Wayland GIF recording: native PipeWire + ffmpeg palettegen/paletteuse
- `record_x11_with_gstreamer()` — X11 fallback using GStreamer ximagesrc pipeline
- `run_recording_with_controls(params, stop_tx)` — Recording with floating stop overlay
- `run_recording_countdown_bar()` — Shows countdown then recording controls
- `run_overlay_recording_request(request)` — Handles C++ overlay recording request

**Key Types (`control_session.rs`):**
- `RecordingControlCommand` — `Pause`, `Resume`, `Restart`, `StopSave`, `StopDiscard`
- `RECORDING_CONTROL_OBJECT_PATH` — `/org/apexshot/RecordingControl`

**Key Functions (`control_session.rs`):**
- `has_active_recording_control()` — Returns whether a recording session is active
- `send_active_recording_command(cmd)` — Sends a command to the active recording session
- `toggle_active_recording_pause()` — Toggles pause/resume on active recording

**Key Types (`stop_overlay.rs`):**
- `StopAction` — `Save`, `Discard`
- `RecordingControlsParams` — `capture_x`/`y`/`w`/`h`, `is_fullscreen`, `show_timer`, `use_shell_mask`
- `StopOverlayError` — `InitError`

**Key Functions (`stop_overlay.rs`):**
- `run_recording_controls(params, stop_tx)` — Shows floating recording control bar
- `run_recording_stop_overlay(...)` — Standalone stop overlay

**Key Functions (`countdown_overlay.rs`):**
- `run_countdown_overlay(seconds)` — Blocks until countdown completes or Escape pressed

**Key Functions (`dim_overlay.rs`):**
- `run_dim_overlay(close_flag)` — Shows fullscreen dim; closes when `AtomicBool` is set

**Key Types (`runtime_keystrokes.rs`):**
*(Removed — click/keystroke runtime overlays have been removed.)*

**Key Functions (`runtime_keystrokes.rs`):**
*(Removed.)*

#### Recording Editor (`recording/editor/`)

**Purpose:** GTK4-based video editor for trimming, dimension conversion, quality adjustment, and audio mode changes. Only supports MP4 files in this version.

**Submodules:**
- `mod.rs` — Module root with `open_recording_editor(path)` and `open_empty_recording_editor()` entry points
- `model.rs` — Core types: `VideoEditState`, `VideoMetadata`, `AudioMode`, `DimensionPreset`
- `ui_support.rs` — GTK4 CSS loading, custom styling classes for editor and empty workspace
- `ffmpeg.rs` — FFmpeg subprocess wrapper for probe, thumbnail, and transcode operations
- `dialogs.rs` — Error dialog and confirm dialog helpers
- `window/mod.rs` — Main GTK4 editor window with timeline, panels, preview, drag-and-drop, export

**Key Types (`model.rs`):**
- `VideoEditState` — Mutable editing state (trim in/out, dimensions, quality, audio mode)
- `VideoMetadata` — Read-only video metadata (path, duration, width, height, codec, fps, bitrate)
- `AudioMode` — `Unchanged`, `Mono`, `Muted`
- `DimensionPreset` — Named presets like `Original`, `Square1x1`, `Vertical9x16`, etc.

**Key Functions (`window/mod.rs`):**
- `open(metadata)` — Opens the editor window with a loaded video
- `open_empty()` — Opens the editor window with empty workspace and drop zone

**Capabilities:**
- Timeline scrub with thumbnail strip
- Trim in/out point selection
- Dimension preset or custom resolution
- Quality slider (0–100)
- Audio mode (unchanged, mono, muted)
- Export via FFmpeg with progress tracking
- Drag-and-drop video file loading
- Re-encoding is skipped when only trimming
- Empty workspace with drop zone and file chooser dialog
- Accessible from tray menu, CLI (`apexshot video-editor`), and global hotkey

---

### PipeWire Engine (`src/pipewire_engine.rs`)

**Purpose:** Native `libpipewire` client providing OBS-style screen capture.
Replaces GStreamer `pipewiresrc` with direct PipeWire API for both single-frame
screenshots and continuous video recording.

**Key Types:**
- `PipeWireCapture` — Full lifecycle wrapper: `ThreadLoopRc` → `ContextRc` → `CoreRc` → `StreamRc`
- `PipeWireFrame` — RGBA pixel data, dimensions, stride, cursor overlay, color space
- `CursorOverlay` — Cursor bitmap and position from `SPA_META_Cursor` metadata
- `NegotiatedFormat` — Format negotiated with compositor (size, framerate, color space)
- `ColorSpace` — SPA video color range (full/limited) and matrix (RGB/BT.601/BT.709)
- `PipeWireError` — Error enum covering init, connect, stream, timeout, format negotiation

**Key Functions:**
- `PipeWireCapture::connect(fd, node_id, max_frames, width_hint, height_hint)` — Open PipeWire stream via portal fd
- `PipeWireCapture::wait_for_frame(timeout)` — Blocking dequeue with timeout
- `PipeWireCapture::try_recv_frame()` — Non-blocking frame dequeue
- `capture_single_frame(fd, node_id, timeout)` — Convenience: connect, grab one frame, disconnect

**Architecture:**
```
Portal (ashpd) → pipewire_fd, node_id
    ↓
PipeWireCapture::connect()
    ├── pw_thread_loop (dedicated thread for PipeWire I/O)
    ├── pw_context + pw_core (connect via fd)
    ├── pw_stream → format negotiation (BGRx/BGRA/RGBx/RGBA, DMA-BUF preferred, SHM fallback)
    ├── process callback → extract buffer (DMA-BUF mmap or SHM memcpy)
    └── Cursor metadata extraction (SPA_META_Cursor via raw spa_buffer)
```

**Format negotiation:** Advertises priority list of video formats (BGRx, BGRA, RGBx, RGBA)
with size range 1×1 to 8192×4320 and framerate 0/1 to 360/1. Accepts whatever the
compositor picks. Color space (BT.601/BT.709/RGB, full/limited range) is reported.

---

### Area Selector Overlay (`src/overlay.rs`)

**Purpose:** GTK4 fullscreen overlay for interactive area selection, used on
non-GNOME Wayland compositors (Hyprland, Sway, KDE) and X11. On GNOME Wayland,
the C++ Qt5 overlay (`capture-overlay/`) handles area selection instead.

**Capabilities:**
- Click-and-drag area selection with resize handles
- Recording panel with mic/speaker toggles, format picker, and
  countdown options
- Settings menu for video/GIF/control preferences
- Window picker mode for selecting application windows
- Fullscreen capture mode
- Crosshair pixel-zoom mode for precise point capture
- Built with GTK4 + `gtk4-layer-shell` for always-on-top behaviour

**Key Types:**
- `AreaSelector` — Main selector struct managing the GTK4 window
- `SelectionArea` — Normalized and validated selection coordinates
- `SelectionError` — Error type for selection failures
- `SelectionResult<T>` — `Result<T, SelectionError>`

**Key Functions:**
- `select_area()` — Shows fullscreen selector and returns `SelectionResult<SelectionArea>`
- `select_area_from_capture(capture)` — Re-crop from existing `CaptureData`
- `select_area_from_image(path)` — Re-crop from saved image file

**Technology:** GTK4 + `gtk4-layer-shell`

**Platform Note:** Used on X11. On Wayland, portal/`ashpd` handles area selection via the native dialog.

---

### Settings Module (`src/settings/`)

**Purpose:** GTK4-based chromeless settings window with custom styling, edge-drag resize, and tab navigation.

**Submodules:**
- `mod.rs` — Main settings window builder, single-instance detection, tab navigation, daemon spawn on open
- `general.rs` — General settings: sounds, tray icon, startup, preview auto-close
- `screenshots.rs` — Screenshot format, cursor, timer, filename prefix
- `recording.rs` — Recording format, fps, quality, overlays, audio
- `annotate.rs` — Default annotation colors, tool preferences, smooth drawing, shadows
- `quick_access.rs` — Quick-access overlay configuration
- `advanced.rs` — Filename patterns, OCR language, clipboard behavior
- `shortcuts.rs` — Global hotkey recording, binding editor, key normalization
- `after_capture.rs` — Per-action after-capture matrix (screenshot vs recording checkboxes)
- `storage.rs` — Export location entry with browse button, hide desktop icons toggle
- `cloud.rs` — ApexShot Cloud login/logout UI and XBackBone self-hosted destination setup
- `about.rs` — Procedural Cairo logo, version, update check links, legal footer
- `actions.rs` — `SaveInputs` struct, save logic, validation, config write
- `ui_support.rs` — Shared CSS, traffic-light buttons, form helpers, style classes
- `windowing.rs` — Edge-drag resize, window drag, dark/light theme detection, reduced-transparency support

**Key Functions:**
- `show_settings_window()` — Spawns settings as GTK4 subprocess (avoids tokio conflict)
- `build_settings_window(app)` — Constructs the full settings UI

**Key Types:**
- `SaveInputs` — Collects references to all settings widgets for save-time value extraction

**Window Constants:**
- `SETTINGS_WINDOW_MIN_WIDTH` = 920
- `SETTINGS_WINDOW_MIN_HEIGHT` = 760

---

### Annotation Persistence (`src/annotations/`)

**Purpose:** Non-destructive annotation storage by image SHA256 hash.

**Submodules:**
- `mod.rs` — Public API for save/load
- `schema.rs` — `AnnotationFile` schema, `SerializableAnnotation`, versioning
- `storage.rs` — Filesystem layout, hash-based paths, original image preservation

**Key Types (`schema.rs`):**
- `AnnotationFile` — `version`, `image_path`, `image_hash`, `canvas_size`, `annotations`, `created_at`/`modified_at`
- `SerializableAnnotation` — Serialized form of a single annotation action

**Key Functions (`storage.rs`):**
- `save_annotations(image_path, annotations)` — Saves annotations by image hash
- `load_annotations(image_path)` — Loads annotations by image hash
- `load_original_image(image_path)` — Returns un-annotated original image path
- `compute_image_hash(image_path)` — SHA256 hash used as storage key

**Storage Locations:**
- Annotations: `~/.local/share/apexshot/annotations/`
- Originals: `~/.local/share/apexshot/originals/`

---

### OCR Module (`src/ocr/`)

**Purpose:** Text recognition using Tesseract, with QR code fallback via `rqrr`.

**Key Types:**
- `OcrConfig` — `language`, `min_confidence`, `auto_copy_to_clipboard`
- `OcrError` — Error enum for OCR failures
- `OcrOutput` — Extracted text with source indication
- `ContentSource` — `QrCode`, `Tesseract`
- `OcrResult<T>` — `Result<T, OcrError>`

**Key Functions:**
- `extract_text(image_bytes, config)` — Extract text from raw image bytes
- `extract_text_from_path(path, config)` — Extract text from image file path
- `copy_to_clipboard(text)` — Copy text to system clipboard

**Supported Languages:**
English, Spanish, French, German, Italian, Portuguese, Chinese (Simplified), Japanese, Russian.

**Behavior:**
1. QR code detection attempted first (`rqrr`)
2. Falls back to Tesseract OCR
3. ML-based text detection (`ocrs`/`rten`) is used in `capture/editor/text_detect.rs` for the highlighter tool

---

### GNOME Integration (`src/gnome_shell.rs`, `src/gnome_integration/`)

**Purpose:** D-Bus communication with the GNOME Shell extension for window stacking, recording masks, and runtime overlays.

**Key Functions (`gnome_shell.rs`):**
- `emit_tracked_window_opened(title)` / `emit_tracked_window_closed(title)` — Window tracking signals on `org.apexshot.TrackedWindow`
- `show_recording_mask(x, y, w, h)` / `hide_recording_mask()` — Recording mask on `org.apexshot.ShellOverlay`
- `set_recording_paused(session_id, paused)` — Notify extension of pause state
- `restart_recording_ui(session_id)` — Notify extension of recording restart
- `end_recording_ui(session_id)` — Notify extension of recording end
- `hide_recording_controls_best_effort()` / `hide_recording_mask_best_effort()` — Best-effort cleanup

**Key Functions (`gnome_integration/`):**
- Extension installation, validation, and metadata parsing helpers

**D-Bus Services:**
- `org.apexshot.TrackedWindow` — Window stacking signals (emitted by daemon)
- `org.apexshot.ShellOverlay` — Mask and recording control (methods called by ApexShot, implemented by extension)

**Extension UUID:** `apexshot-gnome-integration@apexshot.github.io`

---

### Hotkeys Module (`src/hotkeys/`)

**Purpose:** Global hotkey management with GNOME gsettings integration and portal fallback.

**Key Functions:**
- `setup_hotkeys_for_current_desktop()` — Installs hotkeys for the current desktop environment
- `uninstall_hotkeys_for_current_desktop()` — Removes all hotkey bindings
- `reset_hotkey_config()` — Resets hotkey configuration to defaults
- `sync_gnome_hotkeys_for_current_desktop()` — Syncs GNOME custom keybindings to current executable path
- `ensure_desktop_entry_pub()` — Ensures a `.desktop` entry exists for the current binary
- `load_hotkey_config()` — Loads hotkey configuration from disk
- `accel_to_gnome(accel)` — Converts accelerator string to GNOME gsettings format

**GNOME Implementation:**
- Uses gsettings `org.gnome.settings-daemon.plugins.media-keys` custom keybindings
- Each binding spawns `apexshot daemon` subcommand (e.g., `apexshot capture area`)
- Desktop entry Exec line is `apexshot daemon` so GNOME launches actions with the expected app identity

**Non-GNOME Fallback:**
- Portal `GlobalShortcuts` via `ashpd` when GNOME is not detected

---

### Tray Module (`src/tray/`)

**Purpose:** System tray icon via `ksni` (StatusNotifierItem / AppIndicator protocol).

**Key Types:**
- `TrayAction` — Enum of actions triggerable from tray menu:
  `CaptureArea`, `CaptureCrosshair`, `CaptureScreen`, `CaptureWindow`, `OpenRecordingUi`, `OpenVideoEditor`, `RecordScreen`, `StopRecordingSave`, `ShowLastPreview`, `OpenLastCapture`, `OpenSettings`, `Quit`
- `ApexShotTray` — ksni tray icon state struct
- `TrayPresentation` — `Idle` or `Recording { elapsed_text }`

**Key Functions:**
- `spawn_tray(tx)` — Spawns the tray icon on its own thread
- `ApexShotTray::show_recording_timer(text)` — Updates tray to show recording elapsed time
- `ApexShotTray::show_idle()` — Resets tray to idle state

**Icon:**
- Procedurally drawn "A-Mark" logo using Cairo geometric primitives at multiple resolutions

---

### Onboarding Module (`src/onboarding/`)

**Purpose:** First-time setup wizard guiding users through extension installation.

**Submodules:**
- `mod.rs` — Wizard flow controller, `is_onboarding_complete()`, `show_onboarding_window()`
- `welcome.rs` — Welcome screen with app introduction
- `extensions.rs` — GNOME extension and Chrome extension installation steps
- `cloud.rs` — Cloud upload intro (ApexShot Cloud + XBackBone; configure in Settings)
- `complete.rs` — Completion screen with "Get Started" button

**Key Functions:**
- `is_onboarding_complete()` — Checks whether onboarding has been completed
- `show_onboarding_window()` — Opens the onboarding wizard

**Wizard Steps:**
1. Welcome (`welcome.rs`)
2. Extensions — GNOME Shell + Chrome (`extensions.rs`)
3. Cloud Upload — ApexShot Cloud + XBackBone overview (`cloud.rs`)
4. Complete (`complete.rs`)

---

### Cloud Module (`src/cloud/`)

**Purpose:** Upload captures and recordings to remote destinations.

**Submodules:**
- `mod.rs` — Module root
- `upload.rs` — Public `upload_file()` entry point used by preview/editor flows
- `destination.rs` — `Destination::{ApexShot, XBackbone}` routing from config
- `apexshot.rs` — ApexShot Cloud REST upload client
- `auth.rs` — OAuth 2.0 device authorization (`apexshot login` / `logout`)
- `xbackbone.rs` — Self-hosted XBackBone client (API token, test connection, upload)

**Config fields (in `AppConfig`):**
- `cloud_destination` — `"apexshot"` or `"xbackbone"`
- `cloud_backend_url`, `cloud_api_token`, `cloud_refresh_token`, `cloud_install_id`, `cloud_user_email`
- `xbackbone_url`, `xbackbone_api_token`

**Env overrides (see `.env.example`):**
- `APEXSHOT_CLOUD_BACKEND_URL`
- `APEXSHOT_XBACKBONE_URL`, `APEXSHOT_XBACKBONE_TOKEN`

---

### Utils Module (`src/utils/`)

**Purpose:** Shared utility helpers.

**Submodules:**
- `clipboard.rs` — Clipboard operations
- `desktop_env.rs` — Desktop environment detection (GNOME, KDE, etc.)
- `notify.rs` — Desktop notification helpers

---

### Display Backend (`src/backend/`)

**Purpose:** Abstraction over X11 and Wayland display servers with a unified `DisplayBackend` trait.

**Submodules:**
- `mod.rs` — `DisplayBackend` trait, `CaptureData`, `PixelFormat`, `CursorData`, `DisplayError`
- `x11.rs` — `X11Backend` via `x11rb` + MIT-SHM
- `wayland.rs` — `WaylandBackend` with tiered capture (`wlr-screencopy`, KDE ScreenShot2, ScreenCast + PipeWire, or Screenshot portal)
- `kde_screenshot.rs` — KWin `org.kde.KWin.ScreenShot2` client for KDE Plasma Wayland
- `screencopy.rs` — `wlr-screencopy` native Wayland protocol implementation for Hyprland/Sway
- `portal_permissions.rs` — Persistent XDG portal permission setup (`ensure_portal_permissions()`)

**Key Types:**
- `DisplayBackend` — Core trait: `new()`, `capture_screen()`, `capture_area(x,y,w,h)`, `capture_window(id)`, `is_supported()`
- `CaptureData` — `pixels`, `width`, `height`, `stride`, `format`, `cursor: Option<CursorData>`
- `PixelFormat` — `bits_per_pixel`, `bytes_per_pixel`, `red_mask`, `green_mask`, `blue_mask`
  - Constants: `RGB24`, `RGB32`, `RGBA32`, `BGR24`, `BGR32`, `BGRA32`
- `CursorData` — `pixels`, `width`, `height`, `x`, `y`, `xhot`, `yhot`
- `DisplayError` — `UnsupportedBackend`, `InitializationError`, `CaptureError`, `InvalidArea`, `PortalError`, `IoError`

**Platform Note:** `WaylandBackend` uses a tiered capture strategy:
- **GNOME Wayland** — C++ overlay plus XDG Screenshot portal for still screenshots.
- **KDE Plasma Wayland** — `org.kde.KWin.ScreenShot2` via `kde_screenshot.rs` when available.
- **Hyprland / Sway** — `wlr-screencopy` native Wayland protocol.
- **Other compositors** — XDG ScreenCast portal + PipeWire as fallback (implemented; broader DE coverage still needs manual validation)

Recording uses the XDG ScreenCast portal + native PipeWire (`src/pipewire_engine.rs`)
+ ffmpeg on most compositors. On wlroots compositors (Hyprland/Sway),
`wf-recorder` is preferred when installed for native `wlr-screencopy` capture
with lower overhead. Ubuntu GNOME Wayland, Arch GNOME Wayland, Hyprland, and
the Ubuntu/Arch packaging paths are confirmed. Sway/wlroots-like compositors
should follow the same native path but still need more manual coverage. KDE
Plasma, Fedora/RHEL, openSUSE, Niri, and NixOS remain development-stage manual
validation targets. `X11Backend` exists but is not thoroughly tested.

---

### Compositor Helpers (`src/compositor/`)

**Purpose:** Compositor-specific helpers for window geometry, workspaces, and capture affordances.

**Submodules:**
- `mod.rs` — Shared compositor detection/dispatch
- `hyprland.rs`, `sway.rs`, `niri.rs`, `river.rs`, `cosmic.rs` — Per-compositor integrations

---

### Distro Module (`src/distro/`)

**Purpose:** Distribution detection and distro-family helpers used by packaging/support paths.

**Submodules:**
- `mod.rs` — `DistroInfo::detect()` from `/etc/os-release`, helpers like `is_arch()`, `is_debian()`, `is_fedora()`
- `arch/mod.rs` — Arch-specific integration hooks

---

### QR Code Detection (`src/qr/`)

**Purpose:** Fast QR code decoding with raw-byte API to avoid `image` crate version conflicts.

**Key Functions:**
- `detect_and_decode(image_bytes)` — Detect and decode QR codes from raw image bytes
- `detect_and_decode_from_gray(width, height, gray_bytes)` — Decode from raw grayscale data

**Technology:** `rqrr` with raw-byte API.

**Integration:** Called first in the OCR pipeline before falling back to Tesseract.

---

### C++ Overlay Launcher (`src/capture_overlay.rs`)

**Purpose:** Rust wrapper that builds and invokes the C++ Qt5 overlay binary.

**Key Types:**
- `RecordingRequest` — Request to start recording overlay with area selection
- `RecordingType` — Enum for recording variants
- `AreaCaptureResult` / `AreaCapturePathResult` — Result types from C++ overlay
- `CaptureOverlayGuard` — RAII guard for overlay process lifecycle
- `LaunchBlockedReason` — Reason why overlay launch was blocked

**Key Functions:**
- `run_capture_overlay()` — Spawns the C++ overlay process
- `capture_area_via_cpp()` / `capture_crosshair_via_cpp()` / `capture_screen_via_cpp()` — Capture modes delegating to C++
- `capture_area_file_via_cpp()` / `capture_crosshair_file_via_cpp()` / `capture_screen_file_via_cpp()` / `capture_window_file_via_cpp()` — File-returning variants
- `open_recording_ui_via_cpp()` — Opens recording UI in C++ overlay
- `begin_capture_session()` — Starts a capture session
- `is_launch_blocked_error()` — Checks if overlay is already running
- `request_existing_overlay_focus()` — Requests focus on existing overlay window

**Build Integration:**
- CMake compilation triggered automatically by `build.rs`
- Binary directory embedded at compile time via `option_env!("APEXSHOT_CAPTURE_BIN_DIR")`

---

### Daemon Module (`src/daemon/`)

**Purpose:** Single long-running background process providing tray, hotkeys,
D-Bus IPC, and in-process capture/recording. The daemon is the central
orchestrator for non-GNOME users — it handles every capture and recording action
without requiring the Qt overlay or GNOME Shell extension.

**How the daemon handles recording (non-GNOME):**

1. Receives a `RecordScreen` or `RecordArea` action via hotkey, tray click, D-Bus
   `Trigger()`, or CLI relay.
2. For area recording: spawns the GTK4 overlay (`src/overlay.rs`) via
   `select_area_for_recording()`. The user draws a selection rectangle and
   configures recording options directly inside the GTK overlay panel.
3. Builds a `RecordingConfig` from the overlay result and user settings.
4. Calls `recording::start_recording(config)` which selects the backend
   (`wf-recorder` on wlroots when available, native PipeWire + ffmpeg on other
   Wayland compositors, GStreamer `ximagesrc` on X11) and blocks until complete.
5. During recording: the daemon tracks elapsed time for tray display and relays
   pause/resume/stop commands from hotkeys to the active `control_session`.
6. On completion: fires after-capture actions (preview, clipboard, editor, etc.).

**Key Types:**
- `DaemonAction` — Enum of all actions the daemon can execute:
  `CaptureArea`, `CaptureCrosshair`, `CaptureScreen`, `CaptureWindow`,
  `OpenFile`, `OpenFromClipboard`, `RestoreRecentlyClosed`, `ToggleOverlays`,
  `RecordScreen`, `RecordArea`, `OpenRecordingUi`, `OpenVideoEditor`,
  `ToggleRecordingPause`, `StopRecordingSave`, `RestartRecording`, `DiscardRecording`,
  `ShowLastPreview`, `ShowPreviewForPath(PathBuf)`, `OpenLastCapture`, `OpenSettings`,
  `SetTrayVisible(bool)`, `RecordingSessionStarted`/`Paused`/`Resumed`/`Restarted`/`Ended`,
  `RecordingTimerTick`, `SetHotkeySuppressed(bool)`,
  `ImportWebScrollCapture { png_base64, page_url, page_title }`, `Quit`
- `GtkWork` — Enum of GTK operations that must run on the main OS thread
- `RecordingTrayState` — Tracks elapsed time, pause durations for tray timer

**Key Constants:**
- `DAEMON_BUS_NAME` = `"org.apexshot.Daemon"`
- `DAEMON_OBJECT_PATH` = `"/org/apexshot/Daemon"`
- `DAEMON_INTERFACE` = `"org.apexshot.Daemon"`

**Key Functions:**
- `run_daemon_with_gtk_channel(gtk_tx, ...)` / `run_daemon()` — Main daemon entry points
- `trigger_daemon_action(action)` — Async D-Bus RPC to running daemon
- `set_daemon_hotkey_suppressed(suppressed)` — Suppresses/unsuppresses hotkey handling
- `set_daemon_tray_visibility(visible)` — Shows/hides tray icon live
- `notify_daemon_recording_started()` / `paused()` / `resumed()` / `restarted()` / `ended()` — Recording state notifications
- `show_preview_via_daemon(path)` — Shows preview for specific path via daemon
- `import_web_scroll_capture(png_base64, page_url, page_title)` — Imports browser scroll capture
- `start_daemon_subprocess()` — Spawns daemon as background subprocess

---

## External Modules

### C++ Qt5 Capture Overlay (`capture-overlay/`)

**Purpose:** Native C++ Qt5 overlay for region selection, drawing, window picking, and screen capture.

**Build System:** CMake (triggered automatically by `build.rs`)

**Key C++ Files:**
- `src/main.cpp` — Entry point, request parsing
- `src/CaptureOverlay.cpp` / `CaptureOverlay.h` — Main overlay window, drawing canvas
- `src/CaptureOverlay_Drawing.cpp` — Drawing event handling (mouse/pen)
- `src/CaptureOverlay_Events.cpp` — Keyboard and mouse event filters
- `src/CaptureOverlay_HitTest.cpp` — Hit testing for resize/move handles
- `src/WindowPickerOverlay.cpp` / `WindowPickerOverlay.h` — Window enumeration and selection overlay
- `src/ScreenCapture.cpp` / `ScreenCapture.h` — Screen grab logic (X11/Wayland)
- `src/request.cpp` / `request.h` — JSON IPC request/response format with Rust main app

---

### GNOME Shell Extension (`gnome-extension/`)

**Purpose:** JavaScript/GJS extension for GNOME Shell 45–50 providing always-on-top stacking, recording masks, and shell-side recording controls.

**Key Files:**
- `extension.js` — Main extension logic, D-Bus service registration, cleanup
- `controls-ui.js` — Recording controls UI shell elements (pause/stop buttons)
- `controls-ui-layout.js` — Positioning logic for controls UI
- `runtime-overlays.js` — Runtime overlay ownership and shell actor cleanup
- `runtime-overlays-visibility.js` — Show/hide logic for runtime overlays
- `mask-ui.js` — Recording mask shell actor (dimmed region around capture area)
- `session-state.js` — Session tracking, window list management
- `window-list.js` — Window enumeration for window capture
- `screenshot-lock.js` — Screenshot inhibition during recording
- `metadata.json` — Extension metadata (UUID, GNOME versions, name)

**D-Bus Services Exposed:**
- `org.apexshot.TrackedWindow` — Listens to `TrackedWindowOpened`/`TrackedWindowClosed` signals
- `org.apexshot.ShellOverlay` — Implements `ShowMask`, `HideMask`, recording control methods

**UUID:** `apexshot-gnome-integration@apexshot.github.io`

**Supported GNOME Versions:** 45–50 (see `gnome-extension/metadata.json`)

---

### Chrome Web Scroll Extension (`web-scroll-extension/`)

**Purpose:** Browser extension for full-page webpage capture via scroll-and-stitch.

**Key Files:**
- `manifest.json` — Extension manifest (v3)
- `background.js` — Scroll-stitch capture logic, native messaging host communication
- `popup.html` / `popup.js` — Extension popup UI

**Flow:**
1. User clicks extension button on webpage
2. `background.js` scrolls page, stitches screenshots into single PNG
3. Encodes as base64 and sends via native messaging to ApexShot daemon
4. Daemon imports via `ImportWebScrollCapture` D-Bus action
5. Normal preview/editor flow opened

---

### Native Messaging Host (`native-host/`)

**Purpose:** Chrome/Chromium native messaging manifest linking the browser extension to ApexShot.

**Files:**
- `io.github.codegoddy.apexshot.json` — Native messaging host manifest
- `apexshot-native-host` — Symlink/script pointing to ApexShot binary

**Installation:**
- Installed to `/etc/opt/chrome/NativeMessagingHosts/` and `/etc/chromium/NativeMessagingHosts/` by the `.deb` package

---

## Data Structures

### RecordingConfig
Configuration for a recording session.
- `output_path: PathBuf`
- `width, height: Option<u32>`
- `x, y: Option<i32>`
- `cursor: bool`
- `fps: u32`
- `max_resolution: Option<u32>`
- `mono_audio: bool`
- `mic_enabled, speaker_enabled: bool`
- `gif_quality, gif_optimize, gif_max_width: u32`
- `countdown_seconds: u32`

### AppConfig
Main application configuration. See Config Module for full field listing.

### AnnotationFile
Serializable annotation data stored per-image by SHA256 hash.
- `version: String`
- `image_path: String`
- `image_hash: String`
- `canvas_size: CanvasSize`
- `annotations: Vec<SerializableAnnotation>`
- `created_at, modified_at: DateTime<Utc>`

### SaveConfig
Configuration for saving captures.
- `output_dir: Option<PathBuf>`
- `format: ImageFormat`
- `include_cursor: bool`
- `filename_prefix: Option<String>`
- `timestamp_format: Option<String>`

### AnnotateRuntimeConfig
Runtime configuration for the annotation editor.
- `inverse_arrow_direction: bool`
- `smooth_drawing: bool`
- `draw_object_shadow: bool`
- `auto_expand_canvas: bool`
- `show_color_names: bool`
- `always_on_top: bool`
- `show_dock_icon: bool`

---

## Communication Patterns

### D-Bus (Session Bus)
- `org.apexshot.Daemon` — Daemon trigger, hotkey suppression, tray visibility, web scroll import
- `org.apexshot.TrackedWindow` — Signals for always-on-top window stacking (GNOME extension listens)
- `org.apexshot.ShellOverlay` — Methods for recording mask and controls visibility (GNOME extension implements)
- `org.apexshot.RecordingControl` — Recording pause/resume/restart/stop commands

### Native Messaging
- Chrome/Chromium extension ↔ ApexShot daemon via JSON over stdin/stdout
- Host manifest: `io.github.codegoddy.apexshot.json`

### GTK4 Channels
- Daemon uses `std::sync::mpsc` to send `GtkWork` to the main OS thread
- Tray actions sent via `std::sync::mpsc` to daemon action loop
- Recording stop uses `tokio::sync::oneshot` for async completion

### Rust Threads
- `tokio` async runtime for D-Bus, portal, and network I/O
- `std::thread` for tray icon (ksni), hotkey sync, GStreamer, and blocking GTK operations

---

## Error Handling

Most modules use `anyhow::Result<T>` for general error propagation.

**Domain-specific error types:**
- `SaveError` (`capture/mod.rs`) — Pixel format, filename, IO, image encoding errors
- `RecordError` (`recording/mod.rs`) — GStreamer, portal, encoder, GIF errors
- `DisplayError` (`backend/mod.rs`) — Backend initialization, capture, portal errors
- `SelectionError` (`overlay.rs`) — Area selection failures
- `EditorError` (`capture/editor/types.rs`) — Missing file, image load/save errors
- `OcrError` (`ocr/`) — OCR engine failures
- `StopOverlayError` (`recording/stop_overlay.rs`) — GTK initialization failures

---

## Testing

**Unit tests:** Inline `#[cfg(test)]` in source modules.
- `backend/mod.rs` — Pixel format, capture data validation
- `capture/editor.rs` — Tool shortcut mapping, constrained drag, highlighter behavior
- `settings/after_capture.rs` — UI contract (no hardcoded widths)

**Integration tests:** `tests/` directory.
- `desktop_identity.rs` — Desktop environment detection
- `package_metadata.rs` — Deb package metadata validation
- `wayland_backend_test.rs` — Wayland backend integration
- `x11_backend_test.rs` — X11 backend integration
- `window_picker_ui_contract.rs` — UI contract tests
- `wayland_backend_mock_test.rs` — Mock backend tests
- `xbackbone_upload.rs` / `xbackbone_e2e.rs` — XBackBone upload coverage
- `test_dimensions.rs` / `test_layer.rs` — Additional UI/layout helpers

**Test crates:** `pretty_assertions`, `test-case`, `mockall`
