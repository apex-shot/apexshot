# Quick Access Overlay Settings Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the floating screenshot preview obey the Quick Access settings for side, GNOME-extension topmost integration, size, and close behavior without changing the current default left placement or the current baseline preview size.

**Architecture:** Keep the existing GTK preview overlay in `src/capture/preview_overlay.rs`, but add a small config-to-runtime translation layer so the overlay reads one sanitized quick-access runtime config and applies it consistently to both layer-shell and fallback paths. Wire the missing settings out of the GTK settings window, migrate overlay-size semantics so the slider midpoint means “current size”, and cover the non-UI logic with unit tests around pure helper functions.

**Tech Stack:** Rust, GTK4, gtk4-layer-shell, x11rb, serde_yml, cargo test

---

### Task 1: Normalize and migrate Quick Access config values

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs`

**Step 1: Write the failing tests**

Add tests for:
- invalid `quick_access_position` falls back to `"Left"`
- `quick_access_position` only accepts `"Left"` and `"Right"`
- legacy `quick_access_overlay_size: 0.5` is migrated to the new baseline value where the midpoint equals the current preview size
- new overlay-size values are clamped to the supported range

Suggested test names:
- `sanitize_quick_access_position_rejects_unsupported_values`
- `sanitize_migrates_legacy_quick_access_overlay_size`
- `sanitize_clamps_quick_access_overlay_size`

**Step 2: Run tests to verify they fail**

Run: `cargo test sanitize_quick_access -- --nocapture`
Expected: FAIL because the current sanitizer still allows `Top`/`Bottom` and still clamps overlay size to `0.0..=1.0`.

**Step 3: Write the minimal implementation**

In `src/config.rs`:
- change quick-access position sanitization to only allow `Left` and `Right`
- change quick-access overlay-size sanitization to the new runtime range
- preserve current visual size by migrating legacy values so old default `0.5` maps to the new midpoint baseline
- keep `Left` as the fallback default so current installs stay on the existing side unless the user changes it

**Step 4: Run tests to verify they pass**

Run: `cargo test sanitize_quick_access -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "fix: normalize quick access config semantics"
```

### Task 2: Wire the Quick Access settings tab into saved config

**Files:**
- Modify: `src/settings/quick_access.rs`
- Modify: `src/settings/mod.rs`
- Modify: `src/settings/actions.rs`

**Step 1: Update the settings UI inputs**

In `src/settings/quick_access.rs`:
- limit the Position combo box to `Left` and `Right`
- add vertical tick marks to the overlay-size slider
- set the slider midpoint to represent the current preview size baseline
- keep the baseline visual size unchanged

**Step 2: Extend the save-input wiring**

In `src/settings/actions.rs` and `src/settings/mod.rs`:
- add missing Quick Access widgets to `SaveInputs`
- wire `position`, `multi_display`, `overlay_size`, `auto_close_enabled`, `auto_close_action`, `auto_close_interval`, `close_after_dragging`, and `close_after_uploading`
- save all of them into `AppConfig`

**Step 3: Keep widget sensitivity correct**

In `src/settings/actions.rs`:
- keep `Action` and `Interval` disabled when auto-close is off
- do not disable the behavior toggles when auto-close is off, because they override the default pinned state

**Step 4: Verify manually**

Run the settings window through the normal app flow, save changes, then inspect the config file at `~/.config/apexshot/config.yml`.
Expected:
- changing left/right updates `quick_access_position`
- toggling Show on all displays updates `quick_access_multi_display`
- moving the size slider updates `quick_access_overlay_size`
- behavior toggles persist correctly

**Step 5: Commit**

```bash
git add src/settings/quick_access.rs src/settings/mod.rs src/settings/actions.rs
git commit -m "feat: persist quick access settings"
```

### Task 3: Apply Quick Access settings in the preview overlay runtime

**Files:**
- Modify: `src/capture/preview_overlay.rs`
- Test: `src/capture/preview_overlay.rs`
- Reference: `src/gnome_integration/mod.rs`

**Step 1: Write the failing tests**

Add pure helper tests for:
- left/right positioning for both layer-shell and fallback layout decisions
- overlay-size scaling where midpoint means current size
- auto-close disabled => preview starts pinned
- `quick_access_multi_display = false` disables GNOME extension signaling only

Suggested helper surface:
- `preview_dimensions(scale: f64) -> (i32, i32)`
- `preview_side(position: &str) -> PreviewSide`
- `should_emit_extension_events(multi_display: bool, layer_shell_active: bool) -> bool`
- `initial_preview_pinned(auto_close_enabled: bool) -> bool`

Suggested test names:
- `preview_dimensions_keep_current_size_at_midpoint`
- `preview_side_resolves_left_and_right`
- `preview_extension_signals_follow_multi_display_setting`
- `preview_starts_pinned_when_auto_close_is_disabled`

**Step 2: Run tests to verify they fail**

Run: `cargo test preview_ -- --nocapture`
Expected: FAIL because the runtime is still hardcoded to bottom-left, fixed-size, and unconditional extension signaling.

**Step 3: Write the minimal implementation**

In `src/capture/preview_overlay.rs`:
- load config once near the start of `setup_preview_window`
- compute runtime width/height from the new scale semantics while keeping the current visual size at the slider midpoint
- apply side selection to both `configure_window_positioning` and fallback card alignment/margins
- preserve the current height/bottom offset logic
- gate GNOME extension `PreviewOpened/PreviewClosed` emission on `quick_access_multi_display`, but keep layer-shell/X11 app-provided topmost behavior unchanged
- start pinned when auto-close is disabled
- when auto-close is enabled, use `quick_access_auto_close_interval`
- make behavior toggles (`close_after_dragging`, `close_after_uploading`) override the default pinned state
- implement `Close` vs `Hide` as runtime dismiss actions for auto-close/behavior-triggered dismissal paths

**Step 4: Run tests to verify they pass**

Run: `cargo test preview_ -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/capture/preview_overlay.rs
git commit -m "feat: apply quick access overlay settings"
```

### Task 4: End-to-end verification

**Files:**
- Verify: `src/capture/preview_overlay.rs`
- Verify: `src/settings/quick_access.rs`
- Verify: `src/config.rs`

**Step 1: Run targeted tests**

Run: `cargo test sanitize_quick_access preview_ -- --nocapture`
Expected: PASS

**Step 2: Run the project tests/build**

Run: `cargo test`
Expected: PASS

**Step 3: Manual smoke-check the feature**

Verify through the normal screenshot flow:
- default preview still appears on the current left-side placement at the existing height
- switching to `Right` moves the overlay to the right without changing vertical placement
- disabling `Show on all displays` stops GNOME extension signaling but does not remove app-provided topmost behavior
- slider midpoint matches today’s preview size
- disabling auto-close makes the preview start pinned
- dragging/uploading still dismisses when the behavior toggle is enabled
- auto-close timeout uses the Quick Access interval when enabled

**Step 4: Commit**

```bash
git add src/config.rs src/settings/quick_access.rs src/settings/mod.rs src/settings/actions.rs src/capture/preview_overlay.rs
git commit -m "feat: wire quick access overlay behavior to settings"
```
