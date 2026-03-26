# GNOME Recording Mask Design

## Goal

Implement a GNOME-specific live recording mask for ApexShot on GNOME Wayland that keeps the selected recording area clear while dimming the rest of the screen during recording. ApexShot must continue using its own selection UI, recording pipeline, and controls bar.

## Non-Goals

- Replacing ApexShot's current C++ area selector
- Replacing ApexShot's Rust recording pipeline
- Depending on GNOME's built-in screenshot or recording UI for user interaction
- Delivering the same overlay behavior on every compositor in the first pass

## Problem

On GNOME Wayland, generic GTK windows cannot reliably act as a live fullscreen dimming mask above the real desktop. The current app-window-based approach causes occlusion and black-screen behavior because those windows are treated as normal application surfaces rather than compositor-managed shell overlays.

GNOME's built-in screenshot and recording UX can show this effect because it is implemented inside GNOME Shell itself. The effect is not exposed as a reusable public app widget.

## Constraints

- ApexShot UI must remain the user-facing UI.
- The persistent dimmed background must appear during active recording.
- Only the recording controls bar should remain visible during recording.
- GNOME-specific implementation is acceptable for this feature.
- Depending on GNOME Shell internals or a GNOME Shell extension is acceptable if ApexShot still owns the UX.

## Recommended Approach

Use a small GNOME Shell extension to render the live dim-mask overlay inside GNOME Shell while ApexShot continues to own:

- area selection
- countdown
- recording pipeline
- controls bar
- recording lifecycle

The extension should only be responsible for drawing and removing the dimmed mask around a provided rectangle.

## Alternatives Considered

### 1. App-only GTK overlay

Rejected for GNOME Wayland. Normal application windows occlude the desktop and cannot safely reproduce the desired effect.

### 2. Use GNOME Shell private D-Bus methods directly

Rejected as primary design. This would couple ApexShot to GNOME Shell's built-in selection and recording flows rather than preserving ApexShot's own UI and recording pipeline.

### 3. Controls-bar-only fallback

Useful as fallback behavior, but it does not satisfy the product requirement.

## Architecture

### ApexShot side

Add a Rust GNOME integration adapter that:

- detects GNOME Wayland
- checks whether the GNOME extension D-Bus service is available
- sends `ShowMask` and `HideMask` requests
- logs and falls back gracefully when the extension is unavailable

This adapter should be the only Rust module aware of the extension protocol.

### GNOME Shell extension side

Extend the existing `gnome-extension` module so it:

- owns a D-Bus name for overlay control
- receives geometry from ApexShot
- creates shell-managed overlay actors on the stage
- dims everything outside the selected recording rectangle
- leaves the recording area visually clear
- removes the overlay immediately when requested

The extension should not own recording logic, countdown logic, or playback controls.

### Controls bar

The controls bar remains an ApexShot surface and keeps the current placement policy:

- below the selected area when there is room
- top-center for fullscreen recording

The extension is responsible only for the dimmed mask.

## Data Flow

1. User selects a recording area with ApexShot's current selector.
2. ApexShot resolves the final rectangle used for recording.
3. ApexShot runs countdown if enabled.
4. Immediately before recording begins, ApexShot asks the GNOME extension to show the mask for that rectangle.
5. ApexShot starts the recording pipeline.
6. ApexShot shows its own controls bar.
7. On stop, cancel, delete, or error, ApexShot hides the GNOME mask and closes the controls bar.

## D-Bus Contract

Proposed extension D-Bus interface:

- `ShowMask(i x, i y, i width, i height)`
- `HideMask()`
- optional future method: `UpdateMask(i x, i y, i width, i height)`

The initial contract should stay minimal.

## Overlay Rendering Model

The extension should render a single shell-owned overlay container with four dim regions:

- top
- left
- right
- bottom

This preserves a clear hole for the recording area while avoiding transparent application windows. Because the overlay is owned by GNOME Shell, it participates in the compositor scene directly instead of competing with the desktop as a normal app window.

## Error Handling

If the extension is not installed, disabled, incompatible, or crashes:

- recording still starts
- ApexShot logs the failure
- ApexShot falls back to controls-bar-only mode

If the extension disappears during recording:

- do not abort recording
- continue recording normally
- treat the overlay as best-effort UI

## Testing Strategy

Manual verification on GNOME Wayland:

- area recording shows a clear recording region with the rest of the screen dimmed
- controls bar remains visible and correctly placed
- fullscreen recording places controls at top-center
- stop, cancel, and delete remove the overlay
- daemon restart does not leave stale overlays behind
- disabled-extension path falls back cleanly

Regression checks:

- existing non-GNOME recording path unchanged
- existing screenshot path unchanged
- no extra black fullscreen overlay windows created from GTK

## Rollout Plan

### Phase 1

- add GNOME extension overlay actor
- add Rust D-Bus client
- wire show/hide into recording lifecycle
- keep fallback path

### Phase 2

- refine placement and monitor-scale handling
- add reconnect/version checks
- consider pause-state visual treatment if needed

## Open Questions

- Whether the extension should expose a version property for compatibility checks
- Whether the overlay should react to monitor layout changes during an active recording
- Whether fullscreen mode should skip dimming entirely on some setups for performance reasons

## Recommendation

Implement the GNOME-specific recording mask with a GNOME Shell extension and keep all user-facing recording flow in ApexShot. This is the only design that matches the required UX on GNOME Wayland without depending on broken app-window overlays.
