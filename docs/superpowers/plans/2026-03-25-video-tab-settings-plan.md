# Video Tab Settings Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Video tab UI controls to actual recording behavior and persist settings to config, following the same pattern as General tab implementation.

**Architecture:** Settings flow from C++ overlay (Qt5) as JSON → Rust parses into `RecordingRequest` → applies to GStreamer pipeline (videoscale, videorate, audio caps) → persists to `AppConfig`.

**Tech Stack:** C++ Qt5 (overlay), Rust (recording engine), GStreamer (video capture), serde/serde_yml (config persistence).

---

## Chunk 1: C++ — Add Video Tab Settings to JSON Output

### Task 1.1: Add accessor methods for Video tab settings

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay.h:100-107`

- [ ] **Step 1: Add accessors below existing General tab accessors**

```cpp
bool recordShowCountdown() const { return m_showCountdown; }

// Video tab settings
int recordVideoMaxRes() const { return m_videoMaxRes; }
int recordVideoFps() const { return m_videoFps; }
bool recordMono() const { return m_recordMono; }
bool recordOpenEditor() const { return m_openEditor; }
```

- [ ] **Step 2: Commit**

```bash
git add capture-overlay/src/CaptureOverlay.h
git commit -m "feat: add accessor methods for Video tab settings"
```

### Task 1.2: Update `printRecordingJson` signature to include Video tab settings

**Files:**
- Modify: `capture-overlay/src/main.cpp:94-99`

- [ ] **Step 1: Update function signature**

```cpp
void printRecordingJson(const QRect& sel, const char* recordType,
                         bool controls, bool mic, bool speaker,
                         bool clicks, bool keystrokes,
                         bool displayRecTime, bool hidpi, bool doNotDisturb,
                         bool showCursor, bool rememberSelection,
                         bool dimScreen, bool countdown,
                         int videoMaxRes, int videoFps, bool recordMono, bool openEditor)
```

- [ ] **Step 2: Commit**

```bash
git add capture-overlay/src/main.cpp
git commit -m "feat: update printRecordingJson signature for Video tab settings"
```

### Task 1.3: Update JSON output to include Video tab fields

**Files:**
- Modify: `capture-overlay/src/main.cpp:101-123`

- [ ] **Step 1: Update JSON output to include all Video tab fields**

```cpp
void printRecordingJson(const QRect& sel, const char* recordType,
                         bool controls, bool mic, bool speaker,
                         bool clicks, bool keystrokes,
                         bool displayRecTime, bool hidpi, bool doNotDisturb,
                         bool showCursor, bool rememberSelection,
                         bool dimScreen, bool countdown,
                         int videoMaxRes, int videoFps, bool recordMono, bool openEditor)
{
    std::printf("{\"x\":%d,\"y\":%d,\"width\":%d,\"height\":%d,"
                "\"mode\":\"record\",\"record_type\":\"%s\","
                "\"controls\":%s,\"mic\":%s,\"speaker\":%s,"
                "\"clicks\":%s,\"keystrokes\":%s,"
                "\"display_rec_time\":%s,\"hidpi\":%s,"
                "\"notifications\":%s,\"cursor\":%s,"
                "\"remember_selection\":%s,\"dim_screen\":%s,"
                "\"countdown\":%s,"
                "\"video_max_res\":%d,\"video_fps\":%d,"
                "\"record_mono\":%s,\"open_editor\":%s}\n",
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
                countdown ? "true" : "false",
                videoMaxRes,
                videoFps,
                recordMono ? "true" : "false",
                openEditor ? "true" : "false");
    std::fflush(stdout);
}
```

- [ ] **Step 2: Commit**

```bash
git add capture-overlay/src/main.cpp
git commit -m "feat: pass Video tab settings in recording JSON output"
```

### Task 1.4: Update the call site to pass Video tab settings

**Files:**
- Modify: `capture-overlay/src/main.cpp:339-351`

- [ ] **Step 1: Update the function call**

```cpp
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
                   overlay.recordShowCountdown(),
                   overlay.recordVideoMaxRes(),
                   overlay.recordVideoFps(),
                   overlay.recordMono(),
                   overlay.recordOpenEditor());
```

- [ ] **Step 2: Commit**

```bash
git add capture-overlay/src/main.cpp
git commit -m "feat: pass Video tab settings from overlay to JSON"
```

---

## Chunk 2: Rust — Extend RecordingRequest and Parser

### Task 2.1: Extend `RecordingRequest` struct with Video tab fields

**Files:**
- Modify: `src/capture_overlay.rs:169-189`

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
    // Video tab settings
    pub video_max_res: u8,
    pub video_fps: u8,
    pub record_mono: bool,
    pub open_editor: bool,
}
```

- [ ] **Step 2: Commit**

```bash
git add src/capture_overlay.rs
git commit -m "feat: extend RecordingRequest with Video tab settings"
```

### Task 2.2: Update `parse_recording_json` to extract Video tab fields

**Files:**
- Modify: `src/capture_overlay.rs:468-500` (or wherever `parse_recording_json` is defined)

- [ ] **Step 1: Add extraction of Video tab fields with defaults**

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

    // Video tab settings
    let video_max_res = extract_int(json, "video_max_res").unwrap_or(0) as u8;
    let video_fps = extract_int(json, "video_fps").unwrap_or(1) as u8; // Default to 30fps (index 1)
    let record_mono = extract_bool(json, "record_mono").unwrap_or(false);
    let open_editor = extract_bool(json, "open_editor").unwrap_or(false);

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
        video_max_res,
        video_fps,
        record_mono,
        open_editor,
    })
}
```

- [ ] **Step 2: Commit**

```bash
git add src/capture_overlay.rs
git commit -m "feat: parse Video tab settings from recording JSON"
```

---

## Chunk 3: Rust — Extend RecordingConfig for Video Settings

### Task 3.1: Add Video tab fields to `RecordingConfig`

**Files:**
- Modify: `src/recording/mod.rs:46-71`

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
    // Video tab settings
    pub max_resolution: Option<(u32, u32)>,
    pub fps: u32,
    pub mono_audio: bool,
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
            max_resolution: None,
            fps: 30,
            mono_audio: false,
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: add Video tab fields to RecordingConfig"
```

---

## Chunk 4: Rust — AppConfig Persistence

### Task 4.1: Add Video tab settings to `AppConfig`

**Files:**
- Modify: `src/config.rs:17-47`

- [ ] **Step 1: Add Video tab settings fields with `#[serde(default)]`**

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
    // Recording Video tab settings
    pub rec_video_max_res: u8,
    pub rec_video_fps: u8,
    pub rec_video_mono: bool,
    pub rec_video_open_editor: bool,
}
```

- [ ] **Step 2: Add defaults for Video tab fields in `Default` impl**

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
            // Video tab defaults
            rec_video_max_res: 0, // 0 = Original
            rec_video_fps: 1,     // 1 = 30fps
            rec_video_mono: false,
            rec_video_open_editor: false,
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: persist Video tab settings in AppConfig"
```

### Task 4.2: Load/save Video tab settings when recording starts

**Files:**
- Modify: `src/main.rs:1004-1007` (the `RecordingRequested` arm)

- [ ] **Step 1: Update config with Video tab settings**

```rust
Ok(AreaCaptureResult::RecordingRequested(request)) => {
    // Load config
    let mut config = load_config();

    // Update config from overlay settings (General tab)
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

    // Update config from overlay settings (Video tab)
    config.rec_video_max_res = request.video_max_res;
    config.rec_video_fps = request.video_fps;
    config.rec_video_mono = request.record_mono;
    config.rec_video_open_editor = request.open_editor;

    // Remember selection area
    if request.remember_selection {
        config.last_selection_x = Some(request.x);
        config.last_selection_y = Some(request.y);
        config.last_selection_w = Some(request.width);
        config.last_selection_h = Some(request.height);
    }

    // Save config
    let _ = save_config(&config);

    // Build RecordingConfig from request
    let max_resolution = match request.video_max_res {
        0 => None,                      // Original
        1 => Some((1920, 1080)),        // 1080p
        2 => Some((1280, 720)),         // 720p
        _ => None,
    };

    let fps = match request.video_fps {
        0 => 24,
        1 => 30,
        2 => 50,
        3 => 60,
        _ => 30,
    };

    let rec_config = RecordingConfig {
        output_path: /* generate output path */,
        width: Some(request.width as u32),
        height: Some(request.height as u32),
        x: Some(request.x),
        y: Some(request.y),
        cursor: request.cursor,
        hidpi: request.hidpi,
        max_resolution,
        fps,
        mono_audio: request.record_mono,
    };

    // TODO: Start recording with rec_config
    eprintln!("Recording settings saved. Recording not yet wired to engine.");
    std::process::exit(0);
}
```

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: save Video tab settings to config on record start"
```

---

## Chunk 5: Wire Video Settings to GStreamer Pipeline

### Task 5.1: Wire max resolution to GStreamer videoscale

**Files:**
- Modify: `src/recording/mod.rs:410-435` (build_pipeline function)

- [ ] **Step 1: Update `build_pipeline` to apply max resolution scaling**

```rust
async fn build_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<String> {
    let output_str = output_path.to_string_lossy();

    // Get video source
    let video_source = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        get_wayland_source(config.cursor).await?
    } else {
        get_x11_source(config)?
    };

    // HiDPI: downscale to logical resolution (2x)
    let hidpi_filter = if config.hidpi {
        " ! videoscale"
    } else {
        ""
    };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale
                format!(" ! videoscale ! video/x-raw,width={},height={}", max_w, max_h)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(format!(
        "{} ! videoconvert{}{} ! videorate ! video/x-raw,framerate={}/1 ! queue ! {} {} ! {} ! filesink location=\"{}\"",
        video_source, hidpi_filter, resolution_filter, config.fps,
        profile.encoder, profile.props, profile.muxer, output_str
    ))
}
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: wire max resolution to GStreamer videoscale"
```

### Task 5.2: Wire FPS to GStreamer videorate

**Files:**
- Modify: `src/recording/mod.rs:410-435` (already modified in Task 5.1)

- [ ] **Step 1: Verify FPS is in the pipeline string**

The FPS was already added in Task 5.1: `! videorate ! video/x-raw,framerate={}/1`

This ensures the recording will use the specified framerate.

- [ ] **Step 2: Commit** (already committed in Task 5.1)

No additional commit needed - FPS was included in the previous commit.

### Task 5.3: Wire mono audio to GStreamer audio pipeline

**Files:**
- Modify: `src/recording/mod.rs` (audio handling section)

**Note:** The current implementation doesn't have audio pipeline code visible in the files we've read. Audio handling (mic/speaker) will need to be added. For now, we'll add a placeholder comment.

- [ ] **Step 1: Add mono audio caps when building audio pipeline**

If audio pipeline exists, add mono conversion:

```rust
// When building audio pipeline:
let mono_filter = if config.mono_audio {
    " ! audio/x-raw,channels=1"
} else {
    ""
};

// Audio pipeline example:
// pulsesrc ! audioconvert ! audioresample{} ! ...
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: add mono audio caps placeholder for audio pipeline"
```

**Note:** This is a placeholder. Full audio pipeline implementation will require exploring the existing audio handling code and adding the mono conversion there.

---

## Chunk 6: Update GIF Recording Pipeline

### Task 6.1: Update GIF recording to use new config fields

**Files:**
- Modify: `src/recording/mod.rs` (GIF recording function)

- [ ] **Step 1: Ensure GIF recording respects max resolution and FPS**

The GIF recording function `record_gif_rust_with_optional_stop` should also use `max_resolution` and `fps` from `RecordingConfig`.

Update the GIF pipeline construction similar to video recording:

```rust
// In GIF pipeline construction:
let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
    // Apply resolution scaling
    format!(" ! videoscale ! video/x-raw,width={},height={}", max_w, max_h)
} else {
    String::new()
};

// Update framerate in GIF pipeline
let fps_caps = format!("framerate={}/1", config.fps);
```

- [ ] **Step 2: Commit**

```bash
git add src/recording/mod.rs
git commit -m "feat: apply max resolution and FPS to GIF recording"
```

---

## Verification

### After each chunk:
- Build C++ overlay: `cd capture-overlay && cmake --build build`
- Build Rust: `cargo build`
- Run tests: `cargo test`

### Final verification:
1. Toggle each Video tab setting in the overlay
2. Click Record Video → verify JSON stdout includes all 4 Video tab fields
3. Check `~/.config/apexshot/config.yml` contains saved Video tab settings
4. Verify video is downscaled to selected max resolution (use a 4K selection with 720p setting)
5. Verify video framerate matches selected FPS (use mediainfo or ffprobe)
6. Verify audio is mono when record_mono is enabled

## Deferred Features

- **Audio settings button**: Opens system audio settings (not critical for v1)
- **Open Video Editor after recording**: Deferred pending custom video editor implementation

## Success Criteria

1. ✅ All Video tab settings are persisted to config
2. ✅ Max resolution correctly downscales video in GStreamer pipeline
3. ✅ FPS correctly sets framerate in GStreamer pipeline
4. ✅ Mono audio caps are available for audio pipeline (placeholder)
5. ✅ Settings persist between sessions
6. ✅ No regression in existing General tab functionality
