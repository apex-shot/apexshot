# `window.rs` Refactor Plan

## Goal

Split [./backend/src/capture/editor/window.rs](./backend/src/capture/editor/window.rs) into smaller, focused files without changing runtime behavior, UI behavior, signal behavior, or editor state flow.

## Non-negotiable constraints

1. **No behavior changes**
   - The refactor must preserve current functionality exactly.
   - No user-visible UI behavior should change during the split.
   - No keyboard shortcuts, cursor behavior, drag behavior, crop behavior, picker behavior, or save behavior should change.

2. **Compile-safe, incremental migration**
   - The implementation should be done in small steps.
   - Every step should leave the project in a compiling state.
   - Avoid big-bang rewrites.

3. **No logic rewrites unless required for extraction**
   - Prefer moving existing code into new modules with minimal edits.
   - If signatures need to change, keep the changes mechanical and localized.

4. **State/rendering boundaries remain intact**
   - `state.rs` remains responsible for editor state and mutations.
   - `render.rs` remains responsible for drawing and image rendering helpers.
   - `types.rs` remains responsible for enums, structs, and pure helper behavior.
   - `window/*` should own GTK widget construction and event wiring.

---

## Why this split is needed

`window.rs` currently owns too many unrelated responsibilities:

- app/editor bootstrap
- top-level window composition
- toolbar construction
- background sidebar construction
- color picker construction and synchronization
- canvas layout and draw helpers
- eyedropper loupe rendering
- cursor logic
- signal/controller/event wiring

This makes it harder to:

- navigate the file quickly
- add features safely
- change one UI subsystem without risking another
- test or reason about ownership boundaries

---

## Recommended target structure

```text
backend/src/capture/editor/
├── editor.rs
├── color.rs
├── io_ops.rs
├── render.rs
├── selection.rs
├── state.rs
├── types.rs
├── ui_support.rs
└── window/
    ├── mod.rs
    ├── bootstrap.rs
    ├── cursor.rs
    ├── canvas.rs
    ├── color_picker.rs
    ├── background_panel.rs
    ├── toolbar.rs
    ├── footer.rs
    ├── events.rs
    └── widgets.rs
```

---

## Intended responsibilities per file

### `window/mod.rs`
Thin public module entry.

**Owns:**
- module declarations
- public re-exports if needed
- `open_image_editor(...)`
- optionally `setup_editor_window(...)` initially, until bootstrap extraction is complete

**Target outcome:**
- small and easy to read
- acts as the entry point, not the implementation dump

### `window/bootstrap.rs`
Top-level assembly and orchestration.

**Owns:**
- image loading for editor startup
- `EditorState` and `ViewTransform` initialization
- root GTK layout assembly
- invoking the builders from toolbar/color/background/canvas/footer modules
- invoking event wiring
- presenting the window

**Should not own:**
- large signal handlers
- picker internals
- background sidebar internals
- low-level cursor logic

### `window/cursor.rs`
Pointer and cursor selection behavior.

**Move here:**
- `set_window_cursor_name`
- `select_hover_cursor_name`
- `crop_hover_cursor_name`
- `cursor_name_for_view_point`
- optionally `transparent_drag_icon_texture` if it stays cursor/drag-adjacent

**Reason:**
This logic is self-contained and already behaves like a separate subsystem.

### `window/canvas.rs`
Canvas geometry, overlay sizing, loupe drawing, and draw-surface helpers.

**Move here:**
- `crop_canvas_overflow`
- `sample_rendered_color_at_point`
- `sample_editor_color_at_point`
- `eyedropper_loupe_position`
- `draw_eyedropper_loupe`
- canvas widget setup
- overlay/eyedropper ring setup
- canvas size update logic
- drawing area draw function

**Reason:**
These are tied to the editor surface, not to the toolbar or picker.

### `window/color_picker.rs`
All color picker UI, picker synchronization, custom slots, and picker-related interactions.

**Move here:**
- `apply_size_control_ui_state`
- `set_active_color_picker_state`
- `clear_active_color_picker_palette_state`
- `clear_color_picker_trigger_dot_state`
- `set_color_picker_trigger_dot_state`
- swatch column creation
- custom slot creation
- picker panel creation
- picker state synchronization closures
- eyedropper activation from the picker flow
- add/remove custom color interactions
- custom slot drag and drop setup, if kept picker-local

**Reason:**
The picker is already a feature-sized subsystem.

### `window/background_panel.rs`
Background sidebar UI and background preview loading.

**Move here:**
- `BACKGROUND_*` constants
- `background_gradient_asset_path`
- `load_background_preview_image`
- `load_background_gradient_preview_image`
- `parse_wallpaper_setting`
- `detect_system_wallpaper_path`
- `draw_preview_tile_surface`
- wallpaper/grid rebuilding logic
- background sidebar widget construction
- lazy gradient preview loading

**Reason:**
The background panel is independent enough to be developed separately from the rest of the window.

### `window/toolbar.rs`
Toolbar composition and mode switching.

**Move here:**
- toolbar widget construction
- grouped tool button creation
- mode stack creation
- tool-mode visibility logic
- `update_toolbar_for_tool`

**Reason:**
This centralizes all top-bar UI behavior.

### `window/footer.rs`
Footer construction and footer-only widget setup.

**Move here:**
- footer layout
- pin button widget setup
- drag/copy/upload/save visual grouping

**Reason:**
Keeps bootstrap focused and readable.

### `window/events.rs`
All GTK signal/controller hookups.

**Move here:**
- tool button connections
- footer button click handlers
- custom slot interactions if not kept in `color_picker.rs`
- drag gesture for drawing
- click gesture
- motion controller
- key controller
- close-request handler

**Rule:**
This file should wire behavior onto already-built widgets, not build the UI.

### `window/widgets.rs`
Small reusable widget factory helpers used only inside the window module.

**Move here if shared:**
- `build_background_gradient_preview_button`
- `build_background_wallpaper_preview_button`
- `build_background_add_wallpaper_button`
- `build_background_blurred_preview_button`
- `build_background_plain_color_button`
- `build_background_plain_color_cell`

**Note:**
If a widget helper is used only by `background_panel.rs`, it can stay there instead.

---

## Shared return structs to reduce argument explosion

The refactor should avoid passing dozens of GTK widgets between modules one by one.

Introduce small “parts” structs returned by builders.

### Example: `ToolbarParts`

```rust
pub struct ToolbarParts {
    pub root: CenterBox,
    pub tool_buttons: Vec<Button>,
    pub apply_crop_btn: Button,
    pub size_group: GtkBox,
    pub size_down_btn: Button,
    pub size_up_btn: Button,
    pub update_for_tool: Rc<dyn Fn(Tool)>,
}
```

### Example: `CanvasParts`

```rust
pub struct CanvasParts {
    pub root: GtkBox,
    pub overlay: Overlay,
    pub drawing_area: DrawingArea,
    pub scroller: ScrolledWindow,
    pub eyedropper_ring: DrawingArea,
    pub update_content_size: Rc<dyn Fn()>,
}
```

### Example: `ColorPickerParts`

```rust
pub struct ColorPickerParts {
    pub trigger_host: Overlay,
    pub popover: Popover,
    pub color_buttons: Vec<Button>,
    pub sync_for_active_tool: Rc<dyn Fn()>,
    pub apply_picker_color: Rc<dyn Fn(super::types::DrawColor)>,
}
```

### Example: `BackgroundPanelParts`

```rust
pub struct BackgroundPanelParts {
    pub sidebar: GtkBox,
    pub start_gradient_preview_loading: Rc<dyn Fn()>,
}
```

These structs should stay small and only expose what other modules actually need.

---

## Recommended implementation order

This order is designed to keep the refactor safe and compile-friendly.

### Phase 1: Create the new module layout

1. Convert `window.rs` into `window/mod.rs`.
2. Update [./backend/src/capture/editor.rs](./backend/src/capture/editor.rs) so `mod window;` still resolves correctly.
3. Keep behavior unchanged.
4. Do not extract large logic yet.

**Expected outcome:**
- same behavior
- same public API
- only file layout changes

### Phase 2: Extract low-risk helpers first

1. Extract `cursor.rs`.
2. Extract small background preview/widget helpers.
3. Extract any tiny reusable widget factories into `widgets.rs` if helpful.

**Why first:**
- low dependency surface
- low risk of behavior drift
- immediate size reduction in `mod.rs`

### Phase 3: Extract toolbar and footer builders

1. Move toolbar widget construction to `toolbar.rs`.
2. Move footer widget construction to `footer.rs`.
3. Keep click/gesture wiring where it already is for now.

**Expected outcome:**
- top-level window layout becomes easier to read
- UI composition becomes separated from signal logic

### Phase 4: Extract canvas subsystem

1. Move canvas widget creation to `canvas.rs`.
2. Move draw helpers and eyedropper loupe drawing there.
3. Move canvas sizing logic there.
4. Keep exported helpers narrow and explicit.

**Expected outcome:**
- editor surface behavior is isolated
- draw-related code stops competing with toolbar/picker code

### Phase 5: Extract color picker subsystem

1. Move swatch creation.
2. Move custom slot creation.
3. Move picker panel creation.
4. Move picker synchronization/update closures.
5. Keep behavior identical.

**Important:**
- this phase should be done after toolbar/canvas extraction because the picker captures many shared widgets and closures
- use `ColorPickerParts` or a similar struct to prevent a huge parameter list

### Phase 6: Extract background sidebar subsystem

1. Move full background sidebar construction to `background_panel.rs`.
2. Keep lazy preview loading behavior unchanged.
3. Preserve current widget visibility behavior tied to the background tool.

### Phase 7: Extract event wiring last

1. Move signal/controller hookup code into `events.rs`.
2. Keep UI building code out of `events.rs`.
3. Keep `events.rs` focused on connecting prebuilt parts.

**Why last:**
- event wiring touches almost every subsystem
- extracting it too early increases churn and compile errors

### Phase 8: Final cleanup

1. Rename any temporary helpers/parts structs for clarity.
2. Reduce re-export noise.
3. Keep `window/mod.rs` thin.
4. Verify that all moved code still uses existing conventions.

---

## Explicit extraction map

### Move first

#### To `window/cursor.rs`
- `set_window_cursor_name`
- `select_hover_cursor_name`
- `crop_hover_cursor_name`
- `cursor_name_for_view_point`
- maybe `transparent_drag_icon_texture`

#### To `window/background_panel.rs`
- `background_gradient_asset_path`
- `load_background_preview_image`
- `load_background_gradient_preview_image`
- `parse_wallpaper_setting`
- `detect_system_wallpaper_path`
- `draw_preview_tile_surface`
- `rebuild_wallpaper_preview_grid`

#### To `window/widgets.rs` if shared
- `build_background_gradient_preview_button`
- `build_background_wallpaper_preview_button`
- `build_background_add_wallpaper_button`
- `build_background_blurred_preview_button`
- `build_background_plain_color_button`
- `build_background_plain_color_cell`

### Move second

#### To `window/canvas.rs`
- `crop_canvas_overflow`
- `sample_rendered_color_at_point`
- `sample_editor_color_at_point`
- `eyedropper_loupe_position`
- `draw_eyedropper_loupe`
- drawing area setup section
- canvas overlay setup section
- canvas content size update logic
- drawing-area draw handler

#### To `window/color_picker.rs`
- `apply_size_control_ui_state`
- `set_active_color_picker_state`
- `clear_active_color_picker_palette_state`
- `clear_color_picker_trigger_dot_state`
- `set_color_picker_trigger_dot_state`
- all color picker UI construction
- picker state/update closures
- custom slot setup and refresh logic

### Move last

#### To `window/events.rs`
- tool button connections
- custom slot drag/drop connections
- color button click connections
- size button connections
- apply crop connection
- undo/redo/delete/save/close connections
- drag gesture hookup
- click gesture hookup
- motion controller hookup
- key controller hookup
- close-request hookup

---

## Safety rules during implementation

1. **Move, compile, verify, then continue**
   - After each extraction, compile before starting the next one.

2. **Do not change behavior while moving code**
   - No renaming of runtime CSS classes unless necessary.
   - No shortcut remapping.
   - No layout changes.
   - No signal ordering changes unless unavoidable.

3. **Keep function signatures stable where possible**
   - Prefer importing moved helpers rather than redesigning APIs immediately.

4. **Only introduce structs when they reduce complexity**
   - Use builder result structs to reduce argument count.
   - Do not introduce unnecessary abstraction.

5. **Preserve tests and existing public API**
   - `open_image_editor` should remain accessible through the same module path.
   - Existing tests in [./backend/src/capture/editor.rs](./backend/src/capture/editor.rs) should continue to work with minimal or no changes.

---

## Definition of success

The refactor is successful when all of the following are true:

- `window.rs` no longer exists as a 4k+ monolith
- the window implementation is split into focused modules
- behavior is unchanged
- the code compiles at every major phase
- future features can be added without growing one mega-file again

---

## Recommended first implementation step

The first implementation step should be:

1. rename `window.rs` to `window/mod.rs`
2. create `window/cursor.rs`
3. move cursor-related functions there
4. compile and verify no behavior changes

This gives a safe first win with minimal blast radius.
