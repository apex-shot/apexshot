# Module Documentation

This document provides detailed information about each module in the ApexShot codebase.

## Core Modules

### Config Module (`src/config.rs`)

**Purpose:** Centralized configuration management

**Key Functions:**
- `config_path()` - Returns the path to the config file
- `load_config()` - Loads and parses the config file
- `save_config()` - Saves the config to disk
- `sanitized()` - Validates and sanitizes config values

**Config Fields:**
- `preview_auto_close_seconds` - Auto-close timeout for preview
- `start_at_login` - Whether to start at login
- `play_sounds` - Whether to play shutter sounds
- `export_location` - Default export location
- `screenshot_export_location` - Screenshot-specific export location
- `video_export_location` - Video-specific export location
- `rec_filename_pattern` - Recording filename pattern with {Date} and {Time} placeholders
- Recording settings (format, fps, quality, overlays, etc.)
- Screenshot settings (format, cursor, timer, etc.)
- Annotation settings (drawing, colors, etc.)
- Shortcut settings (global hotkeys)
- Advanced settings (filename patterns, OCR, clipboard, etc.)

**File Format:** YAML stored at `~/.config/apexshot/config.yml`

---

### Capture Module (`src/capture/`)

**Purpose:** Screen capture functionality

**Submodules:**
- `editor/` - Image annotation editor
- `preview_overlay.rs` - Preview overlay for captured images
- `mod.rs` - Main capture logic

**Key Functions:**
- `capture_screen()` - Full screen capture
- `capture_area()` - Area selection capture
- `capture_window()` - Window capture
- `generate_filename()` - Generate filename based on config pattern

**Integration:**
- Uses `capture-overlay` C++ module for region selection
- Integrates with GNOME extension for Wayland support
- Supports both X11 and Wayland

---

### Recording Module (`src/recording/`)

**Purpose:** Screen recording with GStreamer

**Submodules:**
- `mod.rs` - Main recording logic
- `runtime_keystrokes.rs` - Keystroke display during recording
- `dnd/` - Do Not Disturb mode during recording

**Key Functions:**
- `run_recording()` - Run recording with GStreamer
- `run_recording_with_controls()` - Recording with controls UI
- `prepare_overlay_recording_request()` - Prepare recording with GNOME overlay
- `record_gif_rust_with_commands()` - GIF recording

**Recording Config:**
- `output_path` - Output file path
- `width`, `height` - Recording dimensions
- `x`, `y` - Recording position
- `cursor` - Whether to show cursor
- `fps` - Frames per second
- `max_resolution` - Maximum resolution
- `mono_audio` - Mono audio
- `mic_enabled`, `speaker_enabled` - Audio sources
- `gif_quality`, `gif_optimize`, `gif_max_width` - GIF settings

**Integration:**
- Uses GStreamer for video encoding
- Integrates with GNOME extension for recording mask
- Supports runtime overlays (clicks, keystrokes, webcam)

---

### Overlay Module (`src/overlay.rs`)

**Purpose:** Native overlay window for UI elements

**Key Functions:**
- `OverlayWindow` - Main overlay window struct
- `show()` - Display the overlay
- `hide()` - Hide the overlay
- `set_position()` - Set overlay position
- `update_content()` - Update overlay content

**Use Cases:**
- Region selection during capture
- Recording controls UI (pause, resume, stop)
- Runtime overlay (clicks, keystrokes, time display)
- Quick access overlay

**Technology:** GTK4

---

### Settings Module (`src/settings/`)

**Purpose:** GTK4-based settings UI

**Submodules:**
- `mod.rs` - Main settings window
- `general.rs` - General settings tab
- `screenshots.rs` - Screenshot settings tab
- `recording.rs` - Recording settings tabs (General, Video, GIF, Overlay)
- `annotate.rs` - Annotation settings tab
- `quick_access.rs` - Quick access settings tab
- `advanced.rs` - Advanced settings tab
- `actions.rs` - Settings save/load actions
- `ui_support.rs` - UI support functions and styles

**Key Components:**
- `build_settings_window()` - Build main settings window
- `build_*_section()` - Build individual setting sections
- `save_settings()` - Save settings to config
- `SaveInputs` struct - Collects all settings inputs

---

### Annotations Module (`src/annotations/`)

**Purpose:** Image annotation editor

**Submodules:**
- `editor/` - Annotation editor UI
- `mod.rs` - Main annotation logic
- `schema.rs` - Annotation serialization schema

**Key Functions:**
- `AnnotationEditor` - Main editor struct
- `load_image()` - Load image for editing
- `save_image()` - Save edited image
- `add_annotation()` - Add annotation object
- `serialize_annotations()` - Serialize to JSON
- `deserialize_annotations()` - Deserialize from JSON

**Annotation Types:**
- Pen (freehand drawing)
- Arrow (directional arrows)
- Text (text labels)
- Number (numbered points)
- Blur (blur regions)
- Crop (image cropping)

---

### OCR Module (`src/ocr/`)

**Purpose:** Text recognition using Tesseract

**Key Functions:**
- `extract_text()` - Extract text from image
- `set_language()` - Set OCR language
- `set_line_breaks()` - Configure line break preservation

**Supported Languages:**
- English, Spanish, French, German, Italian, Portuguese, Chinese (Simplified), Japanese, Russian

**Configuration:**
- Language selection
- Line break preservation

---

### GNOME Integration (`src/gnome_integration/`, `src/gnome_shell.rs`)

**Purpose:** D-Bus communication with GNOME Shell extension

**Key Functions:**
- `current_session_supports_gnome_shell_overlay()` - Check GNOME support
- `emit_tracked_window_opened()` - Emit window open signal
- `emit_tracked_window_closed()` - Emit window close signal
- `shell_controls_visibility_policy()` - Get controls visibility policy

**D-Bus Interfaces:**
- `org.apexshot.TrackedWindow` - Window tracking
- Recording mask communication
- Runtime overlay communication

**Extension UUID:** `apexshot-gnome-integration@apexshot.github.io`

---

### Hotkeys Module (`src/hotkeys/`)

**Purpose:** Global hotkey management

**Key Functions:**
- `register_hotkey()` - Register a global hotkey
- `unregister_hotkey()` - Unregister a hotkey
- `handle_hotkey()` - Handle hotkey press

**Integration:**
- Works with daemon for background hotkey handling
- Platform-specific hotkey registration

---

### Tray Module (`src/tray/`)

**Purpose:** System tray icon and menu

**Key Functions:**
- `create_tray_icon()` - Create system tray icon
- `create_tray_menu()` - Create tray menu
- `update_tray_icon()` - Update tray icon state

**Menu Actions:**
- Capture screen/area/window
- Record screen
- Open settings
- Quit

---

### Onboarding Module (`src/onboarding/`)

**Purpose:** First-time setup wizard

**Submodules:**
- `mod.rs` - Main onboarding flow
- `extensions.rs` - Extension installation (GNOME, Chrome)
- `cloud.rs` - Cloud sync setup (future)

**Steps:**
1. Welcome
2. GNOME Extension installation
3. Chrome Extension setup
4. Cloud Sync (future)
5. Complete

**Key Functions:**
- `build_onboarding_window()` - Build onboarding window
- `build_gnome()` - GNOME extension step
- `build_chrome()` - Chrome extension step
- `is_gnome()` - Check if running on GNOME
- `install_gnome_extension()` - Install GNOME extension

---

### Utils Module (`src/utils/`)

**Purpose:** Utility functions

**Key Functions:**
- Various helper functions for common operations
- File operations
- System information

---

## External Modules

### C++ Qt5 Capture Overlay (`capture-overlay/`)

**Purpose:** Native C++ overlay for region selection

**Key Features:**
- Region selection with visual feedback
- Drawing tools
- Cross-platform window management
- Communication with Rust main app

**Build System:** CMake

---

### GNOME Shell Extension (`gnome-extension/`)

**Purpose:** GNOME Shell integration

**Key Files:**
- `extension.js` - Main extension logic
- `metadata.json` - Extension metadata
- `keystroke-display.js` - Keystroke overlay

**Features:**
- Always-on-top preview windows
- Recording mask support
- Runtime overlay (clicks, keystrokes)
- D-Bus communication

**UUID:** `apexshot-gnome-integration@apexshot.github.io`

**Supported GNOME Versions:** 45-49

---

## Data Structures

### RecordingConfig
Configuration for a recording session
- `output_path: PathBuf`
- `width, height: Option<u32>`
- `x, y: Option<i32>`
- `cursor: bool`
- `fps: u32`
- Various recording-specific options

### AppConfig
Main application configuration (see Config Module)

### AnnotationFile
Serializable annotation data
- `version: String`
- `image_path: String`
- `image_hash: String`
- `canvas_size: CanvasSize`
- `annotations: Vec<SerializableAnnotation>`
- `created_at, modified_at: DateTime<Utc>`

---

## Communication Patterns

### D-Bus Communication
- Session bus for inter-process communication
- Signals for window tracking
- Method calls for extension control

### GTK4 Signals
- UI event handling
- Settings change notifications
- User interaction callbacks

### Rust Channels
- Inter-thread communication
- Async task coordination
- Event propagation

---

## Error Handling

Most modules use `anyhow::Result<T>` for error handling:
- `RecordError` for recording-specific errors
- `CaptureError` for capture-specific errors
- Generic `anyhow::Error` for other errors

---

## Testing

- Unit tests in `tests/` directory
- Module-specific tests in respective modules
- Integration tests for recording module
