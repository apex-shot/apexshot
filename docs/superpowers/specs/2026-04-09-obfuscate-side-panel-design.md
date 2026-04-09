# Obfuscate Side Panel Design

## Goal

Move Obfuscate subtool selection from the top toolbar into the right inspector so the Obfuscate tool uses an inspector-native primary panel, while keeping the shared amount slider in the toolbar and removing the unused `Colors` tab for this tool.

## Scope

This design covers only the Obfuscate tool inspector migration.

Included:
- replacing the toolbar obfuscate method picker with a right-side Obfuscate inspector
- showing a single `Obfuscate` tab with a `Method` section
- preserving the current shared toolbar slider for obfuscation amount
- removing the `Colors` tab for Obfuscate

Excluded:
- migrating the shared toolbar slider into the inspector
- adding color controls to Obfuscate
- changing obfuscation render behavior or method semantics
- redesigning other tool inspectors

## Current State

Today, Obfuscate still behaves like a non-migrated color-capable tool in inspector routing, so it lands on the shared `Colors` surface instead of a tool-specific panel.

At the same time, the actual Obfuscate-specific subtool picker still lives in the toolbar as a popover-based method control. The amount control remains on the shared toolbar slider path, which is also used by other tools and has not been migrated for them.

## Proposed UX

When `Tool::Obfuscate` is active, the right inspector should show a single tab:
- `Obfuscate`

There should be no secondary `Colors` tab for this tool.

The `Obfuscate` tab should contain one section:

### Method

This section shows the existing Obfuscate subtools already supported by current editor state and rendering behavior. The section should use direct inspector option rows rather than a toolbar popover trigger.

Behavior:
- selecting an option updates the active obfuscate method immediately
- the currently active obfuscate method is visibly selected in the inspector
- switching between Obfuscate methods does not change the current amount-slider workflow

## Interaction Model

Obfuscate should split responsibilities cleanly:
- method selection lives in the right inspector
- amount remains on the shared toolbar slider

Reasoning:
- the slider is not Obfuscate-specific and is still shared with other tools
- migrating only the Obfuscate-specific subtool picker keeps the inspector focused on tool-native choices
- moving the slider only for Obfuscate would make this tool inconsistent with the rest of the editor

## Architecture Impact

### Inspector Routing

Obfuscate should stop following the non-migrated color-tool path.

Instead:
- Obfuscate routes to its own inspector surface
- the active tab label should be `Obfuscate`
- no `Colors` tab should be rendered or activated for Obfuscate

### Inspector Surface

The Obfuscate inspector should reuse the existing shared inspector shell helpers already used by Crop, Arrow, Text, and Number:
- same fixed sidepanel width path
- same section framing
- same option-row visual language used by the other migrated tool panels

The tab should contain one `Method` section only.

### Toolbar Changes

The toolbar should stop rendering the Obfuscate-specific method picker once the sidepanel migration is complete.

The shared amount slider should remain available and unchanged when Obfuscate is active.

## Error Handling

- Obfuscate should never fall back to the shared `Colors` tab after this migration
- if the current Obfuscate method is missing from the rendered inspector list, no incorrect row should appear selected
- removing the toolbar method picker must not break the existing method-update path
- the fixed inspector width must remain on the existing shared width constant

## Testing

Implementation should verify:
- Obfuscate routes to a single `Obfuscate` inspector tab
- the `Colors` tab does not appear for Obfuscate
- the `Obfuscate` tab renders a `Method` section
- the section contains the existing Obfuscate subtool choices
- selecting a method updates the active Obfuscate state immediately
- the selected Obfuscate method is visibly active in the inspector
- the shared toolbar slider still appears for Obfuscate and remains unchanged
- the inspector width remains unchanged

## Recommended Rollout

Implement in this order:
1. Build the Obfuscate inspector surface and routing
2. Render Obfuscate methods as inspector-native option rows
3. Connect selection state and method updates to the existing editor-state path
4. Remove the toolbar Obfuscate method picker while leaving the shared slider untouched
5. Verify that Obfuscate no longer uses the shared `Colors` tab
