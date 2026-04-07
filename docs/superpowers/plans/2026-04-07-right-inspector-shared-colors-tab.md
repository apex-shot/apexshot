# Right Inspector Shared Colors Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove Background's embedded plain-color controls and route both Background and color-capable tools through one shared right-inspector Colors tab backed by the existing picker state.

**Architecture:** Keep `background_panel.rs` focused on Background-specific layout/styling controls, and make the inspector shell route Background to `Background | Colors` while color-capable tools render `Colors` directly. Reuse the existing toolbar color-picker hooks, palette, custom slots, and eyedropper behavior so the toolbar and sidebar stay synchronized instead of creating a second color system.

**Tech Stack:** Rust 2021, GTK4, existing ApexShot editor window/state/color/color-picker modules

---

## File map

- Modify: `src/capture/editor/window/background_panel.rs` — remove embedded `Plain color` UI and add source regression tests for the new Background panel structure
- Modify: `src/capture/editor/window/colors_panel.rs` — expand or adapt the shared Colors inspector so it can apply colors to Background as well as annotation tools
- Modify: `src/capture/editor/window/color_picker.rs` — expose shared color-application/custom-slot/eyedropper hooks needed by the inspector panel without changing toolbar visuals
- Modify: `src/capture/editor/window/mod.rs` — wire inspector tab state, mount shared Colors content, and route Background plus color-capable tools
- Modify: `src/capture/editor/window/toolbar.rs` — update tool classification/routing logic for Background and color-capable tools
- Modify: `src/capture/editor/ui_support.rs` — add or adjust inspector styling for the shared Colors surface and remove obsolete Background plain-color styling if it becomes unused
- Test: `src/capture/editor/window/background_panel.rs` — source regression tests for Background plain-color removal
- Test: `src/capture/editor/window/colors_panel.rs` — source regression tests for shared Colors tab markers and Background-specific apply wiring
- Test: `src/capture/editor/window/mod.rs` or `src/capture/editor/window/toolbar.rs` — source regression tests for inspector routing behavior

### Task 1: Add failing regression tests for shared Colors routing and Background plain-color removal

**Files:**
- Modify: `src/capture/editor/window/background_panel.rs`
- Modify: `src/capture/editor/window/colors_panel.rs`
- Modify: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Add a failing source test for Background plain-color removal**

```rust
#[test]
fn background_panel_no_longer_appends_plain_color_section() {
    let source = include_str!("background_panel.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        !production_source.contains("plain_color_section.append(&plain_color_title);")
            && !production_source.contains("background_sidebar_options.append(&plain_color_section);"),
        "Background panel should no longer render the embedded plain-color section",
    );
}
```

- [ ] **Step 2: Add a failing source test for shared Colors panel Background apply markers**

```rust
#[test]
fn colors_panel_contains_background_plain_color_apply_markers() {
    let source = include_str!("colors_panel.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("BackgroundStyle::PlainColor")
            && production_source.contains("selected_tool == Tool::Background"),
        "Colors panel should support applying plain colors for the Background tool",
    );
}
```

- [ ] **Step 3: Add a failing source test for inspector routing**

```rust
#[test]
fn background_and_color_tools_route_into_shared_colors_inspector() {
    let source = include_str!("toolbar.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("Tool::Background")
            && production_source.contains("Tool::Pen")
            && production_source.contains("Tool::Highlighter")
            && production_source.contains("\"Colors\""),
        "Toolbar inspector routing should include Background and color-capable tools in the shared Colors flow",
    );
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run:

```bash
cargo test background_panel_no_longer_appends_plain_color_section -- --nocapture
cargo test colors_panel_contains_background_plain_color_apply_markers -- --nocapture
cargo test background_and_color_tools_route_into_shared_colors_inspector -- --nocapture
```

Expected: all three tests fail because the Background panel still contains `Plain color`, the Colors panel is not yet wired for Background, and the routing source markers are incomplete.

- [ ] **Step 5: Commit the failing tests**

```bash
git add src/capture/editor/window/background_panel.rs src/capture/editor/window/colors_panel.rs src/capture/editor/window/toolbar.rs
git commit -m "test: cover shared colors inspector routing"
```

### Task 2: Expose reusable color-management hooks from the toolbar picker path

**Files:**
- Modify: `src/capture/editor/window/color_picker.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/color_picker.rs`

- [ ] **Step 1: Identify or extract the shared apply/custom-slot helpers from the toolbar picker**

```rust
pub(super) struct SharedColorHooks {
    pub apply_annotation_color: Rc<dyn Fn(DrawColor)>,
    pub apply_background_color: Rc<dyn Fn(DrawColor)>,
    pub load_custom_colors: Rc<dyn Fn() -> PersistedCustomColorSlots>,
    pub save_custom_colors: Rc<dyn Fn(PersistedCustomColorSlots)>,
    pub launch_eyedropper: Rc<dyn Fn()>,
}
```

- [ ] **Step 2: Wire the Background apply hook to `BackgroundStyle::PlainColor` in editor state**

```rust
let apply_background_color: Rc<dyn Fn(DrawColor)> = Rc::new({
    let state = state.clone();
    let drawing_area = drawing_area.clone();
    move |color| {
        let mut st = state.lock().unwrap();
        st.background_style = BackgroundStyle::PlainColor(color);
        st.mark_working_image_dirty();
        drawing_area.queue_draw();
    }
});
```

- [ ] **Step 3: Keep toolbar visuals untouched while exporting these hooks for sidebar use**

```rust
// Reuse the same persistence/apply functions for both toolbar and sidebar surfaces.
let shared_color_hooks = SharedColorHooks {
    apply_annotation_color,
    apply_background_color,
    load_custom_colors,
    save_custom_colors,
    launch_eyedropper,
};
```

- [ ] **Step 4: Run targeted build to verify the hook extraction compiles**

Run:

```bash
cargo build
```

Expected: build succeeds with the toolbar picker still compiling unchanged.

- [ ] **Step 5: Commit the hook extraction**

```bash
git add src/capture/editor/window/color_picker.rs src/capture/editor/window/mod.rs
git commit -m "refactor: share editor color hooks with inspector"
```

### Task 3: Expand the Colors panel so it can drive Background and annotation tools

**Files:**
- Modify: `src/capture/editor/window/colors_panel.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/colors_panel.rs`

- [ ] **Step 1: Update the Colors panel builder API to accept shared hooks and tool context**

```rust
pub(super) fn build_colors_panel(
    selected_tool: Tool,
    shared_hooks: SharedColorHooks,
) -> ColorsPanelParts
```

- [ ] **Step 2: Route palette/custom-color selection through Background or annotation apply logic**

```rust
let apply_selected_color = {
    let shared_hooks = shared_hooks.clone();
    move |color: DrawColor| {
        if selected_tool == Tool::Background {
            (shared_hooks.apply_background_color)(color);
        } else {
            (shared_hooks.apply_annotation_color)(color);
        }
    }
};
```

- [ ] **Step 3: Keep shared custom colors and eyedropper behavior intact**

```rust
let saved_slots = (shared_hooks.load_custom_colors)();
// build shared swatches from `saved_slots`

pick_from_screen_button.connect_clicked({
    let shared_hooks = shared_hooks.clone();
    move |_| (shared_hooks.launch_eyedropper)()
});
```

- [ ] **Step 4: Run the Colors panel tests**

Run:

```bash
cargo test colors_panel_contains_background_plain_color_apply_markers -- --nocapture
```

Expected: PASS once the Background apply markers and shared hook usage exist in `colors_panel.rs`.

- [ ] **Step 5: Commit the shared Colors panel behavior**

```bash
git add src/capture/editor/window/colors_panel.rs src/capture/editor/window/mod.rs
git commit -m "feat: share colors inspector across background and tools"
```

### Task 4: Remove Background plain-color UI and keep only Background-specific controls

**Files:**
- Modify: `src/capture/editor/window/background_panel.rs`
- Test: `src/capture/editor/window/background_panel.rs`

- [ ] **Step 1: Delete the embedded `Plain color` section from `background_panel.rs`**

```rust
// Remove:
// let plain_color_section = GtkBox::new(...)
// let plain_color_title = Label::new(Some("Plain color"));
// let plain_color_grid = GtkBox::new(...)
// background_sidebar_options.append(&plain_color_section);
```

- [ ] **Step 2: Keep the Background stack limited to Background-specific controls**

```rust
background_sidebar_options.append(&background_none_btn);
background_sidebar_options.append(&alignment_section);
background_sidebar_options.append(&gradients_section);
background_sidebar_options.append(&wallpaper_section);
background_sidebar_options.append(&blurred_section);
background_sidebar_options.append(&background_padding_divider_row);
background_sidebar_options.append(&padding_section);
background_sidebar_options.append(&ratio_section);
background_sidebar_options.append(&compact_controls);
```

- [ ] **Step 3: Run the Background panel regression test**

Run:

```bash
cargo test background_panel_no_longer_appends_plain_color_section -- --nocapture
```

Expected: PASS after the plain-color section and append markers are removed.

- [ ] **Step 4: Commit the Background panel cleanup**

```bash
git add src/capture/editor/window/background_panel.rs
git commit -m "refactor: remove background plain color section"
```

### Task 5: Route Background and color-capable tools into the shared Colors inspector behavior

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/toolbar.rs`
- Test: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Classify Background and all color-capable tools for inspector routing**

```rust
fn tool_supports_colors(tool: Tool) -> bool {
    matches!(
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
    )
}
```

- [ ] **Step 2: Route Background to `Background | Colors` and default it to `Background`**

```rust
if selected_tool == Tool::Background {
    inspector_tabs.set_tabs(["Background", "Colors"]);
    inspector_tabs.set_active("Background");
}
```

- [ ] **Step 3: Route color-capable annotation tools to `Colors` only**

```rust
else if tool_supports_colors(selected_tool) {
    inspector_tabs.set_tabs(["Colors"]);
    inspector_tabs.set_active("Colors");
}
```

- [ ] **Step 4: Run the routing regression test**

Run:

```bash
cargo test background_and_color_tools_route_into_shared_colors_inspector -- --nocapture
```

Expected: PASS once Background and the color-capable tool list are part of the shared Colors routing flow.

- [ ] **Step 5: Commit the inspector routing changes**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/toolbar.rs
git commit -m "feat: route background and color tools through colors inspector"
```

### Task 6: Update inspector styling and remove obsolete Background plain-color styling

**Files:**
- Modify: `src/capture/editor/ui_support.rs`
- Test: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Remove or stop relying on obsolete Background plain-color style markers if they become unused**

```rust
// Remove unused selectors such as:
// .editor-background-plain-color-section
// .editor-background-plain-color-grid
// .editor-background-plain-color-row
```

- [ ] **Step 2: Add or keep shared Colors panel styling aligned with the inspector surface**

```rust
            .editor-colors-panel {
                margin-top: 4px;
            }

            .editor-colors-panel-section-title {
                color: #f5f5f7;
                font-size: 14px;
                font-weight: 700;
            }
```

- [ ] **Step 3: Run a targeted build to verify CSS/source tests remain valid**

Run:

```bash
cargo build
```

Expected: build succeeds without stale style references breaking the editor module.

- [ ] **Step 4: Commit the style cleanup**

```bash
git add src/capture/editor/ui_support.rs
git commit -m "style: align shared colors inspector with sidebar"
```

### Task 7: Final verification

**Files:**
- Modify: none

- [ ] **Step 1: Run the focused inspector/background/colors test slice**

Run:

```bash
cargo test background_panel::tests:: -- --nocapture
cargo test colors_panel -- --nocapture
cargo test editor_background_alignment_ -- --nocapture
```

Expected: PASS for the Background panel ordering/removal tests, Colors panel tests, and the existing alignment CSS checks.

- [ ] **Step 2: Run a full build**

Run:

```bash
cargo build
```

Expected: successful build with no compile errors.

- [ ] **Step 3: Manually verify the editor in a desktop session**

Run:

```bash
cargo run -- edit <image-path>
```

Expected:
- Background tool shows `Background | Colors`
- Background tab no longer shows `Plain color`
- Colors tab can set a solid background color
- switching between toolbar picker and sidebar keeps colors synchronized
- color-capable annotation tools show the shared Colors panel
- non-color tools do not incorrectly show the Colors panel

- [ ] **Step 4: Commit the verification checkpoint**

```bash
git add docs/superpowers/plans/2026-04-07-right-inspector-shared-colors-tab.md
git commit -m "docs: add shared colors inspector implementation plan"
```
