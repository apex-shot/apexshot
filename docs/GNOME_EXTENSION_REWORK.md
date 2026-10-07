# GNOME Extension Rework — Response to EGO Rejection

Date: 2026-07-30
Reviewer feedback (JustPerfection, 2026-07-26): the code looked AI-generated, shexli
issues had to be fixed, unnecessary code removed, and timeout cleanup corrected.

This document records what changed, why, and how it was verified. Nothing here is
committed yet.

---

## 1. Scope decision

The old extension tried to be a second UI layer for ApexShot: a panel pill with a
timer and stop button, runtime overlays, a screenshot lock, visibility policies, and
a window-thumbnail service. Most of it was either dead or duplicated something the
app already did.

The extension now does three things, and nothing else:

| Service | D-Bus | Why the extension has to do it |
|---|---|---|
| Recording mask | `org.apexshot.ShellOverlay` — `ShowMask`, `HideMask` | A Wayland client cannot draw above every window |
| Window list | `org.apexshot.WindowList` — `GetWindows`, `ActivateWindowById` | A Wayland client cannot enumerate or focus other windows |
| Preview stacking | consumes `org.apexshot.TrackedWindow` signals | A Wayland client cannot raise itself |

Everything else was deleted on both sides of the D-Bus boundary.

---

## 2. Extension: files

### Deleted (8 modules, 4 test files)

`controls-ui.js` (813 lines), `controls-ui-layout.js`, `mask-ui.js`,
`runtime-overlays.js`, `runtime-overlays-visibility.js`, `screenshot-lock.js`,
`session-state.js`, `gnome-version.js`, plus `tests/controls-ui-layout.test.js`,
`tests/runtime-overlays.test.js`, `tests/screenshot-lock.test.js`,
`tests/session-state-runtime-support.test.js`.

### Shipped now (5 files)

```
extension.js         31 lines   enable() / disable() for the three services
shell-overlay.js                recording mask, St.Widget bands, no timers
window-list.js      ~145 lines  GetWindows + ActivateWindowById
preview-stacking.js             TrackedWindow signals → make_above()
metadata.json
```

`extension.js` went from 725 lines to 31 and now properly subclasses `Extension`
from `resource:///org/gnome/shell/extensions/extension.js`.

---

## 3. What was actually wrong

### Two live crashes

- `getRuntimeOverlaySupportMessage(...)` was called at `controls-ui.js:498` and
  `:603` but defined nowhere — a `ReferenceError` on every call.
- `this._pollPointerState()` was called from a **16 ms repeating timer** in
  `extension.js:642` and was never defined. That is roughly 60 exceptions per
  second for the whole session, and is almost certainly what the reviewer meant by
  "cleanup issues for the timeouts".

### Timer and signal leaks

- The 16 ms pointer poll served no purpose: the `_pointerButtons` state it wrote
  was never read anywhere.
- Two `GLib.idle_add` calls never stored their source ids, so they could not be
  removed.
- `_startControlsTimer` returned `SOURCE_REMOVE` without nulling
  `_controlsTimerSource`, so a later `source_remove()` hit a dead source and
  logged a GLib critical.
- `PreviewStackingHelper.disable()` cleared its map without disconnecting, leaking
  four `MetaWindow` handlers per tracked window.
- A `notify::title` handler was connected and never disconnected.
- A 50 ms watchdog re-applied `make_above()` / `stick()` / `unminimize()` forever.

`preview-stacking.js` replaces both 50 ms timers with `window-created` plus a
one-shot `notify::title` watcher per window, tracked in a Map so `disable()`
disconnects every handler. The only remaining timer is the short recording
countdown, which is removed when the countdown or extension ends.

### Dead code (roughly a third of the extension)

- `_buildControlsChrome()` (~115 lines) was never called, so `_controlsChrome` was
  permanently `null`. That dead-ended `reposition()`, all of
  `controls-ui-layout.js`, `_positionRuntimeOverlayMenu`, and the
  `registerSelfOwnedActor` WeakSet/WeakMap machinery in `session-state.js`.
- Every function in `runtime-overlays-visibility.js` ignored its arguments and
  returned a constant.
- `runtime-overlays.js` was empty functions plus a pointless `Clutter` re-export;
  its `shouldExcludeOverlayEvent` was imported but never called.
- `screenshot-lock.js` was a do-nothing stub.
- `CaptureWindowById` returned `false` unconditionally, and `thumbnail_b64` was
  always the empty string.

### Guideline violations

- The extension class did not extend `Extension`.
- `settings-schema` was declared with no `schemas/` directory.
- `gettext-domain` was declared with no translations.
- `Main.panel._rightBox.insert_child_at_index` reached into private shell
  internals instead of using `Main.panel.addToStatusArea`.
- Deprecated `log()` / `logError()` were used.
- `shell-version` claimed six major releases (45–50).

### AI tells

`// ------` banner dividers, empty `catch (_) {}` blocks, defensive
`typeof x === "function" ? … : fallback` chains, and a dated marker log reading
`"[apexshot] extension enable marker 2026-03-29T02:45Z"`.

### metadata.json

Dropped the bogus `settings-schema` and `gettext-domain`, narrowed `shell-version`
to `["48", "49", "50"]`, bumped to version 4, and rewrote the description to say
plainly what the extension does and that ApexShot must be installed.

---

## 4. Rust: matching cleanup

### `src/gnome_shell.rs`

Removed `RecordingControlsVisibilityPolicy`, `RecordingControlsSpec`,
`RecordingControlsHandle`, `ScreenshotLockHandle`, `show_recording_controls`,
`hide_recording_controls*`, `begin_screenshot_lock`, `end_screenshot_lock`,
`set_recording_paused`, `restart_recording_ui`, `end_recording_ui`,
`toggle_overlay_visibility`, and their argument builders. What remains is
`MaskHandle`, `show_recording_mask`, `hide_recording_mask`, the session-support
predicates, and `run_shell_overlay_method`. The test module is now three tests
covering mask gating and payload geometry.

### `src/recording/mod.rs`

- `PreparedOverlayRecordingRequest` lost `shell_controls_visibility_policy`,
  `runtime_overlay_snapshot`, and `use_shell_controls`.
- `RuntimeOverlaySnapshot` deleted.
- `run_recording_with_controls_with_runtime_overlay` collapsed into
  `run_recording_with_controls`; `run_recording_with_shell_controls` became
  `run_recording_with_shell_mask`, which is the native path plus the mask.
- Tests that asserted on the removed fields were dropped or retargeted at
  `use_shell_mask`.

### `src/recording/control_session.rs`

`notify_shell_overlay()` is gone — it existed only to call the four removed
functions. `session_id` is no longer threaded through side-effect handling, and
the unused `bus_name` field and accessor were removed with the C++ controls
window's launcher.

### Recording feedback (the part that matters for users)

Deleting the panel pill would have left GNOME with no visible recording state, so
two gates that existed only because the pill existed were removed:

- `src/daemon/mod.rs` — `should_show_recording_tray_controls()` returned
  `!current_session_supports_gnome_shell_overlay()`, which suppressed the tray's
  recording appearance on GNOME. `update_recording_tray_if_non_gnome` is now
  `update_recording_tray` and always forwards state.
- `src/recording/indicator_notify.rs` — `should_show_recording_indicator()`
  returned false whenever the extension was on the bus. Removed, so the
  click-to-stop notification runs everywhere.

What a GNOME user sees while recording now: the dimmed mask, a persistent
notification with a stop action, `Recording • 0:42` in the tray, and the usual
global shortcuts. The one real loss is the always-visible seconds counter in the
panel; the tray tooltip and notification carry it instead. Note the `ksni` tray
needs the AppIndicator extension on stock GNOME, which is why the notification
path was re-enabled rather than relying on the tray alone.

**Historical note:** The standalone `WindowPickerOverlay.cpp` / `.h` discussed below
were unused and have been removed. The live Quick Capture Window picker is in
`capture-overlay/src/CaptureOverlay_Window.cpp`; this historical account is retained.

### `capture-overlay/src/WindowPickerOverlay.cpp`

Deleted the `captureWindowThumbnail` lambda and its `resolveCapturedPath` /
`waitForCapturedPath` helpers (about 90 lines, including a 2.2 s busy-wait loop
with `QThread::msleep`). They called `CaptureWindowById`, which never worked. The
`thumbnail_b64` decode went too, since the field was always empty. Cards render as
titled placeholders. Six now-unused Qt includes removed.

---

## 5. Packaging, CI, docs

File lists that enumerated the deleted JS were updated in `packaging/arch/PKGBUILD`,
`packaging/fedora/apexshot.spec`, `packaging/opensuse/apexshot.spec`,
`scripts/fedora-reinstall.sh`, and the zip step in
`.github/workflows/release.yml`.

Docs corrected for the new module list, the 48–50 version range, and the recording
control story: `README.md` (extension is now "Recommended", not "Required"),
`CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/MODULES.md`,
`docs/DEVELOPER_GUIDE.md`, `gnome-extension/README.md`. `SUBMISSION_GUIDE.md` was
rewritten — it still said version 2 and GNOME 45–49, and it now includes the
pre-submission check commands.

---

## 6. Verification

| Check | Command | Result |
|---|---|---|
| shexli | `shexli /tmp/apexshot-ext.zip` | clean (0 findings, 0 errors, 0 warnings) |
| JS syntax | `node --check` on all 5 files + test | pass |
| Extension tests | `gjs -m gnome-extension/tests/window-list.test.js` | 5/5 pass |
| Rust build | `cargo build` | success, no warnings |
| Rust tests | `cargo test --lib` | 579 passed, 0 failed |
| Clippy | `cargo clippy --workspace --all-targets` | clean |
| Qt overlay | rebuilt through `build.rs` cmake step | success |

shexli needs a pinned tree-sitter or it segfaults, and it needs an absolute path:

```bash
uv tool install --force --with "tree-sitter==0.25.2" shexli
~/.local/bin/shexli /absolute/path/to/gnome-extension   # or a .zip
```

The gjs tests need the Mutter and Shell typelibs:

```bash
GI_TYPELIB_PATH=/usr/lib/x86_64-linux-gnu/mutter-18:/usr/lib/gnome-shell \
  gjs -m gnome-extension/tests/window-list.test.js
```

---

## 7. Before resubmitting

- Take fresh screenshots; the old ones show the panel pill, which no longer exists.
- Re-read the diff once with the reviewer's eyes, since "looks AI-generated" is a
  judgement about style as much as content.
- Confirm the mask, window picker, and preview stacking behave correctly in a live
  GNOME session — all verification so far is static plus unit tests.
