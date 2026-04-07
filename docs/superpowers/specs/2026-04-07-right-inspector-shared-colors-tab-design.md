# Right Inspector Shared Colors Tab Design

## Goal
Replace the Background panel's embedded `Plain color` section with a shared right-inspector `Colors` tab that works for both Background and all color-capable annotation tools. The right inspector should become the single sidebar surface for color management while the existing toolbar picker continues to use the same underlying state and persistence model.

## Scope
This change covers the annotate editor right inspector only.

Included:
- remove `Plain color` from the Background tab
- keep `Background | Colors` tabs for the Background tool
- show `Colors` as the primary inspector content for color-capable tools
- reuse the existing toolbar color picker logic, palette, custom colors, and eyedropper flow
- apply chosen colors to Background as `BackgroundStyle::PlainColor(...)`
- keep toolbar and inspector colors in sync through shared editor state

Not included:
- replacing or deleting the toolbar color picker UI
- adding new color models beyond what the current picker already supports
- redesigning non-color tool inspector content

## Tool Behavior

### Background
When the selected tool is `Background`, the inspector shows two tabs:
- `Background`
- `Colors`

The `Background` tab contains only background-layout and background-style controls:
- `None`
- `Alignment`
- gradients
- wallpaper
- blurred
- `Padding`
- `Ratio`
- insert / auto-balance / shadow / corners

The `Plain color` section is removed from this tab.

The `Colors` tab becomes the only place where a solid background color is chosen. Selecting a color there sets the background style to `BackgroundStyle::PlainColor(selected_color)`.

Default tab behavior:
- entering the Background tool opens the `Background` tab
- if the user switches to `Colors`, that tab remains active until tool routing changes it

### Color-Capable Tools
The following tools should show the shared `Colors` inspector content:
- `Pen`
- `Arrow`
- `Line`
- `Box`
- `Circle`
- `Text`
- `Number`
- `Highlighter`
- `Obfuscate`
- `Focus`

For these tools, the inspector shows `Colors` as the primary panel. Choosing a color updates the same editor color state that the toolbar picker already controls.

### Non-Color Tools
Non-color tools keep their current placeholder or tool-specific inspector behavior. This change does not expand the Colors tab to tools that do not consume color state.

## Architecture
The right inspector should use one reusable Colors panel module instead of duplicating color UIs inside each tool panel.

Core structure:
- `background_panel.rs` owns only Background-specific controls
- `colors_panel.rs` owns the shared sidebar color-management surface
- inspector shell / routing decides which tab or panel to show for the selected tool
- toolbar color picker and sidebar Colors panel both call into the same editor-state update hooks

The Colors panel should be wired against existing shared pieces where possible:
- current active color state
- saved custom color slots
- palette swatches
- eyedropper flow
- color application callbacks

Background should use the same Colors panel with a Background-specific apply callback. Annotation tools should use the same panel with the existing draw-color apply callback.

## Interaction Model

### Shared Colors Surface
The Colors panel should mirror the current picker's capabilities rather than introducing a second color model. At minimum it should support:
- showing the active color
- choosing from the existing palette
- showing saved custom colors
- adding the current color to custom colors
- removing a saved custom color
- launching the existing eyedropper path

The sidebar does not need to visually match the toolbar popover exactly, but it must operate on the same underlying data and produce the same results.

### Background Color Selection
When Background is active and the user chooses a color in the Colors tab:
- the selected color becomes the active sidebar color
- background style switches to `BackgroundStyle::PlainColor(selected_color)`
- the canvas redraws immediately

If the background is currently a gradient, wallpaper, blur, or `None`, choosing a solid color in `Colors` replaces that style with `PlainColor`.

### Annotation Color Selection
When a color-capable annotation tool is active and the user chooses a color in `Colors`, the chosen color updates the same tool/editor color state used by the toolbar picker. Existing tool-specific behavior for stroke/fill semantics remains unchanged.

### Sync Rules
Toolbar picker and inspector Colors tab must stay synchronized:
- changing color in the toolbar updates the inspector
- changing color in the inspector updates the toolbar
- adding/removing custom colors from either surface updates the other

## State Model
Inspector state should explicitly track:
- selected tool
- active inspector tab for tools with multiple tabs

Editor color state remains the source of truth for:
- current active annotation color
- saved custom colors
- background plain color when Background is using `PlainColor`

Background does not need a separate independent color-management model. It only needs a way to map shared color selection actions to `BackgroundStyle::PlainColor(...)`.

## File Impact
Expected implementation areas:
- `src/capture/editor/window/background_panel.rs`
  Remove embedded `Plain color` UI and keep only Background-specific controls.
- `src/capture/editor/window/colors_panel.rs`
  Add or expand the reusable shared Colors inspector surface.
- `src/capture/editor/window/mod.rs`
  Extend inspector shell state and panel mounting for shared Colors behavior.
- `src/capture/editor/window/toolbar.rs`
  Route Background to `Background | Colors` and route color-capable tools to `Colors`.
- `src/capture/editor/window/color_picker.rs`
  Expose reusable hooks/helpers needed by the sidebar Colors panel.
- `src/capture/editor/ui_support.rs`
  Add styling for the shared Colors inspector surface.

## Testing
Add regression coverage for:
- Background inspector no longer rendering `Plain color`
- Background routing showing `Background | Colors`
- color-capable tools routing to `Colors`
- shared color panel using the existing persistence/state model
- Background color selection switching style to `PlainColor`
- toolbar and inspector color synchronization where testable at source level

Targeted verification should include:
- `cargo test` filters for inspector/colors/background modules
- `cargo build`

## Constraints
- Do not break the existing toolbar picker behavior
- Do not create separate custom-color stores for toolbar and sidebar
- Do not leave duplicated plain-color controls in Background after the Colors tab is introduced
- Keep Background-specific layout controls out of the shared Colors panel

## Recommended Implementation Strategy
1. Add failing regression tests for inspector routing and Background plain-color removal.
2. Extract or expand shared color-management hooks from the current picker module.
3. Build the reusable `Colors` inspector panel around those hooks.
4. Remove `Plain color` from the Background panel.
5. Route Background and color-capable tools into the shared Colors panel behavior.
6. Add styling and run final verification.
