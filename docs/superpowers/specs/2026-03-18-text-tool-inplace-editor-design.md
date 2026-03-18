# Text Tool In-Place Editor Design

## Overview

Replace the modal-based text tool with an in-place canvas editor. When the user selects the Text tool and clicks on the canvas, a blinking text cursor appears at that position with a blue rounded border, move handles on left/right edges, and a resize handle at the bottom-right.

## UI/UX Specification

### Visual Design

#### Colors
- **Accent Blue**: `#3b82f6` (from slider highlight)
- **Handle Fill**: `#3b82f6`
- **Handle Outline**: `#ffffff` (white, 2px)
- **Border**: `#3b82f6` with 4px border-radius

#### Typography
- Use current text tool font settings (family, size, style from state)
- Default: Sans, 16pt

#### Layout
- **Border**: 4px rounded rectangle surrounding text area
- **Move Handles**: Circles (10px diameter) on left and right edges, vertically centered
- **Resize Handle**: Square box (12x12px) at bottom-right corner
- **Padding**: 8px between text and border

### Components

1. **Text Input Area**
   - GTK Entry widget positioned at click location
   - Transparent background
   - Shows blinking cursor
   - Auto-centers text initially, wraps to paragraph when text exceeds width

2. **Blue Border**
   - Drawn around the Entry
   - 4px border-radius
   - `#3b82f6` color

3. **Move Handles (Left/Right)**
   - Blue circle (`#3b82f6`) with white outline (2px)
   - 10px diameter
   - Draggable to reposition text horizontally

4. **Resize Handle (Bottom-Right)**
   - 12x12px box
   - Blue fill with white outline
   - Draggable to resize text area width

### Behavior

#### Text Input
- Click on canvas → Entry appears at click position with blinking cursor
- Empty initially (no placeholder)
- Supports multi-line (wraps automatically)
- Auto-center text alignment by default
- Switches to left-aligned paragraph mode when text grows beyond initial width

#### Edge Detection
- Border detects screenshot left/right edges
- Text wraps to stay within image bounds
- If user drags text near edge, it constrains to image bounds

#### Commit Actions
- **Click outside**: Commit text to canvas, close editor
- **Escape**: Cancel and close editor (discard text)
- **Another tool selected**: Commit text, close editor
- **Enter**: Commit text, close editor (single-line behavior)

#### Move Handles
- Drag left/right circles to move text horizontally
- Constrained to image bounds
- Cursor changes to "grab" on hover, "grabbing" when dragging

#### Resize Handle
- Drag bottom-right box to resize text area width
- Minimum width: 50px
- Maximum width: image width - current x position
- Text wraps to fit new width

## Technical Implementation

### State Changes

#### New State Fields (EditorState)
```rust
// Text editing state
pub active_text_edit: Option<ActiveTextEdit>,

pub struct ActiveTextEdit {
    pub position: Point,           // Initial click position (image coordinates)
    pub current_text: String,     // Current text content
    pub entry_widget: Entry,      // GTK Entry overlay
    pub bounds: Rect,             // Current text bounds (for border/handles)
    pub is_dragging_move: bool,   // Dragging move handle
    pub is_dragging_resize: bool, // Dragging resize handle
    pub drag_handle: Option<MoveHandle>, // Which handle is being dragged
    pub drag_start: Point,        // Drag start position
}
```

### Files to Modify

1. **types.rs** - Add `ActiveTextEdit` struct
2. **state.rs** - Add state field, update text action handling
3. **ui_support.rs** - Remove `show_text_modal`, `show_text_dialog`, `show_text_edit_dialog`
4. **events.rs** - Add canvas click handler for Text tool (new behavior), add move/resize handle events
5. **render.rs** - Add function to draw text edit overlay (border + handles)
6. **window/mod.rs** - Add overlay drawing for active text edit, handle Entry positioning

### Implementation Phases

1. **Phase 1**: Remove modal and add basic Entry overlay on canvas click
2. **Phase 2**: Add blue rounded border around Entry
3. **Phase 3**: Add left/right move handles with drag functionality
4. **Phase 4**: Add bottom-right resize handle with drag functionality
5. **Phase 5**: Add edge detection and text wrapping
6. **Phase 6**: Handle commit/cancel behaviors

## Acceptance Criteria

1. ✓ Clicking on canvas with Text tool shows blinking cursor at click position (no modal)
2. ✓ Blue rounded border surrounds the text area
3. ✓ Left/right circle handles appear on edges (blue with white outline)
4. ✓ Bottom-right resize box handle appears
5. ✓ Dragging move handles repositions text horizontally
6. ✓ Dragging resize handle changes text area width
7. ✓ Text wraps to stay within image bounds
8. ✓ Clicking outside, Escape, or selecting another tool commits/discards text appropriately
9. ✓ Auto-center text initially, wraps to paragraph when text grows
