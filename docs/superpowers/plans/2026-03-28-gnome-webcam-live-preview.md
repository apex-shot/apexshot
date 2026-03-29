# GNOME Webcam Live Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the GNOME extension’s placeholder webcam tile with a Rust-fed live webcam preview that matches the C++ webcam styling, freezes webcam settings at recording start, and allows only clamped position dragging during recording.

**Architecture:** Rust remains the sole owner of webcam capture, preview frame production, stitching, and recording. A narrow preview contract sends frozen webcam config plus live frames to the GNOME extension, which renders them using shell actors and only sends clamped position updates back.

**Tech Stack:** Rust, existing recording/session pipeline, D-Bus or current GNOME session transport, GNOME Shell extension JS, GJS tests

---

### Task 1: Map the Current Webcam Recording Path and Choose the Preview Transport

**Files:**
- Modify: `docs/superpowers/plans/2026-03-28-gnome-webcam-live-preview.md`
- Inspect: `src/recording/mod.rs`
- Inspect: `src/recording/`
- Inspect: `gnome-extension/extension.js`
- Inspect: `gnome-extension/session-state.js`

Task 1 result:

Rust preview frames originate in the Rust recording/session path after `prepare_overlay_recording_request()` freezes the webcam config and the active recording pipeline is assembled in `src/recording/mod.rs`. Lifecycle and control stay on `org.apexshot.Preview` for `PreviewOpened` / `PreviewClosed`, while live frame payloads use a separate non-session-D-Bus transport. GNOME subscribes in `gnome-extension/extension.js` to the lifecycle signals and, separately, to the frame transport for live image payloads. The frame channel must carry the session id plus an initial config/handshake so GNOME can attach the correct session and reject stale frames; do not reuse the current session D-Bus plumbing for frame blobs.

### Task 2: Add a Frozen Webcam Preview Contract in Rust

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: related Rust recording/session files discovered in Task 1
- Test: Rust test module near the recording/session state implementation

- [ ] **Step 1: Write the failing Rust test for frozen preview config behavior**

Test should prove:
- recording start snapshots webcam enabled/device/size/shape/mirror/position
- mid-recording changes do not mutate the active preview config
- only position updates are accepted after recording starts

- [ ] **Step 2: Run the focused Rust test to verify it fails**

Run: `cargo test frozen_webcam_preview -- --nocapture`

Expected: FAIL because the frozen preview contract does not exist yet.

- [ ] **Step 3: Implement the frozen preview contract in Rust**

Add a struct along these lines in the relevant recording/session module:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WebcamPreviewConfig {
    enabled: bool,
    device: i32,
    size: i32,
    shape: i32,
    mirror: bool,
    rel_x: f64,
    rel_y: f64,
}
```

The active recording session state should store:

```rust
struct ActiveRecordingPreviewState {
    webcam: WebcamPreviewConfig,
    frame_sequence: u64,
}
```

- [ ] **Step 4: Re-run the focused Rust test**

Run: `cargo test frozen_webcam_preview -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit the frozen preview contract**

```bash
git add src/recording/mod.rs
git commit -m "feat: freeze webcam preview config at recording start"
```

### Task 3: Publish Live Preview Frames from Rust

**Files:**
- Modify: Rust webcam capture / recording files identified in Task 1
- Test: focused Rust tests around preview publication if practical

- [ ] **Step 1: Write the failing Rust test for preview frame publication**

Test should prove:
- active webcam capture emits preview frames during recording
- frames are tagged with session identity and monotonic sequence/timestamp
- preview publication stops when recording ends

- [ ] **Step 2: Run the focused Rust test to verify it fails**

Run: `cargo test webcam_preview_frames -- --nocapture`

Expected: FAIL because preview frame publication is not implemented yet.

- [ ] **Step 3: Implement preview publication from the existing webcam pipeline**

Implementation requirements:
- do not open a second webcam capture path
- reuse the existing capture frames used for recording/stitching
- publish a scaled preview representation appropriate for GNOME rendering
- include the frozen preview config with initial session attach or first frame handshake

- [ ] **Step 4: Re-run the focused Rust test**

Run: `cargo test webcam_preview_frames -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit preview frame publication**

```bash
git add src/recording
git commit -m "feat: publish live webcam preview frames for GNOME"
```

### Task 4: Add GNOME-Side Preview Contract Parsing and Testable Render Helpers

**Files:**
- Create: `gnome-extension/webcam-preview-layout.js`
- Modify: `gnome-extension/session-state.js`
- Create: `gnome-extension/tests/webcam-preview-layout.test.js`

- [ ] **Step 1: Write the failing GJS test for frozen preview config parsing and drag clamping**

Test should prove:
- the GNOME helper accepts the frozen preview config fields
- shape/size mapping is derived from the frozen config
- drag results are clamped fully inside the recording rect
- only position is mutable during recording

- [ ] **Step 2: Run the focused GJS test to verify it fails**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`

Expected: FAIL because the helper module does not exist yet.

- [ ] **Step 3: Implement the pure GNOME helper module**

Add helpers like:

```js
export function webcamPreviewBounds(config, recordingRect) { /* ... */ }
export function clampDraggedWebcamPosition(config, recordingRect, nextRelX, nextRelY) { /* ... */ }
export function isMutableWebcamPreviewField(fieldName) { return fieldName === "rel_x" || fieldName === "rel_y"; }
```

- [ ] **Step 4: Re-run the focused GJS test**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`

Expected: PASS

- [ ] **Step 5: Commit the helper module**

```bash
git add gnome-extension/webcam-preview-layout.js gnome-extension/tests/webcam-preview-layout.test.js gnome-extension/session-state.js
git commit -m "feat: add GNOME webcam preview layout helpers"
```

### Task 5: Replace the Placeholder Webcam Tile with a Live Preview Actor in GNOME

**Files:**
- Modify: `gnome-extension/runtime-overlays.js`
- Modify: `gnome-extension/runtime-overlays-visibility.js`
- Test: `gnome-extension/tests/runtime-overlays.test.js`

- [ ] **Step 1: Write the failing GJS test for renderable live webcam preview behavior**

Test should prove:
- webcam remains a renderable overlay kind
- placeholder icon/label rendering is no longer the active path for live preview sessions
- GNOME applies frozen config-driven shape and size

- [ ] **Step 2: Run the focused GJS test to verify it fails**

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`

Expected: FAIL because the placeholder webcam implementation is still in use.

- [ ] **Step 3: Implement the live preview actor**

Implementation requirements:
- consume preview frames from Rust
- update an image-backed shell actor rather than an icon/label placeholder
- use the C++ reference for border radius, dimensions, and mirror presentation
- keep webcam rendering isolated from clicks and keystrokes

- [ ] **Step 4: Re-run the focused GJS test**

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`

Expected: PASS

- [ ] **Step 5: Commit the GNOME webcam renderer**

```bash
git add gnome-extension/runtime-overlays.js gnome-extension/runtime-overlays-visibility.js gnome-extension/tests/runtime-overlays.test.js
git commit -m "feat: render live webcam preview in GNOME overlay"
```

### Task 6: Match the C++ Webcam Styling Exactly

**Files:**
- Inspect: `capture-overlay/src/CaptureOverlay_Webcam.cpp`
- Inspect: `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- Modify: `gnome-extension/runtime-overlays.js`
- Modify: `gnome-extension/webcam-preview-layout.js`
- Test: `gnome-extension/tests/webcam-preview-layout.test.js`

- [ ] **Step 1: Write the failing style regression test**

Test should prove:
- GNOME uses the same size-class mapping as the C++ webcam preview
- GNOME uses the same shape-to-radius mapping as the C++ webcam preview
- no fallback placeholder card styles remain active for webcam rendering

- [ ] **Step 2: Run the focused GJS test to verify it fails**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`

Expected: FAIL because the exact C++ style mapping is not yet fully mirrored.

- [ ] **Step 3: Port the C++ webcam style rules into the GNOME helper and renderer**

Port:
- size presets
- circle/square/rectangle/vertical radius rules
- mirror handling rules
- any visible frame chrome that belongs to the actual preview presentation

- [ ] **Step 4: Re-run the focused GJS test**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`

Expected: PASS

- [ ] **Step 5: Commit the style parity work**

```bash
git add gnome-extension/runtime-overlays.js gnome-extension/webcam-preview-layout.js gnome-extension/tests/webcam-preview-layout.test.js
git commit -m "feat: match GNOME webcam preview styling to C++"
```

### Task 7: Implement Drag Updates and Clamp Them to the Recording Area

**Files:**
- Modify: `gnome-extension/runtime-overlays.js`
- Modify: `gnome-extension/extension.js`
- Modify: Rust session/recording transport files identified earlier
- Test: `gnome-extension/tests/webcam-preview-layout.test.js`
- Test: focused Rust position update test

- [ ] **Step 1: Write the failing GNOME test for drag clamping**

Test should prove:
- dragged preview never leaves the recording rect
- clamp uses full preview dimensions
- resulting position is emitted as relative coordinates

- [ ] **Step 2: Run the focused GJS test to verify it fails**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`

Expected: FAIL because drag update handling is incomplete.

- [ ] **Step 3: Implement drag interaction and outbound position updates**

Implementation requirements:
- drag only webcam preview, not clicks/keystrokes
- compute clamped bounds from frozen size and shape
- send position updates through the active session path
- ignore any attempted mid-recording changes to non-position webcam fields

- [ ] **Step 4: Re-run the focused GJS and Rust tests**

Run: `gjs -m gnome-extension/tests/webcam-preview-layout.test.js`
Expected: PASS

Run: `cargo test webcam_position_update -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit drag/clamp support**

```bash
git add gnome-extension/runtime-overlays.js gnome-extension/extension.js src/recording
git commit -m "feat: clamp GNOME webcam dragging to recording area"
```

### Task 8: Integrate End-to-End Session Lifecycle

**Files:**
- Modify: Rust recording/session files identified in Task 1
- Modify: `gnome-extension/extension.js`
- Modify: `gnome-extension/session-state.js`

- [ ] **Step 1: Write the failing integration-oriented tests where practical**

Cover:
- preview stream attaches only for active recording sessions
- stream stops and actor is destroyed when recording ends
- stale frames from older sessions are ignored

- [ ] **Step 2: Run the focused tests to verify they fail**

Run: `cargo test webcam_preview_session_lifecycle -- --nocapture`
Expected: FAIL

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`
Expected: FAIL or missing lifecycle behavior

- [ ] **Step 3: Implement lifecycle cleanup and stale-session guards**

Requirements:
- destroy preview actor and transport subscription on recording end
- reject stale frame payloads after restart/new session
- ensure GNOME and Rust stay aligned by session id

- [ ] **Step 4: Re-run the focused tests**

Run: `cargo test webcam_preview_session_lifecycle -- --nocapture`
Expected: PASS

Run: `gjs -m gnome-extension/tests/runtime-overlays.test.js`
Expected: PASS

- [ ] **Step 5: Commit lifecycle integration**

```bash
git add src/recording gnome-extension/extension.js gnome-extension/session-state.js gnome-extension/runtime-overlays.js
git commit -m "feat: integrate GNOME webcam preview session lifecycle"
```

### Task 9: Full Verification and Reinstall

**Files:**
- Verify: `gnome-extension/tests/*.test.js`
- Verify: relevant Rust files changed in `src/recording/`

- [ ] **Step 1: Run all GNOME extension tests**

Run: `for f in gnome-extension/tests/*.test.js; do echo "== $f =="; gjs -m "$f" || exit 1; done`

Expected: all tests PASS

- [ ] **Step 2: Run targeted Rust tests for the webcam preview path**

Run: `cargo test webcam_preview -- --nocapture`

Expected: PASS for the preview-related Rust tests added in this work.

- [ ] **Step 3: Reinstall the GNOME extension**

Run:

```bash
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io
cp -r /home/codegoddy/Desktop/apexshot/gnome-extension/. ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io/
gnome-extensions disable apexshot-preview-helper@apexshot.github.io
gnome-extensions enable apexshot-preview-helper@apexshot.github.io
```

Expected: extension reloads with the live webcam preview implementation.

- [ ] **Step 4: Manual verification**

Check:
- live webcam frames appear in GNOME during recording
- styling matches the C++ webcam preview
- drag remains inside the recording rect
- changing size/shape/mirror/device mid-recording does not alter the active session
- next recording session picks up newly chosen webcam settings

- [ ] **Step 5: Commit final integration**

```bash
git add src/recording gnome-extension docs/superpowers/specs/2026-03-28-gnome-webcam-live-preview-design.md docs/superpowers/plans/2026-03-28-gnome-webcam-live-preview.md
git commit -m "feat: add GNOME live webcam preview during recording"
```
