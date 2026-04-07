# Right Inspector Arrow Panel Sections Design

## Goal
Expand the Arrow tool's right-inspector primary tab into a fuller panel with three sections: `Style`, `Thickness`, and `Behavior`. This should make the Arrow panel feel complete without changing the inspector width and without turning this migration slice into a broader net-new arrow feature project.

## Scope
This change covers the Arrow tool's primary right-inspector tab only.

Included:
- keep the existing `Arrow | Colors` inspector tab structure
- add a `Style` section for the existing arrow variants
- add a `Thickness` section for the existing stroke thickness choices already supported by the editor
- add a `Behavior` section only for controls that already map to existing live Arrow state or config
- keep the Arrow panel on the same fixed inspector width used today

Not included:
- redesigning the shared `Colors` tab
- changing Text, Number, or other tool panels in this change
- introducing brand-new Arrow rendering behavior that needs new editor state or render math
- changing the overall inspector shell width or layout model

## Arrow Panel Structure
The Arrow primary tab should be organized as three stacked sections:

### Style
This section shows the existing Arrow style variants already supported by `ArrowStyle`. It should use direct sidebar-native controls rather than toolbar-style dropdown triggers.

### Thickness
This section shows the existing stroke size options used by Arrow today. Because stroke size is already part of current editor behavior, this section should simply surface the same choices in the Arrow panel.

### Behavior
This section should contain Arrow-only behavior controls only when they already map to existing live state or config. The first candidate is surfacing the existing inverse-arrow-direction behavior if it can be cleanly wired as a live Arrow control.

## Behavior Rules
- `Behavior` is Arrow-only in this slice
- each control in `Behavior` must already have a real state or config hook in the current codebase
- if a candidate control needs new render logic, new data model fields, or new interaction semantics, it is out of scope for this implementation and should be deferred to a separate design

## Architecture
The Arrow panel should be built as inspector-native UI, not as reused toolbar dropdown shells.

Core structure:
- the right inspector shell in `src/capture/editor/window/mod.rs` continues to own tab routing
- the Arrow primary surface renders three direct sections: `Style`, `Thickness`, and `Behavior`
- section controls call the same editor-state update hooks already used by current Arrow controls wherever those hooks already exist
- the inspector width continues to use the existing fixed sidebar width path

## Candidate Behavior Control
Recommended first `Behavior` control:
- inverse arrow direction, if it can be mapped to the existing `inverse_arrow_direction` path as a live Arrow toggle

Deferred examples:
- angle snapping
- per-arrow head size
- curve tension
- new double-arrow placement controls

These are deferred because they would expand scope into new Arrow feature work rather than a clean inspector-panel completion.

## Testing
Verification should cover:
- Arrow inspector renders `Style`, `Thickness`, and `Behavior` sections in the primary Arrow tab
- Arrow style changes still update the active Arrow behavior
- Arrow thickness changes still update the existing stroke size state used for Arrow
- any `Behavior` control included in this slice maps to an already-supported live hook
- the `Colors` tab remains unchanged
- inspector width remains unchanged
