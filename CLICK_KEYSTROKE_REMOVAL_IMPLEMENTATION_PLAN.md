# Click/Keystroke Recording Overlay Removal — Implementation Plan

## Goal

Fully complete `CLICK_KEYSTROKE_REMOVAL_PLAN.md` without breaking:

- normal screenshot/editor input
- recording start/stop controls
- webcam/mic/speaker overlays
- scroll capture / remote-control behavior
- GNOME shell controls
- Qt capture overlay compatibility

## Current status summary

Already complete before this implementation pass:

- `src/recording/click_overlay.rs` removed.
- `src/recording/runtime_keystrokes.rs` removed.
- No active `PushKeystroke` references found in `src` / `gnome-extension`.
- `RuntimeOverlaySnapshot` contains mic/speaker/webcam fields only.
- GNOME runtime overlay implementation is mostly reduced to non-click/non-keystroke behavior.

Completed in this implementation pass:

- Created branch `complete-click-keystroke-removal`.
- Removed GTK/Rust overlay click/keystroke state, click-options hit testing, drawing, and event handling.
- Removed GTK recording settings rows/toggle paths for click/keystroke overlays.
- Removed `reis` and `xkbcommon` from `Cargo.toml`; `cargo check` updated `Cargo.lock` accordingly.
- Updated GNOME extension submission/screenshot docs to no longer ask for click/keystroke overlay screenshots.
- Started Qt `capture-overlay` cleanup:
  - removed click/keystroke recording panel tiles from drawing and hit testing,
  - removed click/keystroke settings rows,
  - removed click/keystroke options drawing functions and active event handling,
  - forced Qt click/keystroke result accessors to return `false` for compatibility.

Verification completed at this checkpoint:

- `cargo fmt --all`
- `cargo check`
- `node --check gnome-extension/*.js`

Still incomplete:

- Finish deeper Qt `capture-overlay` cleanup of now-unused members, initializers, setters/getters, helper structs, timer cleanup, and JSON compatibility fields.
- Decide whether to fully remove or temporarily keep Qt JSON fields such as `clicks`, `keystrokes`, `click_size`, and `key_size`; current native Rust parser does not appear to require them.
- Run the final search checklist and broader tests after the Qt cleanup is complete.

---

## Phase 0 — Establish safety baseline

### 0.1 Create branch

```bash
git checkout -b complete-click-keystroke-removal
```

### 0.2 Run current checks before editing

```bash
cargo check
cargo test
node --check gnome-extension/*.js
```

If existing failures exist, record them before changing code.

### 0.3 Repeated search checklist

Run this after each phase:

```bash
rg "rec_clicks|rec_keystrokes|clicks_enabled|keystrokes_enabled|PushKeystroke|click_overlay|runtime_keystrokes|ClickOverlay|Keystroke|keystroke|click_options|click_previews|recClicks|recKeystrokes|recordClicks|recordKeystrokes" src gnome-extension capture-overlay Cargo.toml Cargo.lock
```

Expected final result should only include unrelated generic click handling, intentional compatibility comments, or docs explicitly noting the removal.

---

## Phase 1 — Native GTK overlay UI cleanup

This removes the remaining Rust overlay panel state and drawing without touching unrelated mouse/keyboard handling.

### Files

- `src/overlay/recording/state.rs`
- `src/overlay/recording/hit_testing.rs`
- `src/overlay/drawing.rs`
- `src/overlay/window.rs`

### 1.1 Remove recording state fields

Remove from `RecordingPanelState`:

```rust
rec_clicks
rec_keystrokes
click_options_open
click_slider_dragging
click_dropdown_open
hovered_click_item
click_previews
click_size
click_color
click_style
click_animate
```

Keep unrelated recording state:

- mic
- speaker
- webcam
- countdown
- capture area
- scroll/window controls
- recording start controls

### 1.2 Remove click options hit-testing helpers

Remove only helpers specific to click overlay options, likely:

```rust
click_options_hit_item
click_dropdown_hit_item
click_options_menu_contains
```

Do **not** remove generic hit testing, toolbar hit testing, resize handles, recording panel hit testing, webcam menu hit testing, etc.

### 1.3 Update recording panel tile layout

Remove Clicks and Keystrokes tiles from the GTK recording panel.

Important: avoid index-based breakage.

Current code appears to use numeric tile indices around:

```rust
5 => rec_clicks
6 => rec_keystrokes
```

Replace with explicit remaining tile handling only.

Expected remaining recording controls include:

- mic
- speaker
- webcam
- crop/window/scroll/start controls if currently present

### 1.4 Remove click options drawing

Remove `draw_click_options`.

Remove calls around:

```rust
if st.recording.click_options_open {
    draw_click_options(...)
}
```

Remove unused drawing parameters:

```rust
rec_clicks
rec_keystrokes
```

from drawing functions if they only powered these tiles.

### 1.5 Remove GTK click-options event handling

From `src/overlay/window.rs`, remove blocks handling:

- click options hover
- click options press
- click options drag
- click preview timers
- click dropdowns
- toggling `rec_clicks`
- toggling `rec_keystrokes`

Be careful to preserve:

- normal click gesture setup
- right-click webcam menu
- scroll popup
- window picker
- settings menu
- recording start button
- ESC handling
- drag suppression for remaining popups

### 1.6 Verify Phase 1

```bash
cargo check
rg "rec_clicks|rec_keystrokes|click_options|click_previews|Keystroke" src/overlay
```

Expected: no active references.

---

## Phase 2 — Qt capture overlay cleanup

The Qt overlay still has the largest amount of remaining click/keystroke UI.

### Files

- `capture-overlay/src/CaptureOverlay.h`
- `capture-overlay/src/CaptureOverlay.cpp`
- `capture-overlay/src/CaptureOverlay_Events.cpp`
- `capture-overlay/src/CaptureOverlay_Drawing.cpp`
- `capture-overlay/src/CaptureOverlay_HitTest.cpp`
- `capture-overlay/src/main.cpp`

### 2.1 Remove or disable public API flags

Remove or permanently force false:

```cpp
recordClicksEnabled()
recordKeystrokesEnabled()
setInitialRecClicks(...)
setInitialRecKeystrokes(...)
```

Preferred final state: remove them entirely if all callers can be updated.

Lower-risk transitional state: keep accessors but return `false`, then remove request fields in a later edit.

### 2.2 Remove member variables

Remove members related only to click/keystroke recording overlays, including variants of:

```cpp
m_recClicks
m_recKeystrokes
m_clickOptionsOpen
m_keystrokeOptionsOpen
m_clickPreviews
m_showKeystrokePreview
m_clickSize
m_clickColor
m_clickStyle
m_clickAnimate
m_keySize
m_keyPosition
m_keyAppearance
m_keyFilter
m_keySliderDragging
m_keySliderTrackRect
m_keystrokeOptionsPanelRect
m_keystrokeOptionsClickableRects
m_clickOptionsPanelRect
m_clickOptionsClickableRects
```

Verify exact names before editing.

### 2.3 Remove enum variants

From `RecordPanelTile`, remove:

```cpp
Click
Keystrokes
```

Then update layout/hit-testing code to not allocate rectangles for those tiles.

### 2.4 Remove drawing

Remove:

- click tile drawing
- keystroke tile drawing
- settings rows:
  - `Highlight clicks`
  - `Show keystrokes`
- `drawKeystrokeOptions`
- `drawKeystrokePreview`
- click options drawing if present
- disabled/coming-soon keystroke badge

### 2.5 Remove event handling

Remove:

- click options panel click handling
- keystroke options panel click handling
- toggling click/keystroke tiles
- key preview capture
- slider dragging for click/key size
- hover handling for removed panels

Preserve:

- normal selection mouse events
- toolbar clicks
- window selection
- webcam context menu
- settings menu
- countdown behavior
- ESC behavior

### 2.6 Update JSON result contract

In `capture-overlay/src/main.cpp`, remove output fields:

```json
"clicks"
"keystrokes"
```

Only do this if Rust parsing already no longer expects them.

Recommended low-breakage approach:

1. First hard-code both as false if keeping compatibility is needed.
2. Once native parsing no longer expects them, remove fields in a separate commit.

### 2.7 Verify Phase 2

```bash
cargo check
rg "recClicks|recKeystrokes|recordClicks|recordKeystrokes|m_recClicks|m_recKeystrokes|Keystrokes|keystroke|clickOptions|clickPreviews" capture-overlay
```

Also run the repo-specific Qt build command if available.

---

## Phase 3 — Recording request / config / settings verification

Some parts already appear cleaned, but verify no hidden parser/config expectations remain.

### Files to inspect

- `src/capture_overlay.rs`
- `src/config.rs`
- `src/settings/recording.rs`
- `src/recording/mod.rs`
- `src/gnome_shell.rs`

### 3.1 Ensure capture overlay parser does not require click/key fields

If Qt output still includes hard-coded false, Rust can ignore them.

If Qt output removes them, ensure parsing does not require:

```rust
clicks
keystrokes
rec_clicks
rec_keystrokes
```

### 3.2 Ensure persisted config compatibility

Existing user config may contain old keys. Confirm deserialization ignores unknown fields.

Old keys likely include:

```text
rec_clicks
rec_keystrokes
click_size
click_color
click_style
click_animate
```

No migration is needed if serde ignores unknown fields. If any config structs deny unknown fields, add compatibility handling.

### 3.3 Verify settings UI

Ensure recording settings no longer show:

- highlight clicks
- show keystrokes
- click styling
- keystroke styling

Search:

```bash
rg "Highlight clicks|Show keystrokes|Keystrokes|click_size|click_style|click_animate" src/settings src/config.rs
```

---

## Phase 4 — GNOME extension final cleanup

Runtime code is mostly cleaned, but docs and possible compatibility stubs remain.

### Files

- `gnome-extension/runtime-overlays.js`
- `gnome-extension/session-state.js`
- `gnome-extension/controls-ui.js`
- `gnome-extension/extension.js`
- `gnome-extension/SUBMISSION_GUIDE.md`
- `gnome-extension/screenshots/README.md`

### 4.1 Keep runtime compatibility where useful

It is okay for `runtime-overlays.js` to contain comments like:

```js
// Click and keystroke runtime overlays were removed.
```

But no active UI, D-Bus method, or event capture path should remain.

### 4.2 Remove outdated docs

Update:

- `gnome-extension/SUBMISSION_GUIDE.md`
- `gnome-extension/screenshots/README.md`

Remove claims that screenshots should show:

- keystroke display
- click display

### 4.3 Verify extension syntax

```bash
node --check gnome-extension/*.js
```

If a test runner exists, also run the repo-specific GNOME extension tests.

---

## Phase 5 — Dependency cleanup

### 5.1 Remove unused Rust dependencies

From `Cargo.toml`, remove:

```toml
reis = { version = "0.6.1", features = ["tokio"] }
xkbcommon = "0.9.0"
```

Do **not** remove `x11rb`; it is still used by:

- `src/backend/x11.rs`
- `src/recording/stop_overlay.rs`
- `src/capture/preview_overlay.rs`
- `src/overlay/window.rs`
- editor window code

### 5.2 Update lockfile

```bash
cargo check
```

This should update `Cargo.lock`.

Verify:

```bash
rg "reis|xkbcommon" Cargo.toml Cargo.lock src
```

Expected: no references.

---

## Phase 6 — Tests and final cleanup

### 6.1 Run Rust checks

```bash
cargo fmt --all -- --check
cargo check
cargo test
cargo clippy --workspace --all-targets
```

CI currently does not deny clippy warnings; do not block on unrelated pre-existing warnings unless new ones are introduced.

### 6.2 Run JS checks

```bash
node --check gnome-extension/*.js
```

If package scripts exist, run the relevant test command, for example:

```bash
pnpm test
```

### 6.3 Final search checklist

```bash
rg "rec_clicks|rec_keystrokes|clicks_enabled|keystrokes_enabled|PushKeystroke|click_overlay|runtime_keystrokes|ClickOverlay|Keystroke|keystroke|click_options|click_previews|recClicks|recKeystrokes|recordClicks|recordKeystrokes" src gnome-extension capture-overlay Cargo.toml Cargo.lock
```

Acceptable remaining hits:

- generic normal `click` comments/input handlers
- historical changelog/docs if intentionally retained
- compatibility comments saying click/keystroke overlays were removed

No acceptable hits:

- active recording overlay state
- active UI controls
- active rendering
- active capture/forwarding
- active request fields

---

## Recommended commit structure

Use multiple safe commits rather than one giant commit.

### Commit 1

```text
Remove GTK recording click/keystroke controls
```

### Commit 2

```text
Remove Qt capture overlay click/keystroke controls
```

### Commit 3

```text
Clean up GNOME extension click/keystroke overlay docs
```

### Commit 4

```text
Remove unused keystroke capture dependencies
```

### Commit 5, if needed

```text
Update tests after click/keystroke overlay removal
```

---

## Key risk areas

### Highest risk

`capture-overlay/src/CaptureOverlay_Events.cpp`

This file mixes removed feature handling with core mouse/key handling. Remove only blocks clearly tied to:

- `m_recClicks`
- `m_recKeystrokes`
- `m_clickOptionsOpen`
- `m_keystrokeOptionsOpen`
- `m_showKeystrokePreview`

### Medium risk

`src/overlay/window.rs`

It has many generic click paths. Avoid broad deletion. Use targeted edits around removed state fields.

### Low risk

Dependency cleanup and docs.

---

## Success definition

The work is complete when:

- No click/keystroke recording toggles appear in GTK overlay.
- No click/keystroke recording toggles appear in Qt capture overlay.
- Recordings start without click overlay rendering.
- GNOME shell controls still show recording controls/webcam/mic/speaker.
- No keystroke permission/capture path remains.
- `reis` and `xkbcommon` are removed.
- Full search checklist has no active feature references.
