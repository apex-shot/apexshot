# Arrow Tool Variants Design

## Overview

Enhance the Arrow tool with a sub-tool dropdown offering 4 arrow style variants, following the same sub-tool pattern used by Obfuscate (method selector), Pen/Highlighter (weight selector), and Number (style selector).

## Arrow Variants

| Variant | Arrowhead | Description |
|---------|-----------|-------------|
| **Standard** | End only | Current behavior — smooth line with filled triangle head, spread angle ~0.55 rad |
| **Fancy** | End only | Same structure as Standard but with sharper, more angular arrowhead edges (narrower spread ~0.3 rad) |
| **Curved** | End only | Quadratic Bezier curve. Initial drag creates straight line with 3 control handles (start, mid, end). User drags midpoint to curve. |
| **Double** | Both ends | Same as Curved but with arrowheads on both ends of the curve |

## Data Model

### New enum: `ArrowStyle`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowStyle {
    Standard,
    Fancy,
    Curved,
    Double,
}
```

With methods:
- `display_name() -> &'static str`
- `icon_name() -> &'static str`
- `ALL: &'static [ArrowStyle]`

### Updated `AnnotationAction::Arrow`

```rust
Arrow {
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,        // NEW
    control_points: Option<Vec<Point>>,  // NEW — None for Standard/Fancy, Some([start, mid, end]) for Curved/Double
}
```

### New state field

`EditorState` gains:
- `arrow_style: ArrowStyle` (default: `Standard`)

## Interaction Model

### Standard & Fancy
- Drag start → end (same as current arrow behavior)
- Finalize on mouse release
- No control points

### Curved & Double
1. Drag start → end (creates straight line preview)
2. On mouse release, 3 control handles appear: start, midpoint, end
3. User drags midpoint handle to define the curve (quadratic bezier control point)
4. Start/end handles allow repositioning the arrow endpoints
5. Clicking outside handles or switching tools finalizes the arrow

### Control Handle Rendering
- Small filled circles at start, midpoint, and end
- Dashed line connecting start → midpoint → end (control polygon)
- Handles only visible when the Curved/Double arrow is the active/selected action

## Rendering

### `draw_arrow()` updates

| Style | Rendering |
|-------|-----------|
| Standard | Current implementation — `line_to` + filled triangle head (spread 0.55) |
| Fancy | `line_to` + filled triangle head with narrower spread (0.3 rad ~17°) for sharper look |
| Curved | `move_to(start)` → `curve_to(mid, mid, end)` + filled triangle head at end |
| Double | `move_to(start)` → `curve_to(mid, mid, end)` + filled triangle heads at both ends |

### Control Handle Drawing (new function)
- Draws during draft mode when Curved/Double arrow is being edited
- Visual feedback only — handles are not part of the final annotation

## Toolbar

### Sub-tool dropdown

When Arrow is the active tool, a sub-tool button appears between the primary tools group and the size slider (same position as obfuscate method button). Clicking shows a popover with 4 labeled options.

The trigger button icon updates to reflect the selected variant.

### Show/Hide logic

```
arrow_style_group.set_visible(matches!(tool, Tool::Arrow))
```

## Files to Modify

| File | Changes |
|------|---------|
| `src/capture/editor/types.rs` | Add `ArrowStyle` enum with display_name/icon_name/ALL. Add `style` and `control_points` fields to `AnnotationAction::Arrow`. Add `tool_uses_arrow_style()` helper. |
| `src/capture/editor/state.rs` | Add `arrow_style` field + getter/setter. Use in `draft_action()` and `finalize_drag_action()`. Add control point dragging logic for Curved/Double. |
| `src/capture/editor/render.rs` | Update `draw_arrow()` to handle 4 styles. Add `draw_arrow_control_handles()` function. |
| `src/capture/editor/window/toolbar.rs` | Add `build_arrow_style_controls()` builder. Populate list in `build_toolbar_mode_controls()`. Add to `build_toolbar_tool_updater()` visibility logic. |
| `src/capture/editor/window/events.rs` | Add arrow style button/list to `EventParts`/`EventContext`. Wire click handlers. Handle control point mouse events. |
| `src/capture/editor/window/mod.rs` | Destructure and pass arrow style widgets. |
| `src/capture/editor.rs` | Update tests for new ArrowStyle handling. |

## Implementation Order

1. **ArrowStyle enum + data model** (types.rs)
2. **State management** (state.rs — field, getter/setter, draft/finalize)
3. **Standard & Fancy rendering** (render.rs — just adjust spread angle for Fancy)
4. **Toolbar dropdown** (toolbar.rs, mod.rs — wire sub-tool button)
5. **Events wiring** (events.rs — button clicks)
6. **Curved/Double rendering** (render.rs — bezier + control handles)
7. **Control point interaction** (state.rs + events.rs — drag midpoint)
8. **Tests** (editor.rs)
