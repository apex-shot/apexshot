# Recording ScreenCast Restore Token Design

## Goal

Reduce repeated GNOME Wayland ScreenCast approval prompts for recording by persisting and reusing ScreenCast restore tokens.

## Problem

ApexShot currently requests a fresh ScreenCast session for recording each time. On GNOME Wayland this means the user repeatedly has to approve screen sharing, even when recording the same target again.

The codebase already has restore-token persistence for screenshot/capture flows in `src/backend/wayland.rs`, but recording uses a separate ScreenCast path in `src/recording/mod.rs` and does not reuse that mechanism.

## Requirements

- Persist ScreenCast restore tokens for recording
- Keep separate tokens for `record-screen` and `record-area`
- Try restore-token reuse before interactive portal selection
- Clear invalid tokens automatically and retry interactively
- Save any new restore token returned by the portal after a successful interactive selection
- Keep current behavior as fallback when persistence is unavailable or rejected

## Non-Goals

- Bypassing portal security rules
- Sharing one token between screenshot capture and recording
- Guaranteeing that GNOME never prompts again

## Architecture

### Token storage

Add recording-specific token helpers in the Rust recording path, modeled after the existing helpers in `src/backend/wayland.rs`.

Token scope:
- `record-screen`
- `record-area`

Use ApexShot-managed cache files with stable names, one per recording target.

### Recording flow

In the Wayland recording path:

1. Determine the recording target kind from the requested geometry:
   - fullscreen/screen recording -> `record-screen`
   - area recording -> `record-area`
2. Load the saved restore token for that target if one exists
3. Attempt ScreenCast selection with that restore token and persistent mode
4. If restore succeeds, continue recording without the normal reapproval prompt
5. If restore fails, clear the token and retry with a normal interactive portal request
6. If the interactive request returns a new restore token, persist it for the same target kind

### Failure behavior

- Missing token: normal interactive prompt
- Invalid/revoked token: delete token, retry interactively
- Portal returns no restore token: continue recording without persistence update
- Persistence write failure: continue recording; do not fail the session

## Testing Strategy

Manual checks:
- first `record screen` prompts and saves a token
- second `record screen` reuses the token when portal/compositor allows it
- first `record area` prompts and saves a separate token
- `record screen` does not overwrite `record area` token
- bad/revoked token recovers by prompting once and saving a fresh token

Regression checks:
- normal recording still works when no token exists
- screenshot/capture restore-token behavior remains unchanged
- token persistence does not break GNOME shell mask or controls flow
