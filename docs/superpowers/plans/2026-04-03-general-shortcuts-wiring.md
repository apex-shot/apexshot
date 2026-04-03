# General Shortcuts Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the unwanted general shortcut rows and wire the remaining general shortcut rows to real daemon actions.

**Architecture:** Keep the settings/config plumbing for the supported rows, remove the two unwanted rows from the shortcuts UI, extend hotkey config generation with four real bindings, and add daemon actions that reuse existing last-capture behavior plus minimal new state/hooks for clipboard import, overlay restore, and overlay visibility toggling.

**Tech Stack:** Rust, GTK4, ksni tray daemon, zbus D-Bus IPC, existing ApexShot settings/hotkey/daemon modules

---

### Task 1: Remove unwanted shortcut rows from the settings UI

**Files:**
- Modify: `src/settings/shortcuts.rs`
- Modify: `src/settings/mod.rs`
- Modify: `src/settings/actions.rs`
- Test: `src/settings/shortcuts.rs`

- [ ] Add/adjust a failing shortcuts UI test that asserts `Toggle Desktop Icons` and `Pin to the Screen` are absent while `Open File`, `Open From Clipboard`, `Restore Recently Closed File`, and `Hide/Show Overlays` remain.
- [ ] Run: `cargo test shortcuts_section_ -- --nocapture` and verify the new assertion fails for the removed rows before implementation.
- [ ] Remove the two UI rows and stop exposing their buttons through `ShortcutSettingsWidgets`, `settings/mod.rs`, and `SaveInputs`.
- [ ] Run the same test again and verify it passes.

### Task 2: Emit real hotkey bindings for the four supported general shortcuts

**Files:**
- Modify: `src/hotkeys/mod.rs`
- Test: `src/hotkeys/mod.rs`

- [ ] Add failing tests for `hotkey_config_from_app_config()` proving non-empty values for `shortcut_open_file`, `shortcut_open_from_clipboard`, `shortcut_restore_recently_closed`, and `shortcut_toggle_overlays` produce bindings with stable names/args.
- [ ] Run the focused hotkey test command and verify it fails because those bindings are missing.
- [ ] Add the four `push_binding(...)` calls with explicit names/args.
- [ ] Re-run the focused hotkey test and verify it passes.

### Task 3: Map the new bindings to daemon actions

**Files:**
- Modify: `src/daemon/mod.rs`
- Test: `src/daemon/mod.rs`

- [ ] Add failing unit tests for `binding_to_daemon_action()` covering both name-based and args-based mapping for `open_file`, `open_from_clipboard`, `restore_recently_closed`, and `toggle_overlays`.
- [ ] Run the focused daemon mapping tests and verify they fail before implementation.
- [ ] Add `DaemonAction` variants and extend `binding_to_daemon_action()` accordingly.
- [ ] Re-run the focused daemon mapping tests and verify they pass.

### Task 4: Implement runtime behavior for the four actions

**Files:**
- Modify: `src/daemon/mod.rs`
- Inspect/reuse: existing last-capture open path, clipboard import helpers, overlay lifecycle hooks
- Test: `src/daemon/mod.rs` or nearest existing focused unit-test module

- [ ] Add one failing test at a time for each extractable policy/helper:
  - open-file chooses the last capture path when present
  - clipboard import without image requests an error notification path
  - restore-recently-closed only runs when a closable overlay/file exists
  - overlay visibility toggle flips state deterministically
- [ ] Run each focused test and verify it fails for the expected reason.
- [ ] Implement the minimal helper/state needed in the daemon to support the action loop.
- [ ] Re-run each focused test and verify it passes before moving to the next helper.

### Task 5: Hook manual/runtime-only paths that cannot be unit-proven cleanly

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: any overlay/preview module discovered during Task 4 only if required

- [ ] Wire the daemon action loop so the new actions call the corresponding runtime helpers.
- [ ] Add minimal notification text for clipboard-no-image failure.
- [ ] Keep the change surface minimal: reuse existing preview/open code and only add state/hooks required for restore/toggle.
- [ ] Run `cargo check` and fix compile issues.

### Task 6: Verification

**Files:**
- No new files unless a tiny test helper is required

- [ ] Run focused tests for shortcuts UI, hotkeys, and daemon mapping/helpers.
- [ ] Run `cargo check`.
- [ ] Manually verify in the worktree build:
  - Open File opens the most recent capture/save target
  - Open From Clipboard imports an image and shows an error notification when clipboard has no image
  - Restore Recently Closed File restores the last closed floating overlay/file view
  - Hide/Show Overlays toggles ApexShot overlays

### Task 7: Commit

**Files:**
- Commit the touched settings/hotkeys/daemon files plus spec/plan docs

- [ ] `git add src/settings/shortcuts.rs src/settings/mod.rs src/settings/actions.rs src/hotkeys/mod.rs src/daemon/mod.rs docs/superpowers/specs/2026-04-03-shortcuts-general-actions-design.md docs/superpowers/plans/2026-04-03-general-shortcuts-wiring.md`
- [ ] `git commit -m "feat: wire general shortcuts actions"`
