# Background Daemon And Settings Launch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ApexShot behave like a complete desktop app: first launch runs onboarding, normal launcher opens the settings UI, the daemon stays running in the background for hotkeys/capture flows, and tray visibility follows the `show_menu_bar_icon` setting.

**Architecture:** Keep the UI app and background daemon as separate runtime modes, but make the main launcher orchestrate them. A no-argument launch should ensure the daemon is running with the current config, then open onboarding or settings. The daemon remains the owner of tray actions, hotkeys, screenshot UI, and recording UI, while settings remains the place where background behavior is configured.

**Tech Stack:** Rust, GTK4, zbus/D-Bus, ksni tray, desktop/autostart `.desktop` entries, cargo test.

---

## File Map

**Modify**
- `src/main.rs`
  Launcher orchestration for no-argument startup, daemon bootstrap, and UI routing.
- `src/daemon/mod.rs`
  Reuse or extend daemon process detection/start helpers used by the launcher and settings.
- `src/settings/actions.rs`
  Keep save behavior aligned with the new launcher-managed daemon model.
- `src/settings/windowing.rs`
  Autostart desktop entry content and icon/app identity consistency.
- `src/onboarding/mod.rs`
  Optional onboarding completion handoff if the first-run flow should ensure the daemon is active before closing.
- `packaging/apexshot.desktop`
  Preserve “open settings” semantics while the launcher also ensures the daemon is running.
- `Cargo.toml`
  Version bump for the release carrying this behavior.

**Create**
- `tests/launcher_daemon_behavior.rs`
  Focused regression tests for no-argument launch behavior and daemon bootstrap decisions.

**Existing Tests To Extend Or Consult**
- `tests/desktop_identity.rs`
- `tests/package_metadata.rs`
- `src/settings/actions.rs` existing unit tests around daemon respawn behavior

---

### Task 1: Define Launcher Behavior For First Run Vs Normal Launch

**Files:**
- Modify: `src/main.rs`
- Test: `tests/launcher_daemon_behavior.rs`

- [ ] **Step 1: Write the failing tests for no-argument launch decisions**

```rust
#[test]
fn no_arg_launch_runs_onboarding_when_not_complete_and_requests_daemon_bootstrap() {
    let plan = decide_no_arg_launch(false, false);

    assert_eq!(plan.ui, LaunchUi::Onboarding);
    assert!(plan.ensure_daemon_running);
}

#[test]
fn no_arg_launch_runs_settings_when_onboarding_is_complete_and_requests_daemon_bootstrap() {
    let plan = decide_no_arg_launch(true, false);

    assert_eq!(plan.ui, LaunchUi::Settings);
    assert!(plan.ensure_daemon_running);
}

#[test]
fn no_arg_launch_does_not_restart_daemon_when_already_running() {
    let plan = decide_no_arg_launch(true, true);

    assert_eq!(plan.ui, LaunchUi::Settings);
    assert!(!plan.ensure_daemon_running);
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `cargo test --test launcher_daemon_behavior`

Expected: FAIL because `decide_no_arg_launch`, `LaunchUi`, and the no-argument launcher behavior abstraction do not exist yet.

- [ ] **Step 3: Add a small launcher planning abstraction in `src/main.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchUi {
    Onboarding,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NoArgLaunchPlan {
    ui: LaunchUi,
    ensure_daemon_running: bool,
}

fn decide_no_arg_launch(onboarding_complete: bool, daemon_running: bool) -> NoArgLaunchPlan {
    NoArgLaunchPlan {
        ui: if onboarding_complete {
            LaunchUi::Settings
        } else {
            LaunchUi::Onboarding
        },
        ensure_daemon_running: !daemon_running,
    }
}
```

- [ ] **Step 4: Update the `args.len() < 2` branch to use the plan**

```rust
if args.len() < 2 {
    let onboarding_complete = is_onboarding_complete();
    let daemon_running = apexshot::daemon::is_daemon_running();
    let plan = decide_no_arg_launch(onboarding_complete, daemon_running);

    if plan.ensure_daemon_running {
        if let Err(err) = apexshot::daemon::start_daemon_subprocess() {
            eprintln!("Failed to start ApexShot daemon: {err}");
        }
    }

    match plan.ui {
        LaunchUi::Settings => {
            let _ = show_settings_window();
        }
        LaunchUi::Onboarding => {
            let _ = show_onboarding_window();
        }
    }
    return;
}
```

- [ ] **Step 5: Run the focused test to verify it passes**

Run: `cargo test --test launcher_daemon_behavior`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/launcher_daemon_behavior.rs
git commit -m "Bootstrap daemon from default app launch"
```

---

### Task 2: Make Daemon Process Detection Explicit And Reusable

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/main.rs`
- Test: `tests/launcher_daemon_behavior.rs`

- [ ] **Step 1: Write the failing test for daemon detection**

```rust
#[test]
fn launcher_only_requests_bootstrap_when_daemon_bus_is_unavailable() {
    assert!(!should_start_daemon_for_default_launch(true));
    assert!(should_start_daemon_for_default_launch(false));
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run: `cargo test --test launcher_daemon_behavior launcher_only_requests_bootstrap_when_daemon_bus_is_unavailable`

Expected: FAIL because `should_start_daemon_for_default_launch` does not exist.

- [ ] **Step 3: Add a public daemon-running probe to `src/daemon/mod.rs`**

```rust
pub fn is_daemon_running() -> bool {
    zbus::blocking::Connection::session()
        .ok()
        .and_then(|conn| {
            zbus::blocking::Proxy::new(&conn, DAEMON_BUS_NAME, DAEMON_OBJECT_PATH, DAEMON_INTERFACE)
                .ok()
        })
        .is_some()
}
```

- [ ] **Step 4: Add a tiny helper in `src/main.rs` for the launcher decision**

```rust
fn should_start_daemon_for_default_launch(daemon_running: bool) -> bool {
    !daemon_running
}
```

- [ ] **Step 5: Make `start_daemon_subprocess` safe for launcher reuse**

Verify that `src/daemon/mod.rs` starts `/usr/bin/apexshot daemon` via `current_exe()` and returns `Ok(())` when spawn succeeds without blocking UI startup. If needed, adjust only the spawn path and error message formatting; do not change daemon semantics in this step.

- [ ] **Step 6: Run the focused test to verify it passes**

Run: `cargo test --test launcher_daemon_behavior`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/daemon/mod.rs src/main.rs tests/launcher_daemon_behavior.rs
git commit -m "Add reusable daemon running detection"
```

---

### Task 3: Keep Tray Visibility And Autostart Aligned With Settings

**Files:**
- Modify: `src/settings/actions.rs`
- Modify: `src/settings/windowing.rs`
- Modify: `src/onboarding/mod.rs`

- [ ] **Step 1: Write the failing test for save-time daemon behavior**

Extend the existing unit tests in `src/settings/actions.rs` with one more assertion:

```rust
#[test]
fn tray_visibility_changes_are_applied_without_requiring_manual_daemon_launch() {
    assert!(should_auto_respawn_daemon_for_save_with_env(true, false));
}
```

Also add a test around the autostart entry content if no equivalent exists:

```rust
#[test]
fn autostart_entry_runs_daemon_mode() {
    let source = include_str!("windowing.rs");
    assert!(source.contains("Exec={binary_path} daemon"));
}
```

- [ ] **Step 2: Run the focused tests to verify current behavior**

Run: `cargo test settings::actions -- --nocapture`

Expected: Either FAIL for missing coverage or PASS after confirming the assertions need to be extended. If they already pass, keep the new tests as regression coverage and continue.

- [ ] **Step 3: Keep settings save behavior consistent with launcher-managed daemon sessions**

In `src/settings/actions.rs`, preserve this rule set:
- if daemon is already running, use D-Bus to update tray visibility immediately
- if tray visibility should be on and daemon is not reachable, spawn it
- if auto-managed daemon session is detected and a full respawn is needed, stop and restart it

The minimal production adjustment should stay in the existing save path:

```rust
if set_daemon_tray_visibility(tray_visible) {
    return Ok(());
}

if tray_visible {
    let _ = start_daemon_subprocess();
}
```

- [ ] **Step 4: Make the onboarding completion flow optionally start the daemon**

In `src/onboarding/mod.rs`, after `mark_onboarding_complete()`, trigger best-effort daemon startup before closing the window:

```rust
let _ = crate::daemon::start_daemon_subprocess();
window.close();
```

Apply this only to the completion/skip paths that end onboarding.

- [ ] **Step 5: Keep the autostart entry pointing at daemon mode**

In `src/settings/windowing.rs`, keep:

```rust
Exec={binary_path} daemon
```

and update the icon name to the packaged app icon if needed:

```rust
Icon=apexshot
```

- [ ] **Step 6: Run the focused tests to verify they pass**

Run: `cargo test settings::actions`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/settings/actions.rs src/settings/windowing.rs src/onboarding/mod.rs
git commit -m "Keep daemon behavior aligned with settings state"
```

---

### Task 4: Preserve Main-App Semantics In Desktop Packaging

**Files:**
- Modify: `packaging/apexshot.desktop`
- Modify: `Cargo.toml`
- Test: `tests/desktop_identity.rs`
- Test: `tests/package_metadata.rs`

- [ ] **Step 1: Write or extend the failing packaging assertions**

In `tests/desktop_identity.rs`, keep the current desktop identity checks and add one more assertion for launcher semantics:

```rust
let desktop_entry = include_str!("../packaging/apexshot.desktop");
assert!(desktop_entry.contains("Exec=/usr/bin/apexshot"));
```

- [ ] **Step 2: Run the focused packaging tests to verify the current state**

Run: `cargo test --test desktop_identity`

Expected: PASS after Task 1-3 are complete. If it fails, fix packaging/app-id drift before moving on.

- [ ] **Step 3: Confirm the packaged desktop launcher remains a UI launcher**

Keep `packaging/apexshot.desktop` as:

```ini
[Desktop Entry]
Name=ApexShot
Exec=/usr/bin/apexshot
Icon=apexshot
Type=Application
Terminal=false
```

Do not change it to `apexshot daemon`; the launcher should still open onboarding/settings while Task 1 ensures the daemon is also available in the background.

- [ ] **Step 4: Bump the crate/package version**

Update `Cargo.toml` version once for the release carrying this feature set.

- [ ] **Step 5: Run packaging regression tests**

Run:

```bash
cargo test --test desktop_identity
cargo test --test package_metadata
```

Expected:
- `desktop_identity` PASS
- `package_metadata` PASS

- [ ] **Step 6: Commit**

```bash
git add packaging/apexshot.desktop Cargo.toml tests/desktop_identity.rs tests/package_metadata.rs
git commit -m "Keep desktop launcher focused on app settings flow"
```

---

### Task 5: End-To-End Verification Of Background Behavior

**Files:**
- Modify: `src/main.rs`
- Modify: `src/daemon/mod.rs`
- Test: `tests/launcher_daemon_behavior.rs`

- [ ] **Step 1: Add a final integration-style regression test around the no-argument launcher**

```rust
#[test]
fn default_launch_plan_keeps_ui_opening_behavior_and_background_daemon_behavior_separate() {
    let onboarding = decide_no_arg_launch(false, false);
    assert_eq!(onboarding.ui, LaunchUi::Onboarding);
    assert!(onboarding.ensure_daemon_running);

    let normal = decide_no_arg_launch(true, false);
    assert_eq!(normal.ui, LaunchUi::Settings);
    assert!(normal.ensure_daemon_running);
}
```

- [ ] **Step 2: Run all focused regression tests**

Run:

```bash
cargo test --test launcher_daemon_behavior
cargo test --test desktop_identity
cargo test --test package_metadata
```

Expected: all PASS

- [ ] **Step 3: Run the release build**

Run:

```bash
cargo build --release
```

Expected: `Finished 'release' profile`

- [ ] **Step 4: Manual smoke-check commands**

Run:

```bash
target/release/apexshot
target/release/apexshot daemon
```

Expected:
- no-arg launch opens onboarding/settings and leaves the daemon available
- daemon mode still starts the tray/hotkey backend

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/daemon/mod.rs tests/launcher_daemon_behavior.rs
git commit -m "Finalize default launch and daemon background behavior"
```

---

## Self-Review

- Spec coverage:
  This plan covers the first-download onboarding flow, post-setup settings launch, always-running background daemon, tray visibility driven by `show_menu_bar_icon`, and preservation of hotkey/capture behavior through the daemon.
- Placeholder scan:
  No `TODO`/`TBD` placeholders remain; each task names exact files, commands, and expected results.
- Type consistency:
  The plan consistently uses `LaunchUi`, `NoArgLaunchPlan`, `decide_no_arg_launch`, `is_daemon_running`, and `start_daemon_subprocess` as the key launcher/daemon seam.

Plan complete and saved to `docs/superpowers/plans/2026-04-17-background-daemon-settings-launch.md`. Two execution options:

1. Subagent-Driven (recommended) - I dispatch a fresh subagent per task, review between tasks, fast iteration

2. Inline Execution - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
