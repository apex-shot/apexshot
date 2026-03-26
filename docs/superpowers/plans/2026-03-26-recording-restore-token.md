# Recording ScreenCast Restore Token Implementation Plan

## Goal

Persist and reuse GNOME Wayland ScreenCast restore tokens for recording, with separate tokens for screen and area recording.

## File Map

- Modify: `src/recording/mod.rs`
  Add recording restore-token helpers and wire them into the Wayland ScreenCast flow.
- Optional reference only: `src/backend/wayland.rs`
  Mirror the existing capture-side token behavior and naming approach.

## Task 1: Add Recording Token Helpers

- [ ] Define a small recording target enum for `screen` vs `area`
- [ ] Add helpers to compute token file paths, load tokens, save tokens, and clear tokens
- [ ] Keep storage local to ApexShot cache files and tolerant of I/O failures

## Task 2: Reuse Tokens In Wayland Recording

- [ ] Identify the Wayland ScreenCast entry point for recording
- [ ] Try the saved token first for the correct target kind
- [ ] On restore failure, clear the token and retry interactively
- [ ] Save any newly returned restore token after a successful interactive grant

## Task 3: Verify And Keep Fallback Safe

- [ ] Run `cargo check`
- [ ] Run `cargo build --release`
- [ ] Confirm recording still falls back cleanly when no token exists or restore fails
