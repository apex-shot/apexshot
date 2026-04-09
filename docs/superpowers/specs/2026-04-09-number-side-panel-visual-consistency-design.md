# Number Side Panel Visual Consistency Design

## Goal

Make both sections of the Number tool's right inspector visually match the migrated sidepanel language used by Arrow, Text, and the other inspector panels, without changing Number behavior, routing, or sidepanel width.

## Scope

This design covers only the visual treatment of the Number inspector's existing `Style` and `Size` sections.

Included:
- restyling Number `Style` rows to match the migrated inspector surface language
- restyling Number `Size` rows to match the migrated inspector surface language
- keeping the current Number inspector width, sections, and interaction flow

Excluded:
- changing Number panel routing or tab structure
- adding or removing Number options
- changing Number state update behavior
- changing the shared sidepanel width path
- redesigning other tool panels

## Current State

The Number tool already routes to its own primary inspector surface with `Style` and `Size` sections.

Today, those two sections do not read as visually unified:
- `Style` uses a Number-specific row treatment
- `Size` does not match the same migrated row language used by Arrow and Text

As a result, the Number panel feels internally inconsistent and also less aligned with the other migrated tool panels.

## Proposed UX

When `Tool::Number` is active, both existing sections should remain in place:
- `Style`
- `Size`

The content and order stay the same, but both sections should use the same surface language:
- full-width option rows
- consistent row spacing and padding
- subtle hover surface
- selected surface state
- visible selected indicator

No new icons or controls should be added.

## Architecture

The change should remain local to Number inspector row construction and Number-specific CSS.

Core structure:
- `src/capture/editor/window/mod.rs` continues to build the Number `Style` and `Size` lists
- those lists should use the same inspector-native row composition pattern already used for the migrated sidepanel tools
- Number-specific CSS in `src/capture/editor/ui_support.rs` should align the row surfaces with the other panels
- the existing fixed sidepanel width path must remain unchanged

## Behavior Rules

- selecting a Number style must continue to use the existing Number style update path
- selecting a Number size must continue to use the existing Number size update path
- only the active row in each section should appear selected
- visual updates must stay in sync when the selected Number option changes
- no Number routing or tab-label behavior should change

## Error Handling

- if the current Number selection is not present in a rendered list, no incorrect row should appear selected
- visual consistency work must not break click handling or existing Number state syncing
- the implementation must not introduce a Number-specific sidepanel width constant

## Testing

Implementation should verify:
- the Number inspector still renders `Style` and `Size`
- both Number sections use the same migrated sidepanel surface language
- the selected Number style row is visibly active
- the selected Number size row is visibly active
- Number selection behavior remains unchanged
- sidepanel width remains unchanged

## Recommended Rollout

Implement in this order:
1. Update Number inspector row construction so both sections use the same row composition model
2. Add Number-specific CSS to align `Style` and `Size` with the other sidepanels
3. Verify active-row syncing still works for both sections
4. Confirm the fixed sidepanel width path remains unchanged
