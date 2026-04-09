# Obfuscate Side Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move Obfuscate method selection into a single `Obfuscate` sidepanel tab while keeping the shared toolbar slider in place and preserving the current fixed sidepanel width.

**Architecture:** Build a dedicated Obfuscate inspector surface in `window/mod.rs` using the same shared inspector shell and section helpers as Arrow, Crop, Text, and Number. Route `Tool::Obfuscate` away from the shared `Colors` flow in `toolbar.rs`, render method rows as inspector-native option buttons, and keep the sidepanel width on the existing `BACKGROUND_SIDEBAR_WIDTH` path with no Obfuscate-specific width constant.

**Tech Stack:** Rust, GTK4, existing include-string source tests in `window/mod.rs`, `window/toolbar.rs`, and `ui_support.rs`

---

## File Structure

- Modify: `src/capture/editor/window/mod.rs`
  Responsibility: build the Obfuscate inspector surface, render method rows, sync active selection, and add source tests for routing and width reuse.
- Modify: `src/capture/editor/window/toolbar.rs`
  Responsibility: stop treating Obfuscate as a non-migrated color tool, remove the toolbar method picker from the active toolbar stack, and add routing tests.
- Modify: `src/capture/editor/ui_support.rs`
  Responsibility: add Obfuscate inspector option CSS matching the migrated tool panels and assert width reuse stays on the shared inspector shell.

### Task 1: Lock routing and width behavior with failing source tests

**Files:**
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add a failing toolbar routing test**

Add a test near the existing inspector-routing tests in `src/capture/editor/window/toolbar.rs`:

```rust
#[test]
fn obfuscate_routes_to_a_dedicated_primary_tab_instead_of_shared_colors() {
    let source = include_str!("toolbar.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("Tool::Obfuscate")
            && production_source.contains("\"Obfuscate\"")
            && !production_source.contains("Tool::Obfuscate\")")
            && !production_source.contains("non_migrated_color_tools_default_to_colors_inspector_surface"),
        "Obfuscate should stop using the shared Colors inspector flow and route to its own primary tab",
    );
}
```

- [ ] **Step 2: Add a failing inspector-structure test**

Add a test near the existing Crop/Arrow/Text tests in `src/capture/editor/window/mod.rs`:

```rust
#[test]
fn obfuscate_inspector_renders_method_section_and_reuses_shared_sidebar_width() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("let (obfuscate_inspector, obfuscate_inspector_content) = build_tool_inspector();")
            && production_source.contains("\"Method\"")
            && production_source.contains("sync_obfuscate_option_selection(&obfuscate_method_list")
            && production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
            && !production_source.contains("OBFUSCATE_SIDEBAR_WIDTH"),
        "Obfuscate should render a Method section while reusing the shared fixed sidebar width",
    );
}
```

- [ ] **Step 3: Add a failing CSS contract test**

Add a test near the Arrow/Text/Crop CSS assertions in `src/capture/editor/ui_support.rs`:

```rust
#[test]
fn obfuscate_inspector_option_rows_match_migrated_tool_panels_without_new_width_path() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("button.editor-obfuscate-inspector-option.editor-obfuscate-inspector-option-active {\n                background: rgba(255, 255, 255, 0.08);")
            && production_source.contains(".editor-obfuscate-inspector-check {\n                color: #ff9900;")
            && production_source.contains(".editor-right-inspector {\n                min-width: 210px;\n                width: 210px;\n                max-width: 210px;")
            && !production_source.contains("OBFUSCATE_SIDEBAR_WIDTH"),
        "Obfuscate inspector rows should use the shared sidepanel language without introducing a new width path",
    );
}
```

- [ ] **Step 4: Run the focused red tests**

Run:

```bash
cargo test obfuscate_routes_to_a_dedicated_primary_tab_instead_of_shared_colors --lib
cargo test obfuscate_inspector_renders_method_section_and_reuses_shared_sidebar_width --lib
cargo test obfuscate_inspector_option_rows_match_migrated_tool_panels_without_new_width_path --lib
```

Expected: FAIL because Obfuscate still routes through the shared `Colors` path and has no dedicated inspector CSS yet.

- [ ] **Step 5: Commit the red-state tests**

Run:

```bash
git add src/capture/editor/window/toolbar.rs src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "test: define obfuscate side panel contract"
```

### Task 2: Build the Obfuscate inspector surface and selection syncing

**Files:**
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Add an Obfuscate selection sync helper**

Add a helper beside the Arrow/Crop/Text sync helpers in `src/capture/editor/window/mod.rs`:

```rust
fn sync_obfuscate_option_selection(list: &GtkBox, selected_index: usize) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();
        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        if index == selected_index {
            button.add_css_class("editor-obfuscate-inspector-option-active");
        } else {
            button.remove_css_class("editor-obfuscate-inspector-option-active");
        }

        if let Some(content) = button.child() {
            if let Ok(row) = content.downcast::<GtkBox>() {
                if let Some(check_icon) = row.last_child() {
                    if let Ok(widget) = check_icon.downcast::<gtk4::Widget>() {
                        widget.set_visible(index == selected_index);
                    }
                }
            }
        }

        index += 1;
    }
}
```

- [ ] **Step 2: Rebuild the Obfuscate method list as inspector-native rows**

Replace the toolbar-popover style method row creation in `src/capture/editor/window/mod.rs` with direct inspector rows using the current state’s selected method:

```rust
let selected_obfuscate_method = {
    let st = state.lock().unwrap();
    st.obfuscate_method()
};

for (index, method) in ObfuscateMethod::ALL.iter().enumerate() {
    let btn_box = GtkBox::new(Orientation::Horizontal, 8);
    btn_box.set_margin_start(8);
    btn_box.set_margin_end(8);
    btn_box.set_margin_top(4);
    btn_box.set_margin_bottom(4);

    let label_widget = Label::new(Some(method.display_name()));
    label_widget.set_hexpand(true);
    label_widget.set_xalign(0.0);

    let check_icon = Label::new(Some("✓"));
    check_icon.set_visible(*method == selected_obfuscate_method);
    check_icon.add_css_class("editor-obfuscate-inspector-check");

    btn_box.append(&label_widget);
    btn_box.append(&check_icon);

    let btn = Button::builder()
        .has_frame(false)
        .css_classes([
            "editor-popover-list-item",
            "flat",
            "editor-obfuscate-inspector-option",
        ])
        .child(&btn_box)
        .build();
    if *method == selected_obfuscate_method {
        btn.add_css_class("editor-obfuscate-inspector-option-active");
    }

    obfuscate_method_list.append(&btn);
}
```

- [ ] **Step 3: Build the dedicated Obfuscate inspector surface**

Add the new inspector surface in `src/capture/editor/window/mod.rs` using the shared helper and the existing width path:

```rust
let (obfuscate_inspector, obfuscate_inspector_content) = build_tool_inspector();
obfuscate_method_list.add_css_class("editor-inspector-option-list");
append_inspector_section(
    &obfuscate_inspector_content,
    "Method",
    obfuscate_method_list.upcast_ref(),
);
```

Then register it in the inspector stack:

```rust
obfuscate_inspector.set_visible(true);
inspector_stack.add_named(&obfuscate_inspector, Some("obfuscate"));
```

- [ ] **Step 4: Wire method clicks to existing state without changing the slider path**

Each method row click should continue using the current state setter and draw refresh path:

```rust
let state = state.clone();
let drawing_area = drawing_area.clone();
let obfuscate_method_list_sync = obfuscate_method_list.clone();
btn.connect_clicked(move |_| {
    {
        let mut st = state.lock().unwrap();
        st.set_obfuscate_method(*method);
    }
    sync_obfuscate_option_selection(&obfuscate_method_list_sync, index);
    drawing_area.queue_draw();
});
```

Do not move or duplicate `current_obfuscate_amount` controls into the inspector.

- [ ] **Step 5: Run focused structure tests**

Run:

```bash
cargo test obfuscate_inspector_renders_method_section_and_reuses_shared_sidebar_width --lib
cargo test crop_arrow_text_and_number_route_to_tool_specific_inspector_tabs --lib
```

Expected: PASS for the new Obfuscate structure test and still PASS for the existing multi-tool inspector routing test once Obfuscate is included in the primary-panel set.

- [ ] **Step 6: Commit the inspector surface implementation**

Run:

```bash
git add src/capture/editor/window/mod.rs
git commit -m "feat: add obfuscate inspector panel"
```

### Task 3: Update toolbar routing and remove the toolbar method picker

**Files:**
- Modify: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Route Obfuscate to a dedicated primary tab**

Adjust the inspector routing logic in `src/capture/editor/window/toolbar.rs` so Obfuscate no longer follows the non-migrated color-tool path. The route should match the existing tool-specific primary-panel pattern, for example:

```rust
Tool::Obfuscate => Some(("Obfuscate", "obfuscate")),
```

and Obfuscate should be removed from any shared `colors_mode` fallback grouping.

- [ ] **Step 2: Remove the toolbar Obfuscate method picker from the active toolbar stack**

Stop showing the Obfuscate method picker in toolbar mode controls while leaving the shared slider behavior intact. The result should be:

```rust
obfuscate_method_group.set_visible(false);
size_group.set_visible(true);
```

with no replacement toolbar-specific method popover for Obfuscate.

- [ ] **Step 3: Ensure only the single `Obfuscate` tab appears**

Update the tab-label and active-tab behavior in the same routing path so Obfuscate does not surface the shared `Colors` tab. The visible inspector tabs for Obfuscate should resolve to only the primary tab label.

- [ ] **Step 4: Run focused toolbar routing tests**

Run:

```bash
cargo test obfuscate_routes_to_a_dedicated_primary_tab_instead_of_shared_colors --lib
cargo test background_and_color_tools_route_into_shared_colors_inspector --lib
```

Expected: PASS for the new Obfuscate routing test and PASS for the existing shared-colors routing test with Obfuscate removed from the non-migrated set.

- [ ] **Step 5: Commit the routing change**

Run:

```bash
git add src/capture/editor/window/toolbar.rs
git commit -m "feat: route obfuscate into its own inspector tab"
```

### Task 4: Add Obfuscate inspector CSS and width verification

**Files:**
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add Obfuscate inspector option CSS**

Add CSS beside the Arrow/Text inspector rules in `src/capture/editor/ui_support.rs`:

```rust
button.editor-obfuscate-inspector-option {
    border-radius: 8px;
    color: rgba(241, 241, 243, 0.9);
}

button.editor-obfuscate-inspector-option:hover {
    background: rgba(255, 255, 255, 0.04);
    color: #ffffff;
}

button.editor-obfuscate-inspector-option.editor-obfuscate-inspector-option-active {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.08);
}

.editor-obfuscate-inspector-check {
    color: #ff9900;
    font-size: 13px;
    font-weight: 700;
}
```

Do not change:

```rust
.editor-right-inspector {
    min-width: 210px;
    width: 210px;
    max-width: 210px;
}
```

- [ ] **Step 2: Run focused CSS and width tests**

Run:

```bash
cargo test obfuscate_inspector_option_rows_match_migrated_tool_panels_without_new_width_path --lib
cargo test inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width --lib
```

Expected: PASS for the new Obfuscate CSS contract and PASS for the existing shared width assertion.

- [ ] **Step 3: Commit the CSS pass**

Run:

```bash
git add src/capture/editor/ui_support.rs
git commit -m "style: align obfuscate inspector with side panel language"
```

### Task 5: Final verification

**Files:**
- Verify only: `src/capture/editor/window/mod.rs`
- Verify only: `src/capture/editor/window/toolbar.rs`
- Verify only: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Run the targeted Obfuscate test slice**

Run:

```bash
cargo test obfuscate --lib
```

Expected: PASS for the Obfuscate inspector source tests and any existing Obfuscate state tests covered by that filter.

- [ ] **Step 2: Run adjacent inspector regression tests**

Run:

```bash
cargo test arrow_inspector --lib
cargo test text_inspector --lib
cargo test inspector_tabs_use_text_only_active_state_and_colors_panel_matches_background_width --lib
```

Expected: PASS so the migrated tool panels and the shared fixed-width shell remain intact.

- [ ] **Step 3: Review final scope**

Run:

```bash
git diff --stat HEAD~3..HEAD
```

Expected: only `src/capture/editor/window/mod.rs`, `src/capture/editor/window/toolbar.rs`, and `src/capture/editor/ui_support.rs` changed for implementation; no new width constant or slider migration introduced.

- [ ] **Step 4: Commit the verified final state**

Run:

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/toolbar.rs src/capture/editor/ui_support.rs
git commit -m "feat: move obfuscate methods into the side panel"
```

## Self-Review

Spec coverage:
- single `Obfuscate` tab with one `Method` section is covered in Tasks 2 and 3
- no `Colors` tab for Obfuscate is covered in Tasks 1 and 3
- toolbar amount slider stays unchanged is explicitly preserved in Tasks 2 and 3
- sidepanel width staying unchanged is locked by Tasks 1, 4, and 5

Placeholder scan:
- no `TODO`, `TBD`, or deferred implementation notes remain
- all code-changing steps include concrete code shape or assertions
- all verification steps include exact commands and expected results

Type consistency:
- plan uses existing codebase names: `BACKGROUND_SIDEBAR_WIDTH`, `Tool::Obfuscate`, `set_obfuscate_method`, and `build_tool_inspector`
- new names are consistent across tasks: `sync_obfuscate_option_selection`, `editor-obfuscate-inspector-option`, `editor-obfuscate-inspector-option-active`, and `editor-obfuscate-inspector-check`
