# Recording UI Implementation Plan

## Overview

Add a recording panel UI to the capture overlay for selecting area recording with GIF/Video options.

## Refined UI Design

### Layout (compact, inside selection)

```
Recording panel open:
┌─────────────────────────────────────┐
│                                      │  ← Main toolbar HIDDEN
│         ┌──────────────┐            │
│         │ [Ctrls][Sz][Crop]         │  ← Row 1: 3 icon tiles
│         │ [Mic][Spk][Rec][Clk][Key] │  ← Row 2: 5 icon tiles
│         │ [ Record Video ]          │  ← Row 3: text button
│         │ [ Record GIF ]            │  ← Row 4: text button
│         └──────────────┘            │
│         Resize handles visible      │
└─────────────────────────────────────┘

After clicking Record tile:
┌─────────────────────────────────────┐
│                                      │  ← Main toolbar HIDDEN
│         ┌──────────────┐            │
│         │ [ Record Video ]          │  ← Only these buttons
│         │ [ Record GIF ]            │
│         └──────────────┘            │
└─────────────────────────────────────┘
```

### Interaction Flow

1. User clicks **Recording** icon in main toolbar
2. Main toolbar hides, recording panel appears inside selection
3. User can adjust selection (resize handles remain active)
4. User clicks **Record** tile (Row 2, Slot 3)
5. Tools section hides, only Record Video/GIF buttons remain
6. User clicks Record Video or Record GIF → starts recording

### Tile Layout

| Row | Tiles |
|-----|-------|
| 1 | Controls, Size, Crop |
| 2 | Mic, Speaker, Record, Click, Keystrokes |
| 3 | Record Video |
| 4 | Record GIF |

### Tile Wiring (Phase 1 - UI only)

| Tile | Behavior |
|------|----------|
| Controls | Toggle (UI state only) |
| Size | Read-only, shows selection dimensions |
| Crop | No action (resize handles are the crop UI) |
| Mic | Toggle (UI state only) |
| Speaker | Toggle (UI state only) |
| Record | Mode switch - hides tools section |
| Click | Toggle (UI state only) |
| Keystrokes | Toggle (UI state only) |
| Record Video | Starts video recording |
| Record GIF | Starts GIF recording |

## Protocol Extension

### New Exit Code
- Exit code **6**: Recording request

### JSON Output
```json
{
  "x": 100,
  "y": 200,
  "width": 800,
  "height": 600,
  "mode": "record",
  "record_type": "video",
  "controls": true,
  "mic": false,
  "speaker": false,
  "clicks": false,
  "keystrokes": false
}
```

## Files to Modify

| File | Changes |
|------|---------|
| `capture-overlay/src/CaptureOverlay.h` | Add recording panel state members |
| `capture-overlay/src/CaptureOverlay.cpp` | Render panel, hit-testing, click handling |
| `capture-overlay/src/main.cpp` | Output recording JSON on exit code 6 |
| `src/capture_overlay.rs` | Parse recording result |
| `src/main.rs` | Handoff to recorder |

## Implementation Tasks

### Phase 1: C++ Overlay UI
1. Add recording panel state to CaptureOverlay.h
2. Draw recording panel in CaptureOverlay.cpp
3. Add hit-testing for panel tiles
4. Handle tile clicks and toggles
5. Wire Record Video/GIF to exit with code 6

### Phase 2: Rust Bridge
6. Add RecordingRequest type to capture_overlay.rs
7. Parse recording JSON output
8. Wire to main.rs capture flow

### Phase 3: Recorder Integration
9. Connect to existing recording/mod.rs
10. Pass bounds and type to recorder

## Worktree Location
`.worktrees/recording-ui` on branch `feature/recording-ui`
