# Right Inspector Colors Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the right inspector Colors tab so it manages shared custom colors and eyedropper-driven color capture while leaving the toolbar color picker visually unchanged.

**Architecture:** Reuse the existing toolbar color picker’s persistent custom-color slot model and color-application hooks instead of building a second system. Extend the sidebar Colors panel into a fuller color-management surface with palette colors, current color preview, `My colors`, add/remove actions, and an eyedropper entry point, while both toolbar and sidebar continue to operate on the same underlying data.

**Tech Stack:** Rust 2021, GTK4, existing ApexShot editor color/state/window/ui_support modules

---

## File map

- Modify: `src/capture/editor/window/colors_panel.rs` — expand sidebar Colors panel with shared custom slots, add/remove actions, and eyedropper trigger
- Modify: `src/capture/editor/window/color_picker.rs` — expose/reuse shared custom-color slot and eyedropper hooks without changing toolbar visuals
- Modify: `src/capture/editor/window/mod.rs` — pass shared color hooks into the Colors panel
- Modify: `src/capture/editor/ui_support.rs` — style `My colors`, action buttons, and sidebar custom-slot affordances
- Test: `src/capture/editor/window/colors_panel.rs` — source regression tests for `My colors`, add/remove, and eyedropper markers

### Task 1: Add failing regression tests for expanded Colors sidebar
- [ ] Add source tests in `src/capture/editor/window/colors_panel.rs` asserting production source contains `My colors`, `Add current color`, and `Pick from screen` markers.
- [ ] Add a source test in `src/capture/editor/window/colors_panel.rs` asserting production source contains sidebar custom-slot removal wiring markers.
- [ ] Run: `cargo test colors_panel_management`
- [ ] Verify tests fail for the expected missing markers.

### Task 2: Expose shared hooks from the toolbar color picker module
- [ ] In `src/capture/editor/window/color_picker.rs`, factor or expose the reusable custom-slot persistence/apply helpers needed by the sidebar panel.
- [ ] Expose an eyedropper trigger hook that the sidebar can call without changing toolbar visuals.
- [ ] Keep the toolbar picker UI unchanged.
- [ ] Run: `cargo build`

### Task 3: Expand the sidebar Colors panel with shared color management
- [ ] In `src/capture/editor/window/colors_panel.rs`, add `My colors` backed by the same persisted custom slots used by the toolbar picker.
- [ ] Add an `Add current color` action that stores the current color into the first available slot.
- [ ] Add removal buttons per custom slot that clear only that saved slot.
- [ ] Add a `Pick from screen` button wired to the shared eyedropper trigger.
- [ ] Keep the hex display read-only for this pass.
- [ ] Run: `cargo test colors_panel_management`
- [ ] Verify tests pass.

### Task 4: Wire the expanded Colors panel into the editor window
- [ ] In `src/capture/editor/window/mod.rs`, pass the shared apply/sync/custom-slot/eyedropper hooks into `colors_panel::build_colors_panel`.
- [ ] Ensure toolbar picker and sidebar panel both operate on the same saved custom colors.
- [ ] Keep existing inspector tab behavior intact.
- [ ] Run: `cargo build`

### Task 5: Style the sidebar color-management surface
- [ ] In `src/capture/editor/ui_support.rs`, add CSS for:
  - `My colors` section
  - add/remove action buttons
  - sidebar custom slot layout
  - read-only hex/current color preview affordances
- [ ] Keep the styling aligned with the current inspector visual language.
- [ ] Run: `cargo build`

### Task 6: Final verification
- [ ] Run: `cargo build && cargo test colors_panel_management`
- [ ] Manually verify in a desktop session with `cargo run -- edit <image-path>`:
  - Colors tab shows `My colors`
  - `Add current color` saves into the shared custom list
  - removing a saved color clears only that slot
  - `Pick from screen` updates current color
  - toolbar color picker remains visually unchanged
  - toolbar and sidebar reflect the same saved custom colors
