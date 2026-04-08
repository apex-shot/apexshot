# Crop Side Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Crop’s top-toolbar controls with a right-side inspector that has `Crop` and `Colors` tabs, while preserving the existing explicit crop apply workflow.

**Architecture:** Reuse the existing crop state, apply handler, and crop fill color state instead of introducing a parallel Crop panel model. Build a dedicated Crop inspector surface in `window/mod.rs`, route Crop through the same inspector tab shell used by Arrow, and remove Crop-specific toolbar groups from `window/toolbar.rs` once the inspector controls are live. The implementation must reuse the existing fixed right-inspector width and must not add any new width constant, width override, or panel expansion behavior.

**Tech Stack:** Rust, GTK4, existing Apexshot editor state/event architecture, source-level unit tests in `window/mod.rs`, `window/toolbar.rs`, and `ui_support.rs`

---

## File Structure

- Modify: `src/capture/editor/window/mod.rs`
  - Build the Crop inspector surface, populate aspect ratio rows, dimensions section, reset/apply actions, and route Crop into the right inspector without changing the existing inspector shell width.
- Modify: `src/capture/editor/window/events.rs`
  - Reuse existing crop state transitions from side-panel controls and preserve current apply behavior.
- Modify: `src/capture/editor/window/toolbar.rs`
  - Remove Crop-specific toolbar mode controls while keeping the Crop tool button.
- Modify: `src/capture/editor/ui_support.rs`
  - Add any inspector CSS needed for Crop rows, dimensions display, and action buttons, while keeping Crop `Colors` scoped.
- Optionally modify: `src/capture/editor/state.rs`
  - Only if a narrow helper is required for reset behavior beyond existing fields.
- Test: `src/capture/editor/window/mod.rs`
  - Add source-level tests for Crop inspector sections, routing, and Colors scoping.
- Test: `src/capture/editor/window/toolbar.rs`
  - Add or update source-level tests proving Crop-specific toolbar groups are removed/hidden.
- Test: `src/capture/editor/ui_support.rs`
  - Add CSS presence/assertion tests for Crop inspector styles if new selectors are introduced.

### Task 1: Add failing tests for Crop inspector routing and structure

**Files:**
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add tests near the existing inspector source tests asserting that production code now includes a dedicated Crop inspector and routes Crop to both `crop` and `colors` surfaces:

```rust
    #[test]
    fn crop_routes_to_crop_and_colors_inspector_surfaces() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("inspector_stack.add_named(&crop_inspector, Some(\"crop\"));")
                && production_source.contains("Tool::Crop => Some(\"crop\")")
                && production_source.contains("Tool::Crop")
                && production_source.contains("set_active_inspector_surface(\"colors\");"),
            "Crop should route through a dedicated Crop inspector surface and the shared Colors surface",
        );
    }

    #[test]
    fn crop_inspector_includes_aspect_ratio_dimensions_and_actions_sections() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let (crop_inspector, crop_inspector_content) = build_tool_inspector();")
                && production_source.contains("\"Aspect Ratio\"")
                && production_source.contains("\"Dimensions\"")
                && production_source.contains("\"Actions\""),
            "Crop inspector should render Aspect Ratio, Dimensions, and Actions sections",
        );
    }

    #[test]
    fn crop_inspector_reuses_existing_fixed_sidebar_width() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
                && !production_source.contains("CROP_SIDEBAR_WIDTH")
                && !production_source.contains("crop_inspector.set_width_request("),
            "Crop inspector should reuse the existing fixed sidebar width instead of changing panel width",
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test crop_routes_to_crop_and_colors_inspector_surfaces --lib
cargo test crop_inspector_includes_aspect_ratio_dimensions_and_actions_sections --lib
cargo test crop_inspector_reuses_existing_fixed_sidebar_width --lib
```

Expected: FAIL because the Crop inspector surface and section declarations do not exist yet.

- [ ] **Step 3: Write minimal inspector routing implementation**

In `src/capture/editor/window/mod.rs`, add a dedicated Crop inspector surface using the existing `build_tool_inspector()` pattern and route `Tool::Crop` through it in both the background-tab and colors-tab click handlers.

Use the same shape as existing tool inspectors:

```rust
    let (crop_inspector, crop_inspector_content) = build_tool_inspector();
    append_inspector_section(&crop_inspector_content, "Aspect Ratio", crop_ratio_list.upcast_ref());
    append_inspector_section(&crop_inspector_content, "Dimensions", crop_dimensions_group.upcast_ref());
    append_inspector_section(&crop_inspector_content, "Actions", crop_actions_group.upcast_ref());

    inspector_stack.add_named(&crop_inspector, Some("crop"));
```

And update routing:

```rust
            let surface = match state.lock().unwrap().selected_tool {
                Tool::Background => Some("background"),
                Tool::Crop => Some("crop"),
                Tool::Arrow => Some("arrow"),
                Tool::Text => Some("text"),
                Tool::Number => Some("number"),
                _ => None,
            };
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test crop_routes_to_crop_and_colors_inspector_surfaces --lib
cargo test crop_inspector_includes_aspect_ratio_dimensions_and_actions_sections --lib
cargo test crop_inspector_reuses_existing_fixed_sidebar_width --lib
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs
git commit -m "feat: add crop inspector routing shell"
```

### Task 2: Move aspect ratio and dimensions into the Crop inspector

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Write the failing tests**

Add source tests in `src/capture/editor/window/mod.rs` for the Crop inspector using option rows and a live dimensions display:

```rust
    #[test]
    fn crop_inspector_populates_aspect_ratio_options_from_crop_aspect_ratio_all() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("for crop_type in CropAspectRatio::ALL")
                && production_source.contains("crop_ratio_list.append(&option_button);")
                && production_source.contains("sync_crop_option_selection"),
            "Crop inspector should build aspect ratio rows from CropAspectRatio::ALL and sync the selected option",
        );
    }

    #[test]
    fn crop_dimensions_use_draft_or_selected_crop_rect_in_the_inspector() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("st.draft_crop_rect().or(st.crop_selection)")
                && production_source.contains("crop_width_value.set_label")
                && production_source.contains("crop_height_value.set_label"),
            "Crop dimensions should reflect the active draft or committed crop rect in the side panel",
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test crop_inspector_populates_aspect_ratio_options_from_crop_aspect_ratio_all --lib
cargo test crop_dimensions_use_draft_or_selected_crop_rect_in_the_inspector --lib
```

Expected: FAIL because the inspector still uses the old toolbar widgets.

- [ ] **Step 3: Write minimal implementation**

In `src/capture/editor/window/mod.rs`:
- create a new `crop_ratio_list: GtkBox`
- build inspector option rows for `CropAspectRatio::ALL`
- replace the toolbar-entry-based dimensions display with panel labels such as `crop_width_value` and `crop_height_value`
- add a `sync_crop_option_selection` helper that parallels the Arrow selection syncing pattern

Use inspector row markup rather than menu popovers:

```rust
    let crop_ratio_list = GtkBox::new(Orientation::Vertical, 0);
    crop_ratio_list.add_css_class("editor-inspector-option-list");

    for crop_type in CropAspectRatio::ALL {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(crop_type.label()));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        let tick = Label::new(Some("✓"));
        tick.add_css_class("editor-crop-inspector-check");
        row.append(&label);
        row.append(&tick);
        let option_button = Button::builder()
            .has_frame(false)
            .css_classes(["editor-popover-list-item", "flat", "editor-crop-inspector-option"])
            .child(&row)
            .build();
        crop_ratio_list.append(&option_button);
    }
```

Update the existing crop-size sync closure to target the new labels:

```rust
    if let Some(rect) = st.draft_crop_rect().or(st.crop_selection) {
        crop_width_value.set_label(&rect.width.max(0).to_string());
        crop_height_value.set_label(&rect.height.max(0).to_string());
    } else {
        crop_width_value.set_label("—");
        crop_height_value.set_label("—");
    }
```

In `src/capture/editor/ui_support.rs`, add styles for:
- `button.editor-crop-inspector-option`
- `button.editor-crop-inspector-option.editor-crop-inspector-option-active`
- `.editor-crop-inspector-check`
- dimensions row/value selectors

In `src/capture/editor/window/events.rs`, keep using `st.set_crop_aspect_ratio(crop_type)` and `st.ensure_crop_selection_initialized()` but wire them from the new inspector buttons instead of the old popover items.

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test crop_inspector_populates_aspect_ratio_options_from_crop_aspect_ratio_all --lib
cargo test crop_dimensions_use_draft_or_selected_crop_rect_in_the_inspector --lib
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/ui_support.rs
git commit -m "feat: move crop ratio and dimensions into inspector"
```

### Task 3: Move Reset and Apply into the Crop inspector

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Write the failing tests**

Add source tests in `src/capture/editor/window/mod.rs` and `src/capture/editor/window/toolbar.rs`:

```rust
    #[test]
    fn crop_inspector_actions_include_reset_and_apply_buttons() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("let crop_reset_btn = Button::with_label(\"Reset\");")
                && production_source.contains("let crop_apply_btn = Button::with_label(\"Apply\");"),
            "Crop inspector actions should expose Reset and Apply buttons",
        );
    }
```

```rust
    #[test]
    fn crop_toolbar_mode_group_no_longer_contains_crop_specific_controls() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            !production_source.contains("crop_mode_group.append(&crop_type_group);")
                && !production_source.contains("crop_mode_group.append(&crop_size_group);")
                && !production_source.contains("apply_crop_btn.set_visible(false);"),
            "Crop-specific toolbar controls should be removed from the toolbar once the side panel owns Crop interactions",
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test crop_inspector_actions_include_reset_and_apply_buttons --lib
cargo test crop_toolbar_mode_group_no_longer_contains_crop_specific_controls --lib
```

Expected: FAIL because Crop actions still live in toolbar code.

- [ ] **Step 3: Write minimal implementation**

In `src/capture/editor/window/mod.rs`:
- create `crop_reset_btn` and `crop_apply_btn`
- append them to a `crop_actions_group`
- reuse the current Crop enablement logic by introducing a small shared helper/closure that updates the panel `Apply` button sensitivity

Action shell example:

```rust
    let crop_actions_group = GtkBox::new(Orientation::Horizontal, 8);
    let crop_reset_btn = Button::with_label("Reset");
    crop_reset_btn.add_css_class("editor-secondary-action-button");
    let crop_apply_btn = Button::with_label("Apply");
    crop_apply_btn.add_css_class("editor-done-button");
    crop_apply_btn.set_sensitive(false);
    crop_actions_group.append(&crop_reset_btn);
    crop_actions_group.append(&crop_apply_btn);
```

In `src/capture/editor/window/events.rs`:
- move the current `apply_crop_btn.connect_clicked` behavior to the inspector `crop_apply_btn`
- add `crop_reset_btn.connect_clicked` that clears the active draft/selection using existing crop state fields, updates dimensions, updates button sensitivity, and queues redraw

Use the existing apply logic as the base:

```rust
    crop_apply_btn.connect_clicked(move |_| {
        let apply_result = {
            let mut st = state_apply_crop.lock().unwrap();
            let result = st.apply_crop_selection();
            if result.as_ref().is_ok_and(|applied| *applied) {
                st.set_tool(Tool::Arrow);
            }
            result
        };
        // keep the existing success/failure handling pattern
    });
```

For reset, clear crop state in place:

```rust
    crop_reset_btn.connect_clicked(move |_| {
        let mut st = state_reset_crop.lock().unwrap();
        st.crop_selection = None;
        st.drag_start = None;
        st.drag_current = None;
        drop(st);
        update_crop_size_fields_reset();
        sync_crop_apply_enabled(false);
        if let Some(area) = drawing_area_reset.upgrade() {
            area.queue_draw();
        }
    });
```

In `src/capture/editor/window/toolbar.rs`:
- remove `crop_type_group`, `crop_size_group`, and `crop_mode_group`
- remove the toolbar-right `apply_crop_btn` slot
- keep the Crop tool button itself

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test crop_inspector_actions_include_reset_and_apply_buttons --lib
cargo test crop_toolbar_mode_group_no_longer_contains_crop_specific_controls --lib
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/window/toolbar.rs src/capture/editor/ui_support.rs
git commit -m "feat: move crop actions into inspector"
```

### Task 4: Scope the Crop Colors tab to crop fill only

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Write the failing tests**

Add a source test in `src/capture/editor/window/mod.rs`:

```rust
    #[test]
    fn crop_colors_tab_is_scoped_to_crop_fill_behavior() {
        let source = include_str!("mod.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("Tool::Crop")
                && production_source.contains("sync_colors_panel_for_active_tool();")
                && !production_source.contains("BackgroundAlignment"),
            "Crop colors integration should reuse the shared Colors tab without importing Background-only controls",
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test crop_colors_tab_is_scoped_to_crop_fill_behavior --lib
```

Expected: FAIL if Crop is still excluded from colors routing or if the panel wiring is not explicit enough.

- [ ] **Step 3: Write minimal implementation**

Ensure `Tool::Crop` participates in the shared Colors tab routing in `src/capture/editor/window/mod.rs`:

```rust
            if matches!(
                selected_tool,
                Tool::Crop
                    | Tool::Background
                    | Tool::Pen
                    | Tool::Arrow
                    | Tool::Line
                    | Tool::Box
                    | Tool::Circle
                    | Tool::Text
                    | Tool::Number
                    | Tool::Highlighter
                    | Tool::Obfuscate
                    | Tool::Focus
            ) {
                sync_colors_panel_for_active_tool();
                set_active_inspector_surface("colors");
            }
```

Do not add Background-only sections to Crop. Reuse the existing color update branch already present in `window/events.rs`:

```rust
                if st.selected_tool == Tool::Crop {
                    st.set_crop_background_color(DRAW_COLORS[index]);
                } else if st.selected_tool == Tool::Background {
                    st.background_style = BackgroundStyle::PlainColor(DRAW_COLORS[index]);
                    st.mark_working_image_dirty();
                } else {
                    st.set_color_index(index);
                }
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cargo test crop_colors_tab_is_scoped_to_crop_fill_behavior --lib
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs
git commit -m "feat: route crop through scoped colors tab"
```

### Task 5: Clean up toolbar mode remnants and verify Crop end-to-end behavior

**Files:**
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Write the failing tests**

Add/extend source tests proving the toolbar no longer contains Crop-specific mode stack controls while the Crop button remains:

```rust
    #[test]
    fn toolbar_keeps_crop_tool_button_but_not_crop_mode_stack_controls() {
        let source = include_str!("toolbar.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("crop_tools_group.append(crop_btn);")
                && !production_source.contains("toolbar_mode_stack.add_named(&crop_mode_group, Some(\"crop\"));"),
            "Toolbar should retain the Crop tool entry point but not the old Crop mode stack",
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test toolbar_keeps_crop_tool_button_but_not_crop_mode_stack_controls --lib
```

Expected: FAIL until the old Crop mode stack entry is removed.

- [ ] **Step 3: Write minimal implementation**

In `src/capture/editor/window/toolbar.rs`:
- remove the Crop mode stack page entirely
- keep `crop_tools_group.append(crop_btn);`
- ensure `build_toolbar_tool_updater` does not depend on Crop-specific toolbar controls anymore

In `src/capture/editor/ui_support.rs`:
- remove unused Crop toolbar-only CSS selectors if they are no longer referenced
- keep only selectors still used by the tool button or any retained Crop affordances

- [ ] **Step 4: Run verification**

Run:

```bash
cargo test crop_routes_to_crop_and_colors_inspector_surfaces --lib
cargo test crop_inspector_includes_aspect_ratio_dimensions_and_actions_sections --lib
cargo test crop_inspector_reuses_existing_fixed_sidebar_width --lib
cargo test crop_inspector_populates_aspect_ratio_options_from_crop_aspect_ratio_all --lib
cargo test crop_dimensions_use_draft_or_selected_crop_rect_in_the_inspector --lib
cargo test crop_inspector_actions_include_reset_and_apply_buttons --lib
cargo test crop_toolbar_mode_group_no_longer_contains_crop_specific_controls --lib
cargo test crop_colors_tab_is_scoped_to_crop_fill_behavior --lib
cargo test toolbar_keeps_crop_tool_button_but_not_crop_mode_stack_controls --lib
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/toolbar.rs src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/ui_support.rs
git commit -m "feat: migrate crop controls into side inspector"
```
