# Right Inspector Arrow Text Number Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `Arrow`, `Text`, and `Number` sub-tool controls from the annotate toolbar into the persistent right inspector while preserving the shared `Colors` tab behavior.

**Architecture:** Keep the existing right-inspector shell in `window/mod.rs`, add a focused tool-panels module for the migrated primary tabs, and reuse the same editor-state callbacks the toolbar controls already use today. The toolbar remains the place for tool selection and global actions, while the inspector becomes the home for detailed per-tool settings. The inspector width must remain exactly as it is today for every tool surface; this migration reuses the current sidebar width instead of introducing tool-specific sizing.

**Tech Stack:** Rust, GTK4, existing annotate editor window modules, source-level regression tests with `include_str!`, `cargo test`, `cargo check`

---

## File map

- Create: `src/capture/editor/window/tool_panels.rs` — reusable right-inspector builders for `Arrow`, `Text`, and `Number`
- Modify: `src/capture/editor/window/mod.rs` — inspector routing, tab labels, tool-panel mounting, callback wiring, source regression tests
- Modify: `src/capture/editor/window/toolbar.rs` — remove migrated toolbar groups from visible tool surfaces, keep shared/global controls, source regression tests
- Modify: `src/capture/editor/window/events.rs` — if any moved controls still rely on toolbar-local signal wiring, expose or reuse the same callbacks from inspector widgets
- Modify: `src/capture/editor/ui_support.rs` — sidebar styling helpers if the moved controls need inspector-specific classes
- Test: `src/capture/editor/window/mod.rs` and `src/capture/editor/window/toolbar.rs` source-level regression tests

Width constraint:
- reuse the current inspector width defined by `BACKGROUND_SIDEBAR_WIDTH`
- do not add per-tool width requests, width calculations, or wider/narrower variants for `Arrow`, `Text`, or `Number`
- all migrated inspector panels must render inside the same fixed-width shell the Background panel already uses

### Task 1: Add failing regression tests for inspector routing and toolbar decluttering

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/toolbar.rs`
- Test: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Add a failing source test for `Arrow`, `Text`, and `Number` inspector routing**

Add this test near the existing `#[cfg(test)]` block in `src/capture/editor/window/mod.rs`:

```rust
#[test]
fn arrow_text_and_number_route_to_tool_specific_inspector_tabs() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("Tool::Arrow")
            && production_source.contains("Tool::Text")
            && production_source.contains("Tool::Number")
            && production_source.contains("\"arrow\"")
            && production_source.contains("\"text\"")
            && production_source.contains("\"number\"")
            && production_source.contains("\"colors\""),
        "Inspector routing should expose Arrow, Text, and Number primary panels alongside the shared Colors surface",
    );
}
```

- [ ] **Step 2: Add a failing source test for toolbar cleanup markers**

Add this test near the existing `#[cfg(test)]` block in `src/capture/editor/window/toolbar.rs`:

```rust
#[test]
fn toolbar_no_longer_exposes_arrow_text_and_number_detail_groups() {
    let source = include_str!("toolbar.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        !production_source.contains("text_size_group.set_visible(true)")
            && !production_source.contains("font_family_group.set_visible(true)")
            && !production_source.contains("number_options_group.set_visible(true)")
            && !production_source.contains("arrow_style_group.set_visible(true)"),
        "Toolbar tool-updater should stop making Arrow, Text, and Number detail groups visible after the inspector migration",
    );
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test arrow_text_and_number_route_to_tool_specific_inspector_tabs toolbar_no_longer_exposes_arrow_text_and_number_detail_groups --lib
```

Expected: FAIL because the current inspector still routes those tools through the placeholder or colors-only flow and the toolbar still toggles the migrated groups.

- [ ] **Step 4: Commit the failing-test checkpoint**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/toolbar.rs
git commit -m "test: add right inspector migration regression coverage"
```

### Task 2: Create dedicated inspector panels for `Arrow`, `Text`, and `Number`

**Files:**
- Create: `src/capture/editor/window/tool_panels.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Create a focused tool-panels module**

Create `src/capture/editor/window/tool_panels.rs` with a small public surface:

```rust
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Popover, Scale};

pub struct ArrowPanelParts {
    pub root: GtkBox,
    pub arrow_style_list: GtkBox,
    pub stroke_size_list: GtkBox,
}

pub struct TextPanelParts {
    pub root: GtkBox,
    pub text_size_list: GtkBox,
    pub font_family_list: GtkBox,
}

pub struct NumberPanelParts {
    pub root: GtkBox,
    pub number_options_list: GtkBox,
    pub number_size_list: GtkBox,
    pub number_start_entry: gtk4::Entry,
    pub number_inc_btn: Button,
    pub number_dec_btn: Button,
}
```

- [ ] **Step 2: Build vertical inspector variants of the migrated controls**

In `src/capture/editor/window/tool_panels.rs`, add builder functions that keep the existing control semantics but render them in a right-sidebar layout:

```rust
pub fn build_arrow_panel() -> ArrowPanelParts { /* section title + style list + stroke list */ }

pub fn build_text_panel() -> TextPanelParts { /* section title + text size list + font family list */ }

pub fn build_number_panel() -> NumberPanelParts { /* section title + style list + start controls + size list */ }
```

Use the same option labels and CSS class names already present in `toolbar.rs` wherever practical so existing signal hookup code can be reused.
Set the new panel roots to the same fixed width as the existing inspector shell instead of introducing new width values.

- [ ] **Step 3: Export the new module from `window/mod.rs`**

Add the module declaration beside the existing inspector modules:

```rust
pub mod background_panel;
pub mod colors_panel;
mod tool_panels;
mod toolbar;
```

- [ ] **Step 4: Run a compile check**

Run:

```bash
cargo check
```

Expected: PASS, with the new module compiling even if it is not yet fully wired into the runtime inspector flow.

- [ ] **Step 5: Commit the new panel module**

```bash
git add src/capture/editor/window/tool_panels.rs src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "feat: add right inspector panels for arrow text and number"
```

### Task 3: Mount the new tool panels into the inspector shell and route each tool to its primary tab

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Replace the placeholder-only flow for these tools with named inspector surfaces**

In `src/capture/editor/window/mod.rs`, build and register the new panels next to the existing background and colors surfaces:

```rust
let arrow_inspector = tool_panels::build_arrow_panel();
let text_inspector = tool_panels::build_text_panel();
let number_inspector = tool_panels::build_number_panel();

inspector_stack.add_named(&arrow_inspector.root, Some("arrow"));
inspector_stack.add_named(&text_inspector.root, Some("text"));
inspector_stack.add_named(&number_inspector.root, Some("number"));
```

- [ ] **Step 2: Add a helper that updates the primary tab label and target surface**

Replace the fixed `Background` tab assumption with a small routing helper:

```rust
let set_primary_inspector_tab: Rc<dyn Fn(&str, &str)> = Rc::new({
    let background_tab_btn = background_tab_btn.clone();
    move |label, surface| {
        background_tab_btn.set_label(label);
        background_tab_btn.set_widget_name(surface);
    }
});
```

Then use it inside the tool-selection routing so:
- `Tool::Background` maps to label `Background` + surface `"background"`
- `Tool::Arrow` maps to label `Arrow` + surface `"arrow"`
- `Tool::Text` maps to label `Text` + surface `"text"`
- `Tool::Number` maps to label `Number` + surface `"number"`

- [ ] **Step 3: Default each migrated tool back to its primary tab**

Update the tool-change path so selecting `Arrow`, `Text`, or `Number` calls:

```rust
set_primary_inspector_tab("Arrow", "arrow");
set_active_inspector_surface("arrow");
```

and the corresponding `Text` / `Number` equivalents before any colors-panel sync runs.

- [ ] **Step 4: Re-run the routing test**

Run:

```bash
cargo test arrow_text_and_number_route_to_tool_specific_inspector_tabs --lib
```

Expected: PASS.

- [ ] **Step 5: Commit the routing work**

```bash
git add src/capture/editor/window/mod.rs
git commit -m "feat: route arrow text and number into right inspector tabs"
```

### Task 4: Rewire the moved controls to existing editor state and remove the detailed toolbar groups

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/toolbar.rs`
- Modify: `src/capture/editor/window/events.rs`
- Test: `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Reuse the existing control hookup logic with inspector-owned widgets**

Where `mod.rs` currently wires callbacks to `text_size_list`, `font_family_list`, `number_options_list`, `number_size_list`, `number_start_entry`, `arrow_style_list`, and `stroke_size_list`, switch those references to the corresponding fields from `tool_panels.rs`:

```rust
let arrow_panel = tool_panels::build_arrow_panel();
let text_panel = tool_panels::build_text_panel();
let number_panel = tool_panels::build_number_panel();

let arrow_style_list = arrow_panel.arrow_style_list.clone();
let text_size_list = text_panel.text_size_list.clone();
let font_family_list = text_panel.font_family_list.clone();
let number_options_list = number_panel.number_options_list.clone();
```

Keep the underlying state updates unchanged so the migration only changes the render location.

- [ ] **Step 2: Stop the toolbar updater from exposing the migrated groups**

In `src/capture/editor/window/toolbar.rs`, remove `Arrow`, `Text`, and `Number` from the visibility toggles in `build_toolbar_tool_updater(...)` so the toolbar no longer activates:

```rust
text_size_group.set_visible(false);
font_family_group.set_visible(false);
number_options_group.set_visible(false);
arrow_style_group.set_visible(false);
stroke_size_group.set_visible(false);
```

Keep the remaining non-migrated tool groups untouched.

- [ ] **Step 3: Re-run toolbar regression coverage and compile checks**

Run:

```bash
cargo test toolbar_no_longer_exposes_arrow_text_and_number_detail_groups --lib
cargo check
```

Expected: PASS on both commands.

- [ ] **Step 4: Verify no width-specific code was introduced**

Check that the migration reuses the existing sidebar width rather than creating tool-specific widths:

```bash
rg -n "width_request|set_width_request|BACKGROUND_SIDEBAR_WIDTH" src/capture/editor/window
```

Expected: the migrated panels reuse the existing inspector width path and do not introduce new per-tool width constants or divergent width requests.

- [ ] **Step 5: Commit the control migration**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/toolbar.rs src/capture/editor/window/events.rs src/capture/editor/window/tool_panels.rs
git commit -m "feat: move arrow text and number controls into the right inspector"
```

### Task 5: Final verification for inspector behavior and shared colors sync

**Files:**
- Modify: none unless verification exposes a bug

- [ ] **Step 1: Run the focused automated verification**

Run:

```bash
cargo test arrow_text_and_number_route_to_tool_specific_inspector_tabs toolbar_no_longer_exposes_arrow_text_and_number_detail_groups --lib
cargo check
```

Expected: PASS.

- [ ] **Step 2: Run manual editor verification**

Open the annotate editor and verify:

```text
1. Select Arrow and confirm the inspector shows "Arrow | Colors" and opens on Arrow.
2. Select Text and confirm the inspector shows "Text | Colors" and opens on Text.
3. Select Number and confirm the inspector shows "Number | Colors" and opens on Number.
4. Switch each of those tools to Colors and confirm shared color changes still affect the active tool.
5. Confirm the main toolbar no longer shows Arrow style, Text size/font, or Number options.
6. Re-check Background to confirm "Background | Colors" still behaves as before.
7. Confirm the inspector width is unchanged when switching between Background, Arrow, Text, and Number.
```

- [ ] **Step 3: Commit the verified slice**

```bash
git add -A
git commit -m "feat: finish right inspector migration for arrow text and number"
```

## Self-review

Spec coverage:
- tool-specific primary tabs for `Arrow`, `Text`, and `Number` are covered by Tasks 2 and 3
- shared `Colors` tab preservation is covered by Tasks 3 and 5
- toolbar decluttering is covered by Tasks 1 and 4
- regression checks for unchanged Background behavior are covered by Task 5

Placeholder scan:
- no `TODO` or `TBD` placeholders remain
- every task names exact files and commands

Type consistency:
- the plan uses one new module name, `tool_panels.rs`, consistently
- inspector surfaces are consistently named `"arrow"`, `"text"`, `"number"`, `"background"`, and `"colors"`
