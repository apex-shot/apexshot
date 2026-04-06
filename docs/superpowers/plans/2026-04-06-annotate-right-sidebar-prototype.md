# Annotate Right Sidebar Prototype Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent right inspector sidebar to the annotate editor prototype, move the Background tool controls into it, and show placeholder inspector content for other tools.

**Architecture:** Keep the existing top toolbar and tool-selection flow intact. Restructure the editor root layout so the canvas remains central while a fixed-width inspector panel sits on the right. Reuse the existing background panel widget tree instead of redesigning its internals, and gate inspector content by current tool selection.

**Tech Stack:** Rust 2021, GTK4, existing ApexShot editor window/state/ui_support modules

---

## File map

- Modify: `src/capture/editor/window/mod.rs` — editor root layout, inspector panel creation, tool-change visibility wiring
- Modify: `src/capture/editor/window/toolbar.rs` — ensure tool selection state can drive inspector content cleanly
- Modify: `src/capture/editor/window/background_panel.rs` — expose/build the background controls as a reusable inspector widget
- Modify: `src/capture/editor/ui_support.rs` — inspector/sidebar CSS classes
- Test: `src/capture/editor/window/mod.rs` source-level regression tests for persistent inspector shell

### Task 1: Add failing regression tests for inspector shell wiring

**Files:**
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn editor_layout_includes_persistent_right_inspector_shell() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(production_source.contains("editor-right-inspector"));
}

#[test]
fn editor_layout_tracks_background_inspector_content() {
    let source = include_str!("mod.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(production_source.contains("background_inspector"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test inspector_shell`
Expected: FAIL because the strings/widgets do not exist yet.

- [ ] **Step 3: Write minimal implementation**

Add the test module only.

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test inspector_shell`
Expected: FAIL

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs
git commit -m "test: add editor inspector shell regression tests"
```

### Task 2: Add persistent right inspector layout shell

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Update editor root layout**

Create a horizontal content split so the center canvas stays left and a fixed right inspector panel is appended on the right.

```rust
let workspace = GtkBox::new(Orientation::Horizontal, 0);
workspace.set_hexpand(true);
workspace.set_vexpand(true);

let inspector = GtkBox::new(Orientation::Vertical, 0);
inspector.add_css_class("editor-right-inspector");
inspector.set_width_request(280);
inspector.set_vexpand(true);

workspace.append(&canvas);
workspace.append(&inspector);
root.append(&workspace);
```

- [ ] **Step 2: Add minimal inspector styling**

```css
.editor-right-inspector {
    min-width: 280px;
    background: rgba(20, 20, 20, 0.94);
    border-left: 1px solid rgba(255,255,255,0.08);
    padding: 16px;
}
```

- [ ] **Step 3: Run regression tests**

Run: `cargo test inspector_shell`
Expected: PASS

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "feat: add persistent editor inspector shell"
```

### Task 3: Mount Background controls in the inspector

**Files:**
- Modify: `src/capture/editor/window/background_panel.rs`
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Expose a reusable background inspector widget**

Refactor the background panel builder so it returns the widget to mount in the right inspector without changing its internal controls.

```rust
pub(super) struct BackgroundPanelParts {
    pub root: GtkBox,
}
```

- [ ] **Step 2: Add the Background inspector content to the right panel**

```rust
let background_inspector = background_panel::build_background_panel(...);
inspector.append(&background_inspector.root);
```

- [ ] **Step 3: Keep it prototype-safe**

Do not remove existing background behavior beyond what is needed to render the controls in the inspector.

- [ ] **Step 4: Run targeted tests/build**

Run: `cargo build`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/background_panel.rs src/capture/editor/window/mod.rs
git commit -m "feat: mount background controls in editor inspector"
```

### Task 4: Show placeholder inspector content for non-background tools

**Files:**
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Add placeholder inspector panels**

```rust
let placeholder = Label::new(Some("Tool options coming soon"));
placeholder.add_css_class("editor-inspector-placeholder");
```

- [ ] **Step 2: Switch inspector content by selected tool**

When the Background tool is active, show `background_inspector`; otherwise show the placeholder.

```rust
match current_tool {
    Tool::Background => background_inspector.set_visible(true),
    _ => placeholder.set_visible(true),
}
```

- [ ] **Step 3: Add basic placeholder styling**

```css
.editor-inspector-placeholder {
    opacity: 0.7;
}
```

- [ ] **Step 4: Run verification**

Run: `cargo build && cargo test inspector_shell`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/mod.rs src/capture/editor/ui_support.rs
git commit -m "feat: prototype contextual editor inspector content"
```

### Task 5: Final prototype verification

**Files:**
- Modify: none

- [ ] **Step 1: Run automated verification**

Run: `cargo build && cargo test inspector_shell`
Expected: success

- [ ] **Step 2: Run manual verification**

Run: `cargo run -- edit <image-path>`
Expected: editor opens with persistent right sidebar; Background tool shows full controls there; other tools show placeholder text.

- [ ] **Step 3: Check for regressions**

Verify toolbar still selects tools and canvas remains usable.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: prototype right inspector for annotate editor"
```
