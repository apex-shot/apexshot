# GNOME Runtime Recording Overlays Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist the full recording-panel state, snapshot it at recording start, and render GNOME runtime webcam/click/keystroke overlays in the extension without regressing the existing control bar, dim mask, or recording lifecycle.

**Architecture:** Extend the existing `AppConfig` and `RecordingRequest` path so the capture overlay can persist and hand off all runtime overlay settings. Keep the GNOME Shell extension as the runtime renderer and placement owner, but split new work into focused modules so `gnome-extension/extension.js` remains the entry point instead of the only implementation file.

**Tech Stack:** Rust, serde/serde_yml, GTK4, C++/Qt capture overlay, GNOME Shell extension JavaScript, Gio/GLib D-Bus

---

## File Map

- Modify: `src/config.rs`
  Add persisted recording-panel fields for webcam, click, and keystroke appearance/state, and make the existing `rec_mic` / `rec_speaker` fields part of the explicit runtime overlay snapshot contract with sanitization/default tests.
- Modify: `src/capture_overlay.rs`
  Extend `RecordingRequest`, CLI argument plumbing, and JSON parsing for the new runtime overlay snapshot fields.
- Modify: `src/recording/mod.rs`
  Copy the new request fields into `AppConfig`, build a runtime overlay snapshot for the session, and thread it into GNOME shell controls handoff.
- Modify: `src/gnome_shell.rs`
  Extend the existing shell-bridge D-Bus payload so `ShowControls` can carry runtime overlay snapshot data and later suppression / toggle commands without breaking current methods.
- Modify: `capture-overlay/src/main.cpp`
  Emit the new recording JSON fields and accept persisted defaults for webcam/click/keystroke settings.
- Modify: `capture-overlay/src/CaptureOverlay.h`
  Add explicit state needed for persisted webcam position and any normalized runtime overlay snapshot fields.
- Modify: `capture-overlay/src/CaptureOverlay.cpp`
  Load persisted defaults into in-memory recording-panel state during construction.
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
  Ensure current UI state drives the serialized runtime snapshot and reflects restored config values.
- Modify: `capture-overlay/src/CaptureOverlay_Events.cpp`
  Keep existing editing behavior intact while updating any interactions needed to preserve normalized webcam placement state.
- Modify: `gnome-extension/extension.js`
  Reduce to entry-point wiring and preserve current exported D-Bus surface.
- Create: `gnome-extension/session-state.js`
  Hold per-recording session state, snapshot parsing, non-regression guards, and no-op defaults.
- Create: `gnome-extension/controls-ui.js`
  Move the existing controls bar build/reposition/timer code behind a focused module without changing behavior.
- Create: `gnome-extension/runtime-overlays.js`
  Render webcam, click, and keystroke overlays from the session snapshot and runtime events.
- Create: `gnome-extension/mask-ui.js`
  Optionally extract current mask actor code so extension responsibilities are split cleanly while preserving existing `ShowMask` / `HideMask` behavior.
- Keep in place: preview tracking / preview stacking logic in `gnome-extension/extension.js` unless a later dedicated task extracts it without behavior changes.
- Modify: `gnome-extension/README.md`
  Document the new runtime overlay data flow, config dependence, and fallback behavior.
- Test: `src/config.rs`
  Add unit tests for defaults, sanitization, and serde compatibility of new recording-panel fields.
- Test: `src/recording/mod.rs`
  Add focused tests for request-to-config mapping and runtime snapshot construction.
- Test: `src/gnome_shell.rs`
  Add focused tests around serialization/gating for the extended shell payload.

### Task 1: Persist Recording-Panel Overlay Settings

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs`

- [ ] **Step 1: Write the failing config tests**

Add tests in `src/config.rs` covering defaults and round-trip persistence for the new fields, for example:

```rust
#[test]
fn recording_overlay_settings_have_expected_defaults() {
    let cfg = AppConfig::default();
    assert!(!cfg.rec_mic);
    assert!(!cfg.rec_speaker);
    assert_eq!(cfg.rec_click_style, 0);
    assert_eq!(cfg.rec_key_filter, 0);
    assert_eq!(cfg.rec_webcam_shape, 3);
}
```

- [ ] **Step 2: Run the focused tests to verify failure**

Run: `cargo test recording_overlay_settings_have_expected_defaults -- --nocapture`

Expected: FAIL because the new config fields do not exist yet.

- [ ] **Step 3: Add the new `AppConfig` fields**

Extend `src/config.rs` with persisted recording-panel fields for:

```rust
pub rec_click_size: f64,
pub rec_click_color: u8,
pub rec_click_style: u8,
pub rec_click_animate: bool,
pub rec_key_size: f64,
pub rec_key_position: u8,
pub rec_key_appearance: u8,
pub rec_key_blur_bg: bool,
pub rec_key_filter: u8,
pub rec_webcam_enabled: bool,
pub rec_webcam_size: u8,
pub rec_webcam_shape: u8,
pub rec_webcam_flip: bool,
pub rec_webcam_device: i32,
pub rec_webcam_rel_x: f64,
pub rec_webcam_rel_y: f64,
```

Keep names explicit and aligned with existing `rec_*` config style. Do not add duplicate audio booleans: reuse the existing persisted `rec_mic` and `rec_speaker` fields as the audio visibility/source-of-truth values for snapshot restore.

- [ ] **Step 4: Add defaults and sanitization**

Clamp normalized/configurable values in `AppConfig::sanitized()`:

```rust
self.rec_click_size = self.rec_click_size.clamp(0.0, 1.0);
self.rec_key_size = self.rec_key_size.clamp(0.0, 1.0);
self.rec_webcam_rel_x = self.rec_webcam_rel_x.clamp(0.0, 1.0);
self.rec_webcam_rel_y = self.rec_webcam_rel_y.clamp(0.0, 1.0);
```

Also clamp enum-like indices to their current UI ranges.

- [ ] **Step 5: Run the focused config tests**

Run: `cargo test recording_overlay_settings -- --nocapture`

Expected: PASS for the new config tests.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: persist recording overlay settings"
```

### Task 2: Extend Recording Request and Capture Overlay JSON

**Files:**
- Modify: `src/capture_overlay.rs`
- Modify: `capture-overlay/src/main.cpp`
- Test: `src/capture_overlay.rs`

- [ ] **Step 1: Write the failing parser test**

Add a focused test in `src/capture_overlay.rs` for the new JSON fields, for example:

```rust
#[test]
fn parse_recording_json_reads_runtime_overlay_fields() {
    let json = r#"{"x":1,"y":2,"width":3,"height":4,"record_type":"video","controls":true,
    "mic":false,"speaker":true,"clicks":true,"keystrokes":true,
    "click_size":0.5,"click_color":2,"click_style":1,"click_animate":true,
    "key_size":0.4,"key_position":3,"key_appearance":1,"key_blur_bg":false,"key_filter":1,
    "webcam":true,"webcam_size":2,"webcam_shape":3,"webcam_flip":false,"webcam_device":0,
    "webcam_rel_x":0.75,"webcam_rel_y":0.2}"#;
    let request = parse_recording_json(json).unwrap();
    assert!(!request.mic);
    assert!(request.speaker);
    assert!(request.webcam);
    assert_eq!(request.click_style, 1);
}
```

- [ ] **Step 2: Run the focused parser test to verify failure**

Run: `cargo test parse_recording_json_reads_runtime_overlay_fields -- --nocapture`

Expected: FAIL because `RecordingRequest` does not yet carry those fields.

- [ ] **Step 3: Extend `RecordingRequest`**

Add fields in `src/capture_overlay.rs` for the runtime overlay snapshot:

```rust
pub webcam: bool,
pub click_size: f64,
pub click_color: u8,
pub click_style: u8,
pub click_animate: bool,
pub key_size: f64,
pub key_position: u8,
pub key_appearance: u8,
pub key_blur_bg: bool,
pub key_filter: u8,
pub webcam_size: u8,
pub webcam_shape: u8,
pub webcam_flip: bool,
pub webcam_device: i32,
pub webcam_rel_x: f64,
pub webcam_rel_y: f64,
```

Keep the existing top-level `mic` and `speaker` request fields in the snapshot contract and make them explicit in tests and handoff code rather than treating them as separate legacy-only toggles.

- [ ] **Step 4: Parse and serialize the new fields**

Update `parse_recording_json(...)` in `src/capture_overlay.rs` and `printRecordingJson(...)` in `capture-overlay/src/main.cpp` to read/write the same keys exactly once.

- [ ] **Step 5: Extend CLI handoff defaults**

In `src/capture_overlay.rs`, add CLI arguments for passing persisted defaults into the C++ binary, following the existing `--rec-*`, `--video-*`, and `--gif-*` pattern.

- [ ] **Step 6: Run focused parser tests**

Run: `cargo test parse_recording_json_ -- --nocapture`

Expected: PASS for the parser tests covering the new fields.

- [ ] **Step 7: Commit**

```bash
git add src/capture_overlay.rs capture-overlay/src/main.cpp
git commit -m "feat: extend recording request with overlay snapshot fields"
```

### Task 3: Restore Persisted Overlay State in the C++ Recording Panel

**Files:**
- Modify: `capture-overlay/src/CaptureOverlay.h`
- Modify: `capture-overlay/src/CaptureOverlay.cpp`
- Modify: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- Modify: `capture-overlay/src/CaptureOverlay_Events.cpp`

- [ ] **Step 1: Add a failing restore-path note and focused smoke assertions**

Add a small constructor-level smoke test or comment-backed verification target if C++ unit tests are unavailable, documenting the exact fields that must restore from CLI defaults:

```cpp
// Expected restored state: webcam/click/keystroke options match CLI defaults.
```

If there is an existing C++ test harness for the overlay, prefer a focused test there instead.

- [ ] **Step 2: Add persisted state fields needed for runtime handoff**

In `capture-overlay/src/CaptureOverlay.h`, add explicit state for normalized webcam placement if it does not already exist:

```cpp
double m_webcamRelX;
double m_webcamRelY;
```

Keep them separate from temporary draw rectangles.

- [ ] **Step 3: Load CLI defaults into overlay state**

In `capture-overlay/src/CaptureOverlay.cpp` / `capture-overlay/src/main.cpp`, initialize the current recording-panel state from the persisted config-backed CLI arguments instead of hardcoded defaults.

- [ ] **Step 4: Preserve normalized webcam placement during editing**

In `capture-overlay/src/CaptureOverlay_Drawing.cpp` and `capture-overlay/src/CaptureOverlay_Events.cpp`, compute and update normalized webcam position from the user’s chosen placement relative to the selected recording region.

- [ ] **Step 5: Ensure JSON emission uses current UI state**

When the user starts recording, emit the current click/keystroke/webcam state into the recording JSON, including normalized webcam placement and all style flags.

- [ ] **Step 6: Run the narrowest available verification**

Run the project’s existing capture-overlay build/test command for this component. If no isolated test exists, run the smallest available build or manual smoke check command and record the result in the implementation notes.

Expected: the recording panel still opens and uses restored settings without changing current interactions.

- [ ] **Step 7: Commit**

```bash
git add capture-overlay/src/CaptureOverlay.h capture-overlay/src/CaptureOverlay.cpp capture-overlay/src/CaptureOverlay_Drawing.cpp capture-overlay/src/CaptureOverlay_Events.cpp capture-overlay/src/main.cpp
git commit -m "feat: restore recording overlay state in capture ui"
```

### Task 4: Map Runtime Overlay Snapshot Into Rust Recording Flow

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `src/config.rs`
- Test: `src/recording/mod.rs`

- [ ] **Step 1: Write the failing recording mapping test**

Extend `src/recording/mod.rs` tests with assertions like:

```rust
#[test]
fn prepare_overlay_recording_request_maps_runtime_overlay_settings() {
    assert_eq!(prepared.updated_app_config.rec_click_style, 1);
    assert_eq!(prepared.updated_app_config.rec_webcam_rel_x, 0.75);
}
```

- [ ] **Step 2: Run the focused test to verify failure**

Run: `cargo test prepare_overlay_recording_request_maps_runtime_overlay_settings -- --nocapture`

Expected: FAIL because the mapping and config fields are incomplete.

- [ ] **Step 3: Copy the new request fields into `AppConfig`**

In `prepare_overlay_recording_request(...)`, persist the full recording-panel snapshot:

```rust
app_config.rec_click_style = request.click_style;
app_config.rec_key_filter = request.key_filter;
app_config.rec_webcam_rel_x = request.webcam_rel_x;
```

Also keep the existing audio fields explicit in this mapping:

```rust
app_config.rec_mic = request.mic;
app_config.rec_speaker = request.speaker;
```

- [ ] **Step 4: Introduce a dedicated runtime overlay snapshot struct**

In `src/recording/mod.rs` or `src/gnome_shell.rs`, add a focused struct such as:

```rust
pub struct RuntimeOverlaySnapshot {
    pub mic_visible: bool,
    pub speaker_visible: bool,
    pub webcam_enabled: bool,
    pub webcam_rel_x: f64,
    pub webcam_rel_y: f64,
    pub webcam_size: u8,
    pub webcam_shape: u8,
    pub webcam_flip: bool,
    pub webcam_device: i32,
    pub clicks_enabled: bool,
    pub click_size: f64,
    pub click_color: u8,
    pub click_style: u8,
    pub click_animate: bool,
    pub keystrokes_enabled: bool,
    pub key_size: f64,
    pub key_position: u8,
    pub key_appearance: u8,
    pub key_blur_bg: bool,
    pub key_filter: u8,
}
```

Use the full schema above rather than an abbreviated version so the Rust-to-GNOME contract is explicit before implementation starts.

- [ ] **Step 5: Thread the snapshot into the shell-controls handoff**

Keep the existing recording controls flow intact, but include the new snapshot in the GNOME handoff path only when shell controls are active.

- [ ] **Step 6: Run focused recording tests**

Run: `cargo test prepare_overlay_recording_request_ -- --nocapture`

Expected: PASS for the new request/config/runtime snapshot mapping tests.

- [ ] **Step 7: Commit**

```bash
git add src/recording/mod.rs src/config.rs
git commit -m "feat: map recording overlay snapshot into runtime flow"
```

### Task 5: Extend the GNOME Shell Bridge Without Breaking Current Methods

**Files:**
- Modify: `src/gnome_shell.rs`
- Test: `src/gnome_shell.rs`

- [ ] **Step 1: Write the failing shell payload test**

Add a focused test around the new serialization helper, for example:

```rust
#[test]
fn controls_payload_includes_runtime_overlay_snapshot() {
    let args = controls_method_args(&spec);
    assert!(args.iter().any(|arg| arg.contains("webcam_rel_x")));
}
```

- [ ] **Step 2: Run the focused test to verify failure**

Run: `cargo test controls_payload_includes_runtime_overlay_snapshot -- --nocapture`

Expected: FAIL because the payload helper does not exist yet.

- [ ] **Step 3: Extract shell method argument building into helpers**

Refactor `src/gnome_shell.rs` so `ShowMask` / `ShowControls` argument construction is explicit and testable:

```rust
fn show_controls_args(spec: &RecordingControlsSpec) -> Vec<String>
```

- [ ] **Step 4: Extend `RecordingControlsSpec` with overlay snapshot data**

Add a field such as:

```rust
pub runtime_overlays_json: Option<String>
```

or a typed struct that is serialized centrally before the D-Bus call. Keep the existing positional arguments stable and add the new payload in a backwards-compatible way only after updating the extension interface in the same change.

- [ ] **Step 5: Preserve current no-snapshot behavior**

If no runtime overlay snapshot is provided:

```rust
return existing ShowControls payload only;
```

This is the non-regression guard for older or inactive paths.

- [ ] **Step 6: Run focused shell tests**

Run: `cargo test gnome_shell -- --nocapture`

Expected: PASS for the new payload/gating tests.

- [ ] **Step 7: Commit**

```bash
git add src/gnome_shell.rs
git commit -m "feat: extend gnome shell controls payload for runtime overlays"
```

### Task 6: Split Extension Responsibilities Before Adding New Runtime Overlays

**Files:**
- Modify: `gnome-extension/extension.js`
- Create: `gnome-extension/session-state.js`
- Create: `gnome-extension/controls-ui.js`
- Create: `gnome-extension/mask-ui.js`

- [ ] **Step 1: Add a failing syntax check target**

Document the module split and immediately verify the current extension parses before any behavior changes:

Run: `gjs -m gnome-extension/extension.js`

Expected: current parse baseline is known before refactor.

- [ ] **Step 2: Extract mask logic into `mask-ui.js`**

Move the current `_showMask`, `_hideMask`, and `_ensureMaskGroup` behavior into a focused helper module without changing the exported D-Bus API.

- [ ] **Step 3: Extract controls bar code into `controls-ui.js`**

Move `_showControls`, `_hideControls`, `_buildControlsChrome`, timer updates, and placement helpers into a module consumed by `extension.js`.

- [ ] **Step 4: Add `session-state.js`**

Create a thin session-state module that owns:

```js
currentRect
controlsState
runtimeOverlaySnapshot
shortcutEditActive
```

and defaults to safe no-op values when the new data is absent.

- [ ] **Step 5: Preserve preview tracking in place**

Do not refactor preview tracking / preview stacking in this task. Keep the existing preview helper logic in `gnome-extension/extension.js` untouched except for import/lifecycle plumbing required by the module split.

- [ ] **Step 6: Re-run the extension syntax check**

Run: `gjs -m gnome-extension/extension.js`

Expected: PASS on JavaScript/module syntax with no new parse errors.

- [ ] **Step 7: Commit**

```bash
git add gnome-extension/extension.js gnome-extension/session-state.js gnome-extension/controls-ui.js gnome-extension/mask-ui.js
git commit -m "refactor: split gnome extension recording modules"
```

### Task 7: Render Runtime Webcam / Click / Keystroke Overlays in the Extension

**Files:**
- Modify: `gnome-extension/extension.js`
- Modify: `gnome-extension/controls-ui.js`
- Modify: `gnome-extension/session-state.js`
- Create: `gnome-extension/runtime-overlays.js`

- [ ] **Step 1: Add a failing runtime-overlays contract note**

Write the expected module API first:

```js
export function attachRuntimeOverlays(sessionState) {}
export function updateRuntimeOverlaySnapshot(sessionState, snapshot) {}
export function destroyRuntimeOverlays(sessionState) {}
```

- [ ] **Step 2: Implement `runtime-overlays.js` with no-op defaults**

Create actors for:

```js
webcam tile
click pulse container
keystroke chip container
```

but keep them hidden unless a valid snapshot is present.

- [ ] **Step 3: Update `ShowControls` handling to accept snapshot data**

Parse the runtime overlay payload from the shell bridge and store it in session state without changing current behavior when the payload is missing.

- [ ] **Step 4: Render snapshotted placement and style**

Use the saved snapshot to place:

```js
webcam relative to capture rect
keystroke container at the configured preset position
click indicators with configured size/color/style/animation
mic/speaker indicators using visibility only, without introducing a second styling system
```

Lock style and placement for the session.

- [ ] **Step 5: Preserve current controls-only path**

Verify that if the payload is absent:

```js
controls and mask still behave exactly as today
runtime overlay actors remain detached
```

- [ ] **Step 6: Wire teardown into recording lifecycle end paths**

Ensure `destroyRuntimeOverlays(sessionState)` is called from the same stop/discard/failure cleanup path that currently hides recording controls, so no actors can leak across sessions.

- [ ] **Step 7: Re-run extension syntax check**

Run: `gjs -m gnome-extension/extension.js`

Expected: PASS on syntax after adding the new module.

- [ ] **Step 8: Commit**

```bash
git add gnome-extension/extension.js gnome-extension/controls-ui.js gnome-extension/session-state.js gnome-extension/runtime-overlays.js
git commit -m "feat: render runtime recording overlays in gnome extension"
```

### Task 8: Add Live Visibility Toggles and Self-Input Exclusion

**Files:**
- Modify: `gnome-extension/controls-ui.js`
- Modify: `gnome-extension/runtime-overlays.js`
- Modify: `gnome-extension/session-state.js`
- Modify: `src/gnome_shell.rs`
- Modify: `src/recording/mod.rs`

- [ ] **Step 1: Write the failing visibility/suppression tests**

Add focused Rust tests for toggle command serialization and session-state defaults, for example:

```rust
#[test]
fn controls_toggle_commands_do_not_mutate_snapshot_style() {}
```

If there is no JS test harness, document the manual verification target in the plan execution notes and keep Rust-side serialization covered by tests.

- [ ] **Step 2: Implement visibility-only toggle commands**

Extend the existing runtime controls command path so webcam/clicks/keystrokes/mic/speaker buttons can show or hide overlays without changing the stored session snapshot.

- [ ] **Step 3: Exclude extension-owned UI interactions**

In `gnome-extension/runtime-overlays.js`, do not generate click or keystroke overlay events for:

```js
control bar interactions
extension-owned widgets
shortcut-edit capture widgets
```

- [ ] **Step 4: Keep snapshot style immutable**

Guard the runtime state so toggling a feature back on always reuses the session snapshot:

```js
sessionState.runtimeOverlaySnapshot
```

and never mutates style/layout fields in response to runtime clicks.

- [ ] **Step 5: Run focused verification**

Run the narrowest available Rust tests for the toggle command path and re-run:

`gjs -m gnome-extension/extension.js`

Expected: Rust tests PASS and the extension still parses.

- [ ] **Step 6: Commit**

```bash
git add gnome-extension/controls-ui.js gnome-extension/runtime-overlays.js gnome-extension/session-state.js src/gnome_shell.rs src/recording/mod.rs
git commit -m "feat: add runtime overlay toggles and self-input exclusion"
```

### Task 9: Add Hotkey-Capture Suppression

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/hotkeys/mod.rs`
- Modify: the actual hotkey-editor UI file once identified during implementation

- [ ] **Step 1: Locate the real hotkey editor UI file**

Before editing, identify the concrete GTK or extension file that owns hotkey capture UI.

Run: `rg -n "hotkey|shortcut|accelerator" src gnome-extension`

Expected: one concrete UI owner file is identified and added to this task’s file list before implementation proceeds.

- [ ] **Step 2: Write the failing suppression test**

Add a focused Rust test around the daemon suppression gate, for example:

```rust
#[test]
fn daemon_ignores_hotkeys_while_shortcut_edit_is_active() {}
```

- [ ] **Step 3: Run the focused test to verify failure**

Run: `cargo test daemon_ignores_hotkeys_while_shortcut_edit_is_active -- --nocapture`

Expected: FAIL because no suppression gate exists yet.

- [ ] **Step 4: Add a session-scoped suppression flag**

Implement a minimal daemon/global-hotkey gate that can ignore shortcut activations while editing is active.

- [ ] **Step 5: Wire the real UI into the suppression lifecycle**

The hotkey capture UI must set and clear suppression on:

```text
enter edit
save
cancel
blur
Escape
```

- [ ] **Step 6: Re-run focused hotkey tests**

Run: `cargo test hotkey -- --nocapture`

Expected: the new suppression test passes and no existing hotkey tests regress.

- [ ] **Step 7: Commit**

```bash
git add src/daemon/mod.rs src/hotkeys/mod.rs
git add <hotkey-ui-file-found-in-step-1>
git commit -m "fix: suppress daemon hotkeys during shortcut editing"
```

### Task 10: Update Docs and Run Final Verification

**Files:**
- Modify: `gnome-extension/README.md`
- Modify: `docs/superpowers/specs/2026-03-26-gnome-runtime-recording-overlays-design.md` only if implementation changes the approved design

- [ ] **Step 1: Update extension docs**

Document:

```text
runtime overlay snapshot flow
config persistence of recording-panel settings
non-regression / fallback behavior
extension module split
```

- [ ] **Step 2: Run focused Rust verification**

Run:

```bash
cargo test recording_overlay_settings -- --nocapture
cargo test parse_recording_json_ -- --nocapture
cargo test prepare_overlay_recording_request_ -- --nocapture
cargo test gnome_shell -- --nocapture
cargo test hotkey -- --nocapture
```

Expected: feature-related tests pass. If unrelated pre-existing failures remain, note them separately.

- [ ] **Step 3: Run extension parse verification**

Run: `gjs -m gnome-extension/extension.js`

Expected: no JavaScript syntax/module errors.

- [ ] **Step 4: Run manual GNOME smoke verification**

Verify on GNOME Wayland:

```text
controls bar placement unchanged
dim mask unchanged
preview stacking / preview tracking unchanged
webcam position matches capture overlay choice
click style matches capture overlay choice
keystroke appearance/filter matches capture overlay choice
runtime toggles only show/hide
editing a hotkey does not trigger the daemon
runtime overlays disappear on stop, discard, and failure
extension runtime overlay failure falls back to controls-only behavior
```

- [ ] **Step 5: Commit**

```bash
git add gnome-extension/README.md
git commit -m "docs: document gnome runtime recording overlays"
```
