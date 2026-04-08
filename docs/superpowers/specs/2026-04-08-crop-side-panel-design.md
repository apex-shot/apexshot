# Crop Side Panel Design

## Goal

Move Crop controls from the top toolbar into the right inspector so Crop follows the same side-panel interaction model as Arrow: a tool-specific primary tab plus a scoped `Colors` tab.

## Scope

This design covers only the Crop tool inspector migration.

Included:
- replacing the current top-bar Crop controls with a right-side Crop inspector
- a `Crop` tab with crop-specific controls
- a `Colors` tab scoped only to crop fill color behavior
- preserving the current explicit `Apply` crop workflow

Excluded:
- duplicating the full Background tool inside Crop
- adding gradient, wallpaper, blur, or alignment controls to Crop
- broader refactors to unrelated inspector tools

## Current State

Today, Crop uses top-toolbar controls for:
- crop aspect ratio
- crop size readout
- apply action

The editor already supports:
- draft crop state and crop selection updates
- crop aspect ratio changes applied against the current crop selection
- crop size display updates
- explicit crop apply behavior
- crop background color state used when crop expansion exposes extra canvas

Arrow and Background already establish the newer right-inspector pattern:
- a primary tool tab
- a secondary `Colors` tab where appropriate

## Proposed UX

When `Tool::Crop` is active, the right inspector should expose two tabs:
- `Crop`
- `Colors`

The `Crop` tab becomes the primary Crop surface. The `Colors` tab remains secondary and is limited to crop fill color behavior.

The top toolbar should no longer show Crop-specific aspect ratio, dimensions, or apply controls once this migration is complete.

## Crop Tab

The `Crop` tab should contain three sections.

### Aspect Ratio

Show all supported crop aspect ratios in the side panel using the same inspector option-row language used by Arrow:
- `Freeform`
- `Original`
- `Square`
- `4:3`
- `16:9`
- `21:9`
- `3:2`
- `9:16`

Behavior:
- selecting an aspect ratio applies immediately to the active crop draft/selection
- if Crop mode is active and no crop selection exists yet, the selection should be initialized the same way the current toolbar path does
- the currently active aspect ratio must be visibly selected in the list

### Dimensions

Show the current crop width and height as a live read-only display derived from the active draft crop rect or crop selection.

Behavior:
- updates immediately when the crop box changes
- shows empty state when no crop selection exists
- remains informational only in this first pass; no direct width/height editing

### Actions

Show:
- `Reset`
- `Apply`

Behavior:
- `Reset` clears the current crop selection/draft without leaving Crop mode
- `Apply` commits the crop exactly as the current toolbar `Apply` button does
- `Apply` is only enabled when there is a valid crop selection

## Colors Tab

The Crop `Colors` tab should be intentionally narrow.

It should contain only the crop fill color control used when crop expansion reveals extra canvas. This is not a duplicate of the Background tool.

Included:
- current crop fill color preview/state
- choosing or updating the crop fill color

Excluded:
- background gradients
- wallpapers
- blur styles
- background alignment controls
- broader canvas background behavior

Behavior:
- crop fill color changes preview immediately in Crop mode
- the control continues to write to the existing crop background color state

## Interaction Model

Crop should keep an explicit apply workflow.

Reasoning:
- Crop changes canvas bounds and can discard image content
- immediate auto-apply would make the tool more error-prone than Arrow-style live property changes
- the current editor state already supports draft crop editing followed by explicit commit

Therefore:
- aspect ratio and crop fill preview should update immediately
- crop commit remains gated behind `Apply`

## Architecture Impact

### Inspector Routing

The inspector routing layer should treat Crop similarly to Arrow:
- primary Crop inspector surface for crop controls
- secondary shared `Colors` inspector surface for crop color behavior

### Toolbar Mode Changes

The top toolbar should stop rendering Crop-specific aspect ratio, size, and apply controls once the side-panel migration is complete.

Any generic tool switching affordances can remain, but Crop-specific control groups should be removed from the toolbar mode stack.

### State Reuse

Implementation should reuse existing state and handlers wherever possible:
- crop aspect ratio state
- crop selection initialization
- draft crop rect / crop selection size calculation
- crop background color state
- apply crop action

No new parallel crop state should be introduced.

## Error Handling

- If there is no crop selection, `Dimensions` should show an empty state rather than stale values.
- `Apply` must remain disabled when there is no valid crop selection.
- `Reset` should be a no-op when no crop selection exists.
- Crop `Colors` must not leak Background-tool-only controls into the Crop inspector.

## Testing

Implementation should verify:
- Crop routes to its own inspector surface and the shared `Colors` surface
- the Crop tab renders `Aspect Ratio`, `Dimensions`, and `Actions`
- the Colors tab for Crop is scoped to crop fill color controls only
- selecting an aspect ratio updates crop state immediately
- dimensions update from the active draft crop rect / crop selection
- `Reset` clears the crop draft/selection
- `Apply` uses the existing crop commit path and enablement rules
- top-toolbar crop-specific controls are removed or hidden when this migration is complete

## Recommended Rollout

Implement in this order:
1. Build the Crop inspector surface and routing
2. Move aspect ratio and dimensions into the inspector
3. Move apply/reset actions into the inspector
4. Add the scoped Crop `Colors` tab
5. Remove Crop-specific toolbar controls

This keeps the migration incremental while preserving existing Crop behavior.
