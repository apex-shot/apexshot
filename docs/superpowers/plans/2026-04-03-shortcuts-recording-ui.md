# Shortcuts Tab + Recording UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Settings → Shortcuts the source of truth for runtime hotkeys, add all app shortcuts including recording controls, and add a new hotkey that opens the recording UI directly instead of entering screenshot area capture first.

**Architecture:** Extend `AppConfig` to store the full shortcut set, add a deterministic `AppConfig -> HotkeyConfig` sync path, expand the GTK Shortcuts tab/save plumbing to edit those values, then add a new daemon action plus a dedicated C++ overlay entry path that starts in recording mode. Keep conflicts non-blocking and preserve recording control shortcuts as runtime-only actions during active recording.

**Tech Stack:** Rust, GTK4, Tokio, zbus, serde_yml, C++/Qt capture overlay, Cargo tests

---

## File structure / responsibilities

- Modify: `src/config.rs`
  - Add missing shortcut fields, defaults, sanitization coverage, and config tests.
- Modify: `src/settings/shortcuts.rs`
  - Add top-of-tab shortcut tip, expand rows to cover recording actions, and label recording-only controls.
- Modify: `src/settings/actions.rs`
  - Persist shortcut button values into `AppConfig`, then sync runtime hotkey config after settings save.
- Modify: `src/settings/mod.rs`
  - Thread the new shortcut buttons into `SaveInputs`.
- Modify: `src/hotkeys/mod.rs`
  - Add a public helper that converts `AppConfig` shortcut fields into `HotkeyConfig` bindings and writes them to disk.
- Modify: `src/daemon/mod.rs`
  - Add `OpenRecordingUi` action mapping, D-Bus trigger support, hotkey binding mapping, and tests.
- Modify: `src/main.rs`
  - Add `record ui` command routing/fallback behavior.
- Modify: `src/capture_overlay.rs`
  - Add a dedicated Rust bridge for launching the C++ overlay directly into recording UI mode.
- Modify: `capture-overlay/src/main.cpp`
  - Add a CLI flag to start the overlay in recording mode instead of default screenshot mode.
- Optional small helper changes: `capture-overlay/src/CaptureOverlay*.cpp|h`
  - Only if needed to expose a “start in recording panel” initialization path cleanly.

---

### Task 1: Extend config with the complete shortcut model

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs`

- [ ] **Step 1: Write the failing config tests for new shortcut fields**

Add these tests near the existing shortcut tests in `src/config.rs`:

```rust
#[test]
fn shortcut_defaults_include_open_recording_ui_and_controls() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.shortcut_open_recording_ui, "Ctrl+Alt+R");
    assert_eq!(cfg.shortcut_recording_pause_resume, "Ctrl+Alt+Shift+P");
    assert_eq!(cfg.shortcut_recording_stop_save, "Ctrl+Alt+Shift+S");
    assert_eq!(cfg.shortcut_recording_restart, "Ctrl+Alt+Shift+N");
    assert_eq!(cfg.shortcut_recording_discard, "Ctrl+Alt+Shift+BackSpace");
}

#[test]
fn config_yaml_round_trip_preserves_recording_shortcuts() {
    let original = AppConfig {
        shortcut_open_recording_ui: "Alt+R".into(),
        shortcut_recording_pause_resume: "Alt+P".into(),
        shortcut_recording_stop_save: "Alt+S".into(),
        shortcut_recording_restart: "Alt+N".into(),
        shortcut_recording_discard: "Alt+BackSpace".into(),
        ..AppConfig::default()
    };

    let yaml = serde_yml::to_string(&original).unwrap();
    let loaded: AppConfig = serde_yml::from_str(&yaml).unwrap();

    assert_eq!(loaded.shortcut_open_recording_ui, original.shortcut_open_recording_ui);
    assert_eq!(
        loaded.shortcut_recording_pause_resume,
        original.shortcut_recording_pause_resume
    );
    assert_eq!(
        loaded.shortcut_recording_stop_save,
        original.shortcut_recording_stop_save
    );
    assert_eq!(loaded.shortcut_recording_restart, original.shortcut_recording_restart);
    assert_eq!(loaded.shortcut_recording_discard, original.shortcut_recording_discard);
}
```

- [ ] **Step 2: Run the focused config tests and verify they fail**

Run:

```bash
cargo test shortcut_defaults_include_open_recording_ui_and_controls config_yaml_round_trip_preserves_recording_shortcuts -- --nocapture
```

Expected: compile failure because the new `AppConfig` fields do not exist yet.

- [ ] **Step 3: Add the new shortcut fields and defaults**

Update the shortcut section of `AppConfig` in `src/config.rs`:

```rust
// Shortcut settings
pub shortcut_toggle_desktop_icons: String,
pub shortcut_open_file: String,
pub shortcut_open_from_clipboard: String,
pub shortcut_pin_to_screen: String,
pub shortcut_restore_recently_closed: String,
pub shortcut_toggle_overlays: String,
pub shortcut_capture_area: String,
pub shortcut_capture_crosshair: String,
pub shortcut_capture_previous_area: String,
pub shortcut_capture_fullscreen: String,
pub shortcut_capture_window: String,
pub shortcut_capture_menu: String,
pub shortcut_open_recording_ui: String,
pub shortcut_recording_pause_resume: String,
pub shortcut_recording_stop_save: String,
pub shortcut_recording_restart: String,
pub shortcut_recording_discard: String,
```

And add defaults in `impl Default for AppConfig`:

```rust
shortcut_capture_menu: String::new(),
shortcut_open_recording_ui: "Ctrl+Alt+R".to_string(),
shortcut_recording_pause_resume: "Ctrl+Alt+Shift+P".to_string(),
shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".to_string(),
shortcut_recording_restart: "Ctrl+Alt+Shift+N".to_string(),
shortcut_recording_discard: "Ctrl+Alt+Shift+BackSpace".to_string(),
```

Keep all values title-cased here to match the existing settings-side convention.

- [ ] **Step 4: Re-run the focused config tests and verify they pass**

Run:

```bash
cargo test shortcut_defaults_include_open_recording_ui_and_controls config_yaml_round_trip_preserves_recording_shortcuts -- --nocapture
```

Expected: both tests PASS.

- [ ] **Step 5: Commit the config model change**

```bash
git add src/config.rs
git commit -m "feat: add recording shortcut fields to app config"
```

---

### Task 2: Add a deterministic AppConfig → HotkeyConfig sync path

**Files:**
- Modify: `src/hotkeys/mod.rs`
- Test: `src/hotkeys/mod.rs`

- [ ] **Step 1: Write the failing hotkey sync tests**

Add tests in `src/hotkeys/mod.rs` near the existing hotkey default tests:

```rust
#[test]
fn app_config_shortcuts_map_to_runtime_hotkeys() {
    let cfg = crate::config::AppConfig {
        shortcut_capture_area: "Shift+Super+4".into(),
        shortcut_capture_crosshair: "Ctrl+Alt+X".into(),
        shortcut_capture_fullscreen: "Shift+Super+3".into(),
        shortcut_capture_window: "Shift+Super+5".into(),
        shortcut_open_recording_ui: "Ctrl+Alt+R".into(),
        shortcut_recording_pause_resume: "Ctrl+Alt+Shift+P".into(),
        shortcut_recording_stop_save: "Ctrl+Alt+Shift+S".into(),
        shortcut_recording_restart: "Ctrl+Alt+Shift+N".into(),
        shortcut_recording_discard: "Ctrl+Alt+Shift+BackSpace".into(),
        ..crate::config::AppConfig::default()
    };

    let hotkeys = hotkey_config_from_app_config(&cfg);

    assert!(hotkeys.bindings.iter().any(|binding| {
        binding.name.as_deref() == Some("open_recording_ui")
            && binding.accelerator == "CTRL+ALT+R"
            && binding.args == vec!["record".to_string(), "ui".to_string()]
    }));
}

#[test]
fn blank_shortcuts_are_omitted_from_runtime_hotkeys() {
    let cfg = crate::config::AppConfig {
        shortcut_open_recording_ui: String::new(),
        shortcut_recording_restart: String::new(),
        ..crate::config::AppConfig::default()
    };

    let hotkeys = hotkey_config_from_app_config(&cfg);

    assert!(!hotkeys.bindings.iter().any(|binding| {
        binding.name.as_deref() == Some("open_recording_ui")
    }));
    assert!(!hotkeys.bindings.iter().any(|binding| {
        binding.name.as_deref() == Some("recording_restart")
    }));
}
```

- [ ] **Step 2: Run the hotkey tests and verify they fail**

Run:

```bash
cargo test app_config_shortcuts_map_to_runtime_hotkeys blank_shortcuts_are_omitted_from_runtime_hotkeys -- --nocapture
```

Expected: compile failure because `hotkey_config_from_app_config` does not exist yet.

- [ ] **Step 3: Implement the config-to-hotkey conversion helper**

Add this helper in `src/hotkeys/mod.rs`:

```rust
fn normalize_settings_accel(value: &str) -> String {
    as_portal_trigger(value)
}

pub fn hotkey_config_from_app_config(app_config: &crate::config::AppConfig) -> HotkeyConfig {
    let mut bindings = Vec::new();

    let push_binding = |bindings: &mut Vec<HotkeyBinding>,
                        name: &str,
                        accel: &str,
                        args: &[&str]| {
        let trimmed = accel.trim();
        if trimmed.is_empty() {
            return;
        }
        bindings.push(HotkeyBinding {
            name: Some(name.to_string()),
            accelerator: normalize_settings_accel(trimmed),
            args: args.iter().map(|s| s.to_string()).collect(),
        });
    };

    push_binding(&mut bindings, "capture_area", &app_config.shortcut_capture_area, &["capture", "area"]);
    push_binding(&mut bindings, "capture_crosshair", &app_config.shortcut_capture_crosshair, &["capture", "crosshair"]);
    push_binding(&mut bindings, "capture_screen", &app_config.shortcut_capture_fullscreen, &["capture", "screen"]);
    push_binding(&mut bindings, "capture_window", &app_config.shortcut_capture_window, &["capture", "window"]);
    push_binding(&mut bindings, "open_recording_ui", &app_config.shortcut_open_recording_ui, &["record", "ui"]);
    push_binding(&mut bindings, "recording_pause_resume", &app_config.shortcut_recording_pause_resume, &["recording-control", "pause-resume"]);
    push_binding(&mut bindings, "recording_stop_save", &app_config.shortcut_recording_stop_save, &["recording-control", "stop-save"]);
    push_binding(&mut bindings, "recording_restart", &app_config.shortcut_recording_restart, &["recording-control", "restart"]);
    push_binding(&mut bindings, "recording_discard", &app_config.shortcut_recording_discard, &["recording-control", "discard"]);

    HotkeyConfig { bindings }
}
```

Then add a public save helper:

```rust
pub fn sync_hotkeys_from_app_config(app_config: &crate::config::AppConfig) -> anyhow::Result<()> {
    let path = default_config_path();
    let cfg = hotkey_config_from_app_config(app_config);
    save_hotkey_config(&path, &cfg)
}
```

Do **not** keep `record_screen` / `record_area` in the generated runtime config; this feature replaces recording-start hotkeys with a single `open_recording_ui` entry action.

- [ ] **Step 4: Re-run the hotkey tests and verify they pass**

Run:

```bash
cargo test app_config_shortcuts_map_to_runtime_hotkeys blank_shortcuts_are_omitted_from_runtime_hotkeys -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit the hotkey sync helper**

```bash
git add src/hotkeys/mod.rs
git commit -m "feat: derive runtime hotkeys from app shortcut settings"
```

---

### Task 3: Expand the Shortcuts tab UI and save plumbing

**Files:**
- Modify: `src/settings/shortcuts.rs`
- Modify: `src/settings/mod.rs`
- Modify: `src/settings/actions.rs`

- [ ] **Step 1: Add a focused save-path test for recording shortcuts**

Add a small unit test in `src/settings/actions.rs` by first extracting a pure helper:

```rust
fn button_label_value(button: &Button) -> String {
    let label = button.label().unwrap_or_default();
    if label == "Record shortcut" {
        String::new()
    } else {
        label.to_string()
    }
}
```

Then test it:

```rust
#[test]
fn button_label_value_treats_placeholder_as_empty() {
    let button = Button::with_label("Record shortcut");
    assert_eq!(button_label_value(&button), "");

    button.set_label("Ctrl+Alt+R");
    assert_eq!(button_label_value(&button), "Ctrl+Alt+R");
}
```

- [ ] **Step 2: Run the focused settings test and verify it fails if helper is missing**

Run:

```bash
cargo test button_label_value_treats_placeholder_as_empty -- --nocapture
```

Expected: compile failure until the helper is added.

- [ ] **Step 3: Expand `ShortcutSettingsWidgets` and render the new UI**

In `src/settings/shortcuts.rs`:

1. Add widget fields:

```rust
pub open_recording_ui_btn: Button,
pub recording_pause_resume_btn: Button,
pub recording_stop_save_btn: Button,
pub recording_restart_btn: Button,
pub recording_discard_btn: Button,
```

2. Add a top informational notice before the sections:

```rust
let tip = Label::new(Some(
    "Shortcuts set here are the same hotkeys ApexShot uses. If one does not work, your desktop environment may already be using it. Open your system keyboard settings and disable conflicting shortcuts if you want to reuse them in ApexShot.",
));
tip.set_wrap(true);
tip.set_xalign(0.0);
tip.add_css_class("settings-sub-option-hint");
tip.set_margin_bottom(20);
section.append(&tip);
```

3. Update row creation to support a subtitle for recording-only actions:

```rust
let create_row = |frame: &GtkBox,
                  label_text: &str,
                  hint_text: Option<&str>,
                  current_val: &str,
                  is_muted: bool| -> Button {
    let text_box = GtkBox::new(Orientation::Vertical, 4);
    let lbl = Label::new(Some(label_text));
    lbl.set_xalign(0.0);
    text_box.append(&lbl);

    if let Some(hint) = hint_text {
        let hint_lbl = Label::new(Some(hint));
        hint_lbl.set_xalign(0.0);
        hint_lbl.add_css_class("settings-sub-option-hint");
        text_box.append(&hint_lbl);
    }

    // keep existing button creation on the right
};
```

4. Add a new **Recording** section:

```rust
create_header(&section, "Recording", "media-record-symbolic");
let recording_frame = build_frame();
let open_recording_ui_btn = create_row(
    &recording_frame,
    "Open Recording UI:",
    None,
    &config.shortcut_open_recording_ui,
    false,
);
let recording_pause_resume_btn = create_row(
    &recording_frame,
    "Pause/Resume Recording:",
    Some("Only during recording"),
    &config.shortcut_recording_pause_resume,
    true,
);
let recording_stop_save_btn = create_row(
    &recording_frame,
    "Stop and Save Recording:",
    Some("Only during recording"),
    &config.shortcut_recording_stop_save,
    false,
);
let recording_restart_btn = create_row(
    &recording_frame,
    "Restart Recording:",
    Some("Only during recording"),
    &config.shortcut_recording_restart,
    true,
);
let recording_discard_btn = create_row(
    &recording_frame,
    "Discard Recording:",
    Some("Only during recording"),
    &config.shortcut_recording_discard,
    false,
);
section.append(&recording_frame);
```

- [ ] **Step 4: Thread the new buttons through `SaveInputs` and persist them**

In `src/settings/actions.rs`, add these fields to `SaveInputs`:

```rust
pub shortcut_open_recording_ui: Button,
pub shortcut_recording_pause_resume: Button,
pub shortcut_recording_stop_save: Button,
pub shortcut_recording_restart: Button,
pub shortcut_recording_discard: Button,
```

In `save_settings`, persist all shortcut buttons:

```rust
config.shortcut_toggle_desktop_icons = button_label_value(&inputs.shortcut_toggle_desktop_icons);
config.shortcut_open_file = button_label_value(&inputs.shortcut_open_file);
config.shortcut_open_from_clipboard = button_label_value(&inputs.shortcut_open_from_clipboard);
config.shortcut_pin_to_screen = button_label_value(&inputs.shortcut_pin_to_screen);
config.shortcut_restore_recently_closed = button_label_value(&inputs.shortcut_restore_recently_closed);
config.shortcut_toggle_overlays = button_label_value(&inputs.shortcut_toggle_overlays);
config.shortcut_capture_area = button_label_value(&inputs.shortcut_capture_area);
config.shortcut_capture_crosshair = button_label_value(&inputs.shortcut_capture_crosshair);
config.shortcut_capture_previous_area = button_label_value(&inputs.shortcut_capture_previous_area);
config.shortcut_capture_fullscreen = button_label_value(&inputs.shortcut_capture_fullscreen);
config.shortcut_capture_window = button_label_value(&inputs.shortcut_capture_window);
config.shortcut_open_recording_ui = button_label_value(&inputs.shortcut_open_recording_ui);
config.shortcut_recording_pause_resume = button_label_value(&inputs.shortcut_recording_pause_resume);
config.shortcut_recording_stop_save = button_label_value(&inputs.shortcut_recording_stop_save);
config.shortcut_recording_restart = button_label_value(&inputs.shortcut_recording_restart);
config.shortcut_recording_discard = button_label_value(&inputs.shortcut_recording_discard);
```

Then call the sync helper after `save_config(&config)?;`:

```rust
crate::hotkeys::sync_hotkeys_from_app_config(&config)?;
```

In `src/settings/mod.rs`, wire the new buttons into the `SaveInputs` construction.

- [ ] **Step 5: Re-run the focused settings test, then a broader settings/config pass**

Run:

```bash
cargo test button_label_value_treats_placeholder_as_empty shortcut_defaults_include_open_recording_ui_and_controls -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit the Shortcuts tab UI/save work**

```bash
git add src/settings/shortcuts.rs src/settings/mod.rs src/settings/actions.rs
git commit -m "feat: expand shortcuts settings and save runtime hotkeys"
```

---

### Task 4: Add the new daemon action and runtime mapping

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/main.rs`
- Test: `src/daemon/mod.rs`

- [ ] **Step 1: Write the failing daemon mapping test**

Add this in `src/daemon/mod.rs` tests:

```rust
#[test]
fn binding_to_daemon_action_maps_open_recording_ui_hotkey() {
    let open_recording_ui = crate::hotkeys::HotkeyBinding {
        accelerator: "CTRL+ALT+R".into(),
        args: vec!["record".into(), "ui".into()],
        name: Some("open_recording_ui".into()),
    };

    assert_eq!(
        super::binding_to_daemon_action(&open_recording_ui),
        Some(super::DaemonAction::OpenRecordingUi)
    );
}
```

- [ ] **Step 2: Run the focused daemon test and verify it fails**

Run:

```bash
cargo test binding_to_daemon_action_maps_open_recording_ui_hotkey -- --nocapture
```

Expected: compile failure because `OpenRecordingUi` does not exist yet.

- [ ] **Step 3: Implement the new daemon action and CLI mapping**

In `src/daemon/mod.rs`:

```rust
enum DaemonAction {
    // ...existing variants...
    OpenRecordingUi,
}
```

Add action routing in all three places already used for hotkeys:

```rust
// D-Bus trigger mapping
"open_recording_ui" => DaemonAction::OpenRecordingUi,

// binding.name mapping
"open_recording_ui" | "open-recording-ui" => return Some(DaemonAction::OpenRecordingUi),

// binding.args mapping
Some("record") => match binding.args.get(1).map(|s| s.as_str()) {
    Some("ui") => Some(DaemonAction::OpenRecordingUi),
    Some("screen") => Some(DaemonAction::RecordScreen),
    Some("area") => Some(DaemonAction::RecordArea),
    _ => None,
},
```

Add event-loop handling near the other recording actions:

```rust
DaemonAction::OpenRecordingUi => {
    tokio::spawn(handle_open_recording_ui(action_tx_clone));
}
```

In `src/main.rs`, extend the `record` command to support `ui`:

```rust
let daemon_action = match args[2].as_str() {
    "ui" => Some("open_recording_ui"),
    "screen" => Some("record_screen"),
    "area" => Some("record_area"),
    _ => None,
};
```

And allow the non-daemon fallback path to route into a new helper for direct recording UI.

- [ ] **Step 4: Re-run the focused daemon test and verify it passes**

Run:

```bash
cargo test binding_to_daemon_action_maps_open_recording_ui_hotkey -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit the daemon/main mapping change**

```bash
git add src/daemon/mod.rs src/main.rs
git commit -m "feat: add open recording ui daemon action"
```

---

### Task 5: Launch the C++ overlay directly into recording UI mode

**Files:**
- Modify: `src/capture_overlay.rs`
- Modify: `capture-overlay/src/main.cpp`
- Optional modify: `capture-overlay/src/CaptureOverlay.h`
- Optional modify: `capture-overlay/src/CaptureOverlay*.cpp`
- Test: `src/capture_overlay.rs`

- [ ] **Step 1: Write a focused Rust-side arg-builder test**

First extract a helper in `src/capture_overlay.rs`:

```rust
fn build_recording_ui_args(config: &crate::config::AppConfig) -> Vec<String> {
    let mut args = build_area_init_args(config);
    args.push("--open-recording-ui".into());
    args
}
```

Then add the test:

```rust
#[test]
fn build_recording_ui_args_adds_direct_recording_flag() {
    let args = build_recording_ui_args(&crate::config::AppConfig::default());
    assert!(args.iter().any(|arg| arg == "--area-init"));
    assert!(args.iter().any(|arg| arg == "--open-recording-ui"));
}
```

- [ ] **Step 2: Run the focused overlay test and verify it fails**

Run:

```bash
cargo test build_recording_ui_args_adds_direct_recording_flag -- --nocapture
```

Expected: compile failure until the helper is added.

- [ ] **Step 3: Add the Rust bridge and C++ startup flag**

In `src/capture_overlay.rs`, add:

```rust
pub fn open_recording_ui_via_cpp() -> Result<AreaCapturePathResult, SelectionError> {
    let config = crate::config::load_config();
    let extra_args = build_recording_ui_args(&config);
    let arg_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
    let output = run_capture_binary(&arg_refs, None)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(code) if code == OverlayExitCode::RecordConfigUpdated as i32 => {
            let request = parse_recording_json(stdout.trim())?;
            Ok(AreaCapturePathResult::RecordingRequested(request))
        }
        Some(1) | None => Err(SelectionError::Cancelled),
        Some(code) => Err(SelectionError::InitError(format!(
            "apexshot-capture --open-recording-ui exited with code {code}"
        ))),
    }
}
```

In `capture-overlay/src/main.cpp`, parse the flag and set the initial mode:

```cpp
bool openRecordingUiMode = false;

// argv parse
} else if (std::strcmp(argv[i], "--open-recording-ui") == 0) {
    openRecordingUiMode = true;
}
```

After constructing/configuring `overlay`, force the recording panel open:

```cpp
if (openRecordingUiMode) {
    overlay.openRecordingPanelForShortcut();
}
```

If no such method exists yet, add a minimal one in `CaptureOverlay`:

```cpp
void CaptureOverlay::openRecordingPanelForShortcut()
{
    m_recordingPanelOpen = true;
    m_recordingToolsHidden = false;
    m_captureIntent = CaptureIntent::Area;
    if (m_recordType == RecordType::None) {
        m_recordType = RecordType::Video;
    }
    update();
}
```

The implementation goal is **UI entry only**: it must open the recording panel immediately, not start recording automatically.

- [ ] **Step 4: Wire the daemon/CLI fallback to the new bridge**

In `src/daemon/mod.rs`, implement:

```rust
async fn handle_open_recording_ui(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    match tokio::task::spawn_blocking(crate::capture_overlay::open_recording_ui_via_cpp).await {
        Ok(Ok(crate::capture_overlay::AreaCapturePathResult::RecordingRequested(request))) => {
            if let Err(err) = crate::recording::run_overlay_recording_request_with_gtk(request, None) {
                eprintln!("[daemon] Recording UI failed: {err}");
            }
        }
        Ok(Ok(crate::capture_overlay::AreaCapturePathResult::RecordingConfigUpdated)) => {
            eprintln!("[daemon] Recording UI updated settings only.");
        }
        Ok(Ok(other)) => {
            eprintln!("[daemon] Unexpected recording UI result: {:?}", other);
        }
        Ok(Err(err)) => eprintln!("[daemon] Failed to open recording UI: {err}"),
        Err(err) => eprintln!("[daemon] Recording UI task panicked: {err}"),
    }
}
```

In `src/main.rs`, when `record ui` runs without daemon handling, call the same bridge/helper instead of `run_record("area")`.

- [ ] **Step 5: Re-run the focused overlay test and a recording mapping pass**

Run:

```bash
cargo test build_recording_ui_args_adds_direct_recording_flag binding_to_daemon_action_maps_open_recording_ui_hotkey -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit the direct recording UI launch path**

```bash
git add src/capture_overlay.rs src/daemon/mod.rs src/main.rs capture-overlay/src/main.cpp capture-overlay/src/CaptureOverlay.h capture-overlay/src/CaptureOverlay_*.cpp
git commit -m "feat: open recording ui directly from hotkey"
```

---

### Task 6: Full verification pass before handoff

**Files:**
- Modify if needed: any files touched above
- Test: all touched test locations

- [ ] **Step 1: Run the targeted Rust tests for config, hotkeys, daemon, and overlay helpers**

Run:

```bash
cargo test shortcut_defaults_include_open_recording_ui_and_controls \
  config_yaml_round_trip_preserves_recording_shortcuts \
  app_config_shortcuts_map_to_runtime_hotkeys \
  blank_shortcuts_are_omitted_from_runtime_hotkeys \
  button_label_value_treats_placeholder_as_empty \
  binding_to_daemon_action_maps_open_recording_ui_hotkey \
  build_recording_ui_args_adds_direct_recording_flag \
  binding_to_daemon_action_maps_recording_control_hotkeys \
  -- --nocapture
```

Expected: all listed tests PASS.

- [ ] **Step 2: Run a broader project test pass for touched modules**

Run:

```bash
cargo test config:: hotkeys:: daemon:: recording:: -- --nocapture
```

Expected: PASS, or only unrelated pre-existing failures. If unrelated failures appear, capture them in notes before proceeding.

- [ ] **Step 3: Do a manual runtime verification checklist**

Run/build commands:

```bash
cargo build
cargo run -- settings
```

Manual checks:

```text
[ ] Shortcuts tab shows the top conflict tip.
[ ] Shortcuts tab includes Open Recording UI.
[ ] Recording control rows show “Only during recording”.
[ ] Changing a shortcut and clicking Save updates the persisted app config.
[ ] Saving also updates ~/.config/apexshot/hotkeys.yml.
[ ] Pressing Open Recording UI opens recording UI directly.
[ ] It does not land on screenshot area capture first.
[ ] Recording does not auto-start from that hotkey.
[ ] Pause/Resume, Stop/Save, Restart, and Discard only work during recording.
```

- [ ] **Step 4: Commit final fixes if verification required follow-up changes**

```bash
git add src/config.rs src/settings/shortcuts.rs src/settings/mod.rs src/settings/actions.rs src/hotkeys/mod.rs src/daemon/mod.rs src/main.rs src/capture_overlay.rs capture-overlay/src/main.cpp capture-overlay/src/CaptureOverlay.h capture-overlay/src/CaptureOverlay_*.cpp
git commit -m "test: verify shortcut-driven recording ui flow"
```

Only make this commit if Step 3 required code changes; otherwise skip it.
