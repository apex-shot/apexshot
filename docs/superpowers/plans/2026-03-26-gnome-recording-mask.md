# GNOME Recording Mask Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a GNOME Wayland recording mask that keeps the selected recording region clear and dims the rest of the desktop during active recording while ApexShot keeps using its own selector, recorder, and controls bar.

**Architecture:** Extend the existing GNOME Shell extension so it exposes a tiny D-Bus API for showing and hiding a shell-managed dim mask. Add a Rust adapter that calls that API from the recording lifecycle and falls back cleanly when the extension is unavailable.

**Tech Stack:** Rust, Tokio, GTK4, GStreamer/PipeWire, GNOME Shell extension JS, Gio/GLib D-Bus

---

## File Map

- Modify: `gnome-extension/extension.js`
  Add a second GNOME extension responsibility: export a D-Bus object for recording-mask control and render/remove shell overlay actors for the selected rectangle.
- Modify: `gnome-extension/README.md`
  Document the new recording-mask behavior, D-Bus role, install/enable flow, and troubleshooting.
- Modify: `gnome-extension/metadata.json`
  Rename/describe the extension so it covers both preview stacking and recording-mask support.
- Create: `src/gnome_shell.rs`
  Hold all GNOME-specific D-Bus client logic for `ShowMask` / `HideMask`, environment detection, geometry mapping from existing recording requests, and best-effort logging/fallbacks.
- Modify: `src/lib.rs`
  Register the new Rust module at the crate root so the recording code can import it.
- Modify: `src/main.rs`
  Keep existing GTK work routing intact; only touch if the binary needs explicit imports after the new module is added.
- Modify: `src/recording/mod.rs`
  Call the GNOME mask adapter immediately before recording starts and guarantee hide-on-stop/hide-on-error cleanup.
- Modify: `src/recording/stop_overlay.rs`
  Add an explicit `use_shell_mask` / `disable_dim_windows` control-path flag so GNOME Wayland recording can bypass local dim windows while keeping the controls bar logic intact.
- Test: `src/recording/mod.rs`
  Add focused unit tests around recording-mask decision logic and cleanup hooks where possible.

### Task 1: Add GNOME Shell Mask API

**Files:**
- Modify: `gnome-extension/extension.js`
- Modify: `gnome-extension/metadata.json`

- [ ] **Step 1: Write the failing contract notes in the extension file**

Add inline TODO-style comments near the D-Bus constants describing the new interface:

```js
const MASK_DBUS_NAME = 'org.apexshot.ShellOverlay';
const MASK_DBUS_PATH = '/org/apexshot/ShellOverlay';
const MASK_DBUS_IFACE = 'org.apexshot.ShellOverlay';
```

Expected outcome: the extension file clearly identifies the new API surface before implementation.

- [ ] **Step 2: Implement the D-Bus object skeleton**

In `gnome-extension/extension.js`, add a small exported object with:

```js
ShowMask(x, y, width, height) {}
HideMask() {}
```

using Gio D-Bus exported object plumbing and introspection XML.

- [ ] **Step 3: Implement the shell-owned overlay actor**

Add minimal shell-stage overlay management in `gnome-extension/extension.js`:

```js
this._maskGroup = new St.Widget({ reactive: false });
```

Build four dim regions from the full stage bounds and the supplied rectangle. Keep the selected recording area uncovered.

- [ ] **Step 4: Wire lifecycle cleanup**

Ensure `disable()`:

```js
this._hideMask();
this._unexportMaskDbus();
```

cleans up actors and D-Bus exports even if recording is active.

- [ ] **Step 5: Update extension metadata**

Change `gnome-extension/metadata.json` name/description so the extension describes both preview stacking and recording-mask support.

- [ ] **Step 6: Verify extension files are syntactically coherent**

Run: `gjs -m gnome-extension/extension.js`

Expected: the file parses successfully, or at minimum fails only because GNOME Shell globals are unavailable outside the shell process rather than on JavaScript syntax.

- [ ] **Step 7: Commit**

```bash
git add gnome-extension/extension.js gnome-extension/metadata.json
git commit -m "feat: add gnome shell recording mask api"
```

### Task 2: Add Rust GNOME Adapter

**Files:**
- Create: `src/gnome_shell.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the failing unit test scaffold for environment gating**

Add a small unit-testable helper in `src/gnome_shell.rs` with tests like:

```rust
#[test]
fn gnome_mask_disabled_outside_gnome_wayland() {
    assert!(!should_use_gnome_shell_mask("x11", "gnome"));
}
```

Expected: tests define when the GNOME path should and should not activate.

- [ ] **Step 2: Run the focused test and confirm it fails**

Run: `cargo test gnome_mask_disabled_outside_gnome_wayland -- --nocapture`

Expected: FAIL because `src/gnome_shell.rs` and the helper do not exist yet.

- [ ] **Step 3: Implement the adapter module**

Create `src/gnome_shell.rs` with:

```rust
pub fn should_use_gnome_shell_mask(
    wayland_display: Option<&str>,
    desktop: Option<&str>,
) -> bool

pub struct RecordingMaskGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn geometry_from_request(request: &RecordingRequest) -> RecordingMaskGeometry

pub fn show_recording_mask(geometry: RecordingMaskGeometry) -> anyhow::Result<MaskHandle>
```

and a `MaskHandle` that hides the mask on drop or via an explicit `hide()` call.

- [ ] **Step 4: Add D-Bus call plumbing**

Use `gio`/`glib` or the project’s existing D-Bus stack to call:

```text
org.apexshot.ShellOverlay.ShowMask
org.apexshot.ShellOverlay.HideMask
```

Treat failures as non-fatal and return a best-effort handle.

- [ ] **Step 5: Register the module**

Update `src/lib.rs` module declarations so `src/gnome_shell.rs` is available to the recording code without leaking GNOME-specific details across the codebase. Touch `src/main.rs` only if imports need to be adjusted afterward.

- [ ] **Step 6: Run focused tests**

Run: `cargo test gnome_mask_ -- --nocapture`

Expected: PASS for the new gating/unit tests.

- [ ] **Step 7: Commit**

```bash
git add src/gnome_shell.rs src/lib.rs src/main.rs
git commit -m "feat: add gnome shell mask adapter"
```

### Task 3: Wire Mask Show/Hide Into Recording Lifecycle

**Files:**
- Modify: `src/recording/mod.rs`
- Modify: `src/recording/stop_overlay.rs`

- [ ] **Step 1: Write the failing lifecycle tests**

Add tests in `src/recording/mod.rs` covering:

```rust
#[test]
fn overlay_recording_uses_gnome_mask_only_for_gnome_wayland_area_recording() {}

#[test]
fn overlay_recording_cleans_up_mask_when_recording_finishes() {}
```

Expected: tests describe the intended decision and cleanup flow before implementation.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run: `cargo test overlay_recording_uses_gnome_mask -- --nocapture`

Expected: FAIL because the recording lifecycle does not yet track a shell mask handle.

- [ ] **Step 3: Show the mask before recording starts**

In `src/recording/mod.rs`, after countdown and before the actual recording pipeline starts, call the GNOME adapter when:

```rust
request.mode == record
&& selected area is not empty
&& should_use_gnome_shell_mask(...)
```

- [ ] **Step 4: Thread shell-mask intent into controls params**

Add an explicit flag to `RecordingControlsParams`, for example:

```rust
pub use_shell_mask: bool
```

and set it in `prepare_overlay_recording_request(...)` so the controls-overlay code knows when to skip local dim windows.

- [ ] **Step 5: Hide the mask on every exit path**

Guarantee cleanup on:

```rust
Ok((path, StopAction::Save))
Ok((path, StopAction::Discard))
Err(_)
```

Prefer scoped ownership so cleanup happens automatically even on early returns.

- [ ] **Step 6: Remove remaining app-window mask behavior for GNOME Wayland**

In `src/recording/stop_overlay.rs`, gate `setup_dim_windows(...)` on the new controls flag so GNOME Wayland recording never creates local dim windows when the shell mask path is active. The controls bar should remain, but the shell extension becomes the only dim-mask provider.

- [ ] **Step 7: Run focused recording tests**

Run: `cargo test prepare_overlay_recording_request -- --nocapture`

Run: `cargo test overlay_recording_ -- --nocapture`

Expected: new targeted tests PASS. If unrelated pre-existing tests fail elsewhere, note them separately and do not conflate them with this feature.

- [ ] **Step 8: Commit**

```bash
git add src/recording/mod.rs src/recording/stop_overlay.rs
git commit -m "feat: show gnome mask during recording"
```

### Task 4: Documentation and Manual Verification

**Files:**
- Modify: `gnome-extension/README.md`
- Modify: `docs/superpowers/specs/2026-03-26-gnome-recording-mask-design.md` (only if implementation forces a design update)

- [ ] **Step 1: Update extension documentation**

Document:

```md
- preview stacking support
- recording-mask support
- required extension UUID
- enable command
- D-Bus troubleshooting
```

- [ ] **Step 2: Build the Rust binary**

Run: `cargo build --release`

Expected: PASS.

- [ ] **Step 3: Install the Rust binary**

Run:

```bash
sudo install -m 755 /home/codegoddy/Desktop/apexshot/target/release/apexshot /usr/local/bin/apexshot
```

Expected: install succeeds.

- [ ] **Step 4: Install the GNOME extension files**

Run:

```bash
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io
cp -r gnome-extension/* ~/.local/share/gnome-shell/extensions/apexshot-preview-helper@apexshot.github.io/
gnome-extensions enable apexshot-preview-helper@apexshot.github.io
```

Expected: extension is enabled with the updated code.

- [ ] **Step 5: Restart GNOME Shell session as needed**

On Ubuntu GNOME Wayland, log out/in if hot reload is insufficient.

Expected: the extension process is active and ready to receive D-Bus calls.

- [ ] **Step 6: Run manual recording verification**

Verify:

```text
1. Area recording dims the desktop outside the selected rect.
2. The selected rect remains clear.
3. The controls bar remains visible.
4. Stop/delete removes the mask.
5. Disabling the extension falls back to controls-bar-only mode.
```

- [ ] **Step 7: Commit**

```bash
git add gnome-extension/README.md
git commit -m "docs: document gnome recording mask setup"
```

## Final Verification

- [ ] Run: `cargo build`
- [ ] Run: `cargo build --release`
- [ ] Run: `cargo test gnome_mask_ -- --nocapture`
- [ ] Run: `cargo test overlay_recording_ -- --nocapture`
- [ ] Run: `git status --short`
- [ ] Confirm only intended files changed before merge or further feature work
