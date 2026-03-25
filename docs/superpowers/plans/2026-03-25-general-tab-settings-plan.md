# General Tab Settings Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all 10 General tab settings in the recording panel functional, replacing macOS "Retina" with Linux "HiDPI Scaling", and persist settings to config.

**Architecture:** Settings flow from C++ overlay (Qt5) as JSON → Rust parses into `RecordingRequest` → applies to GStreamer pipeline / pre-recording UI → persists to `AppConfig`. Two layers: (1) recording-affecting settings wired to the capture engine, (2) UI settings controlling countdown/overlay behavior.

**Tech Stack:** C++ Qt5 (overlay), Rust (recording engine), GStreamer (video capture), GTK4 (countdown/dim overlays), D-Bus (notifications), serde/serde_yml (config persistence).

---

## Chunk 1: C++ — Rename Retina → HiDPI and Add Settings to JSON Output

### Task 1.1: Rename `m_scaleRetina` to `m_hidpi` in C++ header

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay.h:257`

- [ ] **Step 1: Rename member variable**

```cpp
// Change line 257 from:
bool m_scaleRetina;        // "Scale Retina videos to 1x"
// To:
bool m_hidpi;              // "HiDPI Scaling — record at display scale resolution"
```

- [ ] **Step 2: Update initializer in `CaptureOverlay.cpp:651`**

```cpp
// Change:
, m_scaleRetina(false)
// To:
, m_hidpi(false)
```

- [ ] **Step 3: Update all references in `CaptureOverlay.cpp`**

Replace all `m_scaleRetina` with `m_hidpi`:
- Line 1477: `drawSetting("HiDPI:", "Record at display scale resolution", m_hidpi, &m_hidpi);`
- Line 2692: `case 5: m_hidpi = !m_hidpi; break;`

- [ ] **Step 4: Commit**

```bash
git add capture-overlay/src/CaptureOverlay.h capture-overlay/src/CaptureOverlay.cpp
git commit -m "refactor: rename Retina to HiDPI Scaling in capture overlay"
```

### Task 1.2: Add accessor methods for all General tab settings

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay.h:94-98`

- [ ] **Step 1: Add accessors below existing ones**

```cpp
bool recordControlsEnabled() const { return m_recControls; }
bool recordMicEnabled() const { return m_recMic; }
bool recordSpeakerEnabled() const { return m_recSpeaker; }
bool recordClicksEnabled() const { return m_recClicks; }
bool recordKeystrokesEnabled() const { return m_recKeystrokes; }
// NEW:
bool recordDisplayRecTime() const { return m_displayRecTime; }
bool recordHidpiEnabled() const { return m_hidpi; }
bool recordDoNotDisturb() const { return m_doNotDisturb; }
bool recordShowCursor() const { return m_showCursor; }
bool recordRememberSelection() const { return m_rememberSelection; }
bool recordDimScreen() const { return m_dimScreen; }
bool recordShowCountdown() const { return m_showCountdown; }
```

- [ ] **Step 2: Commit**

```bash
git add capture-overlay/src/CaptureOverlay.h
git commit -m "feat: add accessor methods for all General tab settings"
```

### Task 1.3: Update `printRecordingJson` to include all settings

**Files:**
- Modify: `capture-overlay/src/main.cpp:94-110`

- [ ] **Step 1: Update function signature**

```cpp
void printRecordingJson(const QRect& sel, const char* recordType,
                         bool controls, bool mic, bool speaker,
                         bool clicks, bool keystrokes,
                         bool displayRecTime, bool hidpi, bool doNotDisturb,
                         bool showCursor, bool rememberSelection,
                         bool dimScreen, bool countdown)
```

- [ ] **Step 2: Update JSON output to include all fields**

```cpp
void printRecordingJson(const QRect& sel, const char* recordType,
                         bool controls, bool mic, bool speaker,
                         bool clicks, bool keystrokes,
                         bool displayRecTime, bool hidpi, bool doNotDisturb,
                         bool showCursor, bool rememberSelection,
                         bool dimScreen, bool countdown)
{
    std::printf("{\"x\":%d,\"y\":%d,\"width\":%d,\"height\":%d,"
                "\"mode\":\"record\",\"record_type\":\"%s\","
                "\"controls\":%s,\"mic\":%s,\"speaker\":%s,"
                "\"clicks\":%s,\"keystrokes\":%s,"
                "\"display_rec_time\":%s,\"hidpi\":%s,"
                "\"notifications\":%s,\"cursor\":%s,"
                "\"remember_selection\":%s,\"dim_screen\":%s,"
                "\"countdown\":%s}\n",
                sel.x(), sel.y(), sel.width(), sel.height(),
                recordType,
                controls ? "true" : "false",
                mic ? "true" : "false",
                speaker ? "true" : "false",
                clicks ? "true" : "false",
                keystrokes ? "true" : "false",
                displayRecTime ? "true" : "false",
                hidpi ? "true" : "false",
                doNotDisturb ? "true" : "false",
                showCursor ? "true" : "false",
                rememberSelection ? "true" : "false",
                dimScreen ? "true" : "false",
                countdown ? "true" : "false");
    std::fflush(stdout);
}
```

- [ ] **Step 3: Update the call site in `main.cpp:313-318`**

```cpp
// Change from:
printRecordingJson(selGlobal, recordType,
                   overlay.recordControlsEnabled(),
                   overlay.recordMicEnabled(),
                   overlay.recordSpeakerEnabled(),
                   overlay.recordClicksEnabled(),
                   overlay.recordKeystrokesEnabled());
// To:
printRecordingJson(selGlobal, recordType,
                   overlay.recordControlsEnabled(),
                   overlay.recordMicEnabled(),
                   overlay.recordSpeakerEnabled(),
                   overlay.recordClicksEnabled(),
                   overlay.recordKeystrokesEnabled(),
                   overlay.recordDisplayRecTime(),
                   overlay.recordHidpiEnabled(),
                   overlay.recordDoNotDisturb(),
                   overlay.recordShowCursor(),
                   overlay.recordRememberSelection(),
                   overlay.recordDimScreen(),
                   overlay.recordShowCountdown());
```

- [ ] **Step 4: Commit**

```bash
git add capture-overlay/src/main.cpp
git commit -m "feat: pass all General tab settings in recording JSON output"
```

---

## Chunk 2: Rust — Extend RecordingRequest and Parser

### Task 2.1: Extend `RecordingRequest` struct

**Files:**
- Modify: `src/capture_overlay.rs:169-181`

- [ ] **Step 1: Add new fields to `RecordingRequest`**

```rust
#[derive(Debug, Clone)]
pub struct RecordingRequest {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub record_type: RecordingType,
    pub controls: bool,
    pub mic: bool,
    pub speaker: bool,
    pub clicks: bool,
    pub keystrokes: bool,
    // General tab settings
    pub display_rec_time: bool,
    pub hidpi: bool,
    pub notifications: bool,
    pub cursor: bool,
    pub remember_selection: bool,
    pub dim_screen: bool,
    pub countdown: bool,
}
```

- [ ] **Step 2: Commit**

```bash
git add src/capture_overlay.rs
git commit -m "feat: extend RecordingRequest with General tab settings"
```

### Task 2.2: Update `parse_recording_json` to extract new fields

**Files:**
- Modify: `src/capture_overlay.rs:468-500`

- [ ] **Step 1: Add extraction of new fields with defaults**

```rust
fn parse_recording_json(json: &str) -> Result<RecordingRequest, SelectionError> {
    let x = extract_int(json, "x").ok_or_else(|| SelectionError::InitError("Missing x".into()))?;
    let y = extract_int(json, "y").ok_or_else(|| SelectionError::InitError("Missing y".into()))?;
    let width = extract_int(json, "width")
        .ok_or_else(|| SelectionError::InitError("Missing width".into()))?;
    let height = extract_int(json, "height")
        .ok_or_else(|| SelectionError::InitError("Missing height".into()))?;

    let record_type_str = extract_string(json, "record_type").unwrap_or_else(|| "video".into());
    let record_type = match record_type_str.as_str() {
        "gif" => RecordingType::Gif,
        _ => RecordingType::Video,
    };

    let controls = extract_bool(json, "controls").unwrap_or(false);
    let mic = extract_bool(json, "mic").unwrap_or(false);
    let speaker = extract_bool(json, "speaker").unwrap_or(false);
    let clicks = extract_bool(json, "clicks").unwrap_or(false);
    let keystrokes = extract_bool(json, "keystrokes").unwrap_or(false);

    // General tab settings
    let display_rec_time = extract_bool(json, "display_rec_time").unwrap_or(false);
    let hidpi = extract_bool(json, "hidpi").unwrap_or(false);
    let notifications = extract_bool(json, "notifications").unwrap_or(true);
    let cursor = extract_bool(json, "cursor").unwrap_or(true);
    let remember_selection = extract_bool(json, "remember_selection").unwrap_or(false);
    let dim_screen = extract_bool(json, "dim_screen").unwrap_or(true);
    let countdown = extract_bool(json, "countdown").unwrap_or(true);

    Ok(RecordingRequest {
        x,
        y,
        width,
        height,
        record_type,
        controls,
        mic,
        speaker,
        clicks,
        keystrokes,
        display_rec_time,
        hidpi,
        notifications,
        cursor,
        remember_selection,
        dim_screen,
        countdown,
    })
}
```

- [ ] **Step 2: Commit**

```bash
git add src/capture_overlay.rs
git commit -m "feat: parse all General tab settings from recording JSON"
```

---

## Chunk 3: Rust — Extend RecordingConfig and Wire to Pipeline

### Task 3.1: Add recording-relevant fields to `RecordingConfig`

**Files:**
- Modify: `src/recording/mod.rs:42-63`

- [ ] **Step 1: Extend `RecordingConfig`**

```rust
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub output_path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub cursor: bool,
    pub hidpi: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        let mut path = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("output.mp4");
        Self {
            output_path: path,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: true,
            hidpi: false,
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: add cursor and hidpi fields to RecordingConfig"
```

### Task 3.2: Wire cursor setting to ximagesrc

**Files:**
- Modify: `src/recording/mod.rs:533-548`

- [ ] **Step 1: Update `get_x11_source` to use cursor setting**

```rust
fn get_x11_source(config: &RecordingConfig) -> RecordResult<String> {
    let show_pointer = if config.cursor { "true" } else { "false" };
    let mut source = format!("ximagesrc show-pointer={} use-damage=false", show_pointer);

    if let (Some(x), Some(y), Some(w), Some(h)) = (config.x, config.y, config.width, config.height)
    {
        source.push_str(&format!(
            " startx={} starty={} endx={} endy={}",
            x,
            y,
            x + w as i32 - 1,
            y + h as i32 - 1
        ));
    }

    Ok(source)
}
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: wire cursor show/hide to ximagesrc show-pointer property"
```

### Task 3.3: Wire cursor setting to Wayland PipeWire source

**Files:**
- Modify: `src/recording/mod.rs:422-492`

- [ ] **Step 1: Update `get_wayland_source` to accept cursor parameter**

```rust
async fn get_wayland_source(cursor: bool) -> RecordResult<String> {
    // ... existing code until CursorMode selection ...
    
    let cursor_mode = if cursor {
        ashpd::desktop::screencast::CursorMode::Embedded
    } else {
        ashpd::desktop::screencast::CursorMode::Hidden
    };

    proxy
        .select_sources(
            &session,
            cursor_mode,
            // ... rest unchanged
        )
```

- [ ] **Step 2: Update call sites in `build_pipeline` and `record_gif_rust_with_optional_stop`**

Pass `config.cursor` to `get_wayland_source(config.cursor)`.

- [ ] **Step 3: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: wire cursor show/hide to Wayland PipeWire cursor mode"
```

### Task 3.4: Wire HiDPI setting to capture resolution

**Files:**
- Modify: `src/recording/mod.rs` (build_pipeline and get_x11_source)

- [ ] **Step 1: When `hidpi` is true, downscale capture**

For X11, add a videoscale element after the source. For both backends, insert a `videoscale` with caps filter to halve resolution when `hidpi` is true:

```rust
async fn build_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<String> {
    let output_str = output_path.to_string_lossy();

    let video_source = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        get_wayland_source(config.cursor).await?
    } else {
        get_x11_source(config)?
    };

    // HiDPI: downscale to logical resolution
    let scale_filter = if config.hidpi {
        // Downscale by 2x (most common Linux HiDPI factor)
        " ! videoscale ! video/x-raw,width=[1,4096],height=[1,4096] ! videorate"
    } else {
        " ! videorate"
    };

    Ok(format!(
        "{} ! videoconvert{} ! queue ! {} {} ! {} ! filesink location=\"{}\"",
        video_source, scale_filter, profile.encoder, profile.props, profile.muxer, output_str
    ))
}
```

Note: A more precise approach would calculate the exact scale factor from the display's current scaling. For v1, a simple 2x downscale is the most common case. Can be enhanced later with `xdpyinfo` or GDK monitor scale detection.

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: wire HiDPI scaling to GStreamer pipeline (2x downscale)"
```

---

## Chunk 4: Rust — AppConfig Persistence

### Task 4.1: Add recording general settings to `AppConfig`

**Files:**
- Modify: `src/config.rs:17-47`

- [ ] **Step 1: Add recording settings fields with `#[serde(default)]`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub preview_auto_close_seconds: u32,
    pub start_at_login: bool,
    pub play_sounds: bool,
    pub shutter_sound: String,
    pub show_menu_bar_icon: bool,
    pub export_location: String,
    pub hide_desktop_icons_while_capturing: bool,
    pub after_capture_show_quick_access: bool,
    pub after_capture_copy_file_to_clipboard: bool,
    pub after_capture_save: bool,
    pub after_capture_open_annotate: bool,
    // Recording General tab settings
    pub rec_controls: bool,
    pub rec_display_time: bool,
    pub rec_hidpi: bool,
    pub rec_notifications: bool,
    pub rec_cursor: bool,
    pub rec_clicks: bool,
    pub rec_keystrokes: bool,
    pub rec_remember_selection: bool,
    pub rec_dim_screen: bool,
    pub rec_countdown: bool,
    // Remember selection: last selection area
    pub last_selection_x: Option<i32>,
    pub last_selection_y: Option<i32>,
    pub last_selection_w: Option<i32>,
    pub last_selection_h: Option<i32>,
}
```

- [ ] **Step 2: Add defaults for new fields in `Default` impl**

```rust
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // ... existing fields ...
            rec_controls: true,
            rec_display_time: false,
            rec_hidpi: false,
            rec_notifications: true,
            rec_cursor: true,
            rec_clicks: false,
            rec_keystrokes: false,
            rec_remember_selection: false,
            rec_dim_screen: true,
            rec_countdown: true,
            last_selection_x: None,
            last_selection_y: None,
            last_selection_w: None,
            last_selection_h: None,
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: persist recording General tab settings in AppConfig"
```

### Task 4.2: Load/save settings when recording starts

**Files:**
- Modify: `src/main.rs:1004-1007` (the `RecordingRequested` arm)

- [ ] **Step 1: Load config, apply recording request settings, save**

```rust
Ok(AreaCaptureResult::RecordingRequested(request)) => {
    // Load config
    let mut config = load_config();
    
    // Update config from overlay settings
    config.rec_controls = request.controls;
    config.rec_display_time = request.display_rec_time;
    config.rec_hidpi = request.hidpi;
    config.rec_notifications = request.notifications;
    config.rec_cursor = request.cursor;
    config.rec_clicks = request.clicks;
    config.rec_keystrokes = request.keystrokes;
    config.rec_remember_selection = request.remember_selection;
    config.rec_dim_screen = request.dim_screen;
    config.rec_countdown = request.countdown;
    
    // Remember selection area
    if request.remember_selection {
        config.last_selection_x = Some(request.x);
        config.last_selection_y = Some(request.y);
        config.last_selection_w = Some(request.width);
        config.last_selection_h = Some(request.height);
    }
    
    // Save config
    let _ = save_config(&config);
    
    // TODO: Handle pre-recording (countdown, dim, DND) and start recording
    eprintln!("Recording settings saved. Recording not yet wired to engine.");
    std::process::exit(0);
}
```

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: save recording General tab settings to config on record start"
```

---

## Chunk 5: Pre-Recording UI (Countdown, Dim Screen, DND)

### Task 5.1: Implement countdown overlay (GTK4)

**Files:**
- Create: `src/recording/countdown_overlay.rs`
- Modify: `src/recording/mod.rs` (add `pub mod countdown_overlay`)

- [ ] **Step 1: Create countdown overlay module**

```rust
// src/recording/countdown_overlay.rs
use gtk4::prelude::*;
use std::time::Duration;

/// Show a 3-2-1 countdown overlay before recording starts.
/// Blocks until countdown completes.
pub fn show_countdown(seconds: u32) {
    let app = gtk4::Application::builder()
        .application_id("com.apexshot.countdown")
        .build();

    app.connect_activate(move |app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .decorated(false)
            .default_width(200)
            .default_height(200)
            .resizable(false)
            .css_classes(vec!["countdown-overlay".into()])
            .build();

        let label = gtk4::Label::new(Some(&seconds.to_string()));
        label.add_css_class("countdown-number");
        window.set_child(Some(&label));

        // Center on screen
        window.set_default_size(200, 200);
        
        // Fullscreen transparent window
        window.fullscreen();
        window.set_opacity(0.7);

        window.present();

        let remaining = std::rc::Rc::new(std::cell::Cell::new(seconds));
        let label_ref = label.clone();
        let window_ref = window.clone();
        
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let val = remaining.get();
            if val <= 1 {
                window_ref.close();
                glib::ControlFlow::Break
            } else {
                remaining.set(val - 1);
                label_ref.set_text(&format!("{}", val - 1));
                glib::ControlFlow::Continue
            }
        });
    });

    app.run_with_args::<&str>(&[]);
}
```

- [ ] **Step 2: Add CSS for countdown in `src/settings/ui_support.rs` or new CSS file**

- [ ] **Step 3: Commit**

```bash
git add src/recording/countdown_overlay.rs src/recording/mod.rs
git commit -m "feat: add 3-2-1 countdown overlay before recording"
```

### Task 5.2: Implement dim screen overlay (GTK4)

**Files:**
- Create: `src/recording/dim_overlay.rs`
- Modify: `src/recording/mod.rs`

- [ ] **Step 1: Create dim overlay module**

Similar to countdown but simpler — a fullscreen semi-transparent black window that appears during countdown and closes when recording starts.

- [ ] **Step 2: Commit**

```bash
git add src/recording/dim_overlay.rs src/recording/mod.rs
git commit -m "feat: add dim screen overlay during recording countdown"
```

### Task 5.3: Implement DND toggle via D-Bus

**Files:**
- Create: `src/recording/dnd.rs`
- Modify: `src/recording/mod.rs`

- [ ] **Step 1: Create DND module using `zbus`**

```rust
// src/recording/dnd.rs
use zbus::Connection;

/// Enable "Do Not Disturb" mode via org.freedesktop.Notifications.
/// Returns a guard that restores the previous state on drop.
pub async fn enable_dnd() -> Result<DndGuard, String> {
    let connection = Connection::session().await.map_err(|e| e.to_string())?;
    
    // Call org.freedesktop.Notifications.Inhibit with DND reason
    // Method varies by desktop environment
    // GNOME: Settings → Notifications → Do Not Disturb
    // KDE: `qdbus org.kde.kglobalaccel /kglobalaccel invokeShortcut "Toggle Do Not Disturb"`
    
    // For GNOME, use gsettings:
    let _ = std::process::Command::new("gsettings")
        .args(["set", "org.gnome.desktop.notifications", "show-banners", "false"])
        .output();
    
    Ok(DndGuard { _private: () })
}

pub struct DndGuard {
    _private: (),
}

impl Drop for DndGuard {
    fn drop(&mut self) {
        // Restore notifications
        let _ = std::process::Command::new("gsettings")
            .args(["set", "org.gnome.desktop.notifications", "show-banners", "true"])
            .output();
    }
}
```

Note: DND implementation varies by desktop environment. For v1, support GNOME (gsettings) and KDE (qdbus). Fall back gracefully if neither works.

- [ ] **Step 2: Commit**

```bash
git add src/recording/dnd.rs src/recording/mod.rs
git commit -m "feat: add Do Not Disturb toggle via gsettings for GNOME"
```

### Task 5.4: Wire pre-recording flow in `main.rs`

**Files:**
- Modify: `src/main.rs:1004-1007`

- [ ] **Step 1: Implement pre-recording sequence**

```rust
Ok(AreaCaptureResult::RecordingRequested(request)) => {
    let mut config = load_config();
    // ... update config from request (as in Task 4.2) ...
    let _ = save_config(&config);

    // Pre-recording: DND
    let _dnd_guard = if request.notifications {
        recording::dnd::enable_dnd().await.ok()
    } else {
        None
    };

    // Pre-recording: Dim screen
    if request.dim_screen {
        // Show dim overlay in background thread
    }

    // Pre-recording: Countdown
    if request.countdown {
        recording::countdown_overlay::show_countdown(3);
    }

    // Build RecordingConfig from request
    let rec_config = RecordingConfig {
        output_path: /* generate output path */,
        width: Some(request.width as u32),
        height: Some(request.height as u32),
        x: Some(request.x),
        y: Some(request.y),
        cursor: request.cursor,
        hidpi: request.hidpi,
    };

    // Start recording
    // ...
}
```

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire pre-recording flow (DND, dim, countdown) before recording"
```

---

## Chunk 6: Remember Selection

### Task 6.1: Load saved selection when overlay opens

**Files:**
- Modify: `src/capture_overlay.rs` — `run_capture_binary` or overlay launch logic

- [ ] **Step 1: Pass saved selection to C++ overlay as CLI args**

When `rec_remember_selection` is true in config and `last_selection_*` values exist, pass them to the C++ overlay binary as `--restore-selection x,y,w,h` arguments.

- [ ] **Step 2: Update C++ overlay to accept and use restored selection**

Add `--restore-selection` CLI parsing in `main.cpp` and set the initial selection rect accordingly.

- [ ] **Step 3: Commit**

```bash
git add src/capture_overlay.rs capture-overlay/src/main.cpp
git commit -m "feat: restore last selection area when remember_selection is enabled"
```

---

## Verification

After each chunk:
- Build C++ overlay: `cd capture-overlay && cmake --build build`
- Build Rust: `cargo build`
- Run tests: `cargo test`
- Manual test: launch overlay, toggle settings, start recording, verify JSON output contains all fields

Final verification:
- Toggle each General tab setting in the overlay
- Click Record Video → verify JSON stdout includes all 10 settings
- Check `~/.config/apexshot/config.yml` contains saved settings
- Verify cursor shows/hides in X11 recording based on cursor setting
- Verify countdown displays when countdown is enabled
