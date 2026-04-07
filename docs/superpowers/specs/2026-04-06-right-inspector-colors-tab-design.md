# Right Inspector Colors Tab Design

## Goal
Add a shared Colors tab to the annotate editor’s right inspector without removing or replacing the existing toolbar color picker. The right sidebar should become tool-aware: Background shows `Background | Colors`, while color-capable annotation tools show `Colors` as their primary inspector content. The Colors tab should also grow into a sidebar-based color management surface that shares the same persistent custom color data used by the toolbar picker.

## Scope
Phase 1 introduces the inspector tab shell and a reusable Colors panel prototype. The Colors panel should be visible for color-capable tools and remain parallel to the toolbar picker. Select and Crop continue to use placeholder inspector content for now.

Phase 2 expands the Colors tab so it can manage shared custom colors using the same storage/model already used by the toolbar color picker. The toolbar picker stays visually intact for now.

## Tool behavior
### Background
- Show tabs: `Background`, `Colors`
- Default active tab: `Background`

### Color-capable tools
- Tools: `Pen`, `Arrow`, `Line`, `Box`, `Circle`, `Text`, `Number`, `Highlighter`, `Obfuscate`, `Focus`
- Show tab/header state for `Colors`
- Show the shared Colors panel as the inspector content

### Non-color tools
- Tools: `Select`, `Crop`
- Keep placeholder inspector content for this phase

## Architecture
The right inspector becomes a small reusable shell with:
- a tab/header row
- a content stack
- reusable panels for:
  - `background`
  - `colors`
  - `placeholder`

The existing background controls remain in their own reusable builder module and become one tab panel. A new colors panel module is added as a sibling reusable panel.

The Colors panel should share the same persistence and custom-color slot model as the toolbar picker rather than introducing a second saved-color system.

## Colors panel
The expanded Colors panel should include:
- title: `Colors`
- shared palette swatch grid based on the existing toolbar color palette concept
- current color preview
- read-only hex display for the active color
- `My colors` section backed by the same persistent custom color slots used by the toolbar picker
- `Add current color` action
- `Pick from screen` / eyedropper trigger

## Interaction model
The Colors tab should distinguish among three concepts:
1. **Current color**
   - visible preview chip
   - read-only hex text
2. **Palette**
   - built-in swatches for quick selection
3. **My colors**
   - persistent custom saved colors
   - clicking a saved color applies it
   - removing a saved color clears only that slot

### Eyedropper behavior
- Eyedropper selection updates the current active color first
- The chosen color is not automatically saved into `My colors`
- The user explicitly saves it via `Add current color`

### Custom color behavior
- `Add current color` stores the active color into the first available custom slot using the shared persistent slot data
- Removing a custom color clears only that saved slot, not the current active editor color
- Toolbar picker and sidebar Colors tab should reflect the same underlying custom color list

## State model
Inspector state should separately track:
- selected tool
- which inspector tab is active when multiple tabs are available

For Background, default to the Background tab when entering that tool. For color-capable tools, default/show the Colors panel. The implementation should avoid removing current toolbar state flows.

The Colors panel should reuse existing editor hooks for:
- applying a chosen color to the current tool/editor state
- syncing current active color from editor state
- launching the existing eyedropper flow where possible

## File impact
- `src/capture/editor/window/mod.rs`
  - build inspector shell with tabs and stack
  - mount background/colors/placeholder panels
  - route active tool into inspector state
  - pass shared color hooks into the Colors panel
- `src/capture/editor/window/toolbar.rs`
  - extend tool updater so it can classify color-capable tools and drive inspector shell visibility/state
- `src/capture/editor/window/background_panel.rs`
  - keep reusable as background tab content
- `src/capture/editor/window/colors_panel.rs`
  - reusable shared colors inspector panel
  - current color preview
  - palette section
  - my colors section
  - add/remove actions
  - eyedropper trigger
- `src/capture/editor/window/color_picker.rs`
  - share persistence and custom-color helpers rather than duplicating logic
  - expose or reuse eyedropper/custom-color hooks as needed
- `src/capture/editor/ui_support.rs`
  - inspector tab styling and colors-panel styling
  - custom slot styling in the sidebar context

## Testing
Add regression-style tests for:
- inspector tabs/header shell exists
- colors inspector content exists
- tool updater recognizes color-capable tools
- background mode exposes both Background and Colors pathways
- colors panel contains sidebar markers for `My colors`, add/remove actions, and eyedropper support

## Constraints
- Do not remove or redesign the toolbar color picker yet
- Keep the change prototype-safe and incremental
- Share the existing persistent custom color model/data instead of creating a new one
- Keep sidebar hex display read-only for this pass
- Favor reusable inspector panel builders over growing `mod.rs`

## Recommended implementation strategy
Use a semi-wired prototype with shared color management:
1. keep the tabbed inspector shell
2. expand the reusable Colors panel module
3. reuse the toolbar picker’s custom-color persistence model
4. add sidebar `My colors`, add/remove actions, and eyedropper entry point
5. keep toolbar picker visually unchanged
6. allow both surfaces to operate on the same saved custom colors
