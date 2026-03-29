# Recording Handoff Countdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current video-recording handoff with a Rust-owned centered countdown and prevent GNOME runtime previews from appearing before active recording starts.

**Architecture:** Keep C++ as selection/preview UI only. Rust owns the video-recording countdown and recording-start lifecycle, then initializes GNOME controls/runtime overlays only after recording begins. Screenshot flow remains unchanged.

**Tech Stack:** Rust, GTK4 overlay code, GNOME Shell extension JavaScript, existing recording/session state code

---

## File Structure

- Modify: `src/recording/mod.rs` to split pre-recording handoff from active recording overlay initialization.
- Modify: `src/recording/stop_overlay.rs` or the existing countdown overlay module to add the centered countdown variant for video recording.
- Modify: `src/gnome_shell.rs` if needed so GNOME controls are only shown after recording start.
- Modify: `gnome-extension/session-state.js` only if runtime activation payload shape changes.
- Modify: `gnome-extension/runtime-overlays.js` to enforce active-recording-only rendering.
- Test: `gnome-extension/tests/runtime-overlays.test.js`
- Test: Rust tests near `src/recording/mod.rs` or countdown module for gating behavior.

### Task 1: Lock recording lifecycle boundaries

**Files:**
- Modify: `src/recording/mod.rs`

- [ ] **Step 1: Write the failing Rust test for preview gating**

Add a unit test that asserts screenshot and pre-recording flows do not produce a runtime overlay snapshot for GNOME activation.

- [ ] **Step 2: Run the targeted Rust test**

Run: `cargo test recording:: -- --nocapture`

Expected: a gating assertion fails or the new test is not implemented yet.

- [ ] **Step 3: Refactor the lifecycle split**

Move GNOME `show_recording_controls` and runtime preview setup so they occur only after recording has actually transitioned into active video recording.

- [ ] **Step 4: Re-run the targeted Rust test**

Run: `cargo test recording:: -- --nocapture`

Expected: the new gating test passes.

### Task 2: Add the centered video handoff countdown

**Files:**
- Modify: `src/recording/countdown_overlay.rs`
- Modify: `src/recording/mod.rs`

- [ ] **Step 1: Write the failing test or focused harness for countdown mode selection**

Add coverage showing video recording chooses the centered handoff countdown while screenshot keeps the existing countdown path.

- [ ] **Step 2: Run the targeted countdown test**

Run: `cargo test countdown -- --nocapture`

Expected: FAIL before implementation.

- [ ] **Step 3: Implement the centered countdown overlay**

Add a centered circular numeric countdown variant that matches the intended handoff flow and call it only for video recording start.

- [ ] **Step 4: Re-run the targeted countdown test**

Run: `cargo test countdown -- --nocapture`

Expected: PASS.

### Task 3: Prevent GNOME previews before active recording

**Files:**
- Modify: `gnome-extension/runtime-overlays.js`
- Test: `gnome-extension/tests/runtime-overlays.test.js`

- [ ] **Step 1: Write the failing GNOME test**

Add a test asserting the webcam/runtime overlay actors are not created or shown when recording is not active.

- [ ] **Step 2: Run the GNOME test file**

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`

Expected: FAIL on the new assertion.

- [ ] **Step 3: Implement active-recording gating**

Guard runtime overlay attachment and preview polling behind explicit active-recording state, not just the presence of snapshot data.

- [ ] **Step 4: Re-run the GNOME test**

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`

Expected: PASS.

### Task 4: Verify transition ordering

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `src/gnome_shell.rs` if needed

- [ ] **Step 1: Add temporary logging at the handoff boundary**

Log countdown start, countdown end, recording start, and GNOME controls show points.

- [ ] **Step 2: Run a manual video recording flow**

Run: `cargo build --release` then start the daemon and begin a video recording.

Expected order:
- C++ panel closes
- centered countdown appears
- recording starts
- GNOME controls appear

- [ ] **Step 3: Remove any temporary logging that is no longer needed**

Keep only diagnostics that are useful long-term.

### Task 5: Full verification

**Files:**
- Modify: none unless verification exposes a bug

- [ ] **Step 1: Run GNOME extension tests**

Run: `for f in gnome-extension/tests/*.test.js; do gjs -m "$f"; done`

Expected: all pass.

- [ ] **Step 2: Run focused Rust verification**

Run: `cargo check --bin apexshot`

Expected: PASS.

- [ ] **Step 3: Rebuild release artifacts**

Run: `cargo build --release`

Expected: PASS and updated `target/release/apexshot`.

- [ ] **Step 4: Refresh the GNOME extension**

Run:
`mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io`
`cp -r /home/codegoddy/Desktop/apexshot/gnome-extension/. ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io/`
`gnome-extensions disable apexshot-preview-helper@apexshot.github.io`
`gnome-extensions enable apexshot-preview-helper@apexshot.github.io`

Expected: extension reloads cleanly.

- [ ] **Step 5: Manual sudo install step**

Run manually:
`sudo install -m 755 /home/codegoddy/Desktop/apexshot/target/release/apexshot /usr/local/bin/apexshot`
`sudo install -m 755 /home/codegoddy/Desktop/apexshot/target/release/apexshot-capture /usr/local/bin/apexshot-capture`

Expected: the daemon launched via desktop entry matches the current source tree.
