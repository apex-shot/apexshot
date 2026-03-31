# Capture Overlay Toolbar Redesign

## Summary

Redesign the capture overlay toolbar so it has a distinct ApexShot identity instead of reading like a clone of another capture tool. The new direction is a cinematic, speed-first "pilot cockpit" layout that replaces the current single floating toolbar with two side-mounted control clusters around the active selection.

## Problem

The current capture overlay toolbar in `src/overlay.rs` is a centered frosted-glass strip with evenly spaced icon cells plus a neighboring size panel. Its overall silhouette, placement, and glossy treatment are too close to existing tools in the category. Even if the icon artwork changes, the product can still feel derivative because the interaction model is the same:

- one horizontal bar
- centered above or below the selection
- uniform icon pills
- adjacent size readout card

That makes the toolbar easy to use, but it does not create a memorable or ownable visual identity for ApexShot.

## Goals

- Make the capture overlay immediately recognizable as ApexShot
- Change the toolbar silhouette and placement enough that it no longer reads like a clone
- Preserve fast first-click access for common actions
- Keep the capture area visually dominant
- Reuse the current drawing approach where practical so the redesign is implementable in the existing GTK Cairo overlay

## Non-Goals

- Redesigning the entire editor or recording controls UI in this pass
- Adding new capture tools or changing the underlying capture workflow
- Turning the overlay into a fully animated or heavily staged interface that slows down quick captures

## Chosen Direction

Use a cinematic cockpit layout with asymmetric side rails:

- a primary left rail for capture mode tools
- a compact right rail for selection metadata and commit/exit actions

This is the recommended direction because it creates a clearly different silhouette while staying compatible with the existing overlay code, which already computes custom panel positions and draws all chrome directly with Cairo.

## Alternatives Considered

### 1. Cinema-frame controls

Place controls along the top, bottom, or corners of the selection border.

Pros:
- dramatic
- visually tied to the capture frame

Cons:
- competes with resize affordances
- risks making the selection edge too busy
- more likely to interfere with precision pointer work

### 2. Split top and bottom strips

Keep horizontal controls, but divide them into separate bands.

Pros:
- easy migration from the current layout
- low implementation risk

Cons:
- still visually close to familiar screenshot tool patterns
- not distinct enough for the stated goal

### 3. Pilot cockpit side rails

Place controls on the sides of the selection with different roles per side.

Pros:
- strongest unique silhouette
- keeps the capture area clean
- preserves quick access when structured correctly

Cons:
- requires updated layout and hit-testing logic
- needs careful sizing on small selections

## Layout Design

### Left rail: primary tools

The left side of the selection becomes the main tool rail.

Behavior:
- vertically stacked controls
- anchored to the vertical center of the selection by default
- clamps to the viewport when the selection is near the screen edge
- remains outside the capture rectangle with a consistent air gap

Contents:
- screenshot capture
- record area
- scroll capture
- OCR
- color picker
- pin

Visual treatment:
- taller, narrower body than the current toolbar
- larger icon cells than today
- active tool gets a brighter internal glow and a stronger edge accent
- labels are reduced or removed from the default state to keep the silhouette clean
- optional tooltip-style labels may appear on hover if needed for clarity

### Right rail: status and actions

The right side becomes a slimmer support rail.

Behavior:
- vertically aligned to the selection center, matching the left rail
- uses smaller cards stacked with deliberate spacing
- keeps quick-confirm actions separate from mode selection

Contents:
- size readout card
- primary confirm action
- cancel/close action

Optional later addition:
- a small hint row for modifier keys if discoverability becomes a problem

Visual treatment:
- more compact than the left rail
- size card appears as an instrument readout, not a generic badge
- confirm button gets the strongest emphasis on this side

## Visual Language

The redesign should feel cinematic, but still consistent with ApexShot's existing materials.

Keep:
- dark translucent surfaces
- frosted or blurred backing where available
- bright icon rendering
- crisp shadows and layered highlights

Change:
- move away from soft glossy white pill styling
- reduce the "floating mobile sheet" feel
- replace uniform rounded pills with tensioned rectangular cards using tighter radii and optional chamfer-like corner cuts
- use warmer accent lighting, such as ember, amber, or restrained red-orange highlights, instead of pure white gloss as the main signature

The overall result should feel like capture instrumentation suspended around the frame, not a general-purpose toolbar.

## Motion

Motion should support identity without reducing speed.

Recommended motion:
- when a selection becomes valid, left and right rails fade and slide in from their respective sides
- hover states should brighten and tighten rather than bounce
- state changes should finish quickly and avoid theatrical delays

Do not use:
- long easing curves
- scale-pop effects on every hover
- ornamental animation that competes with selection precision

## Responsiveness Rules

The layout must remain usable when the selection is small or close to screen edges.

Rules:
- if both side rails fit, use the default cockpit layout
- if one side has insufficient room, stack both rails on the side with more room while preserving role separation
- if the selection is too small for full-height rails, collapse the left rail to fewer visible labels and tighten card spacing
- as a final fallback, use a compact horizontal emergency layout, but only when side placement is impossible

The fallback exists for robustness, but the default and preferred experience is the split side-rail layout.

## Interaction Model

- Hovering a control highlights only that control, not the full rail
- The selected mode remains visibly armed until another mode is chosen
- Confirm and cancel remain spatially separated from mode switching
- Size information stays visible without competing with the primary tool rail
- The capture rectangle remains the central visual focus at all times

## Implementation Notes

Current implementation constraints in `src/overlay.rs`:

- `draw_feature_toolbar()` draws a single tools panel plus one size panel
- `compute_toolbar_layout()` computes one grouped horizontal placement
- `toolbar_hit_at()` assumes one array of item cells plus one size panel

Implementation should restructure this into a more expressive layout model:

- replace the single grouped layout with explicit left-rail and right-rail rects
- split item hit-testing by rail role
- allow separate card sizing for tools, size readout, and action buttons
- preserve the existing Cairo rendering style where possible by extending `draw_frosted_panel()` or adding a second chrome helper for the new card shape

This should remain a drawing and layout refactor, not a workflow rewrite.

## Testing

Verify:
- rails position correctly for large and small selections
- rails do not render off-screen near each display edge
- hover and click hit-testing still matches visual bounds
- selection resizing remains unobstructed
- confirm and cancel actions remain easy to reach
- the fallback layout activates only when needed

Manual review should include screenshots with selections near:
- top-left corner
- top-right corner
- bottom-left corner
- bottom-right corner
- center of screen
- very small selection
- wide but short selection
- tall but narrow selection

## Success Criteria

- The toolbar no longer presents as a centered clone-style strip
- The overlay gains a distinct, ownable silhouette
- Core actions remain fast to access
- The redesign feels cinematic without sacrificing precision
