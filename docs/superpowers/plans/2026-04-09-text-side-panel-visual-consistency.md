# Text Side Panel Visual Consistency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Text inspector's `Size` and `Font` rows visually match the Arrow sidepanel without changing Text behavior, section order, routing, or sidepanel width.

**Architecture:** Keep the change local to the right-inspector UI layer. Build Text inspector rows in `window/mod.rs` using the same row composition pattern as Arrow, add Text-specific active-state syncing in `window/events.rs`, and define matching Text CSS in `ui_support.rs`. The shared fixed width path must remain `BACKGROUND_SIDEBAR_WIDTH`; do not introduce a Text-specific width constant or change the inspector shell width.

**Tech Stack:** Rust, GTK4, existing include-string source tests in `window/mod.rs` and `ui_support.rs`

---

## File Structure

- Modify: `src/capture/editor/window/mod.rs`
  Responsibility: build Text inspector rows, add Text row active-state helpers, preserve existing `BACKGROUND_SIDEBAR_WIDTH` usage.
- Modify: `src/capture/editor/window/events.rs`
  Responsibility: sync Text inspector active rows anywhere the editor already syncs `text_size_label` and `font_family_label`.
- Modify: `src/capture/editor/ui_support.rs`
  Responsibility: add Text inspector CSS and source-level assertions for the new visual language while preserving the current fixed inspector width assertions.

### Task 1: Lock the width and visual contract with failing source tests

**Files:**
- Modify: `src/capture/editor/ui_support.rs`
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Add a failing CSS contract test for Text inspector visuals**

Add a test near the existing Arrow and Crop inspector CSS tests in `src/capture/editor/ui_support.rs`:

```rust
#[test]
fn text_inspector_option_rows_match_arrow_visual_language_without_changing_panel_width() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("button.editor-text-inspector-option.editor-text-inspector-option-active {\n                background: rgba(255, 255, 255, 0.08);")
            && production_source.contains(".editor-text-inspector-check {\n                color: #ff9900;")
            && production_source.contains(".editor-right-inspector {\n                min-width: 210px;\n                width: 210px;\n                max-width: 210px;")
            && !production_source.contains("TEXT_SIDEBAR_WIDTH"),
        "Text inspector rows should mirror Arrow selection styling without introducing a new sidepanel width path",
    );
}
```

- [ ] **Step 2: Add a failing structure test for Text row composition**

Add a test near the existing Arrow inspector tests in `src/capture/editor/window/mod.rs`:

```rust
#[test]
fn text_inspector_rows_use_label_plus_tick_layout_and_shared_sidebar_width() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("check_icon.add_css_class(\"editor-text-inspector-check\");")
            && production_source.contains("btn.add_css_class(\"editor-text-inspector-option-active\");")
            && production_source.contains("sync_text_option_selection(&text_size_list")
            && production_source.contains("sync_text_option_selection(&font_family_list")
            && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
            && !production_source.contains("TEXT_SIDEBAR_WIDTH"),
        "Text inspector rows should use explicit selected ticks while reusing the existing fixed sidebar width",
    );
}
```

- [ ] **Step 3: Run the focused failing tests**

Run:

```bash
cargo test text_inspector_option_rows_match_arrow_visual_language_without_changing_panel_width text_inspector_rows_use_label_plus_tick_layout_and_shared_sidebar_width --lib -- src/capture/editor/ui_support.rs src/capture/editor/window/mod.rs
```

Expected: FAIL because the new Text-specific classes and sync helper do not exist yet.

- [ ] **Step 4: Commit the test-only red state**

Run:

```bash
git add src/capture/editor/ui_support.rs src/capture/editor/window/mod.rs
git commit -m "test: define text inspector visual consistency contract"
```

### Task 2: Build Text inspector rows and active-state syncing

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/events.rs`

- [ ] **Step 1: Add a Text row sync helper in `window/mod.rs`**

Add a helper beside `sync_arrow_option_selection` / `sync_crop_option_selection`:

```rust
fn sync_text_option_selection(list: &GtkBox, selected_index: Option<usize>, active_class: &str) {
    let mut index = 0usize;
    let mut child = list.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if let Ok(button) = widget.clone().downcast::<Button>() {
            let is_active = selected_index == Some(index);
            if is_active {
                button.add_css_class(active_class);
            } else {
                button.remove_css_class(active_class);
            }

            if let Some(content) = button.child() {
                if let Ok(row) = content.downcast::<GtkBox>() {
                    if let Some(check) = row.last_child().and_then(|w| w.downcast::<Label>().ok()) {
                        check.set_visible(is_active);
                    }
                }
            }
            index += 1;
        }
        child = next;
    }
}
```

- [ ] **Step 2: Rebuild Text `Size` and `Font` rows using Arrow-style row content**

Replace the plain labeled button construction in `src/capture/editor/window/mod.rs` with row boxes like:

```rust
let btn_box = GtkBox::new(Orientation::Horizontal, 8);
btn_box.set_margin_start(8);
btn_box.set_margin_end(8);
btn_box.set_margin_top(4);
btn_box.set_margin_bottom(4);

let label_widget = Label::new(Some(&label));
label_widget.set_hexpand(true);
label_widget.set_xalign(0.0);

let check_icon = Label::new(Some("✓"));
check_icon.set_visible(size == selected_size);
check_icon.add_css_class("editor-text-inspector-check");

btn_box.append(&label_widget);
btn_box.append(&check_icon);

let btn = Button::builder()
    .has_frame(false)
    .css_classes([
        "editor-popover-list-item",
        "flat",
        "editor-text-inspector-option",
    ])
    .child(&btn_box)
    .build();
if size == selected_size {
    btn.add_css_class("editor-text-inspector-option-active");
}
```

Use the current editor state to derive `selected_size` and `selected_font_family` before populating the lists:

```rust
let (selected_size, selected_font_family) = {
    let st = state.lock().unwrap();
    (
        Some(st.selected_text_action_size().unwrap_or(st.text_size) as i32),
        Some(st.selected_text_font_family().unwrap_or_else(|| st.text_font_family.clone())),
    )
};
```

- [ ] **Step 3: Sync Text row visuals on click in `window/mod.rs`**

Extend each Text row click handler so it updates the row selection immediately after mutating state:

```rust
sync_text_option_selection(
    &text_size_list,
    [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72]
        .iter()
        .position(|candidate| *candidate == size),
    "editor-text-inspector-option-active",
);
```

and:

```rust
sync_text_option_selection(
    &font_family_list,
    ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"]
        .iter()
        .position(|candidate| *candidate == family_str.as_str()),
    "editor-text-inspector-option-active",
);
```

- [ ] **Step 4: Pass Text inspector lists into `EventContext`**

Add fields to `src/capture/editor/window/events.rs`:

```rust
pub text_size_list: gtk4::Box,
pub font_family_list: gtk4::Box,
```

Then populate them from `setup_editor_window` in `src/capture/editor/window/mod.rs`:

```rust
text_size_list: text_size_list.clone(),
font_family_list: font_family_list.clone(),
```

- [ ] **Step 5: Mirror Text row sync in `wire_editor_events`**

Anywhere `wire_editor_events` already updates `text_size_label` and `font_family_label`, also update the active row classes:

```rust
sync_text_option_selection(
    &text_size_list_click,
    [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72]
        .iter()
        .position(|candidate| *candidate == size as i32),
    "editor-text-inspector-option-active",
);
sync_text_option_selection(
    &font_family_list_click,
    ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"]
        .iter()
        .position(|candidate| *candidate == family.as_str()),
    "editor-text-inspector-option-active",
);
```

Cover at least these paths:
- selected text action on click
- text re-edit / double-click selection path
- empty-area new text path fallback state

- [ ] **Step 6: Run the focused structure tests**

Run:

```bash
cargo test text_inspector_rows_use_label_plus_tick_layout_and_shared_sidebar_width arrow_inspector_style_and_thickness_rows_include_tick_indicators --lib -- src/capture/editor/window/mod.rs
```

Expected: PASS for the new Text structure test and still PASS for the existing Arrow test.

- [ ] **Step 7: Commit the Text row behavior wiring**

Run:

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs
git commit -m "feat: sync text inspector active option rows"
```

### Task 3: Add Text inspector CSS and verify width is unchanged

**Files:**
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add Text inspector CSS matching Arrow's surface language**

Add CSS beside the Arrow inspector rules in `src/capture/editor/ui_support.rs`:

```rust
button.editor-text-inspector-option {
    border-radius: 8px;
    color: rgba(241, 241, 243, 0.9);
}

button.editor-text-inspector-option:hover {
    background: rgba(255, 255, 255, 0.04);
    color: #ffffff;
}

button.editor-text-inspector-option.editor-text-inspector-option-active {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.08);
}

.editor-text-inspector-check {
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

- [ ] **Step 2: Run the focused CSS tests**

Run:

```bash
cargo test text_inspector_option_rows_match_arrow_visual_language_without_changing_panel_width inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width arrow_inspector_active_option_uses_subtle_surface_and_orange_tick --lib -- src/capture/editor/ui_support.rs
```

Expected: PASS for the new Text CSS test, PASS for the shared fixed-width inspector test, and PASS for the existing Arrow styling test.

- [ ] **Step 3: Commit the CSS pass**

Run:

```bash
git add src/capture/editor/ui_support.rs
git commit -m "style: align text inspector rows with arrow panel"
```

### Task 4: Final verification

**Files:**
- Verify only: `src/capture/editor/window/mod.rs`
- Verify only: `src/capture/editor/window/events.rs`
- Verify only: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Run the targeted editor test slice**

Run:

```bash
cargo test text_inspector --lib
```

Expected: PASS for the new Text inspector tests.

- [ ] **Step 2: Run the adjacent inspector regression tests**

Run:

```bash
cargo test arrow_inspector crop_inspector inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width --lib
```

Expected: PASS so Arrow, Crop, and shared inspector width assertions remain intact.

- [ ] **Step 3: Review the final diff for scope control**

Run:

```bash
git diff --stat HEAD~3..HEAD
```

Expected: only `src/capture/editor/window/mod.rs`, `src/capture/editor/window/events.rs`, and `src/capture/editor/ui_support.rs` changed for implementation; no new width constants or inspector shell width changes introduced.

- [ ] **Step 4: Commit the verified final state**

Run:

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/ui_support.rs
git commit -m "feat: unify text inspector visuals with arrow panel"
```

## Self-Review

Spec coverage:
- visual parity for Text `Size` and `Font` rows is covered in Tasks 2 and 3
- active-state treatment for current Text size and font is covered in Tasks 2 and 4
- sidepanel width staying unchanged is locked by Tasks 1, 3, and 4
- no routing, ordering, or behavior changes are introduced because all steps stay within existing state-update and inspector-shell paths

Placeholder scan:
- no `TODO`, `TBD`, or deferred implementation notes remain
- every code-changing step includes the concrete code shape or assertion to add
- every verification step has an explicit command and expected result

Type consistency:
- plan uses existing names from the codebase: `BACKGROUND_SIDEBAR_WIDTH`, `text_size_list`, `font_family_list`, `selected_text_action_size`, `selected_text_font_family`, and `set_selected_text_font_family`
- new names are consistent across tasks: `sync_text_option_selection`, `editor-text-inspector-option`, `editor-text-inspector-option-active`, and `editor-text-inspector-check`
