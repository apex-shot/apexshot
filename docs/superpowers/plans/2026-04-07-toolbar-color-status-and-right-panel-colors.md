# Toolbar Color Status And Right Panel Colors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the toolbar color picker with a passive swatch-plus-hex status chip, move all color editing into the right `Colors` tab, and remove duplicated current-color UI from the right panel.

**Architecture:** Keep editor color state as the single source of truth. The toolbar becomes a read-only projection of the active tool color, while the right `Colors` tab remains the only editing surface and is auto-selected when a color-capable tool is chosen.

**Tech Stack:** Rust, GTK4, existing editor state/sync closures, existing right-inspector colors panel, existing toolbar mode controls.

---

### Task 1: Replace Toolbar Picker With Read-Only Status Chip

**Files:**
- Modify: `src/capture/editor/window/color_picker.rs`
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Write the failing toolbar source test**

```rust
#[test]
fn toolbar_uses_read_only_color_status_chip_instead_of_picker_trigger() {
    let source = include_str!("toolbar.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("editor-toolbar-color-status")
            && production_source.contains("editor-toolbar-color-status-swatch")
            && production_source.contains("editor-toolbar-color-status-label")
            && !production_source.contains("color_picker_trigger_host"),
        "Toolbar should use a read-only color status chip instead of the picker trigger host",
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test toolbar_uses_read_only_color_status_chip_instead_of_picker_trigger -- --nocapture`
Expected: FAIL because the toolbar still appends `color_picker_trigger_host`.

- [ ] **Step 3: Write the minimal toolbar status implementation**

```rust
let color_status = GtkBox::new(Orientation::Horizontal, 8);
color_status.add_css_class("editor-toolbar-color-status");

let color_status_swatch = GtkBox::new(Orientation::Horizontal, 0);
color_status_swatch.add_css_class("editor-toolbar-color-status-swatch");
color_status_swatch.set_widget_name("editor-toolbar-color-status-swatch");

let color_status_label = Label::new(Some("#121212"));
color_status_label.add_css_class("editor-toolbar-color-status-label");
color_status_label.set_xalign(0.0);

color_status.append(&color_status_swatch);
color_status.append(&color_status_label);
```

```rust
pub struct ToolbarModeParts {
    pub root: GtkBox,
    pub toolbar_mode_stack: Stack,
    pub color_status_swatch: GtkBox,
    pub color_status_label: Label,
    // keep existing fields unchanged
}
```

```rust
pub(super) fn build_toolbar_mode_controls(
    /* existing args minus color_picker_trigger_host */
) -> ToolbarModeParts {
    let color_group = GtkBox::new(Orientation::Horizontal, 0);
    color_group.add_css_class("editor-color-group");
    color_group.append(&color_status);
    // keep the rest of the toolbar construction unchanged
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test toolbar_uses_read_only_color_status_chip_instead_of_picker_trigger -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/color_picker.rs src/capture/editor/window/toolbar.rs src/capture/editor/window/mod.rs
git commit -m "refactor: replace toolbar color picker with status chip"
```

### Task 2: Sync Toolbar Status Chip From Shared Color State

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/color_picker.rs`
- Test: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Write the failing sync source test**

```rust
#[test]
fn toolbar_color_status_syncs_from_shared_active_color() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("editor-toolbar-color-status-swatch")
            && production_source.contains("color_status_label.set_label")
            && production_source.contains("BackgroundStyle::PlainColor")
            && production_source.contains("draw_color_to_hex(active_color)"),
        "Toolbar color status should mirror the shared active color, including Background plain color",
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test toolbar_color_status_syncs_from_shared_active_color -- --nocapture`
Expected: FAIL because no shared status-chip sync exists yet.

- [ ] **Step 3: Write the minimal sync closure**

```rust
let sync_toolbar_color_status: Rc<dyn Fn()> = Rc::new({
    let state = state.clone();
    let color_status_swatch = color_status_swatch.clone();
    let color_status_label = color_status_label.clone();
    move || {
        let active_color = {
            let st = state.lock().unwrap();
            if st.selected_tool == Tool::Background {
                if let BackgroundStyle::PlainColor(color) = st.background_style {
                    color
                } else {
                    st.selected_color
                }
            } else {
                st.selected_color
            }
        };

        color_status_label.set_label(&format!("#{}", draw_color_to_hex(active_color)));
        let (r, g, b, _) = draw_color_to_rgba_u8(active_color);
        let alpha = active_color.a.clamp(0.0, 1.0);
        let css = format!(
            "#editor-toolbar-color-status-swatch {{ background: rgba({r}, {g}, {b}, {alpha:.3}); }}"
        );
        toolbar_color_css_provider.load_from_data(&css);
    }
});
```

```rust
let sync_shared_colors_for_active_tool: Rc<dyn Fn()> = Rc::new({
    let sync_toolbar_color_status = sync_toolbar_color_status.clone();
    let sync_picker_for_active_tool = sync_picker_for_active_tool.clone();
    let sync_colors_panel_for_active_tool = sync_colors_panel_for_active_tool.clone();
    move || {
        sync_toolbar_color_status();
        sync_picker_for_active_tool();
        sync_colors_panel_for_active_tool();
    }
});
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test toolbar_color_status_syncs_from_shared_active_color -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/color_picker.rs
git commit -m "feat: sync toolbar color status from shared editor color"
```

### Task 3: Auto-Route Color Tools Into The Right Colors Tab

**Files:**
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Write the failing routing test**

```rust
#[test]
fn color_capable_tools_default_to_colors_inspector_surface() {
    let source = include_str!("toolbar.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("set_active_inspector_surface(\"colors\")")
            && production_source.contains("Tool::Background")
            && production_source.contains("Tool::Pen")
            && production_source.contains("Tool::Focus"),
        "Color-capable tools should switch the right inspector to the Colors surface",
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test color_capable_tools_default_to_colors_inspector_surface -- --nocapture`
Expected: FAIL because tool routing still defaults `Background` to the background inspector.

- [ ] **Step 3: Write the minimal routing change**

```rust
if matches!(
    tool,
    Tool::Background
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

```rust
background_tab_btn.set_visible(matches!(tool, Tool::Background));
colors_tab_btn.set_visible(colors_mode);
background_inspector.set_visible(false);
colors_inspector.set_visible(colors_mode);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test color_capable_tools_default_to_colors_inspector_surface -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/toolbar.rs src/capture/editor/window/mod.rs
git commit -m "feat: route color tools into colors inspector"
```

### Task 4: Remove Duplicated Current Color Section From Right Colors Tab

**Files:**
- Modify: `src/capture/editor/window/colors_panel.rs`
- Test: `src/capture/editor/window/colors_panel.rs`

- [ ] **Step 1: Write the failing duplication test**

```rust
#[test]
fn colors_panel_no_longer_renders_current_color_summary_section() {
    let source = include_str!("colors_panel.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        !production_source.contains("Current color")
            && !production_source.contains("editor-colors-panel-current-row")
            && !production_source.contains("editor-sidebar-current-color-preview"),
        "Colors panel should not duplicate the current color summary once the toolbar owns that status",
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test colors_panel_no_longer_renders_current_color_summary_section -- --nocapture`
Expected: FAIL because the `Current color` row still exists.

- [ ] **Step 3: Write the minimal panel cleanup**

```rust
// remove:
// - current_title
// - current_row
// - current_preview
// - current_value
// - current_section append

content.append(&helper);
content.append(&spectrum_section);
content.append(&palette_section);
content.append(&custom_section);
content.append(&actions);
```

```rust
// keep sync focused on:
helper.set_label(/* background-aware copy */);
set_active_color_button(&palette_buttons, palette_index_for_color(active_color));
let picker = PickerColorState::from_color(active_color);
*picker_state.borrow_mut() = picker;
update_picker_ui(picker);
refresh_custom_slots();
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test colors_panel_no_longer_renders_current_color_summary_section -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/colors_panel.rs
git commit -m "refactor: remove duplicated current color section"
```

### Task 5: Style The Toolbar Status Chip And Remove Picker-Only UI Debt

**Files:**
- Modify: `src/capture/editor/ui_support.rs`
- Modify: `src/capture/editor/window/color_picker.rs`
- Test: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Write the failing style test**

```rust
#[test]
fn toolbar_color_status_chip_has_swatch_and_hex_styles() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains(".editor-toolbar-color-status {")
            && production_source.contains(".editor-toolbar-color-status-swatch {")
            && production_source.contains(".editor-toolbar-color-status-label {"),
        "Toolbar color status chip should have dedicated swatch and label styles",
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test toolbar_color_status_chip_has_swatch_and_hex_styles -- --nocapture`
Expected: FAIL because the new status chip styles do not exist yet.

- [ ] **Step 3: Write the minimal styles and cleanup**

```css
.editor-toolbar-color-status {
    min-height: 34px;
    padding: 0 10px;
    border-radius: 9px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    spacing: 8px;
}

.editor-toolbar-color-status-swatch {
    min-width: 16px;
    min-height: 16px;
    border-radius: 999px;
    border: 1px solid rgba(255, 255, 255, 0.14);
}

.editor-toolbar-color-status-label {
    color: rgba(245, 245, 247, 0.92);
    font-size: 12px;
    font-weight: 700;
}
```

```rust
// remove unused toolbar popover-only exports if the toolbar no longer needs them:
// - trigger_host
// - popover
// - color_picker_dot
// - color_class_names
// - set_picker_panel_visibility
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test toolbar_color_status_chip_has_swatch_and_hex_styles -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/ui_support.rs src/capture/editor/window/color_picker.rs
git commit -m "style: add toolbar color status chip"
```

### Task 6: Final Verification

**Files:**
- Verify: `src/capture/editor/window/mod.rs`
- Verify: `src/capture/editor/window/toolbar.rs`
- Verify: `src/capture/editor/window/colors_panel.rs`
- Verify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Run focused regression tests**

Run: `cargo test toolbar_color_status_syncs_from_shared_active_color -- --nocapture`
Expected: PASS

Run: `cargo test color_capable_tools_default_to_colors_inspector_surface -- --nocapture`
Expected: PASS

Run: `cargo test colors_panel_no_longer_renders_current_color_summary_section -- --nocapture`
Expected: PASS

Run: `cargo test toolbar_color_status_chip_has_swatch_and_hex_styles -- --nocapture`
Expected: PASS

- [ ] **Step 2: Run existing shared-colors regressions**

Run: `cargo test colors_panel_contains_shared_color_management_markers -- --nocapture`
Expected: PASS

Run: `cargo test colors_panel_matches_background_content_width -- --nocapture`
Expected: PASS

Run: `cargo test background_and_color_tools_route_into_shared_colors_inspector -- --nocapture`
Expected: PASS

- [ ] **Step 3: Run build verification**

Run: `cargo build`
Expected: exit code 0

- [ ] **Step 4: Manual UI verification**

Run: `apexshot daemon`
Expected:
- toolbar shows a swatch + `#HEX` status chip instead of the color picker
- selecting a color-capable tool switches the right panel to `Colors`
- editing color from the right panel updates the toolbar status chip live
- the right panel no longer shows a duplicated `Current color` summary row

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/toolbar.rs src/capture/editor/window/colors_panel.rs src/capture/editor/ui_support.rs src/capture/editor/window/color_picker.rs
git commit -m "feat: move color editing fully into right inspector"
```
