# Video Tab Settings — Functional Implementation

## Context

The recording panel's Controls settings menu has a Video tab with 5 settings. Currently all are UI-only — they display controls but have no effect on recording behavior. This design wires them to actual functionality, following the same pattern as the General tab implementation.

## Requirements

1. Wire Max resolution dropdown to GStreamer pipeline (downscale video)
2. Wire Video FPS dropdown to GStreamer pipeline (set framerate)
3. Wire Record mono checkbox to GStreamer pipeline (convert stereo to mono)
4. Persist all Video tab settings to `~/.config/apexshot/config.yml`
5. Pass settings from C++ overlay → Rust recording engine via JSON
6. Defer: Audio settings button (opens system audio config)
7. Defer: Open Video Editor after recording (custom editor will be built later)

## Settings Map

| # | Setting | Type | Effect |
|---|---------|------|--------|
| 1 | Max resolution | Dropdown (Original/1080p/720p) | Downscale video if capture area exceeds selected resolution |
| 2 | Video FPS | Dropdown (24/30/50/60) | Set recording framerate |
| 3 | Audio settings | Button | Open system audio settings (deferred) |
| 4 | Record mono | Checkbox | Convert stereo audio to mono in recording |
| 5 | Open Video Editor | Checkbox | Launch editor after recording (deferred) |

## Architecture

### Data Flow

```
C++ Overlay (Qt5)                    Rust Backend
┌─────────────────┐                 ┌──────────────────────┐
│ Video tab UI    │                 │ parse_recording_json │
│ m_videoMaxRes   │──JSON──stdout──▶│ RecordingRequest     │
│ m_videoFps      │                 │   .video_max_res     │
│ m_recordMono    │                 │   .video_fps         │
│ m_openEditor    │                 │   .record_mono       │
└─────────────────┘                 │   .open_editor       │
                                    └──────┬───────────────┘
                                           │
                             ┌─────────────┴─────────────┐
                             ▼                           ▼
                       AppConfig.update()         RecordingConfig
                       (persist settings)         (recording params)
```

### C++ Changes

**`CaptureOverlay.h`**
- No new member variables (all already exist: `m_videoMaxRes`, `m_videoFps`, `m_recordMono`, `m_openEditor`)
- Add accessor methods for Video tab settings

**`main.cpp` — `printRecordingJson()`**
- Add Video tab settings to JSON output:
  ```json
  {
    "record_type": "video",
    "x": 0, "y": 0, "width": 1920, "height": 1080,
    "video_max_res": 1,
    "video_fps": 1,
    "record_mono": false,
    "open_editor": false
  }
  ```

### Rust Changes

**`capture_overlay.rs` — `RecordingRequest`**
- Add fields: `video_max_res: u8`, `video_fps: u8`, `record_mono: bool`, `open_editor: bool`
- Update `parse_recording_json()` to extract them with defaults

**`config.rs` — `AppConfig`**
- Add Video tab settings fields with `#[serde(default)]`
- Persist to `~/.config/apexshot/config.yml`

**`recording/mod.rs` — `RecordingConfig`**
- Add: `max_resolution: Option<(u32, u32)>`, `fps: u32`, `mono_audio: bool`
- Apply max resolution via `videoscale ! video/x-raw,width=W,height=H`
- Apply FPS via `videorate ! video/x-raw,framerate=N/1`
- Apply mono via `audio/x-raw,channels=1`

**`recording/mod.rs` — `build_pipeline`**
- Modify pipeline string to include resolution scaling
- Add framerate caps to videorate element
- Add mono audio caps when audio sources are enabled

## Implementation Details

### Max Resolution

**Logic:**
```rust
match video_max_res {
    0 => None,                          // Original - no scaling
    1 => Some((1920, 1080)),            // 1080p
    2 => Some((1280, 720)),             // 720p
    _ => None,
}
```

**GStreamer pipeline:**
```
ximagesrc ! videoconvert ! videoscale ! video/x-raw,width=1920,height=1080 ! videorate ! ...
```

**Preserve aspect ratio:** The `videoscale` element will automatically maintain aspect ratio. We only downscale, never upscale.

### Video FPS

**Logic:**
```rust
match video_fps {
    0 => 24,
    1 => 30,
    2 => 50,
    3 => 60,
    _ => 30,
}
```

**GStreamer pipeline:**
```
videorate ! video/x-raw,framerate=30/1 ! queue ! ...
```

### Mono Audio

**Logic:**
When `record_mono` is true, apply audio caps to force single channel output.

**GStreamer pipeline (audio section):**
```
pulsesrc ! audioconvert ! audioresample ! audio/x-raw,channels=1 ! ...
```

**Applies to:** Both microphone and speaker audio sources when either is enabled.

## Implementation Order

1. C++ JSON output — add Video tab settings to `printRecordingJson()`
2. Rust `RecordingRequest` — extend struct and parser
3. Rust `AppConfig` — add persistence fields
4. Max resolution — wire to GStreamer videoscale
5. Video FPS — wire to GStreamer videorate
6. Mono audio — wire to GStreamer audio caps

## Deferred Features

### Audio Settings Button

**Future implementation:**
```rust
// Open system audio settings
if let Ok(_) = std::process::Command::new("gnome-control-center")
    .args(["sound"])
    .spawn() {
    // Success
} else if let Ok(_) = std::process::Command::new("systemsettings5")
    .args(["kcm_pulseaudio"])
    .spawn() {
    // Success
}
```

### Open Video Editor

**Status:** Deferred pending custom video editor implementation.

**Future behavior:**
After recording completes, if `open_editor` is true, launch the custom ApexShot video editor with the recorded file path.

## Testing

### Manual Testing

1. Toggle each Video tab setting in the overlay
2. Click Record Video → verify JSON stdout includes all 4 settings
3. Check `~/.config/apexshot/config.yml` contains saved settings
4. Verify video is downscaled to selected max resolution
5. Verify video framerate matches selected FPS
6. Verify audio is mono when record_mono is enabled

### Automated Testing

- Unit tests for `parse_recording_json()` with Video tab fields
- Unit tests for resolution mapping (1080p, 720p)
- Unit tests for FPS mapping (24, 30, 50, 60)

## Success Criteria

1. All Video tab settings are persisted to config
2. Max resolution correctly downscales video in GStreamer pipeline
3. FPS correctly sets framerate in GStreamer pipeline
4. Mono audio correctly converts stereo to mono
5. Settings persist between sessions
6. No regression in existing General tab functionality
