# Right Inspector Arrow Text Number Migration Design

## Goal
Move the `Arrow`, `Text`, and `Number` tool-specific sub-controls out of the annotate editor main toolbar and into the persistent right inspector so the toolbar becomes less crowded without changing the underlying editing behavior. Each migrated tool should follow the same two-tab inspector pattern already established for `Background`: a tool-specific primary tab plus the shared `Colors` tab.

## Scope
This change covers the annotate editor right inspector and main toolbar only.

Included:
- route `Arrow`, `Text`, and `Number` to dedicated primary inspector tabs
- keep `Colors` as the second tab for each of those tools
- move the existing tool-specific sub-controls for those tools from the main toolbar into the right inspector
- keep the moved controls wired to the same editor state and callbacks they use today
- remove the migrated sub-control widgets from the main toolbar

Not included:
- redesigning the shared `Colors` tab behavior
- changing on-canvas text editing behavior
- introducing new tool capabilities or new state models
- redesigning non-migrated tool panels in this change

## Tool Behavior
### Arrow
The inspector shows `Arrow | Colors` when the Arrow tool is active. The default active tab is `Arrow`. The `Arrow` tab contains the arrow-specific controls that currently live in the toolbar, beginning with arrow style or variant selection and any other Arrow-only controls already supported by the current editor wiring.

### Text
The inspector shows `Text | Colors` when the Text tool is active. The default active tab is `Text`. The `Text` tab contains the text-specific controls that currently live in the toolbar, such as text sizing and any existing text-only appearance settings. Canvas-based text placement and inline editing continue to work as they do now.

### Number
The inspector shows `Number | Colors` when the Number tool is active. The default active tab is `Number`. The `Number` tab contains the number-tool controls that currently live in the toolbar, including existing badge or marker style controls already supported by the editor.

### Shared Colors Tab
For all three tools, `Colors` remains the shared inspector surface owned by the existing colors-panel implementation. The tab behavior should match the current `Background | Colors` pattern so the right inspector feels consistent across color-capable tools.

### Tab Selection Rules
- selecting `Arrow`, `Text`, or `Number` opens that tool's primary tab by default
- if the user manually switches to `Colors`, the inspector may keep that tab active until normal routing rules intentionally reset it
- switching between these tools should preserve the inspector shell and swap only the primary tab label and content

## Architecture
The migration should reuse the existing right-inspector shell and editor-state wiring rather than introducing inspector-only state.

Core structure:
- `src/capture/editor/window/mod.rs` owns inspector routing and decides which primary tool panel is mounted beside `Colors`
- `src/capture/editor/window/colors_panel.rs` continues to own the shared color-management surface
- `src/capture/editor/window/toolbar.rs` stops rendering the migrated `Arrow`, `Text`, and `Number` sub-tool controls
- tool-specific inspector panel builders own the moved UI for `Arrow`, `Text`, and `Number`

The inspector panels should call the same update hooks and mutate the same editor state that the toolbar controls use today. This keeps behavior stable while changing only where the controls render.

## Toolbar Responsibilities After Migration
The main toolbar should focus on high-level controls such as tool selection and any remaining global actions or compact status affordances. It should no longer be the place where `Arrow`, `Text`, and `Number` expose their detailed sub-tool settings.

## File Impact
- `src/capture/editor/window/mod.rs`
- `src/capture/editor/window/toolbar.rs`
- `src/capture/editor/window/colors_panel.rs`
- new or updated tool-specific inspector panel module(s) for `Arrow`, `Text`, and `Number`
- `src/capture/editor/ui_support.rs` if sidebar-specific styling is needed for the migrated controls

## Testing
Verification should cover both routing and regression behavior:
- the inspector shows `Arrow | Colors`, `Text | Colors`, and `Number | Colors` for the migrated tools
- selecting each tool defaults to its primary tab
- the moved controls still update the same editor state as before
- the main toolbar no longer renders the migrated sub-tool controls
- the shared `Colors` tab remains synchronized with the active tool color state
- `Background` inspector behavior remains unchanged
