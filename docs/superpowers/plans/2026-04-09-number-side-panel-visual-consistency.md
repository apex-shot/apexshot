# Number Side Panel Visual Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make both the `Style` and `Size` sections of the Number inspector visually match the migrated sidepanel panels without changing Number behavior, routing, or sidepanel width.

**Architecture:** Keep the change local to Number inspector row construction in `window/mod.rs` and Number-specific CSS in `ui_support.rs`. Rebuild both Number lists using the same inspector-native row composition pattern already used by the migrated sidepanel tools, and keep the sidepanel width on the shared `BACKGROUND_SIDEBAR_WIDTH` path with no Number-specific width constant.

**Tech Stack:** Rust, GTK4, existing include-string source tests in `window/mod.rs` and `ui_support.rs`

---

## File Structure

- Modify: `src/capture/editor/window/mod.rs`
  Responsibility: build Number `Style` and `Size` rows with a unified inspector-native row composition and preserve existing active-state behavior.
- Modify: `src/capture/editor/ui_support.rs`
  Responsibility: add Number-specific CSS and source assertions for unified Number row styling while protecting the shared sidepanel width path.

### Task 1: Lock the Number styling contract with failing tests

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add a failing structure test for Number row composition**

Add a test near the Text/Obfuscate row tests in `src/capture/editor/window/mod.rs`:

```rust
#[test]
fn number_inspector_style_and_size_rows_use_matching_row_composition() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("editor-number-style-check")
            && production_source.contains("editor-number-size-check")
            && production_source.contains("editor-number-style-option-active")
            && production_source.contains("editor-number-size-option-active")
            && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
            && !production_source.contains("NUMBER_SIDEBAR_WIDTH"),
        "Number Style and Size rows should share the same inspector-native composition while reusing the shared sidebar width",
    );
}
```

- [ ] **Step 2: Add a failing CSS contract test**

Add a test near the Arrow/Text/Obfuscate CSS tests in `src/capture/editor/ui_support.rs`:

```rust
#[test]
fn number_inspector_rows_match_migrated_sidepanel_surface_language_without_new_width_path() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("button.editor-number-style-option.editor-number-style-option-active {\n                background: rgba(255, 255, 255, 0.08);")
            && production_source.contains("button.editor-number-size-option.editor-number-size-option-active {\n                background: rgba(255, 255, 255, 0.08);")
            && production_source.contains(".editor-number-style-check {\n                color: #ff9900;")
            && production_source.contains(".editor-number-size-check {\n                color: #ff9900;")
            && production_source.contains(".editor-right-inspector {\n                min-width: 210px;\n                width: 210px;\n                max-width: 210px;")
            && !production_source.contains("NUMBER_SIDEBAR_WIDTH"),
        "Number inspector rows should match the migrated sidepanel surface language without introducing a new width path",
    );
}
```

- [ ] **Step 3: Run the focused red tests**

Run:

```bash
cargo test number_inspector_style_and_size_rows_use_matching_row_composition --lib
cargo test number_inspector_rows_match_migrated_sidepanel_surface_language_without_new_width_path --lib
```

Expected: FAIL because Number `Style` and `Size` do not yet share the same row composition or active-row surface language.

- [ ] **Step 4: Commit the red-state tests**

Run:

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "test: define number inspector visual consistency contract"
```

### Task 2: Rebuild Number `Style` and `Size` rows with unified inspector composition

**Files:**
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Update Number `Style` rows to use the migrated sidepanel pattern**

Replace the current mixed Image-based selected marker row with the same label-plus-check row model used by the migrated panels:

```rust
let btn_box = GtkBox::new(Orientation::Horizontal, 8);
btn_box.set_margin_start(8);
btn_box.set_margin_end(8);
btn_box.set_margin_top(4);
btn_box.set_margin_bottom(4);

let label = Label::new(Some(style.label()));
label.set_hexpand(true);
label.set_xalign(0.0);

let check_icon = Label::new(Some("✓"));
check_icon.set_visible(style == super::numbering_style::NumberingStyle::default());
check_icon.add_css_class("editor-number-style-check");

btn_box.append(&label);
btn_box.append(&check_icon);
```

Keep the existing `editor-number-style-option` button class and existing state behavior.

- [ ] **Step 2: Update Number `Size` rows to use the same row model**

Replace the plain label-only button creation for `number_size_list` in `src/capture/editor/window/mod.rs` with the same label-plus-check structure:

```rust
let btn_box = GtkBox::new(Orientation::Horizontal, 8);
btn_box.set_margin_start(8);
btn_box.set_margin_end(8);
btn_box.set_margin_top(4);
btn_box.set_margin_bottom(4);

let label = Label::new(Some(size.label()));
label.set_hexpand(true);
label.set_xalign(0.0);

let check_icon = Label::new(Some("✓"));
check_icon.set_visible(size == super::numbering_style::NumberSize::default());
check_icon.add_css_class("editor-number-size-check");

btn_box.append(&label);
btn_box.append(&check_icon);
```

Build the button with:

```rust
let btn = Button::builder()
    .has_frame(false)
    .css_classes([
        "editor-popover-list-item",
        "flat",
        "editor-number-size-option",
    ])
    .child(&btn_box)
    .build();
```

- [ ] **Step 3: Ensure active classes are applied for both sections**

For the currently selected/default rows, add the matching active classes in `src/capture/editor/window/mod.rs`:

```rust
btn.add_css_class("editor-number-style-option-active");
```

and:

```rust
btn.add_css_class("editor-number-size-option-active");
```

This step must not alter any Number state update or routing logic.

- [ ] **Step 4: Run the focused structure test**

Run:

```bash
cargo test number_inspector_style_and_size_rows_use_matching_row_composition --lib
```

Expected: PASS once both Number sections use the same migrated row composition pattern.

- [ ] **Step 5: Commit the Number row composition update**

Run:

```bash
git add src/capture/editor/window/mod.rs
git commit -m "feat: align number inspector row composition"
```

### Task 3: Align Number CSS with the other migrated sidepanels

**Files:**
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add Number `Style` and `Size` row surface CSS**

Add Number-specific CSS beside the Arrow/Text/Obfuscate inspector rules in `src/capture/editor/ui_support.rs`:

```rust
button.editor-number-style-option,
button.editor-number-size-option {
    border-radius: 8px;
    color: rgba(241, 241, 243, 0.9);
}

button.editor-number-style-option:hover,
button.editor-number-size-option:hover {
    background: rgba(255, 255, 255, 0.04);
    color: #ffffff;
}

button.editor-number-style-option.editor-number-style-option-active,
button.editor-number-size-option.editor-number-size-option-active {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.08);
}

.editor-number-style-check,
.editor-number-size-check {
    color: #ff9900;
    font-size: 13px;
    font-weight: 700;
}
```

Do not modify:

```rust
.editor-right-inspector {
    min-width: 210px;
    width: 210px;
    max-width: 210px;
}
```

- [ ] **Step 2: Run the focused CSS contract test**

Run:

```bash
cargo test number_inspector_rows_match_migrated_sidepanel_surface_language_without_new_width_path --lib
```

Expected: PASS once both Number sections share the same migrated sidepanel surface language.

- [ ] **Step 3: Run the shared width regression**

Run:

```bash
cargo test inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width --lib
```

Expected: PASS, proving the Number styling pass did not alter the shared sidepanel width path.

- [ ] **Step 4: Commit the Number CSS pass**

Run:

```bash
git add src/capture/editor/ui_support.rs
git commit -m "style: align number inspector with sidepanel surfaces"
```

### Task 4: Final verification

**Files:**
- Verify only: `src/capture/editor/window/mod.rs`
- Verify only: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Run the targeted Number visual test slice**

Run:

```bash
cargo test number_inspector --lib
```

Expected: PASS for the new Number visual tests. If unrelated pre-existing Number behavior tests still fail under this broad filter, note them separately and rely on the focused new tests as direct verification of this styling change.

- [ ] **Step 2: Run adjacent migrated-panel regression tests**

Run:

```bash
cargo test arrow_inspector --lib
cargo test text_inspector --lib
```

Expected: PASS so Number styling changes do not regress the already-migrated panel styling patterns.

- [ ] **Step 3: Run `cargo check`**

Run:

```bash
cargo check
```

Expected: PASS with no new warnings introduced by the Number styling changes.

- [ ] **Step 4: Review final scope**

Run:

```bash
git diff --stat HEAD~2..HEAD
```

Expected: only `src/capture/editor/window/mod.rs` and `src/capture/editor/ui_support.rs` changed for implementation, with no new width constant or Number routing changes introduced.

- [ ] **Step 5: Commit the verified final state**

Run:

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "feat: unify number inspector visuals"
```

## Self-Review

Spec coverage:
- visual consistency for both Number `Style` and `Size` sections is covered in Tasks 2 and 3
- no behavior, routing, or width changes are preserved by the scope of Tasks 2 through 4
- shared sidepanel width protection is explicitly covered in Tasks 1, 3, and 4

Placeholder scan:
- no `TODO`, `TBD`, or deferred implementation notes remain
- each code-changing step includes the concrete code shape to add
- each verification step includes exact commands and expected results

Type consistency:
- plan uses existing codebase names: `editor-number-style-option`, `editor-number-size-option`, `BACKGROUND_SIDEBAR_WIDTH`, `NumberingStyle::ALL`, and `NumberSize::ALL`
- new names are consistent across tasks: `editor-number-style-option-active`, `editor-number-size-option-active`, `editor-number-style-check`, and `editor-number-size-check`
