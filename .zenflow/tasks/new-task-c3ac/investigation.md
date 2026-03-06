# Investigation

## Bug summary
- Reported behavior: running `cargo run -- daemon` does not make the app respond to hotkeys after the project files were copied into a new directory.
- Observed architecture: the active application entrypoint is the Rust binary in `src/main.rs`; the C++ UI code under `ksnip/` exists in the repo, but it is not part of the Cargo binary that provides the daemon and hotkey flow.
- No runtime logs were provided, so this investigation is based on the current code paths.

## Root cause analysis

### Primary cause
On **GNOME**, starting the daemon is **not sufficient to register desktop hotkeys**.

Evidence from the code:
- `src/daemon/mod.rs` explicitly skips daemon-side hotkey registration on GNOME and assumes hotkeys are provided by **GNOME custom keybindings + D-Bus IPC**.
- The daemon logs `GNOME detected — hotkeys via gsettings custom keybindings + D-Bus IPC.` and does not call the fallback listener in that branch.
- Hotkey installation on GNOME is handled separately by `cargo run -- hotkeys setup` or `cargo run -- hotkeys install` in `src/main.rs` and `src/hotkeys/mod.rs`.

That means a user can run `cargo run -- daemon`, see the daemon come up, and still have **zero active system hotkeys** unless the GNOME keybindings were installed beforehand.

### Likely path-move regression
The repo move strongly suggests a second issue: **installed GNOME keybindings can become stale after copying the project to a new directory**.

Evidence from the code:
- `install_gnome_custom_keybindings()` resolves the current executable path with `resolve_action_exe()`.
- It writes a GNOME custom-keybinding `command` using `gnome_binding_command(&action_exe, &binding.args)`.
- GNOME custom keybindings therefore store an **absolute executable path**.

If keybindings were installed while the binary lived in the old directory, GNOME may still be trying to launch the old path. Running the daemon from the new directory does **not** refresh those bindings automatically.

### Additional finding
The daemon messaging is slightly misleading for GNOME:
- `src/daemon/mod.rs` prints `Ready. Listening for hotkeys and tray events.`
- But on GNOME, that does **not** imply it has registered hotkeys itself; it is only ready to receive D-Bus actions from external keybindings.

## Affected components
- `src/daemon/mod.rs`
  - GNOME branch bypasses in-process hotkey listener and assumes external GNOME keybindings.
  - Startup logging can suggest hotkeys are active even when they are not installed.
- `src/hotkeys/mod.rs`
  - GNOME keybinding installation writes absolute commands tied to the current executable path.
  - Moving the repo/build output can invalidate previously installed bindings.
- `src/main.rs`
  - Hotkey setup/install is a separate command path from `daemon`, so users can start the daemon without actually installing hotkeys.
- `ksnip/`
  - Contains C++ hotkey/UI code, but it is not the code path used by the Rust daemon launched with Cargo.

## Existing test coverage
- I did not find regression tests covering the daemon/hotkey setup flow.
- Current tests appear focused on backend capture behavior, not desktop hotkey registration.

## Proposed solution
1. **Short-term fix path**
   - Reinstall/sync GNOME hotkeys after a repo move by running the existing GNOME hotkey install flow again.
   - If the project is meant to be used from a stable path, prefer the existing install path that copies the binary to `/usr/local/bin/apexshot` so GNOME bindings do not depend on a temporary Cargo build location.

2. **Code fix to implement next**
   - Add a GNOME startup check in the daemon that verifies whether ApexShot custom keybindings exist and whether their stored `command` matches the current executable path.
   - If bindings are missing or stale, either:
     - automatically reinstall/update them, or
     - fail loudly with a clear message telling the user to run `hotkeys install/setup`.

3. **UX fix**
   - Adjust daemon startup logging on GNOME so it does not claim to be listening for hotkeys unless the GNOME keybindings are actually present and point to the current executable.

4. **Regression coverage**
   - Add tests around command generation / stale-path detection in `src/hotkeys/mod.rs`.
   - Add a focused integration-style test for the GNOME setup decision path where possible.

## Recommended implementation direction
The safest implementation is to keep the existing GNOME architecture (custom keybindings -> command -> D-Bus -> daemon), but add:
- binding presence validation,
- stale command-path detection,
- and clearer startup behavior/errors.

This matches the current design instead of trying to switch GNOME back to direct daemon-side grabs.

## Implementation notes
- Added GNOME keybinding validation in `src/hotkeys/mod.rs` that checks whether the managed GNOME custom-keybinding entries exist and whether their stored `command` and `binding` values still match the current executable path and configured accelerator.
- Added `sync_gnome_hotkeys_for_current_desktop()` so daemon startup can automatically repair stale or missing GNOME bindings after the repo or build output moves.
- Updated `src/daemon/mod.rs` so the GNOME daemon path validates/syncs custom keybindings on startup and prints a more accurate readiness message for the GNOME custom-keybinding + D-Bus flow.
- Added regression tests covering the expected GNOME binding snapshot and the stale-command-path case caused by moving the project.
- Verification initially failed because the copied repo still had stale generated CMake cache files under `target/` pointing at the old path (`/home/codegoddy/Desktop/apexshot`). Running `cargo clean` cleared the stale build artifacts and allowed verification to pass.

## Test results
- `cargo fmt` ✅
- `cargo test gnome_binding_snapshot` ✅
- `cargo check` ✅

## Preview latency follow-up
- User report: after a screenshot is captured, the preview overlay still appears noticeably late on GNOME Wayland even after deferring preview thumbnail decoding.
- Runtime logs showed the slower path was earlier in the pipeline: the C++ capture helper already writes a PNG file, but the daemon was still decoding that PNG into `CaptureData`, re-encoding it to the final output file, and then spawning the preview process which decoded it again.
- Updated `src/daemon/mod.rs` to use the new file-preserving C++ capture helpers (`capture_area_file_via_cpp`, `capture_screen_file_via_cpp`, `capture_window_file_via_cpp`) across the direct area/screen/window handlers and the window-picker sentinel paths.
- The daemon now routes those PNGs through `save_existing_png_and_open()` so PNG captures can be moved into the final save location and previewed without the extra decode/re-encode cycle.
- OCR-triggered area captures still decode into `CaptureData` because OCR needs pixel access, and Rust backend fallback paths are unchanged.

## Additional verification
- `cargo fmt` ✅
- `cargo test gnome_binding_snapshot --lib` ✅
- `cargo check` ✅
