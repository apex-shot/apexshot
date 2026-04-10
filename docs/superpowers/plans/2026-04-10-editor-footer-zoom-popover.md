# Editor Footer Zoom Popover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the footer pin control with a zoom percentage trigger that opens a compact in-editor popover with zoom actions and instructional rows.

**Architecture:** Keep the change local to the editor footer, event wiring, and shared editor CSS. Build the zoom trigger and popover in the footer module, pass the new widgets through window assembly into event wiring, and reuse the current view transform plus existing fit behavior for the three zoom actions.

**Tech Stack:** Rust, GTK4, existing editor window modules, source-string unit tests via `include_str!`, `cargo test`

---

## File Structure

- Modify: `src/capture/editor/window/footer.rs`
  Responsible for building the footer UI, including the new zoom trigger, popover, action buttons, and instructional rows.
- Modify: `src/capture/editor/window/mod.rs`
  Responsible for unpacking the new footer parts and threading them into `EventContext`.
- Modify: `src/capture/editor/window/events.rs`
  Responsible for wiring the zoom trigger, applying zoom steps, fitting the canvas back to viewport, updating the footer percentage label, and closing the popover after action clicks.
- Modify: `src/capture/editor/ui_support.rs`
  Responsible for compact footer zoom trigger styling and the popover surface language derived from the side-panel visuals.

### Task 1: Build the Footer Zoom Trigger Shell

**Files:**
- Modify: `src/capture/editor/window/footer.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/footer.rs`
- Test: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Write the failing structural tests for the new footer UI**

```rust
#[test]
fn footer_replaces_pin_button_with_zoom_trigger_and_popover() {
    let source = include_str!("footer.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("pub zoom_button: Button,")
            && production_source.contains("pub zoom_label: Label,")
            && production_source.contains("pub zoom_popover: Popover,")
            && production_source.contains("zoom_popover.set_position(gtk4::PositionType::Top);")
            && production_source.contains("zoom_popover.set_parent(&zoom_button);")
            && !production_source.contains("pub pin_btn: Button,")
            && !production_source.contains("pub pin_icon: Image,"),
        "Footer should expose a zoom button, label, and popover instead of the pin button state",
    );
}

#[test]
fn footer_popover_contains_three_actions_and_two_instruction_rows() {
    let source = include_str!("footer.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("let zoom_in_btn = Button::with_label(\"Zoom In\");")
            && production_source.contains("let zoom_out_btn = Button::with_label(\"Zoom Out\");")
            && production_source.contains("let fit_to_screen_btn = Button::with_label(\"Fit to Screen\");")
            && production_source.contains("let scroll_hint = Label::new(Some(\"Zoom with the scroll wheel\"));")
            && production_source.contains("let pan_hint = Label::new(Some(\"Pan with the right button\"));"),
        "Footer popover should include the three action rows and the two instructional rows from the approved design",
    );
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run: `cargo test footer_ --lib`

Expected: FAIL because `FooterParts` and `build_footer` still expose the old pin button fields.

- [ ] **Step 3: Implement the footer zoom trigger and thread it through window assembly**

```rust
use gtk4::{prelude::*, Box as GtkBox, Button, Label, Orientation, Popover};

pub(super) struct FooterParts {
    pub root: GtkBox,
    pub zoom_button: Button,
    pub zoom_label: Label,
    pub zoom_popover: Popover,
    pub zoom_in_btn: Button,
    pub zoom_out_btn: Button,
    pub fit_to_screen_btn: Button,
    pub drag_btn: Button,
    pub copy_btn: Button,
    pub upload_btn: Button,
}

pub(super) fn build_footer(copy_icon_name: &str, upload_icon_name: &str) -> FooterParts {
    let zoom_button = Button::new();
    zoom_button.set_has_frame(false);
    zoom_button.add_css_class("editor-footer-zoom-button");

    let zoom_label = Label::new(Some("100%"));
    zoom_label.add_css_class("editor-footer-zoom-label");
    zoom_button.set_child(Some(&zoom_label));

    let zoom_popover = Popover::new();
    zoom_popover.set_has_arrow(false);
    zoom_popover.set_autohide(true);
    zoom_popover.set_position(gtk4::PositionType::Top);
    zoom_popover.add_css_class("editor-popover");
    zoom_popover.add_css_class("editor-footer-zoom-popover");
    zoom_popover.set_parent(&zoom_button);

    let zoom_list = GtkBox::new(Orientation::Vertical, 0);
    zoom_list.add_css_class("editor-popover-list");
    zoom_list.add_css_class("editor-footer-zoom-list");

    let zoom_in_btn = Button::with_label("Zoom In");
    zoom_in_btn.add_css_class("editor-popover-list-item");
    zoom_in_btn.add_css_class("flat");

    let zoom_out_btn = Button::with_label("Zoom Out");
    zoom_out_btn.add_css_class("editor-popover-list-item");
    zoom_out_btn.add_css_class("flat");

    let fit_to_screen_btn = Button::with_label("Fit to Screen");
    fit_to_screen_btn.add_css_class("editor-popover-list-item");
    fit_to_screen_btn.add_css_class("flat");

    let scroll_hint = Label::new(Some("Zoom with the scroll wheel"));
    scroll_hint.add_css_class("editor-footer-zoom-hint");

    let pan_hint = Label::new(Some("Pan with the right button"));
    pan_hint.add_css_class("editor-footer-zoom-hint");

    zoom_list.append(&zoom_in_btn);
    zoom_list.append(&zoom_out_btn);
    zoom_list.append(&fit_to_screen_btn);
    zoom_list.append(&scroll_hint);
    zoom_list.append(&pan_hint);
    zoom_popover.set_child(Some(&zoom_list));

    // mod.rs unpacking target
    let footer_parts = footer::build_footer(icon_names::LINK, icon_names::ARROW_UPLOAD);
    let zoom_button = footer_parts.zoom_button;
    let zoom_label = footer_parts.zoom_label;
    let zoom_popover = footer_parts.zoom_popover;
```

- [ ] **Step 4: Run the focused tests to verify they pass**

Run: `cargo test footer_ --lib`

Expected: PASS with both new footer structure tests green.

- [ ] **Step 5: Commit the footer shell work**

```bash
git add src/capture/editor/window/footer.rs src/capture/editor/window/mod.rs
git commit -m "feat: add footer zoom trigger shell"
```

### Task 2: Wire Zoom Actions and Label Sync

**Files:**
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Test: `src/capture/editor/window/events.rs`
- Test: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Write the failing event-wiring tests**

```rust
#[test]
fn event_context_uses_zoom_footer_fields_instead_of_pin_state() {
    let source = include_str!("events.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("pub zoom_button: Button,")
            && production_source.contains("pub zoom_label: Label,")
            && production_source.contains("pub zoom_popover: Popover,")
            && production_source.contains("pub zoom_in_btn: Button,")
            && production_source.contains("pub zoom_out_btn: Button,")
            && production_source.contains("pub fit_to_screen_btn: Button,")
            && !production_source.contains("pub pin_btn: Button,")
            && !production_source.contains("pub initial_pin_state: bool,"),
        "EventContext should be updated to drive footer zoom controls instead of pinning state",
    );
}

#[test]
fn footer_zoom_actions_update_transform_and_label() {
    let source = include_str!("events.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("zoom_button.connect_clicked(move |_| {")
            && production_source.contains("zoom_popover.popup();")
            && production_source.contains("zoom_in_btn.connect_clicked(move |_| {")
            && production_source.contains("zoom_out_btn.connect_clicked(move |_| {")
            && production_source.contains("fit_to_screen_btn.connect_clicked(move |_| {")
            && production_source.contains("zoom_label.set_label(&format!(\"{}%\","))
            && production_source.contains("drawing_area.queue_draw();"),
        "Footer zoom actions should open the popover, update the transform, refresh the footer label, and redraw the canvas",
    );
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run: `cargo test footer_zoom_ --lib`

Expected: FAIL because `events.rs` still wires pinning and has no footer zoom action handlers.

- [ ] **Step 3: Implement the zoom event wiring and fit-to-screen helper path**

```rust
pub(super) struct EventContext {
    pub zoom_button: Button,
    pub zoom_label: Label,
    pub zoom_popover: Popover,
    pub zoom_in_btn: Button,
    pub zoom_out_btn: Button,
    pub fit_to_screen_btn: Button,
    // existing fields continue below
}

fn update_zoom_footer_label(label: &Label, transform: &Arc<Mutex<ViewTransform>>) {
    let percent = {
        let t = transform.lock().unwrap();
        (t.scale * 100.0).round().clamp(1.0, 999.0) as i32
    };
    label.set_label(&format!("{}%", percent));
}

fn apply_zoom_step(
    transform: &Arc<Mutex<ViewTransform>>,
    factor: f64,
    zoom_label: &Label,
    drawing_area: &DrawingArea,
) {
    {
        let mut t = transform.lock().unwrap();
        t.scale = (t.scale * factor).clamp(0.1, 8.0);
    }
    update_zoom_footer_label(zoom_label, transform);
    drawing_area.queue_draw();
}

zoom_button.connect_clicked(move |_| {
    zoom_popover.popup();
});

zoom_in_btn.connect_clicked(move |b| {
    apply_zoom_step(&transform_zoom_in, 1.1, &zoom_label_zoom_in, &drawing_area_zoom_in);
    if let Some(popover) = b.ancestor(Popover::static_type()) {
        popover.downcast::<Popover>().unwrap().popdown();
    }
});

zoom_out_btn.connect_clicked(move |b| {
    apply_zoom_step(&transform_zoom_out, 1.0 / 1.1, &zoom_label_zoom_out, &drawing_area_zoom_out);
    if let Some(popover) = b.ancestor(Popover::static_type()) {
        popover.downcast::<Popover>().unwrap().popdown();
    }
});

fit_to_screen_btn.connect_clicked(move |b| {
    {
        let mut t = transform_fit.lock().unwrap();
        *t = ViewTransform::fit(
            t.image_width,
            t.image_height,
            drawing_area_fit.width() as f64,
            drawing_area_fit.height() as f64,
        );
    }
    update_zoom_footer_label(&zoom_label_fit, &transform_fit);
    drawing_area_fit.queue_draw();
    if let Some(popover) = b.ancestor(Popover::static_type()) {
        popover.downcast::<Popover>().unwrap().popdown();
    }
});
```

- [ ] **Step 4: Run the focused tests to verify they pass**

Run: `cargo test footer_zoom_ --lib`

Expected: PASS with event wiring tests green and no pin-specific footer state remaining.

- [ ] **Step 5: Commit the event wiring work**

```bash
git add src/capture/editor/window/events.rs src/capture/editor/window/mod.rs
git commit -m "feat: wire footer zoom popover actions"
```

### Task 3: Add Compact Popover Styling That Reuses Side-Panel Surface Language

**Files:**
- Modify: `src/capture/editor/ui_support.rs`
- Test: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Write the failing CSS regression tests**

```rust
#[test]
fn footer_zoom_popover_reuses_inspector_surface_language_without_sidebar_dimensions() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains(".editor-footer-zoom-popover {\n                padding: 8px;")
            && production_source.contains("background: rgba(20, 20, 22, 0.96);")
            && production_source.contains("border: 1px solid rgba(255, 255, 255, 0.08);")
            && production_source.contains("border-radius: 12px;")
            && !production_source.contains(".editor-footer-zoom-popover {\n                min-height: 100%;")
            && !production_source.contains(".editor-footer-zoom-popover {\n                width: 210px;"),
        "Footer zoom popover should borrow inspector surface styling without becoming a full-height or fixed-width sidebar",
    );
}

#[test]
fn footer_zoom_rows_distinguish_actions_from_instructional_rows() {
    let source = include_str!("ui_support.rs");
    let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(
        production_source.contains("button.editor-footer-zoom-action {\n                min-height: 32px;")
            && production_source.contains("button.editor-footer-zoom-action:hover {\n                background: rgba(255, 255, 255, 0.08);")
            && production_source.contains(".editor-footer-zoom-hint {\n                color: rgba(241, 241, 243, 0.68);"),
        "Footer zoom styling should keep action rows interactive and hint rows clearly non-interactive",
    );
}
```

- [ ] **Step 2: Run the focused tests to verify they fail**

Run: `cargo test footer_zoom_popover --lib`

Expected: FAIL because the footer zoom popover CSS classes do not exist yet.

- [ ] **Step 3: Implement the footer zoom CSS and align footer button classes with it**

```rust
// footer.rs class hooks
zoom_in_btn.add_css_class("editor-footer-zoom-action");
zoom_out_btn.add_css_class("editor-footer-zoom-action");
fit_to_screen_btn.add_css_class("editor-footer-zoom-action");

// ui_support.rs CSS block
.editor-footer-zoom-popover {
    padding: 8px;
    background: rgba(20, 20, 22, 0.96);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
}

.editor-footer-zoom-list {
    spacing: 4px;
}

button.editor-footer-zoom-button {
    min-height: 24px;
    padding: 0 8px;
    border-radius: 8px;
    background: transparent;
}

button.editor-footer-zoom-action {
    min-height: 32px;
    padding: 0 12px;
    border-radius: 8px;
    background: transparent;
    color: rgba(241, 241, 243, 0.92);
    text-align: left;
}

button.editor-footer-zoom-action:hover {
    background: rgba(255, 255, 255, 0.08);
}

.editor-footer-zoom-hint {
    margin-top: 4px;
    color: rgba(241, 241, 243, 0.68);
}
```

- [ ] **Step 4: Run the focused tests to verify they pass**

Run: `cargo test footer_zoom_popover --lib`

Expected: PASS with the new compact popover styling locked in.

- [ ] **Step 5: Commit the styling work**

```bash
git add src/capture/editor/window/footer.rs src/capture/editor/ui_support.rs
git commit -m "style: add compact footer zoom popover styling"
```

### Task 4: Final Verification and Regression Sweep

**Files:**
- Modify: `src/capture/editor/window/footer.rs`
- Modify: `src/capture/editor/window/mod.rs`
- Modify: `src/capture/editor/window/events.rs`
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Run the full focused editor test batch**

Run: `cargo test footer_ --lib`

Expected: PASS with all new footer zoom tests green.

- [ ] **Step 2: Run a wider editor window regression batch**

Run:
- `cargo test obfuscate_inspector_uses_a_fresh_list_instead_of_reusing_toolbar_popover_state --lib`
- `cargo test toolbar_places_color_status_before_done_button_in_right_controls --lib`
- `cargo test number_inspector_style_and_size_rows_use_matching_row_composition --lib`

Expected: PASS, confirming the new footer popover work did not regress nearby editor window and popover structure tests.

- [ ] **Step 3: Run formatting if needed**

Run: `cargo fmt --all`

Expected: command succeeds with no formatting errors.

- [ ] **Step 4: Review the final diff**

Run: `git diff -- src/capture/editor/window/footer.rs src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/ui_support.rs`

Expected: diff shows only the footer zoom trigger, wiring, and compact popover styling changes described in the approved spec.

- [ ] **Step 5: Commit the verified final state**

```bash
git add src/capture/editor/window/footer.rs src/capture/editor/window/mod.rs src/capture/editor/window/events.rs src/capture/editor/ui_support.rs
git commit -m "feat: replace footer pin with zoom popover"
```
