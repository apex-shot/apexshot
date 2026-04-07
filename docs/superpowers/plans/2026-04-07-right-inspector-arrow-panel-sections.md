# Right Inspector Arrow Panel Sections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the Arrow inspector tab into three direct sections, `Style`, `Thickness`, and `Behavior`, using existing Arrow hooks and keeping the inspector width unchanged.

**Architecture:** Keep the existing inspector shell and `Arrow | Colors` routing in `window/mod.rs`, but build the Arrow primary surface as a dedicated inspector-native panel instead of a reused toolbar dropdown shell. Reuse the existing Arrow style and stroke-size state hooks, and add a `Behavior` section only if it can map to the existing `inverse_arrow_direction` path without introducing new Arrow state or render logic.

**Tech Stack:** Rust, GTK4, existing annotate editor window modules, source-level regression tests with `include_str!`, `cargo test`, `cargo check`

---

## File map

- Modify: `src/capture/editor/window/mod.rs` — Arrow inspector layout, direct section builders, source regression tests
- Modify: `src/capture/editor/window/events.rs` — wire the Arrow inspector section widgets into existing Arrow state hooks
- Modify: `src/capture/editor/window/toolbar.rs` — keep only the shared line/arrow stroke toolbar surface where still needed, update source regression tests if layout markers change
- Modify: `src/capture/editor/ui_support.rs` — inspector section/list styling for the Arrow panel
- Test: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/toolbar.rs`

Width constraint:
- keep using the existing fixed inspector width path
- do not add any Arrow-specific width constants or alternate panel widths

### Task 1: Add failing regression tests for Arrow panel sections

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Add a failing source test for the three Arrow sections**

Add this test in `src/capture/editor/window/mod.rs` near the existing inspector tests:

```rust
#[test]
fn arrow_inspector_includes_style_thickness_and_behavior_sections() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("append_inspector_section(&arrow_inspector_content, \"Style\"")
            && production_source.contains("append_inspector_section(&arrow_inspector_content, \"Thickness\"")
            && production_source.contains("append_inspector_section(&arrow_inspector_content, \"Behavior\""),
        "Arrow inspector should render Style, Thickness, and Behavior sections",
    );
}
```

- [ ] **Step 2: Add a failing source test for width reuse**

Add this companion test in `src/capture/editor/window/mod.rs`:

```rust
#[test]
fn arrow_inspector_reuses_existing_fixed_sidebar_width() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);")
            && !production_source.contains("ARROW_SIDEBAR_WIDTH"),
        "Arrow inspector should reuse the shared fixed sidebar width instead of introducing a new width path",
    );
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test arrow_inspector_includes_style_thickness_and_behavior_sections --lib
cargo test arrow_inspector_reuses_existing_fixed_sidebar_width --lib
```

Expected: the section test fails until `Thickness` and `Behavior` are added; the width test may already pass if the current builder still reuses `BACKGROUND_SIDEBAR_WIDTH`.

- [ ] **Step 4: Commit the regression tests**

```bash
git add src/capture/editor/window/mod.rs
git commit -m "test: add Arrow inspector section regression coverage"
```

### Task 2: Build the Arrow inspector as a dedicated three-section panel

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Replace the current Arrow panel body with three direct sections**

In `src/capture/editor/window/mod.rs`, keep the existing Arrow inspector surface, but structure it as:

```rust
let (arrow_inspector, arrow_inspector_content) = build_tool_inspector();
append_inspector_section(&arrow_inspector_content, "Style", arrow_style_list.upcast_ref());
append_inspector_section(&arrow_inspector_content, "Thickness", arrow_thickness_list.upcast_ref());
append_inspector_section(&arrow_inspector_content, "Behavior", arrow_behavior_group.upcast_ref());
```

Use fresh Arrow inspector-native widget trees for all three sections instead of toolbar trigger buttons.

- [ ] **Step 2: Build a direct `Thickness` list for Arrow**

Create an Arrow-only thickness list in `src/capture/editor/window/mod.rs` using the same four existing stroke sizes:

```rust
let arrow_thickness_list = GtkBox::new(Orientation::Vertical, 0);
for (label, _size, weight) in [
    ("Thin", 2.0_f64, PenWeight::Small),
    ("Medium", 4.0_f64, PenWeight::Medium),
    ("Thick", 7.0_f64, PenWeight::Large),
    ("Very Thick", 12.0_f64, PenWeight::ExtraLarge),
] {
    // build sidebar-native button rows
}
```

This list is Arrow-only for the inspector even though the underlying stroke-size state is shared with Line.

- [ ] **Step 3: Build an Arrow `Behavior` section only from existing hooks**

Create a compact `Behavior` group in `src/capture/editor/window/mod.rs` with a single existing-hook candidate:

```rust
let arrow_behavior_group = GtkBox::new(Orientation::Vertical, 8);
let inverse_direction_toggle = gtk4::CheckButton::with_label("Reverse direction");
arrow_behavior_group.append(&inverse_direction_toggle);
```

Do not add any other behavior rows unless they map directly to already-supported Arrow state/config.

- [ ] **Step 4: Add minimal inspector styling**

In `src/capture/editor/ui_support.rs`, add or adjust classes for:

```css
.editor-inspector-section
.editor-inspector-section-body
.editor-inspector-option-list
.editor-inspector-toggle-row
```

Keep them within the existing fixed-width panel and avoid any new width rules beyond the shared sidebar path.

- [ ] **Step 5: Run the section tests and compile check**

Run:

```bash
cargo test arrow_inspector_includes_style_thickness_and_behavior_sections --lib
cargo test arrow_inspector_reuses_existing_fixed_sidebar_width --lib
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit the Arrow panel structure**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "feat: add Arrow inspector style thickness and behavior sections"
```

### Task 3: Wire Arrow thickness and behavior to existing state hooks

**Files:**
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Wire the Arrow thickness list to the existing stroke-size hook**

In `src/capture/editor/window/events.rs`, add a dedicated loop for the new Arrow inspector thickness list that mirrors the existing stroke-size button wiring:

```rust
let arrow_thickness_sizes: [(f64, PenWeight); 4] = [
    (2.0, PenWeight::Small),
    (4.0, PenWeight::Medium),
    (7.0, PenWeight::Large),
    (12.0, PenWeight::ExtraLarge),
];
```

Each row should call:

```rust
st.set_stroke_size(size);
```

and then queue a redraw.

- [ ] **Step 2: Wire the Arrow behavior toggle only if it maps to existing state**

If the current editor already exposes `inverse_arrow_direction` as mutable runtime state, wire the new `Reverse direction` toggle to that path:

```rust
inverse_direction_toggle.connect_toggled({
    let state = state.clone();
    let drawing_area = drawing_area.downgrade();
    move |toggle| {
        {
            let mut st = state.lock().unwrap();
            st.inverse_arrow_direction = toggle.is_active();
        }
        if let Some(area) = drawing_area.upgrade() {
            area.queue_draw();
        }
    }
});
```

If runtime mutation is not already a supported path, stop here and keep `Behavior` out of implementation rather than inventing a new model.

- [ ] **Step 3: Sync the `Behavior` control from current state**

Add a small sync closure in `src/capture/editor/window/mod.rs` so the Arrow behavior toggle reflects current editor state when Arrow becomes active:

```rust
let sync_arrow_behavior_controls: Rc<dyn Fn()> = Rc::new({
    let state = state.clone();
    let inverse_direction_toggle = inverse_direction_toggle.clone();
    move || {
        let st = state.lock().unwrap();
        inverse_direction_toggle.set_active(st.inverse_arrow_direction);
    }
});
```

Call that sync from the Arrow tool routing path before the Arrow panel becomes visible.

- [ ] **Step 4: Run focused verification**

Run:

```bash
cargo test capture::editor::window --lib
cargo check
```

Expected: PASS.

- [ ] **Step 5: Commit the Arrow hook wiring**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/window/events.rs
git commit -m "feat: wire Arrow inspector thickness and behavior controls"
```

### Task 4: Final verification for Arrow-only panel completion

**Files:**
- Modify: none unless verification exposes a bug

- [ ] **Step 1: Run the final automated verification**

Run:

```bash
cargo test capture::editor::window --lib
cargo check
```

Expected: PASS.

- [ ] **Step 2: Run manual Arrow verification**

Open the annotate editor and verify:

```text
1. Select Arrow and confirm the primary tab shows Style, Thickness, and Behavior sections.
2. Change Arrow style and confirm new arrows use the selected style.
3. Change Arrow thickness and confirm new arrows use the selected thickness.
4. Toggle Reverse direction, if included, and confirm the Arrow behavior changes immediately.
5. Switch to Colors and back to Arrow and confirm the panel state stays coherent.
6. Confirm the panel width is unchanged from the existing inspector width.
7. Confirm Text, Number, and Background panels still render within the same sidebar shell.
```

- [ ] **Step 3: Commit the verified Arrow panel slice**

```bash
git add -A
git commit -m "feat: finish Arrow inspector panel sections"
```

## Self-review

Spec coverage:
- Arrow-only scope is covered by Tasks 2 through 4
- `Style`, `Thickness`, and `Behavior` sections are covered by Tasks 1 and 2
- behavior limited to existing hooks is enforced in Task 3
- unchanged width is covered by Tasks 1, 2, and 4

Placeholder scan:
- no `TODO` or `TBD` placeholders remain
- each task names exact files and commands

Type consistency:
- the plan consistently refers to `arrow_thickness_list`, `arrow_behavior_group`, and `inverse_direction_toggle`
- `Behavior` is consistently limited to existing Arrow hooks only
