# Text Side Panel Visual Consistency Design

## Goal

Make the Text tool's right-inspector option rows visually match the Arrow sidepanel so the inspector feels consistent across tools, without changing Text control behavior, section order, or panel routing.

## Scope

This design covers only the visual treatment of Text inspector controls in the primary Text tab.

Included:
- restyling Text `Size` and `Font` option rows to match the Arrow inspector option language
- adding a visible active-state treatment for the currently selected text size and font
- keeping the existing Text inspector width, sections, and interaction flow

Excluded:
- changing Text inspector section order or adding new sections
- moving controls between the toolbar and the inspector
- changing how text size or font selection updates editor state
- redesigning Arrow, Crop, Number, or the shared `Colors` tab

## Current State

The Text tool already routes to its own primary inspector surface with `Size` and `Font` sections. Unlike Arrow, those sections currently render as generic popover-list buttons with plain labels and no tool-specific active-row styling.

Arrow already establishes the target visual language for inspector option rows:
- full-width row buttons
- subtle hover surface
- selected surface state
- a trailing orange check indicator for the active item

## Proposed UX

When `Tool::Text` is active, the `Size` and `Font` sections should keep their current content and ordering, but each option row should look like an Arrow inspector row.

That means:
- each row remains a button
- the row fills the available inspector width
- the option label is left-aligned
- the active option shows the same selected-surface treatment used by Arrow
- the active option shows a trailing orange check mark

No other Text inspector structure should change.

## Architecture

The change should remain local to the Text inspector row construction and CSS.

Core structure:
- `src/capture/editor/window/mod.rs` continues to build the Text `Size` and `Font` lists
- those lists should switch from plain labeled buttons to row containers that mirror Arrow's inspector row composition
- active Text selections should be reflected through Text-specific CSS classes rather than borrowing Arrow-specific class names
- `src/capture/editor/ui_support.rs` should define Text inspector option styles that match Arrow's visual treatment

## Behavior Rules

- selecting a Text size must continue to use the existing text-size update path
- selecting a font family must continue to use the existing selected-text font update path
- only the active Text size row should appear selected within `Size`
- only the active font family row should appear selected within `Font`
- switching between tools must not leak Text active-state styling into other inspectors

## Error Handling

- if Text state falls back to a size or font not currently in the rendered list, no incorrect row should appear selected
- rebuilding the inspector lists must preserve correct active-row visuals after state changes and tool switches
- visual parity work must not break button click handling or focus behavior

## Testing

Implementation should verify:
- the Text inspector still renders `Size` and `Font`
- Text option rows visually match Arrow's inspector option treatment
- the active Text size row shows the selected state and orange check
- the active font row shows the selected state and orange check
- changing Text size updates both editor state and the active visual selection
- changing font family updates both editor state and the active visual selection
- Arrow inspector visuals remain unchanged

## Recommended Rollout

Implement in this order:
1. Update Text inspector option row construction to support label-plus-check layout
2. Add Text-specific inspector option CSS matching Arrow's visual language
3. Wire active-state syncing for Text size and font selections
4. Verify tool switching and repeated Text selection updates still render correctly
