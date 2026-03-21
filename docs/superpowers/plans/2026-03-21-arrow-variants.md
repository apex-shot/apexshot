# Arrow Variants Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a sub-tool dropdown to the Arrow tool with 4 style variants: Standard, Fancy, Curved, and Double.

**Architecture:** Follow the established sub-tool pattern used by Obfuscate (method selector), Pen/Highlighter (weight selector). Add `ArrowStyle` enum, wire through types → state → toolbar → events → render layers. Curved/Double variants add Bezier rendering and control point interaction.

**Tech Stack:** Rust, GTK4 (gtk4 crate), Cairo for rendering

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/capture/editor/types.rs` | `ArrowStyle` enum definition, `AnnotationAction::Arrow` updated with `style` + `control_points` |
| `src/capture/editor/state.rs` | `arrow_style` state field, getter/setter, draft/finalize with style, control point dragging |
| `src/capture/editor/render.rs` | `draw_arrow()` dispatch by style, `draw_arrow_head()`, `draw_arrow_control_handles()` |
| `src/capture/editor/window/toolbar.rs` | `build_arrow_style_controls()`, populate list, visibility toggle |
| `src/capture/editor/window/events.rs` | Arrow style button wiring, control point mouse events |
| `src/capture/editor/window/mod.rs` | Destructure new toolbar parts, pass to events |
| `src/capture/editor.rs` | Update existing tests for new Arrow fields |

---

## Chunk 1: Data Model — ArrowStyle enum + AnnotationAction update

### Task 1: Create ArrowStyle enum in types.rs

**File:** `src/capture/editor/types.rs`

- [ ] **Step 1: Add ArrowStyle enum after ObfuscateMethod (line ~71)**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowStyle {
    Standard,
    Fancy,
    Curved,
    Double,
}

impl ArrowStyle {
    pub const ALL: [Self; 4] = [Self::Standard, Self::Fancy, Self::Curved, Self::Double];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::Fancy => "Fancy",
            Self::Curved => "Curved",
            Self::Double => "Double",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Standard => "go-next-symbolic",
            Self::Fancy => "go-next-symbolic",
            Self::Curved => "path-bezier-symbolic",
            Self::Double => "object-flip-horizontal-symbolic",
        }
    }
}
```

### Task 2: Update AnnotationAction::Arrow in types.rs

**File:** `src/capture/editor/types.rs`

- [ ] **Step 1: Update the Arrow variant (line 431-436)**

Change from:
```rust
Arrow {
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
},
```

To:
```rust
Arrow {
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<[Point; 3]>, // [start_handle, mid_handle, end_handle] for Curved/Double
},
```

### Task 3: Update all match arms on AnnotationAction::Arrow

These files pattern match on `Arrow { start, end, color, stroke_size }` — update to include `style` and `control_points`:

**File:** `src/capture/editor/render.rs`
- [ ] Line 79-84: `draw_annotation_action` dispatch — add `..` to pattern, pass `style`/`control_points` to `draw_arrow`
- [ ] Line 163-169: `draw_draft_action` dispatch — same pattern update

**File:** `src/capture/editor/state.rs`
- [ ] Line 1315: `selected_action_color` — add `..` to Arrow pattern
- [ ] Line 1343: `set_selected_action_color` — add `..` to Arrow pattern
- [ ] Line 2078: `clamp_action_to_image` — add `..` to Arrow pattern

### Task 4: Update all AnnotationAction::Arrow construction sites

**File:** `src/capture/editor/state.rs`
- [ ] Line 2231-2236: `draft_action` — add `style: self.arrow_style, control_points: None`
- [ ] Line 2377-2382: `finalize_drag_action` — add `style: self.arrow_style, control_points: None`

**File:** `src/capture/editor.rs` (tests)
- [ ] Line 431-436: test `undo_redo_stack_behaves_correctly` — add `style: ArrowStyle::Standard, control_points: None`

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Passes (may fail on other pattern matches — fix any remaining)

- [ ] **Step 3: Commit**

```bash
git add src/capture/editor/types.rs src/capture/editor/render.rs src/capture/editor/state.rs src/capture/editor.rs
git commit -m "feat: add ArrowStyle enum and update AnnotationAction::Arrow data model"
```

---

## Chunk 2: State Management — arrow_style field

### Task 5: Add arrow_style to EditorState

**File:** `src/capture/editor/state.rs`

- [ ] **Step 1: Add field to EditorState struct (near line 46, after obfuscate fields)**

```rust
pub arrow_style: ArrowStyle,
```

- [ ] **Step 2: Initialize in EditorState::new() (near line 283, after obfuscate_method)**

```rust
arrow_style: ArrowStyle::Standard,
```

- [ ] **Step 3: Add getter/setter methods (near line 951, after obfuscate_method getter/setter)**

```rust
pub fn set_arrow_style(&mut self, style: ArrowStyle) {
    self.arrow_style = style;
}

pub fn arrow_style(&self) -> ArrowStyle {
    self.arrow_style
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/state.rs
git commit -m "feat: add arrow_style state field with getter/setter"
```

---

## Chunk 3: Rendering — Standard & Fancy arrow styles

### Task 6: Update draw_arrow() for Standard and Fancy

**File:** `src/capture/editor/render.rs`

- [ ] **Step 1: Add arrowhead drawing helper function (before draw_arrow, ~line 772)**

```rust
fn draw_arrow_head(
    context: &gtk4::cairo::Context,
    tip: Point,
    angle: f64,
    head_length: f64,
    spread: f64,
    color: DrawColor,
) {
    let left_x = tip.x - head_length * (angle - spread).cos();
    let left_y = tip.y - head_length * (angle - spread).sin();
    let right_x = tip.x - head_length * (angle + spread).cos();
    let right_y = tip.y - head_length * (angle + spread).sin();

    context.move_to(tip.x, tip.y);
    context.line_to(left_x, left_y);
    context.line_to(right_x, right_y);
    context.close_path();
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();
}
```

- [ ] **Step 2: Update draw_arrow() signature to accept ArrowStyle**

Change signature from:
```rust
pub fn draw_arrow(context, start, end, color, stroke_size)
```
To:
```rust
pub fn draw_arrow(context, start, end, color, stroke_size, style: ArrowStyle, control_points: Option<[Point; 3]>)
```

- [ ] **Step 3: Update draw_arrow() body with style dispatch**

Replace the head drawing section with:
```rust
pub fn draw_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<[Point; 3]>,
) {
    let stroke = stroke_size.max(0.5);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke + 0.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < 0.1 && dy.abs() < 0.1 {
        return;
    }

    let angle = dy.atan2(dx);
    let line_length = (dx * dx + dy * dy).sqrt().max(1.0);

    // Draw the line/curve
    match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            if let Some([_s, mid, _e]) = control_points {
                context.move_to(start.x, start.y);
                context.curve_to(mid.x, mid.y, mid.x, mid.y, end.x, end.y);
            } else {
                context.move_to(start.x, start.y);
                context.line_to(end.x, end.y);
            }
        }
        _ => {
            context.move_to(start.x, start.y);
            context.line_to(end.x, end.y);
        }
    }
    let _ = context.stroke();

    // Draw arrowhead(s)
    let head_length = (stroke * 4.8)
        .clamp(12.0, 120.0)
        .min((line_length * 0.75).max(8.0));

    let spread = match style {
        ArrowStyle::Fancy => 0.3,   // Sharp/narrow head
        _ => 0.55,                   // Standard spread
    };

    // End arrowhead (all styles)
    let end_angle = if let (ArrowStyle::Curved | ArrowStyle::Double, Some([_s, mid, _e])) = (style, control_points) {
        (end.y - mid.y).atan2(end.x - mid.x)
    } else {
        angle
    };
    draw_arrow_head(context, end, end_angle, head_length, spread, color);

    // Start arrowhead (Double only)
    if matches!(style, ArrowStyle::Double) {
        let start_angle = if let Some([_s, mid, _e]) = control_points {
            (start.y - mid.y).atan2(start.x - mid.x) + std::f64::consts::PI
        } else {
            angle + std::f64::consts::PI
        };
        draw_arrow_head(context, start, start_angle, head_length, spread, color);
    }
}
```

- [ ] **Step 4: Update draw_annotation_action dispatch (line 79-84)**

Change:
```rust
AnnotationAction::Arrow { start, end, color, stroke_size } =>
    draw_arrow(context, *start, *end, *color, *stroke_size),
```
To:
```rust
AnnotationAction::Arrow { start, end, color, stroke_size, style, control_points } =>
    draw_arrow(context, *start, *end, *color, *stroke_size, *style, *control_points),
```

- [ ] **Step 5: Update draw_draft_action dispatch (line 163-169)**

Same pattern — add `style` and `control_points` to the destructuring and pass them:
```rust
AnnotationAction::Arrow { start, end, color, stroke_size, style, control_points } =>
    draw_arrow(context, *start, *end, color.with_alpha(0.82), *stroke_size, *style, *control_points),
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check`

- [ ] **Step 7: Run tests**

Run: `cargo test`

- [ ] **Step 8: Commit**

```bash
git add src/capture/editor/render.rs
git commit -m "feat: implement Standard and Fancy arrow rendering"
```

---

## Chunk 4: Toolbar — Arrow style dropdown

### Task 7: Add arrow style dropdown builder

**File:** `src/capture/editor/window/toolbar.rs`

- [ ] **Step 1: Add build_arrow_style_controls() function (after build_pen_weight_dropdown, ~line 266)**

```rust
fn build_arrow_style_controls() -> (GtkBox, Button, Popover, GtkBox) {
    let arrow_style_group = GtkBox::new(Orientation::Horizontal, 4);
    arrow_style_group.add_css_class("editor-arrow-style-group");
    arrow_style_group.add_css_class("editor-tools-group");
    arrow_style_group.set_visible(false);

    let arrow_style_button = Button::new();
    arrow_style_button.set_has_frame(false);
    arrow_style_button.set_focusable(false);
    arrow_style_button.add_css_class("editor-tool-button");
    arrow_style_button.add_css_class("flat");
    arrow_style_button.set_tooltip_text(Some("Arrow style"));

    let arrow_style_icon = Image::from_icon_name(ArrowStyle::Standard.icon_name());
    arrow_style_button.set_child(Some(&arrow_style_icon));

    let arrow_style_popover = Popover::new();
    arrow_style_popover.set_has_arrow(false);
    arrow_style_popover.set_autohide(true);
    arrow_style_popover.add_css_class("editor-popover");
    arrow_style_popover.set_parent(&arrow_style_button);

    let arrow_style_list = GtkBox::new(Orientation::Vertical, 0);
    arrow_style_list.add_css_class("editor-popover-list");
    arrow_style_popover.set_child(Some(&arrow_style_list));

    let p_popover = arrow_style_popover.clone();
    arrow_style_button.connect_clicked(move |_| {
        p_popover.popup();
    });

    arrow_style_group.append(&arrow_style_button);

    (arrow_style_group, arrow_style_button, arrow_style_popover, arrow_style_list)
}
```

- [ ] **Step 2: Add arrow style fields to ToolbarModeParts struct (~line 71)**

Add after `number_options_group`:
```rust
pub arrow_style_group: GtkBox,
#[allow(dead_code)]
pub arrow_style_button: Button,
#[allow(dead_code)]
pub arrow_style_popover: Popover,
pub arrow_style_list: GtkBox,
```

- [ ] **Step 3: Build and populate arrow style list in build_toolbar_mode_controls() (~line 760, after pen weight population)**

```rust
// Build arrow style selector
let (arrow_style_group, arrow_style_button, arrow_style_popover, arrow_style_list) =
    build_arrow_style_controls();

for style in ArrowStyle::ALL {
    let btn_box = GtkBox::new(Orientation::Horizontal, 8);
    btn_box.set_margin_start(8);
    btn_box.set_margin_end(8);
    btn_box.set_margin_top(4);
    btn_box.set_margin_bottom(4);

    let icon = Image::from_icon_name(style.icon_name());
    let label_widget = Label::new(Some(style.display_name()));

    btn_box.append(&icon);
    btn_box.append(&label_widget);

    let btn = Button::builder()
        .has_frame(false)
        .css_classes(["editor-popover-list-item", "flat"])
        .child(&btn_box)
        .build();

    arrow_style_list.append(&btn);
}
```

- [ ] **Step 4: Append arrow_style_group to standard_mode_group (~line 808)**

Add after `pen_weight_group` append:
```rust
standard_mode_group.append(&arrow_style_group);
```

- [ ] **Step 5: Add arrow_style fields to ToolbarModeParts return (~line 830)**

Add to the struct literal:
```rust
arrow_style_group,
arrow_style_button,
arrow_style_popover,
arrow_style_list,
```

- [ ] **Step 6: Add visibility toggle in build_toolbar_tool_updater() (~line 922)**

Add `arrow_style_group: &GtkBox` parameter and clone it, then in the closure:
```rust
let is_arrow_tool = matches!(tool, Tool::Arrow);
arrow_style_group.set_visible(is_arrow_tool);
```

- [ ] **Step 7: Update call site in mod.rs (~line 511)**

Pass `&arrow_style_group` to `build_toolbar_tool_updater`.

- [ ] **Step 8: Verify compilation**

Run: `cargo check`

- [ ] **Step 9: Commit**

```bash
git add src/capture/editor/window/toolbar.rs src/capture/editor/window/mod.rs
git commit -m "feat: add arrow style dropdown to toolbar"
```

---

## Chunk 5: Events — Wire arrow style dropdown

### Task 8: Wire arrow style button events

**File:** `src/capture/editor/window/events.rs`

- [ ] **Step 1: Add arrow style fields to EventContext struct (~line 95)**

```rust
pub arrow_style_button: Button,
pub arrow_style_list: gtk4::Box,
```

- [ ] **Step 2: Destructure in mod.rs (~line 202)**

Add to the toolbar destructuring:
```rust
arrow_style_group,
arrow_style_button,
arrow_style_popover: _,
arrow_style_list,
```

- [ ] **Step 3: Pass to EventContext in mod.rs (~line 1619)**

```rust
arrow_style_button: arrow_style_button.clone(),
arrow_style_list: arrow_style_list.clone(),
```

- [ ] **Step 4: Wire arrow style list items in events.rs (after obfuscate method wiring, ~line 782)**

```rust
// Wire up arrow style list items
let styles = ArrowStyle::ALL;

let arrow_style_button = arrow_style_button.clone();

let mut style_idx = 0usize;
let mut child_opt = arrow_style_list.first_child();
while let Some(child) = child_opt {
    child_opt = child.next_sibling();

    let Ok(button) = child.clone().downcast::<Button>() else {
        continue;
    };

    let Some(&style) = styles.get(style_idx) else {
        break;
    };
    style_idx += 1;

    let state_arrow_style = state.clone();
    let drawing_area_arrow_style = drawing_area.downgrade();
    let arrow_style_button = arrow_style_button.clone();

    button.connect_clicked(move |b| {
        {
            let mut st = state_arrow_style.lock().unwrap();
            st.set_arrow_style(style);
        }

        // Update the trigger button icon
        if let Some(child) = arrow_style_button.child() {
            if let Ok(img) = child.downcast::<Image>() {
                img.set_icon_name(Some(style.icon_name()));
            }
        }

        if let Some(popover) = b.ancestor(Popover::static_type()) {
            popover.downcast::<Popover>().unwrap().popdown();
        }
        if let Some(area) = drawing_area_arrow_style.upgrade() {
            area.queue_draw();
        }
    });
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`

- [ ] **Step 6: Commit**

```bash
git add src/capture/editor/window/events.rs src/capture/editor/window/mod.rs
git commit -m "feat: wire arrow style dropdown events"
```

---

## Chunk 6: Curved/Double — Control point rendering and interaction

### Task 9: Draw control handles for Curved/Double arrows

**File:** `src/capture/editor/render.rs`

- [ ] **Step 1: Add draw_arrow_control_handles() function (after draw_arrow)**

```rust
pub fn draw_arrow_control_handles(
    context: &gtk4::cairo::Context,
    handles: [Point; 3],
    color: DrawColor,
) {
    // Draw dashed control polygon
    context.set_source_rgba(color.r, color.g, color.b, 0.4);
    context.set_line_width(1.0);
    context.set_dash(&[4.0, 4.0], 0.0);
    context.move_to(handles[0].x, handles[0].y);
    context.line_to(handles[1].x, handles[1].y);
    context.line_to(handles[2].x, handles[2].y);
    let _ = context.stroke();
    context.set_dash(&[], 0.0);

    // Draw handle circles
    for (i, handle) in handles.iter().enumerate() {
        let radius = if i == 1 { 6.0 } else { 4.0 }; // Midpoint handle is larger
        context.arc(handle.x, handle.y, radius, 0.0, 2.0 * std::f64::consts::PI);
        context.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        let _ = context.fill_preserve();
        context.set_source_rgba(color.r, color.g, color.b, color.a);
        context.set_line_width(1.5);
        let _ = context.stroke();
    }
}
```

- [ ] **Step 2: Call control handles in draw_draft_action when Curved/Double arrow has control_points**

In `draw_draft_action` (render.rs ~line 163), after the `draw_arrow` call, add:
```rust
if matches!(style, ArrowStyle::Curved | ArrowStyle::Double) {
    if let Some(handles) = control_points {
        draw_arrow_control_handles(context, handles, *color);
    }
}
```

### Task 10: Control point creation and dragging in state.rs

**File:** `src/capture/editor/state.rs`

- [ ] **Step 1: Add state fields for arrow control point editing**

Add to EditorState struct (~line 40):
```rust
pub arrow_editing_controls: bool,          // true when control handles are visible
pub arrow_control_dragging: Option<usize>, // Some(index) if user is dragging a handle
```

Initialize in `new()`:
```rust
arrow_editing_controls: false,
arrow_control_dragging: None,
```

- [ ] **Step 2: Update finalize_drag_action for Curved/Double (~line 2377)**

After creating the Arrow action, if style is Curved or Double, set `arrow_editing_controls = true` and initialize control points at midpoint:
```rust
Tool::Arrow => {
    let style = self.arrow_style;
    let control_points = match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            let mid = Point {
                x: (start.x + end.x) / 2.0,
                y: (start.y + end.y) / 2.0,
            };
            self.arrow_editing_controls = true;
            Some([start, mid, end])
        }
        _ => None,
    };
    Some(AnnotationAction::Arrow {
        start,
        end,
        color,
        stroke_size,
        style,
        control_points,
    })
}
```

- [ ] **Step 3: Add method to check if a point is near a control handle**

```rust
const CONTROL_HANDLE_HIT_RADIUS: f64 = 10.0;

pub fn arrow_control_handle_at(&self, point: Point) -> Option<usize> {
    if !self.arrow_editing_controls {
        return None;
    }
    let action = self.selected_action()?;
    if let AnnotationAction::Arrow { control_points: Some(handles), .. } = action {
        for (i, handle) in handles.iter().enumerate() {
            let dx = point.x - handle.x;
            let dy = point.y - handle.y;
            if (dx * dx + dy * dy).sqrt() < CONTROL_HANDLE_HIT_RADIUS {
                return Some(i);
            }
        }
    }
    None
}
```

- [ ] **Step 4: Add method to update control handle position**

```rust
pub fn move_arrow_control_handle(&mut self, index: usize, new_pos: Point) {
    let Some(action_index) = self.selected_action_index else { return };
    let Some(action) = self.actions.get_mut(action_index) else { return };
    if let AnnotationAction::Arrow { control_points: Some(handles), start, end, .. } = action {
        match index {
            0 => { *start = new_pos; handles[0] = new_pos; }
            1 => { handles[1] = new_pos; } // Midpoint — curves the arrow
            2 => { *end = new_pos; handles[2] = new_pos; }
            _ => {}
        }
    }
}
```

- [ ] **Step 5: Add method to finalize control editing**

```rust
pub fn finalize_arrow_control_editing(&mut self) {
    self.arrow_editing_controls = false;
    self.arrow_control_dragging = None;
}
```

### Task 11: Wire control point mouse events in events.rs

**File:** `src/capture/editor/window/events.rs`

- [ ] **Step 1: In the mouse press handler, check for control handle hit before normal drag**

When tool is Arrow and mouse is pressed, check `state.arrow_control_handle_at(point)`. If a handle is hit, set `state.arrow_control_dragging = Some(handle_index)` and skip normal drag.

- [ ] **Step 2: In the mouse drag handler, update control handle position**

If `state.arrow_control_dragging.is_some()`, call `state.move_arrow_control_handle(index, point)` and redraw.

- [ ] **Step 3: In the mouse release handler, clear control dragging**

If `state.arrow_control_dragging.is_some()`, set it to `None` and redraw.

- [ ] **Step 4: When switching tools or deselecting, call `finalize_arrow_control_editing()`**

In tool switch handlers and when clicking outside, call `state.finalize_arrow_control_editing()`.

- [ ] **Step 5: Verify compilation**

Run: `cargo check`

- [ ] **Step 6: Run all tests**

Run: `cargo test`

- [ ] **Step 7: Commit**

```bash
git add src/capture/editor/render.rs src/capture/editor/state.rs src/capture/editor/window/events.rs
git commit -m "feat: implement Curved/Double arrow control point rendering and interaction"
```

---

## Chunk 7: Final verification and cleanup

### Task 12: Full verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 3: Manual test checklist**

1. Open editor → Arrow tool is default, no sub-tool button visible
2. Click Arrow button → sub-tool dropdown appears with 4 options
3. Select Standard → draw arrow → works like before (smooth head, end only)
4. Select Fancy → draw arrow → sharp/narrow arrowhead
5. Select Curved → draw arrow → control handles appear → drag midpoint → arrow curves
6. Select Double → draw arrow → control handles → curve → arrowheads on both ends
7. Switch tools while Curved arrow is being edited → handles disappear, arrow finalizes
8. Undo/redo works for all arrow styles
9. Color picker works for all arrow styles
10. Stroke size slider works for all arrow styles

- [ ] **Step 4: Final commit (if any fixes needed)**

```bash
git add -A && git commit -m "fix: final adjustments for arrow variants"
```
