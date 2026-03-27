# GNOME Runtime Recording Overlays Design

## Goal

Extend ApexShot's GNOME recording experience so the recording-time UI shown by the GNOME Shell extension follows the settings chosen in the pre-record recording panel.

This includes:

- webcam placement and appearance
- click indicator appearance
- keystroke appearance and filtering
- live visibility of mic, speaker, webcam, clicks, and keystrokes while recording
- persistence of recording-panel settings into config, not just the General, Video, and GIF tab settings

## Non-Goals

- Moving recording controls out of the GNOME Shell extension
- Replacing the Rust recording pipeline
- Allowing full live restyling or repositioning of overlays during an active recording
- Replacing the existing capture-overlay recording panel

## Problem

The GNOME Shell extension already owns the pieces that are hard to keep stable on GNOME Wayland:

- controls bar placement
- dim background / recording mask
- shell-level visual positioning during recording

However, the extension currently does not fully reflect the broader recording-panel settings chosen before recording starts. Only the settings already routed through the existing config path are persisted consistently, and runtime overlays are not yet fully driven by the same pre-record choices.

There is also a shortcut-editing issue: when the user clicks a hotkey capture field, the running daemon reacts to those keys as global shortcuts. That behavior conflicts with editing and would also pollute any keystroke overlay if left unchanged.

## Constraints

- The extension must remain the owner of recording-time shell placement and dim-background behavior.
- The pre-record capture overlay remains the authoring UI for recording overlay appearance.
- Runtime overlay style should match what the user saw before recording started.
- Runtime behavior must be stable on GNOME Wayland.
- Hotkey capture must not trigger daemon actions while editing.
- Extension changes must be additive and must not break current implementations.

## Non-Regression Requirement

This work must preserve today's extension behavior unless the new runtime overlay path is explicitly active for a recording session.

The following existing behaviors must remain intact:

- control bar placement
- dim-mask lifecycle
- preview stacking / preview tracking behavior
- current recording lifecycle hooks
- existing D-Bus entry points already used by ApexShot

If any new runtime overlay path fails:

- recording must continue
- the extension must fall back to the current behavior
- new overlay rendering must fail soft rather than destabilize the extension

## Recommended Approach

Use a snapshot-and-toggle model.

At recording start:

- ApexShot snapshots all recording-panel overlay settings from the capture overlay.
- Rust stores those values into config and passes the session snapshot to the runtime recording flow.
- The GNOME Shell extension receives the snapshot and uses it as the source of truth for live overlay rendering.

During recording:

- style and placement are frozen for the active session
- only feature visibility may change live
- toggling a feature back on reuses the snapshotted appearance for that session

This preserves consistency between pre-record preview and runtime behavior while keeping compositor-sensitive placement in the extension.

## Why The Extension Owns Runtime Rendering

The control bar was moved into the extension because normal app windows were unreliable for:

- keeping the dim background stable
- pinning the controls relative to the recording area
- staying visually correct inside GNOME Shell's compositor model

The same reasoning applies to runtime webcam, click, and keystroke overlays. They should be rendered in the extension, not in a separate GTK/C++ runtime surface, because they need the same shell-level placement guarantees.

The extension should therefore own:

- controls bar placement
- dim mask ownership
- runtime webcam placement
- runtime click indicator rendering
- runtime keystroke rendering
- live visibility toggles

Rust/C++ should provide configuration and runtime events, not shell positioning.

## Runtime Behavior Model

### Session snapshot

When recording starts, ApexShot creates a session snapshot that includes:

- webcam enabled state
- webcam normalized position relative to the selected capture region
- webcam size
- webcam shape
- webcam flip state
- click overlay enabled state
- click size
- click color
- click style
- click animation enabled state
- keystroke overlay enabled state
- keystroke position
- keystroke appearance
- keystroke blur-background flag
- keystroke filter mode
- mic visible state
- speaker visible state

This snapshot is immutable for styling and placement for the lifetime of the recording session.

### Live changes allowed during recording

The runtime controls may only toggle visibility for:

- mic
- speaker
- webcam
- clicks
- keystrokes

No live restyling is allowed during recording:

- no webcam repositioning
- no webcam shape/size restyling
- no click style changes
- no keystroke style/filter changes

### Webcam position

The webcam position chosen in the capture overlay before recording must be the position used during recording.

That position should be stored relative to the selected recording region rather than as absolute screen coordinates. The extension should resolve the final on-screen placement using the recording bounds it already manages.

### Keystroke display

The keystroke overlay must respect the pre-record settings for:

- position
- appearance
- blur background
- filter mode

If the filter is set to command-only, normal typing must not appear in the overlay.

### Click display

The click overlay must respect the pre-record settings for:

- size
- color
- style
- animation mode

## Self-Input Exclusion

ApexShot must not treat its own runtime UI interactions as captured recording overlays.

That means:

- clicks on the extension's control bar or runtime UI must not produce click indicators in the recording overlay
- key presses used to interact with ApexShot-owned controls must not appear in the keystroke overlay

This applies both to normal recording-time controls and to any shortcut-editing UI.

## Hotkey Capture / Editing

Shortcut editing requires a dedicated suppression path.

When a hotkey capture widget enters editing mode:

- ApexShot sets a temporary `shortcut-edit-active` state
- the global daemon hotkey path ignores matching key events while that state is active
- the keystroke overlay ignores those same key events

Capture mode ends on:

- save / accept
- cancel
- blur / focus loss
- `Escape`

This suppression should be enforced in two places:

- the UI side, to avoid local leakage into overlay rendering
- the daemon/global shortcut side, to avoid actual command execution if focus handling is imperfect

## Config Persistence

Recording-panel settings must be persisted into config the same way the General, Video, and GIF tab settings already are.

The persisted config must include:

- webcam enabled state
- webcam position
- webcam size
- webcam shape
- webcam flip
- selected webcam device if appropriate for existing device-handling rules
- click enabled state
- click size
- click color
- click style
- click animation flag
- keystroke enabled state
- keystroke position
- keystroke appearance
- keystroke blur-background flag
- keystroke filter
- mic enabled state
- speaker enabled state

The next time the recording panel opens, these values should be restored so the user sees the same recording setup they last chose.

## Architecture

### Capture overlay / C++ responsibilities

- expose the authoring UI for webcam, click, and keystroke settings
- maintain the editable pre-record state
- return those settings in the recording request payload

### Rust responsibilities

- extend config persistence to include the full recording-panel state
- load persisted recording-panel state into the capture overlay defaults
- include the full runtime overlay snapshot in the recording session handoff
- provide runtime event/state updates to the extension
- provide shortcut-edit suppression hooks for the daemon/global hotkey path

### GNOME Shell extension responsibilities

- receive the runtime overlay snapshot for the active session
- render runtime webcam, click, and keystroke overlays
- apply shell-stable placement relative to the active recording region
- own live visibility toggles
- exclude ApexShot-owned UI interactions from overlay rendering
- keep existing control-bar and mask flows working unchanged when no runtime overlay snapshot is provided

## Extension Structure Direction

`gnome-extension/extension.js` is already carrying multiple responsibilities and will keep growing if new recording-time UI is added directly into the same file. As part of implementation planning, the extension work should be divided into focused sub-files where practical.

Recommended split:

- `extension.js` as the entry point and lifecycle wiring
- one module for preview tracking / preview stacking
- one module for dim-mask actors and geometry updates
- one module for recording controls placement and actions
- one module for runtime recording overlays (webcam, clicks, keystrokes)
- one module for shared session state / D-Bus message handling

This refactor should be incremental and non-destructive:

- no behavior-only rewrite
- no broad unrelated cleanup
- move stable pieces behind small modules while preserving current interfaces
- keep the main entry file thin so future recording features do not accumulate in one place

## Data Flow

1. User configures webcam, click, keystroke, and recording toggles in the capture overlay.
2. ApexShot persists those settings into config.
3. User starts recording.
4. ApexShot snapshots the current overlay settings into the recording session payload.
5. Rust starts the recording pipeline and sends the runtime snapshot to the GNOME Shell extension.
6. The extension renders overlays using the snapshotted style and placement.
7. During recording, the extension may toggle feature visibility on or off without changing style or placement.
8. On stop, discard, or failure, the extension removes runtime overlays together with the rest of the recording UI.

## Failure Behavior

If the extension cannot render part of the runtime overlay state:

- recording must continue
- the control bar and dim-mask behavior should continue if available
- missing runtime overlay features should fail soft rather than abort recording

If shortcut suppression cannot be established:

- ApexShot should prefer disabling shortcut capture for that session rather than allowing real daemon actions to fire during editing

## Testing Strategy

Manual verification:

- webcam appears during recording at the same position chosen before recording
- webcam shape, size, and flip match the pre-record settings
- click indicators match the chosen size, color, style, and animation
- keystroke overlay matches the chosen position, appearance, blur, and filter
- command-only filtering hides normal typing
- toggling webcam, clicks, keystrokes, mic, and speaker during recording only changes visibility
- runtime toggles do not reposition or restyle overlays
- clicks on ApexShot's own runtime UI do not create click indicators
- editing a hotkey does not trigger the daemon
- editing a hotkey does not inject keystrokes into the recording overlay
- closing and reopening the app restores all recording-panel settings from config

Regression checks:

- existing control bar placement remains unchanged
- existing dim-mask behavior remains unchanged
- existing General, Video, and GIF persistence remains unchanged
- non-GNOME paths continue to behave as before unless explicitly extended later

## Rollout Plan

### Phase 1

- persist the full recording-panel state into config
- restore those values into the capture overlay
- extend the recording session payload with the runtime overlay snapshot

### Phase 2

- render webcam, click, and keystroke overlays from the snapshot in the GNOME extension
- add runtime visibility toggles for supported features

### Phase 3

- add shortcut-edit suppression across UI and daemon paths
- add self-input exclusion for runtime click and keystroke overlays

## Recommendation

Keep the GNOME Shell extension as the runtime rendering host for recording overlays, because it already owns the hard compositor-sensitive responsibilities. Make the capture overlay the authoring surface, make Rust the persistence and session-snapshot bridge, and keep runtime behavior strict: snapshot style and placement at record start, then allow live show/hide only.
