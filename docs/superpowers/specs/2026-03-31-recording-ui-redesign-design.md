# Pre-Recording UI Redesign

## Summary

Redesign the pre-recording UI in the C++ capture overlay so it belongs to the same visual family as the new capture cockpit while adopting a layout appropriate for recording setup. The new direction is a two-module recording deck anchored outside the selection, below the frame by default, with deeper settings and per-tool menus allowed to appear inside the selected region.

## Problem

The current pre-recording UI in `capture-overlay/src/CaptureOverlay_Drawing.cpp::drawRecordingPanel()` is rendered as two stacked panels inside the selected region. This has two problems:

- it competes with the frame the user is trying to configure
- it does not align with the new capture cockpit direction, so the product identity feels fragmented

The current panel also exposes too many controls in one surface. That makes it flexible, but not especially elegant, and it places setup chrome on top of the thing the user is framing.

## Goals

- Keep the recording setup UI in the same family as the new capture cockpit
- Move the main pre-recording deck outside the selected region
- Preserve a clear hierarchy between primary recording choices and secondary toggles
- Keep advanced settings contextual by opening menus inside the selection area
- Avoid regressions to the capture toolbar layout and interactions

## Non-Goals

- Redesigning the live recording controls shown after recording starts
- Rebuilding the GNOME runtime controls in this pass
- Changing the recording feature set or settings model
- Converting the recording setup into a minimal single-button flow

## Chosen Direction

Use a split recording deck outside the capture region:

- a primary module for recording mode and start actions
- a secondary module for quick toggles and settings entry points

Both modules should be anchored below the selected region by default, with fallback above when there is not enough room. Menus and advanced settings remain allowed to open inside the selection area where they can feel contextual to the frame.

## Alternatives Considered

### 1. Keep recording UI inside the selection

Pros:
- no new placement logic
- short pointer travel from frame to controls

Cons:
- covers the framing target
- fights the visual role of the selection
- does not match the new outside-the-frame cockpit language

### 2. Single wide deck below the selection

Pros:
- simple mental model
- easy to implement

Cons:
- recording has more controls than capture
- becomes too wide or too dense quickly
- weak hierarchy between primary and secondary actions

### 3. Split two-module deck below the selection

Pros:
- strongest hierarchy
- scalable for recording complexity
- consistent with the new distributed capture UI direction

Cons:
- more placement logic
- requires clear spacing and fallback behavior

## Layout Design

### Primary module

This is the main recording control deck and should carry the strongest visual weight.

Responsibilities:
- record type selection (`Video` / `GIF`)
- region state and framing-related actions
- start recording action

Behavior:
- anchored below the selection
- visually dominant compared with the support module
- remains visible while the user adjusts the selection

Visual treatment:
- stronger warm accent treatment
- clearer typography and hierarchy
- larger and more deliberate action surfaces than the support module

### Secondary module

This is the support deck and should remain visibly subordinate.

Responsibilities:
- direct toggles for `mic`, `speaker`, and `webcam`
- entry points for `clicks`, `keystrokes`, and deeper settings

Behavior:
- linked to the main module as one deck system
- can sit adjacent to the primary module with a clear spacing gap
- should not visually overpower the main recording decision path

### Contextual settings and menus

Advanced settings should not live permanently in the outer deck.

Rules:
- deeper menus may open inside the selected region
- these menus should feel contextual to the thing they affect
- the outer deck remains focused and readable

This preserves power without forcing the main pre-recording UI to become bloated.

## Visual Language

The pre-recording deck should belong to the same family as the capture cockpit:

- smoked or frosted dark surfaces
- sharper card edges and more instrument-like geometry
- warm cinematic accents instead of the existing indigo-forward treatment
- stronger primary/secondary hierarchy
- reduced “floating generic panel” feel

It should not be a copy of the capture toolbar. Recording is a denser workflow, so it needs a broader console form while preserving the shared family materials, contrast, spacing logic, and accent behavior.

## Placement Rules

Default behavior:
- deck sits below the selected region
- horizontally centered relative to the frame where possible

Fallback behavior:
- if there is not enough room below, place the deck above the selection
- do not default to side docking
- keep the two modules linked even in fallback placement

The deck must never obscure the selection by default.

## Information Hierarchy

Always visible in the outer deck:
- start action
- recording type
- mic toggle
- speaker toggle
- webcam toggle

Available but not always expanded:
- clicks options
- keystrokes options
- advanced settings for output quality and behavior

This creates a balanced deck: essential setup is immediate, advanced setup is one interaction away.

## Interaction Model

- the deck stays visible during selection adjustment, matching the capture setup behavior
- the main module visually leads the eye toward the start action
- support toggles provide fast on/off control without dominating the layout
- menus opened from the support deck may appear inside the selection area
- the selected frame remains visually legible at all times

## Implementation Notes

Current implementation center:
- `capture-overlay/src/CaptureOverlay_Drawing.cpp::drawRecordingPanel()`

Current characteristics:
- two stacked panels rendered inside the selection
- fixed tile grid with top and bottom sections
- settings menu already rendered as a separate popup path

Recommended implementation direction:
- compute a new outside-the-selection deck layout for the pre-recording UI
- split the current panel into explicit primary and secondary module rects
- preserve existing settings-menu and sub-menu mechanics where possible
- move only the outer deck first; do not rewrite every submenu in the same step unless needed

This should be treated as a recording-setup UI redesign, not as a feature rewrite.

## Testing

Verify:
- the pre-recording deck renders below the selection when space is available
- the fallback above-selection placement works near the bottom of the screen
- the deck remains outside the selected frame
- the main module remains visually primary
- the support module still exposes core toggles
- deeper menus can still open inside the selection area
- selection adjustment does not break deck placement

Manual scenarios should include:
- small selection near screen center
- large selection
- selection near bottom edge
- selection near top edge
- narrow selection
- wide selection
- fullscreen recording setup

## Success Criteria

- The pre-recording UI feels like part of the same ApexShot family as the capture cockpit
- The main deck no longer covers the selected region
- The recording setup reads clearly as primary controls plus support controls
- Advanced settings remain accessible without bloating the main deck
