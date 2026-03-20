# Number Tool Enhancement Design

## Overview

Enhance the numbering tool with multiple numbering styles (numeric, alphabetic, roman), customizable starting number, size options, and fix a bug where thin connecting lines appear between numbers.

## Data Model

### New Types

**NumberingStyle enum** (`src/capture/editor/numbering_style.rs`):
```rust
pub enum NumberingStyle {
    Numeric,      // 1, 2, 3, 4...
    Uppercase,    // A, B, C, D...
    Lowercase,    // a, b, c, d...
    Roman,        // i, ii, iii, iv...
}
```

**NumberSize enum**:
```rust
pub enum NumberSize {
    Small,      // radius: 12, font: 11
    Medium,     // radius: 15, font: 14 (default)
    Large,      // radius: 20, font: 18
    ExtraLarge, // radius: 25, font: 22
}
```

### State Changes

**EditorState additions**:
```rust
pub numbering_style: NumberingStyle,
pub numbering_start: u32,
pub number_size: NumberSize,
```

**AnnotationAction::Number update**:
```rust
AnnotationAction::Number {
    position: Point,
    number: u32,
    color: DrawColor,
    style: NumberingStyle,   // NEW
    size: NumberSize,        // NEW
}
```

## UI Components

### Number Tool Dropdown

When Number tool is selected, a dropdown appears with "123" label:

```
+-----------------------------+
|  1, 2, 3, 4...              |  <- Radio-style selection
|  A, B, C, D...              |
|  i, ii, iii, iv...          |
|  a, b, c, d...              |
+-----------------------------+
|  Starting number: [___]     |  <- Entry input
+-----------------------------+
|  Size                    >  |  <- Submenu with arrow
+-----------------------------+
```

### Size Submenu

```
+-----------------+
|  Small          |
|  Medium         |
|  Large          |
|  Extra Large    |
+-----------------+
```

### New Icon

Custom SVG icon showing "1" inside a circle, replacing the current generic number icon.

### Toolbar Parts Addition

```rust
pub number_options_group: GtkBox,
pub number_options_button: Button,
pub number_options_popover: Popover,
pub number_style_list: GtkBox,
pub number_start_entry: Entry,
pub number_size_button: Button,
pub number_size_popover: Popover,
pub number_size_list: GtkBox,
```

## Number Conversion Logic

### Formatting

```rust
impl NumberingStyle {
    pub fn format(&self, number: u32) -> String {
        match self {
            Self::Numeric => number.to_string(),
            Self::Uppercase => Self::to_alpha(number, true),
            Self::Lowercase => Self::to_alpha(number, false),
            Self::Roman => Self::to_roman(number),
        }
    }
}
```

### Alpha Conversion (Excel-style)

- 1=A, 2=B, ..., 26=Z, 27=AA, 28=AB...

### Roman Numeral Conversion

- Standard conversion: 1=i, 2=ii, 3=iii, 4=iv, 5=v...

### Starting Number Handling

- When user changes `numbering_start`, `next_number` is set to that value
- Dropdown stays open after selecting style (allows editing starting number)
- Dropdown closes when clicking outside or selecting size
- `sync_next_number()` respects current style when finding max

### Size Rendering

```rust
impl NumberSize {
    pub fn radius(&self) -> f64 {
        match self {
            Self::Small => 12.0,
            Self::Medium => 15.0,
            Self::Large => 20.0,
            Self::ExtraLarge => 25.0,
        }
    }

    pub fn font_size(&self) -> f64 {
        match self {
            Self::Small => 11.0,
            Self::Medium => 14.0,
            Self::Large => 18.0,
            Self::ExtraLarge => 22.0,
        }
    }
}
```

## Bug Fix: Connecting Lines

### Root Cause

In `draw_number()`, `fill_preserve()` keeps the arc path in Cairo's context. When the next number is drawn, Cairo connects the old path to the new arc, creating a thin line between numbers.

### Fix

```rust
pub fn draw_number(context: &gtk4::cairo::Context, position: Point, number: u32,
                   color: DrawColor, style: NumberingStyle, size: NumberSize) {
    context.new_path();  // Clear any existing path

    context.arc(position.x, position.y, size.radius(), 0.0, TAU);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();  // Use fill() instead of fill_preserve()

    context.new_path();  // New path for border
    context.arc(position.x, position.y, size.radius(), 0.0, TAU);
    context.set_source_rgba(0.02, 0.03, 0.05, 0.42);
    context.set_line_width(1.5);
    let _ = context.stroke();

    context.new_path();  // Clear before text
    let label = style.format(number);
    // ... text rendering
}
```

### Constants to Remove

From `color.rs`:
- Remove `NUMBER_RADIUS: f64 = 15.0`
- Remove `NUMBER_FONT_SIZE: f64 = 14.0`

## File Changes Summary

### New File

- `src/capture/editor/numbering_style.rs` - `NumberingStyle` and `NumberSize` enums

### Modified Files

| File | Changes |
|------|---------|
| `types.rs` | Add `style` and `size` to `AnnotationAction::Number` |
| `state.rs` | Add style/size/start fields; update `add_number_marker()` and `sync_next_number()` |
| `render.rs` | Update `draw_number()` signature and fix path bug |
| `color.rs` | Remove `NUMBER_RADIUS` and `NUMBER_FONT_SIZE` constants |
| `toolbar.rs` | Add number options dropdown UI components |
| `events.rs` | Wire up dropdown selections and entry changes |
| `mod.rs` | Add `pub mod numbering_style;` |

### Icon

- Create SVG: `data/icons/scalable/actions/number-one-symbolic.svg`
- Update icon name reference in toolbar builder

## Implementation Order

1. Create `numbering_style.rs` with enums and conversion logic
2. Update `types.rs` with new `AnnotationAction::Number` fields
3. Update `state.rs` with new fields and methods
4. Update `render.rs` with bug fix and new signature
5. Remove constants from `color.rs`
6. Update `mod.rs` to include new module
7. Add UI components in `toolbar.rs`
8. Wire events in `events.rs`
9. Create new icon
10. Test and verify
