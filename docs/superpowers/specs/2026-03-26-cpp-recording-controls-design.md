# C++ Recording Controls Design

## Goal

Replace the Rust recording controls bar with a C++ controls bar implemented in `apexshot-capture`, matching the visual layout shown by the user:

- stop icon with live elapsed timer beside it
- pause icon
- restart icon
- delete icon
- menu icon

The controls must own the visual presentation and timer behavior, while Rust continues to own the recording engine.

## Problem

The current recording controls and countdown are rendered in Rust. This has produced repeated UX mismatches:

- countdown styling and positioning do not match the screenshot overlay style
- controls bar layout does not match the intended design
- GNOME Wayland integration became harder because multiple UI responsibilities were split across C++ and Rust

The screenshot-style countdown and polished selection UI already live in the C++ overlay, so the recording controls should follow the same visual system.

## Non-Goals

- Replacing the Rust recording pipeline
- Replacing the GNOME shell dim-mask extension
- Reworking screenshot preview or annotate UI

## Requirements

- Recording controls UI must be written in C++
- Controls bar must visually match the requested screenshot direction
- Timer must show the real elapsed recording time
- Pause/resume must function
- Restart must function by discarding the current recording and starting a new recording with the same session geometry/settings
- Delete must discard the current recording
- Stop must save/finalize the current recording
- Menu icon may remain non-functional initially if needed, but the button must exist visually

## Recommended Architecture

### C++ responsibilities

- Render the controls bar
- Position the controls bar
- Render the live timer text
- Handle button interactions
- Launch in a dedicated `record-controls` mode after recording starts
- Send control commands to Rust

### Rust responsibilities

- Start and stop recordings
- Pause/resume pipeline state
- Restart recording sessions
- Discard/delete output when requested
- Manage shell-mask lifecycle
- Launch and monitor the C++ controls process

## Control Protocol

Rust and C++ need a small session control protocol.

### Commands from C++ to Rust

- `pause`
- `resume`
- `restart`
- `stop`
- `discard`

### State from Rust to C++

- `recording_started`
- `recording_paused`
- `recording_resumed`
- `recording_stopped`
- `recording_failed`

The simplest transport should be a local IPC channel scoped to the active recording session. A small local socket or session-local D-Bus interface are both acceptable.

## Timer Ownership

The timer must be visually owned by C++, but Rust should remain the source of truth for when recording actually begins and whether it is paused.

Recommended behavior:

- C++ timer starts only after Rust confirms the recording is live
- C++ freezes the timer on pause
- C++ resumes the timer on resume
- C++ resets the timer on restart

## Restart Semantics

Restart should behave as a true restart:

1. Rust stops the current recording
2. Rust discards the unfinished file
3. Rust starts a new recording with the same geometry and settings
4. Rust relaunches or resets the controls state
5. C++ timer returns to `0:00`

## Placement

The C++ controls bar should reuse the existing intended placement logic:

- below the selected recording region when possible
- above it if needed
- top-center for fullscreen

## Rollout Strategy

### Phase 1

- Introduce a C++ `record-controls` mode
- Introduce a minimal Rust control endpoint
- Support stop and discard first
- Keep pause/restart buttons visually present, but only if function can be added immediately without destabilizing recording

### Phase 2

- Add pause/resume pipeline control
- Add restart behavior
- Remove the Rust controls bar path from the overlay recording flow

## Failure Behavior

- If the C++ controls process crashes, Rust must keep recording and expose a safe fallback stop path
- If Rust recording fails, the C++ controls process must exit and the shell mask must clear
- If restart fails, recording should fail closed rather than leave a half-attached controls UI

## Testing Strategy

Manual verification:

- controls bar matches the requested layout
- timer increments in real time while recording
- timer freezes on pause and resumes correctly
- restart discards the current output and starts a fresh recording
- delete discards output and clears the mask
- stop saves output and clears the mask
- controls placement is correct for area and fullscreen recording

Regression checks:

- shell-mask still clears correctly
- screenshot countdown remains unchanged
- no Rust fullscreen countdown is reintroduced

## Recommendation

Make C++ the single owner of recording controls everywhere, with Rust reduced to a recording engine plus session control endpoint. This removes the remaining split-brain UI path and aligns recording UX with the visual system already established in the C++ overlay.
