# Right Inspector Colors Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a tool-aware Colors tab to the annotate editor right inspector while keeping the existing toolbar color picker untouched.

**Architecture:** Convert the right inspector into a small tabbed shell with reusable panel widgets for Background, Colors, and Placeholder content. Keep Background as its own reusable panel, add a new Colors panel module that can read and update editor color state, and extend inspector/tool routing so Background shows `Background | Colors` while color-capable tools show `Colors` only.

**Tech Stack:** Rust 2021, GTK4, existing ApexShot editor window/state/toolbar/ui_support modules

---

## File map

- Modify: `src/capture/editor/window/mod.rs` — inspector shell, tabs, panel stack, tool-driven inspector state wiring
- Modify: `src/capture/editor/window/toolbar.rs` — color-tool classification and inspector routing hooks
- Modify: `src/capture/editor/window/background_panel.rs` — keep reusable as Background tab content
- Create: `src/capture/editor/window/colors_panel.rs` — reusable shared Colors inspector panel
- Modify: `src/capture/editor/ui_support.rs` — inspector tabs and colors-panel styling
- Test: `src/capture/editor/window/mod.rs` — source regression tests for tab shell and colors inspector wiring
- Test: `src/capture/editor/window/toolbar.rs` — source regression test for color-tool routing markers

### Task 1: Add failing regression tests for inspector tabs and colors panel wiring
- [ ] Add source tests in `src/capture/editor/window/mod.rs` asserting production source contains `editor-inspector-tabs`, `colors_inspector`, and `Background | Colors` / `Colors` routing markers.
- [ ] Add a source test in `src/capture/editor/window/toolbar.rs` asserting production source contains a color-tool classifier branch.
- [ ] Run: `cargo test inspector_tabs colors_tool`
- [ ] Verify tests fail for the expected missing markers.

### Task 2: Create reusable Colors inspector panel
- [ ] Create `src/capture/editor/window/colors_panel.rs` with a reusable `ColorsPanelParts { root: GtkBox }` builder.
- [ ] Build a prototype panel containing a title, helper copy, selected color preview, and shared swatch grid based on the existing editor palette concept.
- [ ] Wire swatch clicks to existing editor color state only; do not remove or alter the toolbar color picker UI.
- [ ] Run a targeted build: `cargo build`

### Task 3: Convert the right inspector into a tabbed shell
- [ ] In `src/capture/editor/window/mod.rs`, replace the single-content inspector with a shell that contains a tab/header row and a content stack.
- [ ] Mount reusable `background_inspector`, `colors_inspector`, and `placeholder_inspector` panels into that shell.
- [ ] Add tab buttons for `Background` and `Colors`, and keep placeholder content for non-supported tools.
- [ ] Run: `cargo test inspector_tabs`
- [ ] Verify new tests pass.

### Task 4: Route tools into the new inspector behavior
- [ ] In `src/capture/editor/window/toolbar.rs`, classify color-capable tools: `Pen`, `Arrow`, `Line`, `Box`, `Circle`, `Text`, `Number`, `Highlighter`, `Obfuscate`, `Focus`.
- [ ] Update the tool updater closure so:
  - Background shows tabs `Background | Colors` and defaults to Background
  - color-capable tools show `Colors`
  - Select/Crop keep placeholder content
- [ ] Preserve current toolbar color picker behavior unchanged.
- [ ] Run: `cargo test colors_tool && cargo build`

### Task 5: Style the tabbed inspector
- [ ] In `src/capture/editor/ui_support.rs`, add CSS classes for inspector tabs, active tab state, colors panel sections, swatches, and preview.
- [ ] Keep the visual style aligned with the existing dark inspector language.
- [ ] Run: `cargo build`

### Task 6: Final verification
- [ ] Run: `cargo build && cargo test inspector_tabs colors_tool`
- [ ] Manually verify in a desktop session with `cargo run -- edit <image-path>`:
  - Background shows `Background | Colors`
  - color-capable tools show `Colors`
  - Select/Crop still show placeholder
  - toolbar color picker remains present and untouched
  - colors sidebar swatches update editor color state
