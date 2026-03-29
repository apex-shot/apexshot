# GNOME Webcam Live Preview Design

## Goal

Render a real live webcam preview inside the GNOME extension during recording while Rust remains the sole owner of webcam capture, stitching, and recording. The GNOME preview must match the C++ webcam preview styling and behavior closely, with only drag position remaining mutable during recording.

## Scope

This design covers:

- Rust-owned webcam preview frame production during recording
- A frozen webcam preview config snapshot captured at recording start
- GNOME Shell rendering of live webcam frames using that frozen config
- Dragging the GNOME webcam preview only within the active recording rect

This design does not cover:

- Mid-recording webcam device swaps
- Mid-recording webcam size or shape changes
- Mid-recording mirror toggles
- Independent GNOME webcam capture

## Current State

The GNOME extension currently renders a placeholder webcam tile in `gnome-extension/runtime-overlays.js`. It does not display live webcam frames. The C++ capture overlay has the visual treatment the GNOME preview should match, but the GNOME implementation currently uses a different, simplified shell-side card.

## User-Facing Requirements

- The GNOME extension must show a real live webcam preview during recording.
- Rust must continue to handle webcam capture, stitching, and recording.
- The GNOME preview must match the C++ webcam preview style rather than the current placeholder tile.
- The webcam configuration used by GNOME must be whatever the user had set before recording started.
- During recording, only preview position may change.
- Dragging must be clamped so the full preview always remains inside the recording area.

## Recommended Architecture

### Option A: Rust Pushes Live Preview Frames to GNOME

Rust owns webcam capture and periodically publishes preview frames plus a frozen webcam preview config to the GNOME extension.

Advantages:

- One webcam capture path
- No split ownership between Rust and GNOME
- GNOME remains a pure renderer
- Styling can change independently from capture logic

Disadvantages:

- Requires a preview transport between Rust and GNOME
- Requires shell-side image updates from streamed frames

### Option B: GNOME Captures Webcam Independently

The GNOME extension opens the webcam itself and tries to mirror Rust recording behavior.

Advantages:

- Fewer frame transport mechanics from Rust to GNOME

Disadvantages:

- Two webcam pipelines
- State drift risk
- Device and mirror mismatch risk
- More failure modes on Wayland/GNOME Shell

### Decision

Use Option A. Rust remains the single source of truth for webcam capture and publishes a live preview stream plus a frozen preview config contract to GNOME.

## Frozen Preview Contract

At recording start, Rust captures a webcam preview snapshot that GNOME uses for the full session.

Frozen fields:

- webcam enabled state
- selected webcam device identity
- preview size
- preview shape
- mirror flag
- initial relative position

Mutable during recording:

- preview relative position only

Deferred until next recording:

- device changes
- size changes
- shape changes
- mirror changes

This keeps the preview stable while still letting the user reposition it.

## Frame Transport

Rust should publish preview frames over a dedicated lightweight preview channel used only during an active recording session.

The transport contract should include:

- session identifier
- monotonic frame sequence or timestamp
- encoded frame payload suitable for GNOME rendering
- frozen webcam preview config metadata needed at initial attach time

The exact encoding can be finalized in implementation planning, but the contract must preserve low enough latency for a convincing live preview while staying simple enough to integrate into the current recording flow.

## GNOME Rendering Model

The GNOME extension replaces the current placeholder webcam actor with an image-backed live preview actor.

Responsibilities:

- receive preview frames from Rust
- decode or apply them into the shell actor
- render the preview using the frozen config
- apply mirror, size, shape, and border radius from the frozen config
- allow drag updates only for preview position
- clamp drag updates to the recording rect

Non-responsibilities:

- opening the webcam
- choosing devices
- changing shape or size policy
- deciding recording composition logic

## Visual Matching

The GNOME webcam preview should match the C++ webcam preview, not the current shell placeholder tile.

That means:

- same sizing logic for the frozen size classes
- same border radius logic for circle, square, rectangle, and vertical variants
- same mirrored labeling/behavior expectations where applicable
- no placeholder icon/label card in place of a real frame

If the C++ preview includes additional frame styling details, those should be treated as the reference and ported to the GNOME renderer rather than approximated with a new shell-only visual style.

## Dragging and Clamping

Dragging is allowed during recording, but the full preview must remain inside the recording rect.

Rules:

- drag uses the frozen preview dimensions
- clamp based on the full preview bounds, not just the pointer hotspot
- the preview may not cross outside the active recording area
- position updates are written back as relative coordinates so Rust and GNOME stay synchronized

## Data Flow

1. User starts recording with webcam settings already chosen.
2. Rust freezes the webcam preview config for the session.
3. Rust begins publishing live preview frames and the frozen config to GNOME.
4. GNOME attaches the live preview actor using the frozen config.
5. GNOME renders incoming frames and applies shape, size, mirror, and border radius.
6. If the user drags the preview, GNOME clamps the new position to the recording rect and sends only the updated position back through the session path.
7. When recording ends, Rust stops the preview channel and GNOME destroys the preview actor.

## Error Handling

- If no preview frames are available, GNOME should not fall back to the current placeholder card during an active session without an explicit policy decision.
- If the preview channel disconnects mid-recording, GNOME should hide the preview actor or show a minimal failure state that does not diverge visually from the C++ experience more than necessary.
- If the frozen config is invalid, GNOME should refuse to render the preview rather than inventing local defaults that may differ from Rust.

## Testing Strategy

### Rust

- test frozen webcam preview config generation at recording start
- test that mid-recording config changes do not alter the active preview contract
- test preview position update handling and clamping contract inputs/outputs

### GNOME

- test preview render config application from the frozen contract
- test drag clamping so the preview cannot leave the recording rect
- test that only position changes are accepted during recording
- test that placeholder-only webcam rendering is no longer used for active live preview sessions

### Manual

- verify GNOME live preview visually matches the C++ webcam preview
- verify preview remains inside the recording area while dragging
- verify device, size, shape, and mirror changes do not mutate the active session until the next recording
- verify recording output still comes from Rust stitching rather than shell-side composition

## Implementation Notes

- Keep the preview contract narrow. GNOME should not infer settings from unrelated state when Rust can provide them explicitly.
- Avoid introducing a second rendering interpretation of webcam settings inside GNOME.
- Prefer explicit synchronization of position updates over shell-local drift.

## Open Decisions For Planning

- exact IPC transport format for preview frames
- frame encoding and update cadence
- whether position updates are sent continuously during drag or committed on drag end
- precise C++ visual reference points that must be copied into the GNOME actor styling
