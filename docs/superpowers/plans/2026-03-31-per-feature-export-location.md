# Per-Feature Export Location Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the shared export directory into separate screenshot and video save locations, move the screenshot picker into the Screenshots tab, add a video picker to Recording, and remove the unsupported desktop-icons setting.

**Architecture:** Extend `AppConfig` with dedicated screenshot/video path fields while keeping the old shared field only as a migration source. Recompose the settings tabs so each feature owns its own save path control, then switch screenshot and recording save flows to read the new per-feature locations.

**Tech Stack:** Rust, GTK4, serde_yml, existing ApexShot settings/config modules

---

### Task 1: Split config storage and migration

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing config tests**

```rust
#[test]
fn sanitize_migrates_legacy_shared_export_location() {
    let raw = r#"
export_location: /tmp/shared
"#;

    let cfg = serde_yml::from_str::<AppConfig>(raw).unwrap().sanitized();

    assert_eq!(cfg.screenshot_export_location, "/tmp/shared");
    assert_eq!(cfg.video_export_location, "/tmp/shared");
}

#[test]
fn sanitize_preserves_explicit_per_feature_export_locations() {
    let raw = r#"
export_location: /tmp/shared
screenshot_export_location: /tmp/screens
video_export_location: /tmp/video
"#;

    let cfg = serde_yml::from_str::<AppConfig>(raw).unwrap().sanitized();

    assert_eq!(cfg.screenshot_export_location, "/tmp/screens");
    assert_eq!(cfg.video_export_location, "/tmp/video");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test sanitize_migrates_legacy_shared_export_location sanitize_preserves_explicit_per_feature_export_locations -- --nocapture`
Expected: FAIL because `AppConfig` does not yet contain the new path fields or migration behavior

- [ ] **Step 3: Write minimal implementation**

```rust
pub struct AppConfig {
    pub export_location: String,
    pub screenshot_export_location: String,
    pub video_export_location: String,
}

impl AppConfig {
    pub fn sanitized(mut self) -> Self {
        self.export_location = self.export_location.trim().to_string();
        self.screenshot_export_location = self.screenshot_export_location.trim().to_string();
        self.video_export_location = self.video_export_location.trim().to_string();

        if self.screenshot_export_location.is_empty() && !self.export_location.is_empty() {
            self.screenshot_export_location = self.export_location.clone();
        }
        if self.video_export_location.is_empty() && !self.export_location.is_empty() {
            self.video_export_location = self.export_location.clone();
        }

        self
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test sanitize_migrates_legacy_shared_export_location sanitize_preserves_explicit_per_feature_export_locations -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: split screenshot and video export locations"
```

### Task 2: Move settings controls into feature tabs

**Files:**
- Modify: `src/settings/mod.rs`
- Modify: `src/settings/screenshots.rs`
- Modify: `src/settings/recording.rs`
- Modify: `src/settings/actions.rs`

- [ ] **Step 1: Write the failing UI/save tests or assertions**

```rust
// Add a focused save-path assertion near existing settings save tests:
assert_eq!(config.screenshot_export_location, "/tmp/screens");
assert_eq!(config.video_export_location, "/tmp/video");
```

- [ ] **Step 2: Run targeted verification to confirm current behavior is missing**

Run: `cargo check`
Expected: Existing code still compiles, but General still owns storage and save path wiring still points at `export_location`

- [ ] **Step 3: Recompose the settings UI and save inputs**

```rust
// Remove storage from General composition in src/settings/mod.rs
general_tab_section.append(&general.section);
general_tab_section.append(&after_capture_separator);
general_tab_section.append(&after_capture.wrapper);

// Add dedicated entries/buttons in screenshots.rs and recording.rs
pub struct ScreenshotsWidgets {
    pub export_location_entry: Entry,
    pub export_location_button: Button,
}

pub struct RecordingWidgets {
    pub video_export_location_entry: Entry,
    pub video_export_location_button: Button,
}

// Save them in src/settings/actions.rs
config.screenshot_export_location = inputs.screenshot_export_location.text().to_string();
config.video_export_location = inputs.video_export_location.text().to_string();
```

- [ ] **Step 4: Remove the unsupported desktop-icons control**

```rust
// Drop hide_desktop_icons from SaveInputs and from the General/Storage UI.
// Do not render the checkbox and do not write it on save.
```

- [ ] **Step 5: Run verification**

Run: `cargo check`
Expected: PASS, with General no longer showing storage or desktop-icons, and Screenshots/Recording owning the save path entries

- [ ] **Step 6: Commit**

```bash
git add src/settings/mod.rs src/settings/screenshots.rs src/settings/recording.rs src/settings/actions.rs
git commit -m "refactor: move export settings into feature tabs"
```

### Task 3: Switch runtime save flows to per-feature locations

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/capture/mod.rs`
- Modify: any helper that currently resolves `export_location`

- [ ] **Step 1: Find and cover the current shared-path call sites**

```rust
// Replace reads of config.export_location with feature-specific lookups:
let screenshot_dir = config.screenshot_export_location.clone();
let video_dir = config.video_export_location.clone();
```

- [ ] **Step 2: Run targeted verification before the fix**

Run: `cargo check`
Expected: Identify call sites still reading the shared path through compiler errors or search results after config changes

- [ ] **Step 3: Implement the runtime path split**

```rust
// Screenshot path resolution
let save_dir = if config.screenshot_export_location.is_empty() {
    pictures_dir_fallback()
} else {
    PathBuf::from(&config.screenshot_export_location)
};

// Recording path resolution
let save_dir = if config.video_export_location.is_empty() {
    videos_dir_fallback()
} else {
    PathBuf::from(&config.video_export_location)
};
```

- [ ] **Step 4: Run verification**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Manual smoke test**

Run:
- open settings
- set different screenshot and video folders
- click `Save`
- take a screenshot
- record a short video

Expected:
- screenshot lands in the screenshot folder
- recording lands in the video folder

- [ ] **Step 6: Commit**

```bash
git add src/daemon/mod.rs src/capture/mod.rs
git commit -m "feat: use per-feature export locations at runtime"
```
