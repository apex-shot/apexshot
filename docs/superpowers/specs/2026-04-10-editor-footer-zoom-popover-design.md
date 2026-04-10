# Editor Footer Zoom Popover Design

## Goal

Replace the editor footer pin control with a zoom trigger that shows the current zoom percentage and opens a compact zoom popover inside the editor canvas area.

## Scope

This design covers only the footer zoom trigger and its anchored popover in the screenshot editor.

Included:
- replacing the footer pin button with a zoom percentage trigger
- opening a compact popover above the footer trigger, inside the editor surface
- adding `Zoom In`, `Zoom Out`, and `Fit to Screen` actions
- showing instructional rows for scroll-wheel zoom and right-button pan
- styling the popover with the same surface language as the migrated side panels, without turning it into a full-height panel

Excluded:
- changing toolbar traffic-light zoom behavior
- adding a persistent zoom side panel
- changing the editor's overall footer layout beyond the left control swap
- adding new canvas navigation modes beyond the existing zoom and pan behavior

## Current State

The footer left slot is currently occupied by a pin button built in `src/capture/editor/window/footer.rs`.

The editor already maintains canvas zoom state through the shared view transform, and the codebase already uses GTK popovers for compact option surfaces in the toolbar. The existing side panels also provide the visual language the new popover should borrow from.

## Proposed UX

The footer's left control becomes a text-based zoom trigger such as `100%`.

When clicked:
- a popover appears above the trigger, not below it
- the popover stays visually inside the editor, anchored in the bottom-left canvas region rather than extending outside the editor shell
- the popover is only as tall and wide as needed to fit its content

Popover content:
- `Zoom In`
- `Zoom Out`
- `Fit to Screen`
- `Zoom with the scroll wheel`
- `Pan with the right button`

Interaction rules:
- the footer trigger shows only the current zoom percentage
- `Zoom In` increases zoom in fixed steps
- `Zoom Out` decreases zoom in fixed steps
- `Fit to Screen` restores the fitted canvas transform for the current editor viewport
- the last two rows are instructional only and are not clickable
- whenever zoom changes, the footer percentage updates to match the current view transform

## Architecture

The change should stay localized to the editor footer and event wiring.

Core structure:
- `src/capture/editor/window/footer.rs` builds a dedicated zoom trigger and anchored popover instead of the current pin control
- `src/capture/editor/window/events.rs` wires trigger clicks and zoom actions, and keeps the zoom percentage label in sync with the active transform
- `src/capture/editor/window/mod.rs` continues to pass footer parts into the editor event context with the new zoom-specific fields
- `src/capture/editor/ui_support.rs` provides any needed footer button or popover CSS so the popover visually matches the side-panel surface language while remaining compact

Placement model:
- the popover should be parented and positioned from the footer zoom trigger
- the popover should render upward from the footer trigger so it stays above the control
- the popover surface should remain bounded to compact menu-like dimensions rather than inheriting side-panel height or width behavior

## Styling

The zoom popover should visually read as part of the editor's migrated inspector system, but in a smaller popup form.

Styling requirements:
- reuse the restrained side-panel surface language for background, border radius, border, spacing, and hover states
- do not reuse any full-height or fixed-width side-panel layout behavior
- keep the popover compact, with enough room for all five rows and no unnecessary empty space
- preserve a clear distinction between clickable action rows and non-clickable instructional rows
- maintain alignment and spacing that feel consistent with the rest of the editor UI

## Behavior Rules

- clicking the footer zoom percentage toggles or opens the zoom popover
- clicking `Zoom In`, `Zoom Out`, or `Fit to Screen` applies the change immediately
- action rows should close the popover after activation unless keeping it open proves necessary for repeated zoom actions
- instructional rows must not mutate editor state
- the zoom label must always reflect the latest effective zoom percentage shown on canvas

## Error Handling

- if the editor cannot compute a valid fitted transform, `Fit to Screen` should leave the current transform unchanged rather than applying a broken state
- zoom actions must clamp to safe scale limits so repeated clicks cannot produce unusable values
- if the popover cannot be positioned above the trigger, it should still stay attached to the zoom control rather than detaching into a free-floating overlay

## Testing

Implementation should verify:
- the footer pin control is replaced by a zoom percentage trigger
- clicking the trigger opens a compact popover above the footer control
- the popover stays visually inside the editor and does not expand into a side panel
- `Zoom In` updates the canvas transform and footer percentage
- `Zoom Out` updates the canvas transform and footer percentage
- `Fit to Screen` restores the fitted view for the current editor viewport
- instructional rows render with the intended non-interactive styling
- the popover styling matches the side-panel surface language without adopting side-panel dimensions

## Recommended Rollout

Implement in this order:
1. Replace the footer pin slot with a zoom trigger and popover shell
2. Wire the zoom actions to existing transform state and fit behavior
3. Add compact popover styling that borrows from the side-panel surface language
4. Verify placement, behavior, and footer percentage syncing
