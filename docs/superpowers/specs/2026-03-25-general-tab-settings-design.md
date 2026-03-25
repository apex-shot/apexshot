# General Tab Settings — Functional Implementation

## Context

The recording panel's Controls settings menu has a General tab with 10 toggle settings.
Currently all are UI-only — they toggle visually but have no effect on recording behavior.
The "Retina" setting is macOS-specific and must be replaced with a Linux-equivalent.

## Requirements

1. Make all 10 General tab settings functional
2. Replace "Retina" with "HiDPI Scaling" (capture at logical vs physical resolution)
3. Persist settings to `~/.config/apexshot/config.yml` between sessions
4. Wire settings from C++ overlay → Rust recording engine

## Settings Map

| # | Setting | Type | Effect |
|---|---------|------|--------|
| 1 | Controls | bool | Show/hide controls during recording (stop overlay) |
| 2 | Menu bar | bool | Display elapsed time in stop overlay |
| 3 | HiDPI Scaling | bool | Capture at logical resolution vs physical |
| 4 | Notifications | bool | Suppress system notifications (D-Bus DND) |
| 5 | Cursor | bool | Show/hide mouse cursor in capture |
| 6 | Clicks | bool | Highlight mouse clicks in capture |
| 7 | Keyboard | bool | Overlay keystrokes on recording |
| 8 | Remember selection | bool | Save/restore last selection area |
| 9 | Dim screen | bool | Dim screen during countdown |
| 10 | Show countdown | bool | Show 3-2-1 countdown before recording |

## Architecture

### Data Flow

```
C++ Overlay (Qt5)                    Rust Backend
┌─────────────────┐                 ┌──────────────────────┐
│ Settings toggles │                 │ parse_recording_json │
│ m_controls       │──JSON──stdout──▶│ RecordingRequest     │
│ m_displayRecTime │                 │   .controls          │
│ m_hidpi          │                 │   .display_rec_time  │
│ m_doNotDisturb   │                 │   .hidpi             │
│ m_showCursor     │                 │   .notifications     │
│ m_recClicks      │                 │   .cursor            │
│ m_recKeystrokes  │                 │   .clicks            │
│ m_rememberSel    │                 │   .keystrokes        │
│ m_dimScreen      │                 │   .remember_selection│
│ m_showCountdown  │                 │   .dim_screen        │
└─────────────────┘                 │   .countdown         │
                                     └──────┬───────────────┘
                                            │
                              ┌─────────────┴─────────────┐
                              ▼                           ▼
                        AppConfig.update()         RecordingConfig
                        (persist settings)         (recording params)
```

### C++ Changes

**`CaptureOverlay.h`**
- Rename `m_scaleRetina` → `m_hidpi`
- No new member variables (all already exist)

**`CaptureOverlay.cpp`**
- Replace "Retina" label text with "HiDPI Scaling"
- Replace "Scale Retina videos to 1x" desc with "Record at display scale resolution"
- Update toggle case for HiDPI

**`main.cpp` — `printRecordingJson()`**
- Add all settings to JSON output:
  ```json
  {
    "record_type": "video",
    "x": 0, "y": 0, "width": 1920, "height": 1080,
    "controls": true,
    "display_rec_time": false,
    "hidpi": false,
    "notifications": true,
    "cursor": true,
    "clicks": false,
    "keystrokes": false,
    "remember_selection": false,
    "dim_screen": true,
    "countdown": true
  }
  ```

### Rust Changes

**`capture_overlay.rs` — `RecordingRequest`**
- Add fields: `hidpi`, `notifications`, `cursor`, `clicks`, `keystrokes`, `remember_selection`, `dim_screen`, `countdown`, `display_rec_time`
- Update `parse_recording_json()` to extract them with defaults

**`config.rs` — `AppConfig`**
- Add `last_selection: Option<SelectionRect>` struct for remember selection
- Add `#[serde(default)]` for backward compatibility

**`recording/mod.rs` — `RecordingConfig`**
- Add: `cursor: bool`, `hidpi: bool`, `clicks: bool`
- Apply cursor via `ximagesrc` property `show-pointer`
- Apply hidpi by adjusting pipeline resolution scaling

**`main.rs` — Recording flow**
- Pre-recording sequence:
  1. If `notifications` → call D-Bus to enable DND
  2. If `dim_screen` → show GTK4 dim overlay
  3. If `countdown` → show 3-2-1 countdown
  4. Start recording with config
- Post-recording: restore notifications if DND was enabled

**`recording/stop_overlay.rs`**
- If `controls` is false → don't show stop overlay
- If `display_rec_time` is true → show elapsed timer

## Implementation Order

1. C++ JSON output — add all settings to `printRecordingJson()`
2. Rust `RecordingRequest` — extend struct and parser
3. Rust `AppConfig` — add persistence fields
4. Cursor/Hidpi — wire to GStreamer pipeline
5. Countdown/Dim — GTK4 overlay windows before recording
6. Notifications — D-Bus DND toggle
7. Keyboard — keystroke overlay (deferred if complex)
8. Remember selection — save/load last area

## Open Questions

- **Keyboard overlay**: Needs a keystroke listener + overlay compositor. Complex. Can be deferred.
- **Clicks highlight**: ximagesrc `show-pointer` shows cursor but doesn't highlight clicks. Needs post-processing or custom overlay. Can defer.
