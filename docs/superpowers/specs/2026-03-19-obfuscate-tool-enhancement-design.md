# Obfuscate Tool Enhancement Design

## Overview

Enhance the obfuscate tool with a method selector dropdown and per-method intensity controls. The toolbar icon changes to pixelate, and a dropdown selector appears before the slider when the obfuscate tool is active.

## UI/UX Specification

### Visual Design

#### Icons
- **Toolbar Icon**: Pixelate (4-square grid icon)
- **Dropdown**: Shows icon + name for each method
- **Method Icons**:
  1. **Pixelate**: 4-square grid pattern
  2. **Blur (Secure)**: Solid water drop
  3. **Blur (Smooth)**: Outlined water drop
  4. **Blackout**: Crescent moon

#### Layout
- Obfuscate tool button stays in current toolbar position
- Method selector button (icon only) appears before slider when Obfuscate tool is active
- Slider visible for all methods except Blackout
- Dropdown style matches text size picker (GTK Popover)

#### Colors
- Use existing toolbar styling (inherit from codebase)
- Selected dropdown item has subtle background highlight

### Components

1. **Obfuscate Tool Button**
   - Icon: Pixelate grid icon
   - Behavior: Unchanged (selects obfuscate tool)

2. **Method Selector Button**
   - Position: Before size slider, only when Obfuscate tool active
   - Style: Icon only (matches toolbar icon)
   - Click: Opens dropdown popover

3. **Dropdown Popover**
   - 4 options with icon + name
   - Selected item highlighted
   - Click outside closes dropdown

4. **Intensity Slider**
   - Visible: Pixelate, Blur (Secure), Blur (Smooth)
   - Hidden: Blackout (no intensity needed)
   - Each method remembers its last intensity value

### Behaviors

#### Tool Selection
- Selecting Obfuscate tool → show method selector + slider
- Selecting any other tool → hide method selector + slider

#### Method Selection
- Click dropdown item → update current method
- Slider resets to last used value for selected method
- Icon updates to match selected method

#### Intensity Control
- **Pixelate**: 1-24 pixel block size
- **Blur (Secure)**: 1-24 blur radius
- **Blur (Smooth)**: 1-24 blur radius
- **Blackout**: No slider (full opacity black overlay)

#### Keyboard Shortcuts
- Current shortcuts (C, B) still work
- Shortcut applies current selected obfuscate method

## Technical Implementation

### State Changes

```rust
// New state fields in EditorState
pub obfuscate_method: ObfuscateMethod,
pub obfuscate_pixelate_amount: f64,  // 1-24
pub obfuscate_blur_secure_amount: f64,  // 1-24
pub obfuscate_blur_smooth_amount: f64,  // 1-24
```

### New Types

```rust
pub enum ObfuscateMethod {
    Pixelate,
    BlurSecure,
    BlurSmooth,
    Blackout,
}
```

### Files to Modify

1. **types.rs**
   - Update `ObfuscateMethod` enum
   - Update `AnnotationAction::Obfuscate` to use new method enum

2. **state.rs**
   - Add method selection state
   - Add per-method intensity values
   - Update `set_tool` to show/hide method selector

3. **toolbar.rs**
   - Add method selector button (like text_size_button)
   - Add dropdown popover with 4 options
   - Wire up selection handler

4. **render.rs**
   - Update to use current method for annotation rendering
   - Implement Blackout rendering (solid overlay)

5. **events.rs**
   - Update obfuscate button handler
   - Add method selector click handler

6. **window/mod.rs**
   - Handle method selector visibility based on active tool

### Dropdown Implementation Pattern

Follow existing text_size_button pattern:
```rust
let method_button = Button::new();
method_button.add_css_class("flat");
let method_popover = Popover::new();
method_popover.set_parent(&method_button);

for method in ObfuscateMethod::variants() {
    let btn = Button::builder()
        .label(&method.display_name())
        .build();
    method_list.append(&btn);
}
```

## Acceptance Criteria

1. ✓ Toolbar icon changes to pixelate grid icon
2. ✓ Method selector appears before slider when Obfuscate tool active
3. ✓ Dropdown shows 4 options with icon + name
4. ✓ Selecting method updates icon and slider behavior
5. ✓ Blackout hides slider (no intensity needed)
6. ✓ Each method remembers its intensity value
7. ✓ Dropdown closes on selection or click outside
