# C++ Recording Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Rust-owned recording controls with a C++ controls bar in `apexshot-capture`, while Rust keeps the recording engine and executes control commands from the C++ UI.

**Architecture:** Add a dedicated C++ `record-controls` mode plus a minimal Rust recording-session control endpoint. Start with stop/discard control and timer ownership, then add pause/resume and restart before removing the Rust controls path from overlay recording.

**Tech Stack:** Rust, Tokio, GTK4, GStreamer/PipeWire, C++/Qt, local IPC

---

## File Map

- Modify: `capture-overlay/src/main.cpp`
  Add a dedicated `record-controls` mode and argument parsing for session control.
- Modify: `capture-overlay/src/CaptureOverlay*.{h,cpp}`
  Reuse visual language and shared positioning/countdown concepts where helpful, but keep record-controls as a dedicated path.
- Create: `capture-overlay/src/RecordingControlsWindow.h`
  Define the dedicated C++ recording controls window.
- Create: `capture-overlay/src/RecordingControlsWindow.cpp`
  Implement layout, timer display, button interactions, and IPC command emission.
- Modify: `src/recording/mod.rs`
  Launch the C++ controls process for overlay recording instead of the Rust controls bar.
- Modify: `src/gnome_shell.rs`
  Keep shell-mask lifecycle coordinated with the new C++ controls process.
- Modify: `src/daemon/mod.rs`
  Remove daemon dependence on the Rust controls bar path for overlay recording once the C++ path is wired.
- Modify: `src/main.rs`
  Keep only the minimal control-launch plumbing required by the new architecture.
- Modify: `src/recording/stop_overlay.rs`
  Decommission the Rust controls UI path once the C++ replacement is verified.

### Task 1: Introduce C++ Record Controls Mode

**Files:**
- Modify: `capture-overlay/src/main.cpp`
- Create: `capture-overlay/src/RecordingControlsWindow.h`
- Create: `capture-overlay/src/RecordingControlsWindow.cpp`

- [ ] **Step 1: Write the dedicated C++ controls class skeleton**

Create `RecordingControlsWindow` with constructor inputs for:

```cpp
session id
capture geometry
fullscreen flag
initial paused/running state
```

- [ ] **Step 2: Implement the requested visual layout**

Match the requested control order:

```text
stop + timer | pause | restart | delete | menu
```

- [ ] **Step 3: Implement live timer rendering in C++**

Add a timer that:

```cpp
starts at recording_started
freezes on pause
resumes on resume
resets on restart
```

- [ ] **Step 4: Add the new `record-controls` mode to `main.cpp`**

Parse arguments and launch `RecordingControlsWindow` instead of the area-init overlay.

- [ ] **Step 5: Build the C++ binary**

Run: `cmake --build capture-overlay/build`

Expected: PASS.

### Task 2: Add Minimal Rust Control Endpoint

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Define the control command model**

Introduce a small Rust enum for:

```rust
Pause
Resume
Restart
Stop
Discard
```

- [ ] **Step 2: Add a session-local control endpoint**

Implement a minimal control transport for one active recording session at a time.

- [ ] **Step 3: Support stop and discard first**

Map the incoming commands onto the current recording lifecycle safely.

- [ ] **Step 4: Build Rust**

Run: `cargo build`

Expected: PASS.

### Task 3: Launch C++ Controls Instead of Rust Controls

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `src/recording/stop_overlay.rs`

- [ ] **Step 1: Launch `apexshot-capture` in `record-controls` mode after recording starts**

Pass geometry/fullscreen/session metadata into the C++ process.

- [ ] **Step 2: Stop using the Rust controls window for overlay recording**

Keep the old path only as temporary fallback until verified.

- [ ] **Step 3: Preserve shell-mask lifecycle**

Make sure controls launch and exit do not leave the mask stuck on.

- [ ] **Step 4: Build and verify**

Run:

```bash
cargo build
cargo build --release
cmake --build capture-overlay/build
```

Expected: PASS.

### Task 4: Add Pause/Resume and Restart Behavior

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `capture-overlay/src/RecordingControlsWindow.cpp`

- [ ] **Step 1: Add pause/resume control handling**

Wire the pause button to actual pipeline pause/resume behavior.

- [ ] **Step 2: Add restart handling**

Restart must discard the current file and begin a fresh session with the same settings.

- [ ] **Step 3: Reset C++ timer state correctly**

Pause freezes, resume continues, restart returns to `0:00`.

- [ ] **Step 4: Manual verify behavior**

Check:

```text
pause works
resume works
restart works
timer behaves correctly
```

### Task 5: Remove Rust Controls UI From This Flow

**Files:**
- Modify: `src/recording/stop_overlay.rs`
- Modify: `src/main.rs`
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: Remove overlay-recording dependence on the Rust controls UI**

Keep only fallback code if absolutely necessary.

- [ ] **Step 2: Confirm no fullscreen Rust countdown remains in overlay recording**

The C++ overlay/countdown path should be the only pre-start countdown path.

- [ ] **Step 3: Final verification**

Run:

```bash
cargo build
cargo build --release
cmake --build capture-overlay/build
git status --short
```

Expected: PASS with only intended files changed.
