# Recording Handoff Countdown Design

**Date:** 2026-03-29

## Goal

Clean up the transition from the C++ selection/preview UI into the GNOME recording UI by showing a centered countdown for video recordings, and ensure runtime previews only exist during active recording.

## Scope

This design changes only the video recording handoff and runtime overlay activation rules.

It does not change:
- screenshot countdown behavior
- screenshot capture flow
- the C++ UI's role as selection/preview UI
- Rust ownership of recording/stitching

## Desired Behavior

### Video recording flow

1. The user configures the recording in the C++ area-init UI.
2. The user presses `Record`.
3. The C++ recording panel disappears immediately.
4. Rust shows a centered countdown overlay like the provided circular reference.
5. When the countdown completes, Rust starts the actual recording session.
6. Only after recording starts does GNOME initialize the recording controls and runtime overlays.

### Screenshot flow

Screenshot mode keeps the current countdown behavior unchanged.

### Runtime overlay gating

Runtime overlays must only exist during active video recording.

That means:
- no GNOME webcam preview during tool initialization
- no GNOME webcam preview during area selection
- no GNOME webcam preview during screenshot capture
- no GNOME runtime overlay actors before the recording session has actually started

## Ownership

### C++ UI

The C++ UI remains preview-only.

Its responsibilities:
- area selection
- pre-recording settings
- local preview before record is pressed

It should not own the handoff countdown or GNOME runtime overlays.

### Rust

Rust owns:
- the centered handoff countdown for video recording
- recording start timing
- activation of GNOME recording controls
- activation of GNOME runtime overlays

This keeps the transition order in one place and avoids GNOME preview state appearing too early.

### GNOME extension

The GNOME extension should only render controls and runtime overlays after Rust declares the recording session active.

## Transition Sequence

### Video recording

1. C++ area-init returns the recording request.
2. Rust decides this is a video recording path.
3. Rust shows the centered countdown overlay.
4. Rust waits for the countdown to finish.
5. Rust starts the recording session.
6. Rust publishes GNOME controls and runtime overlay state.
7. GNOME renders the recording UI and allowed overlays.

### Screenshot capture

1. C++ area-init returns the screenshot request.
2. Existing screenshot countdown behavior remains unchanged.
3. No GNOME runtime overlays are created.

## Countdown UI Requirements

The new handoff countdown should:
- be centered on screen
- be circular
- show a large numeric countdown
- appear before GNOME recording UI initialization
- be used only for video recording

The current video-recording countdown should be removed or bypassed once this new flow is in place.

## Preview Visibility Rules

The webcam preview bug is a lifecycle bug as much as a rendering bug.

The rule is:
- preview visibility is tied to active recording state, not selection state

So the system should only create or update GNOME webcam preview actors when:
- recording mode is `video`
- recording has actually started
- runtime overlay webcam visibility is enabled

## Risks

- If GNOME controls are still initialized before recording start, previews may still flash early.
- If the old countdown path remains active, the user may see two countdowns.
- If screenshot and video paths are not separated clearly enough, screenshot behavior may regress.

## Testing Requirements

Manual verification must cover:
- video recording: C++ panel disappears, centered countdown shows, then GNOME controls appear
- screenshot: existing countdown remains unchanged
- screenshot: no GNOME webcam/runtime overlays appear
- video selection before recording: no GNOME webcam/runtime overlays appear
- active video recording: runtime overlays appear only after recording begins

Automated verification should cover:
- countdown gating logic for video vs screenshot
- runtime overlay activation only after recording-start state is true
- screenshot flow never creating runtime overlay state
