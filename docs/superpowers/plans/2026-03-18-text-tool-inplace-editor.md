# Text Tool In-Place Editor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the modal-based text tool with an in-place canvas editor that shows a blinking cursor, blue border, move handles, and resize handle directly on the canvas.

**Architecture:** Use GTK4 Entry widget positioned as an overlay on the canvas. Draw custom border and handles using Cairo rendering. Entry handles text input, cursor blinking, and multi-line wrapping automatically.

**Tech Stack:** Rust, GTK4, Cairo, existing editor state management

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src/capture/editor/types.rs` | Add `ActiveTextEdit` struct for tracking active text editing session |
| `src/capture/editor/state.rs` | Add state field, modify text action creation logic |
| `src/capture/editor/ui_support.rs` | Remove modal functions, add handle drawing utilities |
| `src/capture/editor/events.rs` | Add click handler for Text tool, handle drag events for move/resize |
| `src/capture/editor/render.rs` | Add overlay rendering for border and handles |
| `src/capture/editor/window/mod.rs` | Add Entry overlay positioning, integrate with canvas draw |

---

## Chunk 1: State and Types

### Task 1: Add ActiveTextEdit Type

**Files:**
- Modify: `src/capture/editor/types.rs`

- [ ] **Step 1: Read the types.rs file to find where to add the struct**

```bash
# Read the file to understand structure
# Find around line 400 where AnnotationAction::Text is defined
```

- [ ] **Step 2: Add ActiveTextEdit struct after the existing types**

Add this after the AnnotationAction enum (around line 425):

```rust
#[derive(Debug, Clone)]
pub enum MoveHandle {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeHandle {
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct TextEditBounds {
    pub rect: Rect,
    pub move_handles: Vec<(MoveHandle, Point)>,
    pub resize_handle: Option<(ResizeHandle, Point)>,
}

impl TextEditBounds {
    pub fn new(position: Point, width: f64, height: f64) -> Self {
        let rect = Rect::new(position.x as i32, position.y as i32, width as i32, height as i32);
        
        let left_center = Point { x: position.x, y: position.y + height / 2.0 };
        let right_center = Point { x: position.x + width, y: position.y + height / 2.0 };
        let resize_pos = Point { x: position.x + width, y: position.y + height };
        
        Self {
            rect,
            move_handles: vec![
                (MoveHandle::Left, left_center),
                (MoveHandle::Right, right_center),
            ],
            resize_handle: Some((ResizeHandle::BottomRight, resize_pos)),
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/capture/editor/types.rs
git commit -m "feat(editor): add ActiveTextEdit types for in-place text editing"
```

---

### Task 2: Add State Fields

**Files:**
- Modify: `src/capture/editor/state.rs`

- [ ] **Step 1: Read the EditorState struct**

```bash
# Read lines 1-60 to see the struct definition
```

- [ ] **Step 2: Add import for new types**

Add to the imports (around line 11-13):
```rust
use super::types::{MoveHandle, TextEditBounds, ResizeHandle};
```

- [ ] **Step 3: Add state fields to EditorState**

Add after `select_effect_rebuild_pending` (around line 38):

```rust
pub active_text_edit: Option<ActiveTextEdit>,
pub active_text_entry: Option<gtk4::Entry>,
pub active_text_bounds: Option<TextEditBounds>,
pub active_text_is_dragging: bool,
pub active_text_drag_handle: Option<MoveHandle>,
pub active_text_drag_start: Option<Point>,
```

- [ ] **Step 4: Initialize new fields in EditorState::new**

Find the `new` function and add:
```rust
active_text_edit: None,
active_text_entry: None,
active_text_bounds: None,
active_text_is_dragging: false,
active_text_drag_handle: None,
active_text_drag_start: None,
```

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/state.rs
git commit -m "feat(editor): add state fields for active text editing"
```

---

## Chunk 2: Remove Modal Functions

### Task 3: Remove Text Modal Functions

**Files:**
- Modify: `src/capture/editor/ui_support.rs`

- [ ] **Step 1: Find and remove show_text_modal function**

Search for `fn show_text_modal` - it's around line 1948.
Remove the entire function (lines 1948-2100 approximately).

- [ ] **Step 2: Find and remove show_text_dialog function**

Search for `pub fn show_text_dialog` - around line 2079.
Remove this function.

- [ ] **Step 3: Find and remove show_text_edit_dialog function**

Search for `pub fn show_text_edit_dialog` - around line 2123.
Remove this function.

- [ ] **Step 4: Commit**

```bash
git add src/capture/editor/ui_support.rs
git commit -m "feat(editor): remove text modal functions"
```

---

## Chunk 3: Add Render Functions for Border and Handles

### Task 4: Add Text Edit Overlay Rendering

**Files:**
- Modify: `src/capture/editor/render.rs`

- [ ] **Step 1: Read render.rs to find where to add functions**

Look at line 371 where `draw_selection_handles` is defined - we'll add similar functions.

- [ ] **Step 2: Add imports for new types**

Add after existing imports:
```rust
use super::types::{MoveHandle, ResizeHandle, TextEditBounds};
```

- [ ] **Step 3: Add draw_text_edit_border function**

Add after `draw_selection_handles` (around line 401):

```rust
const TEXT_EDIT_BORDER_COLOR: (f64, f64, f64) = (0.231, 0.510, 0.965); // #3b82f6
const TEXT_EDIT_BORDER_WIDTH: f64 = 2.0;
const TEXT_EDIT_BORDER_RADIUS: f64 = 4.0;

pub fn draw_text_edit_border(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);
    let _ = context.save();
    
    let rect = &bounds.rect;
    let x = rect.x as f64;
    let y = rect.y as f64;
    let width = rect.width as f64;
    let height = rect.height as f64;
    
    // Draw rounded rectangle border
    context.set_source_rgba(
        TEXT_EDIT_BORDER_COLOR.0,
        TEXT_EDIT_BORDER_COLOR.1,
        TEXT_EDIT_BORDER_COLOR.2,
        1.0,
    );
    context.set_line_width(TEXT_EDIT_BORDER_WIDTH / scale);
    
    let radius = TEXT_EDIT_BORDER_RADIUS;
    context.begin_path();
    context.move_to(x + radius, y);
    context.line_to(x + width - radius, y);
    context.arc(x + width - radius, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
    context.line_to(x + width, y + height - radius);
    context.arc(x + width - radius, y + height - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
    context.line_to(x + radius, y + height);
    context.arc(x + radius, y + height - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    context.line_to(x, y + radius);
    context.arc(x + radius, y + radius, radius, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2);
    context.close_path();
    
    let _ = context.stroke();
    let _ = context.restore();
}
```

- [ ] **Step 4: Add draw_text_edit_handles function**

Add after the border function:

```rust
const MOVE_HANDLE_RADIUS: f64 = 5.0;
const MOVE_HANDLE_OUTLINE_WIDTH: f64 = 2.0;
const RESIZE_HANDLE_SIZE: f64 = 12.0;

pub fn draw_text_edit_handles(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    active_handle: Option<MoveHandle>,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);
    let _ = context.save();
    
    // Draw move handles (left and right circles)
    for (handle, center) in &bounds.move_handles {
        let is_active = active_handle.is_some_and(|h| h == *handle);
        let radius = MOVE_HANDLE_RADIUS + if is_active { 1.0 } else { 0.0 };
        
        // White outline
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
        context.arc(center.x, center.y, radius / scale, 0.0, std::f64::consts::TAU);
        let _ = context.stroke();
        
        // Blue fill
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        context.arc(center.x, center.y, (radius - MOVE_HANDLE_OUTLINE_WIDTH) / scale, 0.0, std::f64::consts::TAU);
        let _ = context.fill();
    }
    
    // Draw resize handle (bottom-right box)
    if let Some((_, resize_pos)) = &bounds.resize_handle {
        let size = RESIZE_HANDLE_SIZE;
        let half = size / 2.0;
        
        // White outline
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
        context.rectangle(
            resize_pos.x - half / scale,
            resize_pos.y - half / scale,
            size / scale,
            size / scale,
        );
        let _ = context.stroke();
        
        // Blue fill
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        context.rectangle(
            resize_pos.x - half / scale + MOVE_HANDLE_OUTLINE_WIDTH / scale,
            resize_pos.y - half / scale + MOVE_HANDLE_OUTLINE_WIDTH / scale,
            (size - MOVE_HANDLE_OUTLINE_WIDTH * 2.0) / scale,
            (size - MOVE_HANDLE_OUTLINE_WIDTH * 2.0) / scale,
        );
        let _ = context.fill();
    }
    
    let _ = context.restore();
}
```

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/render.rs
git commit -m "feat(editor): add render functions for text edit border and handles"
```

---

## Chunk 4: Add Canvas Click Handler for Text Tool

### Task 5: Modify Text Tool Click Behavior

**Files:**
- Modify: `src/capture/editor/window/events.rs`

- [ ] **Step 1: Find the Text tool click handler in events.rs**

Search for `Tool::Text =>` around line 1134.

- [ ] **Step 2: Replace the modal call with new behavior**

Replace the existing `Tool::Text` match arm:

```rust
Tool::Text => {
    // Start in-place text editing
    let (selected_color, text_size, font_family) = {
        let st = state_click.lock().unwrap();
        (st.selected_color, st.text_size, st.text_font_family.clone())
    };
    
    // Create text edit state
    let text_bounds = TextEditBounds::new(
        image_point,
        200.0, // Initial width
        text_size + 16.0, // Initial height (text size + padding)
    );
    
    // Store in state
    {
        let mut st = state_click.lock().unwrap();
        st.active_text_bounds = Some(text_bounds);
        // Entry will be created and managed in window/mod.rs
    }
    
    // Queue redraw
    if let Some(area) = drawing_area_click.upgrade() {
        area.queue_draw();
    }
}
```

- [ ] **Step 3: Add imports for new types**

Add to the imports in events.rs:
```rust
use super::super::types::{MoveHandle, TextEditBounds};
```

- [ ] **Step 4: Commit**

```bash
git add src/capture/editor/window/events.rs
git commit -m "feat(editor): modify text tool click to start in-place editing"
```

---

## Chunk 5: Add Entry Overlay to Window

### Task 6: Add Entry Widget and Overlay Drawing

**Files:**
- Modify: `src/capture/editor/window/mod.rs`

- [ ] **Step 1: Read mod.rs to understand the structure**

Look for how the drawing area is set up and where drawing happens.

- [ ] **Step 2: Add imports**

Add imports for the new types:
```rust
use super::types::{MoveHandle, TextEditBounds};
use super::render::{draw_text_edit_border, draw_text_edit_handles};
```

- [ ] **Step 3: Find where actions are drawn and add text edit overlay**

Search for where `draw_action` is called in the draw function. Add after drawing all actions:

```rust
// Draw active text edit overlay (border + handles)
if let Some(bounds) = st.active_text_bounds.as_ref() {
    draw_text_edit_border(context, bounds, t.scale);
    draw_text_edit_handles(context, bounds, None, t.scale); // None = no active handle initially
}
```

- [ ] **Step 4: Add Entry creation and positioning**

This requires adding a function to create and position the Entry widget. The Entry needs to:
- Be created when Text tool is selected and canvas is clicked
- Be positioned at the click location (converted from image to view coordinates)
- Be sized based on the bounds
- Be destroyed when editing ends

Add this function in mod.rs:

```rust
fn create_text_entry_overlay(
    window: &ApplicationWindow,
    drawing_area: &DrawingArea,
    bounds: &TextEditBounds,
    initial_text: &str,
    color: DrawColor,
    font: &FontSettings,
) -> Entry {
    let entry = Entry::new();
    entry.set_text(initial_text);
    entry.set_halign(gtk4::Align::Start);
    entry.set_valign(gtk4::Align::Start);
    
    // Set font
    let pango_font = format!("{} {}", font.family, font.size as i32);
    entry.set_css_classes(&["text-entry-overlay"]);
    
    // Position and size
    let x = bounds.rect.x as f64;
    let y = bounds.rect.y as f64;
    let width = bounds.rect.width as i32;
    let height = bounds.rect.height as i32;
    
    entry.set_margin_start(x as i32);
    entry.set_margin_top(y as i32);
    entry.set_width_request(width);
    entry.set_height_request(height);
    
    // Make it expand
    entry.set_hexpand(true);
    entry.set_vexpand(true);
    
    // Set text color via CSS
    let css_provider = CssProvider::new();
    let css = format!(
        ".text-entry-overlay {{ color: rgba({}, {}, {}, {}); background: transparent; }}",
        (color.r * 255.0) as u8,
        (color.g * 255.0) as u8,
        (color.b * 255.0) as u8,
        color.a
    );
    css_provider.load_from_string(&css);
    entry.get_style_context().add_provider(&css_provider, 400);
    
    entry
}
```

- [ ] **Step 5: Add CSS for the Entry overlay**

Add to the CSS section in ui_support.rs:

```rust
.text-entry-overlay {
    background: transparent;
    border: none;
    outline: none;
}
```

- [ ] **Step 6: Commit**

```bash
git add src/capture/editor/window/mod.rs
git add src/capture/editor/ui_support.rs
git commit -m "feat(editor): add Entry overlay for text editing"
```

---

## Chunk 6: Add Move and Resize Handle Dragging

### Task 7: Implement Handle Dragging

**Files:**
- Modify: `src/capture/editor/window/events.rs`

- [ ] **Step 1: Add motion and button press/release handlers for handles**

In the motion controller, add logic to detect when hovering over handles:

```rust
// In motion handler, add after existing logic:
let text_bounds = state_motion.lock().unwrap().active_text_bounds.clone();
if let Some(bounds) = text_bounds {
    // Check if hovering over move handles
    for (handle, center) in &bounds.move_handles {
        let dx = x - center.x;
        let dy = y - center.y;
        if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_RADIUS * 2.0 {
            // Set cursor to grab
            if let Some(window) = window_motion.upgrade() {
                set_window_cursor_name(&window, Some("grab"));
            }
            return;
        }
    }
    // Check resize handle
    if let Some((_, resize_pos)) = &bounds.resize_handle {
        let dx = x - resize_pos.x;
        let dy = y - resize_pos.y;
        if dx.abs() < RESIZE_HANDLE_SIZE && dy.abs() < RESIZE_HANDLE_SIZE {
            if let Some(window) = window_motion.upgrade() {
                set_window_cursor_name(&window, Some("nwse-resize"));
            }
            return;
        }
    }
}
```

- [ ] **Step 2: Add button press handler for starting drag**

Add to the button press event handler:

```rust
// Check for text edit handle click
if let Some(bounds) = state_press.lock().unwrap().active_text_bounds.as_ref() {
    let view_point = Point { x, y };
    let t = *transform_press.lock().unwrap();
    let image_point = t.view_to_image(view_point);
    
    // Check move handles
    for (handle, center) in &bounds.move_handles {
        let center_view = t.image_to_view(*center);
        let dx = x - center_view.x;
        let dy = y - center_view.y;
        if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_RADIUS * 2.5 {
            state_press.lock().unwrap().active_text_is_dragging = true;
            state_press.lock().unwrap().active_text_drag_handle = Some(*handle);
            state_press.lock().unwrap().active_text_drag_start = Some(image_point);
            return;
        }
    }
    
    // Check resize handle
    if let Some((_, resize_pos)) = &bounds.resize_handle {
        let resize_view = t.image_to_view(*resize_pos);
        let dx = x - resize_view.x;
        let dy = y - resize_view.y;
        if dx.abs() < RESIZE_HANDLE_SIZE * 1.5 && dy.abs() < RESIZE_HANDLE_SIZE * 1.5 {
            state_press.lock().unwrap().active_text_is_dragging = true;
            state_press.lock().unwrap().active_text_drag_handle = None; // None = resize
            state_press.lock().unwrap().active_text_drag_start = Some(image_point);
            return;
        }
    }
}
```

- [ ] **Step 3: Add motion handler for dragging**

In the motion handler, update position during drag:

```rust
// Handle text edit dragging
if let Some(bounds) = state_motion.lock().unwrap().active_text_bounds.as_mut() {
    if state_motion.lock().unwrap().active_text_is_dragging {
        if let Some(start_point) = *state_motion.lock().unwrap().active_text_drag_start {
            let view_point = Point { x, y };
            let t = *transform_motion.lock().unwrap();
            let current_point = t.view_to_image(view_point);
            
            let dx = current_point.x - start_point.x;
            let dy = current_point.y - start_point.y;
            
            let handle = state_motion.lock().unwrap().active_text_drag_handle;
            
            match handle {
                Some(MoveHandle::Left) | Some(MoveHandle::Right) => {
                    // Move text horizontally
                    bounds.rect.x = (bounds.rect.x as f64 + dx) as i32;
                    bounds.rect.x = bounds.rect.x.max(0); // Constrain to image bounds
                }
                None => {
                    // Resize - adjust width
                    let new_width = (bounds.rect.width as f64 + dx).max(50.0);
                    bounds.rect.width = new_width as i32;
                }
                _ => {}
            }
            
            // Update handle positions
            let height = bounds.rect.height as f64;
            bounds.move_handles[0].1 = Point { 
                x: bounds.rect.x as f64, 
                y: bounds.rect.y as f64 + height / 2.0 
            };
            bounds.move_handles[1].1 = Point { 
                x: bounds.rect.x as f64 + bounds.rect.width as f64, 
                y: bounds.rect.y as f64 + height / 2.0 
            };
            if let Some((_, pos)) = &mut bounds.resize_handle {
                pos.x = bounds.rect.x as f64 + bounds.rect.width as f64;
                pos.y = bounds.rect.y as f64 + bounds.rect.height as f64;
            }
            
            // Update drag start for next frame
            state_motion.lock().unwrap().active_text_drag_start = Some(current_point);
            
            // Queue redraw
            if let Some(area) = drawing_area_motion.upgrade() {
                area.queue_draw();
            }
            return;
        }
    }
}
```

- [ ] **Step 4: Add button release to end drag**

In the button release handler:

```rust
// End text edit drag
if state_release.lock().unwrap().active_text_is_dragging {
    state_release.lock().unwrap().active_text_is_dragging = false;
    state_release.lock().unwrap().active_text_drag_handle = None;
    state_release.lock().unwrap().active_text_drag_start = None;
}
```

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/window/events.rs
git commit -m "feat(editor): add move and resize handle dragging"
```

---

## Chunk 7: Handle Commit Behaviors

### Task 8: Add Text Commit Logic

**Files:**
- Modify: `src/capture/editor/window/events.rs`, `src/capture/editor/state.rs`

- [ ] **Step 1: Add function to commit text to actions**

In state.rs, add a method to finalize text:

```rust
pub fn commit_text_edit(&mut self, bounds: &TextEditBounds, text: String, color: DrawColor, font: FontSettings) {
    if text.trim().is_empty() {
        return;
    }
    
    let position = Point {
        x: bounds.rect.x as f64,
        y: bounds.rect.y as f64,
    };
    
    self.add_action(AnnotationAction::Text {
        position,
        text,
        color,
        font,
    });
    
    self.active_text_edit = None;
    self.active_text_entry = None;
    self.active_text_bounds = None;
}

pub fn cancel_text_edit(&mut self) {
    self.active_text_edit = None;
    self.active_text_entry = None;
    self.active_text_bounds = None;
}
```

- [ ] **Step 2: Add keyboard handler for Escape and Enter**

In events.rs, add key press handler:

```rust
let key_controller = EventControllerKey::new();
let state_key = state.clone();
let drawing_area_key = drawing_area.downgrade();
let window_key = window.downgrade();

key_controller.connect_key_pressed(move |_, key, _, _| {
    let keyval = key;
    
    if keyval == gdk4::KEY_Escape {
        // Cancel text editing
        state_key.lock().unwrap().cancel_text_edit();
        if let Some(area) = drawing_area_key.upgrade() {
            area.queue_draw();
        }
        // Remove Entry overlay
        // ... (need to track Entry widget)
        return Propagation::Stop;
    }
    
    if keyval == gdk4::KEY_Return || keyval == gdk4::KEY_KP_Enter {
        // Commit text
        let bounds = state_key.lock().unwrap().active_text_bounds.clone();
        let entry = state_key.lock().unwrap().active_text_entry.as_ref();
        if let (Some(bounds), Some(entry)) = (bounds, entry) {
            let text = entry.text().to_string();
            let st = state_key.lock().unwrap();
            let color = st.selected_color;
            let font = FontSettings {
                family: st.text_font_family.clone(),
                size: st.text_size,
                style: FontStyle::Normal,
                decoration: TextDecoration::None,
                alignment: TextAlignment::Left,
            };
            st.commit_text_edit(&bounds, text, color, font);
        }
        if let Some(area) = drawing_area_key.upgrade() {
            area.queue_draw();
        }
        return Propagation::Stop;
    }
    
    Propagation::Proceed
});

drawing_area.add_controller(key_controller);
```

- [ ] **Step 3: Add click-outside-to-commit**

In the canvas click handler, check if click is outside text bounds:

```rust
// Check if click is outside active text edit area
if let Some(bounds) = state_click.lock().unwrap().active_text_bounds.as_ref() {
    let t = *transform_click.lock().unwrap();
    let click_image = t.view_to_image(Point { x, y });
    
    let in_bounds = click_image.x >= bounds.rect.x as f64
        && click_image.x <= (bounds.rect.x + bounds.rect.width) as f64
        && click_image.y >= bounds.rect.y as f64
        && click_image.y <= (bounds.rect.y + bounds.rect.height) as f64;
    
    if !in_bounds {
        // Commit text
        let bounds = state_click.lock().unwrap().active_text_bounds.clone();
        if let Some(bounds) = bounds {
            let text = state_click.lock().unwrap()
                .active_text_entry.as_ref()
                .map(|e| e.text().to_string())
                .unwrap_or_default();
            
            if !text.trim().is_empty() {
                let st = state_click.lock().unwrap();
                let color = st.selected_color;
                let font = FontSettings {
                    family: st.text_font_family.clone(),
                    size: st.text_size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                st.commit_text_edit(&bounds, text, color, font);
            } else {
                state_click.lock().unwrap().cancel_text_edit();
            }
        }
        
        if let Some(area) = drawing_area_click.upgrade() {
            area.queue_draw();
        }
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add src/capture/editor/state.rs src/capture/editor/window/events.rs
git commit -m "feat(editor): add text commit behaviors"
```

---

## Chunk 8: Edge Detection and Text Wrapping

### Task 9: Add Edge Detection

**Files:**
- Modify: `src/capture/editor/window/events.rs`, `src/capture/editor/state.rs`

- [ ] **Step 1: Add image bounds to state**

In state.rs, the image dimensions should already be available via `base_image.width()` and `base_image.height()`.

- [ ] **Step 2: Constrain handle movement to bounds**

In the drag handler, add constraints:

```rust
// Constrain to image bounds
let image_width = state_motion.lock().unwrap().base_image.width() as f64;
let image_height = state_motion.lock().unwrap().base_image.height() as f64;

// In move handler:
bounds.rect.x = bounds.rect.x.max(0).min(image_width - bounds.rect.width as f64) as i32;

// In resize handler:
bounds.rect.width = bounds.rect.width.max(50).min(image_width - bounds.rect.x as f64) as i32;
```

- [ ] **Step 3: Commit**

```bash
git add src/capture/editor/window/events.rs src/capture/editor/state.rs
git commit -m "feat(editor): add edge detection for text editing"
```

---

## Testing

### Manual Test Checklist

- [ ] Select Text tool from toolbar
- [ ] Click on canvas - Entry appears with blinking cursor at click position
- [ ] Blue rounded border appears around Entry
- [ ] Left/right circle handles appear on edges (blue with white outline)
- [ ] Bottom-right resize box appears
- [ ] Type text - it appears in the Entry
- [ ] Drag left handle - text moves left/right
- [ ] Drag right handle - text moves left/right
- [ ] Drag resize handle - text area width changes
- [ ] Text wraps when it reaches edge
- [ ] Click outside - text is committed
- [ ] Press Escape - text is discarded
- [ ] Press Enter - text is committed
- [ ] Select another tool - text is committed
