# Editor Export and Scroll-Capture Behavior Design

Date: 2026-04-23
Project: ApexShot
Status: Approved in chat

## Summary

Fix editor behavior so the saved output matches the preview for crop and background-related tools, with special attention to shadow rendering and scroll captures. Also fix scroll-capture zoom/viewport behavior so tall captures open in a sensible position and keep the top of the capture reachable.

This work is intentionally scoped to:
- crop tool
- background tool and its effects, especially shadow
- normal screenshot captures
- scroll captures

This work should avoid changing annotation/drawing behavior unless a shared rendering layer must be touched, because annotation export currently matches preview.

## Problem Statement

The editor preview currently shows acceptable results for crop/background effects, but the final saved output diverges from what the user saw before pressing Done.

Observed issues:
- Normal screenshots: saved shadow rendering does not look like the preview and does not read visually as a proper shadow.
- Scroll captures: configured background can disappear in the saved output.
- Scroll captures: shadow/background export can produce severe distortion or masking artifacts, including large black regions around the final image.
- Crop/background combinations are part of the affected surface area and must export consistently.
- Scroll captures: zoom/viewport behavior is poor for tall images. The image appears vertically centered, so the top of the captured content is not immediately visible/reachable as users expect.

## Goals

1. Saved output for crop/background/shadow matches the editor preview as closely as possible.
2. Scroll capture export handles large/tall images without losing background or producing distortion artifacts.
3. Normal screenshot export remains correct.
4. Annotation/drawing output remains unchanged in behavior.
5. Tall scroll captures open with a sensible initial view and predictable zoom/pan behavior.
6. Add regression coverage for the affected behavior.

## Non-Goals

- Redesigning annotation/drawing tools.
- Introducing new visual effects.
- Large editor refactors unrelated to preview/export parity.
- Changing UX for normal screenshots if current centered behavior is already correct.

## Likely Root Cause

The most likely failure is preview/export divergence:
- preview composition and saved-output composition are not using the same layout/effect model
- export likely recomputes geometry differently from preview
- scroll capture dimensions and masks likely amplify the mismatch because of extreme aspect ratios and extra padding/shadow/background math

The fact that annotation output already matches preview suggests the bug is concentrated in crop/background composition rather than the entire export pipeline.

## Recommended Approach

Use one shared composition model for preview and export for the affected tools.

Recommended option:
- keep the current annotation export path intact unless shared code changes are necessary
- audit where preview computes crop/background/shadow layout
- audit where save/export recomputes those values
- move crop/background export onto a shared composition step or shared composition inputs so preview and export use the same:
  - source bounds
  - crop rect
  - padding
  - background fill/image/gradient selection
  - rounded corner settings
  - shadow geometry and blur/offset extents
  - final composed canvas size
  - image placement within the final composed canvas

For scroll captures specifically:
- export should derive final bounds from the composed preview model, not from a separate raw-dimensions reconstruction path
- this should eliminate missing backgrounds, clipping, and black/distorted mask artifacts caused by geometry mismatches

## Alternatives Considered

### Option A: Patch scroll export only
Pros:
- lower immediate scope
- may fix the worst screenshot shown by the user quickly

Cons:
- leaves normal screenshot shadow mismatch unresolved
- preserves the fundamental preview/export drift
- likely causes future regressions when crop/background logic changes

### Option B: Tweak only shadow math
Pros:
- very small patch if only one shadow parameter is wrong

Cons:
- does not address missing scroll backgrounds
- does not address black surround/distortion artifacts
- does not solve preview/export inconsistency as a class of bugs

### Option C: Shared composition model for preview and export
Pros:
- solves the real issue: preview and export disagreement
- handles both normal and scroll captures
- reduces long-term maintenance risk

Cons:
- slightly broader than a hotfix
- requires careful isolation to avoid disturbing working annotation behavior

Recommendation: Option C.

## Detailed Design

### 1. Composition boundary

Define a shared composition boundary for the affected tools. The boundary should answer:
- what is the visible image region after crop?
- what is the background canvas size?
- where does the cropped image sit inside that canvas?
- what extra extents are introduced by padding, rounded corners, and shadow?
- what final output size should be produced?

Preview and export should both consume this same composition result, or both call the same pure helper that derives it.

### 2. Export behavior

For crop/background/shadow export:
- compute final composition geometry once
- render background first
- render the cropped image in its computed placement
- apply clipping/masking exactly as preview expects
- apply shadow with the same extents and positioning used by preview
- produce the final image using the composed canvas dimensions

This should prevent:
- background disappearing in saved output
- shadows being clipped or stretched incorrectly
- final export including unintended black regions

### 3. Scroll capture handling

Scroll captures need special care because they are unusually tall and more sensitive to centering/clipping assumptions.

For scroll captures:
- use the same composition path as normal captures for crop/background/shadow
- avoid any special-case export path that recomputes final geometry independently from preview
- ensure final surface sizing includes all background/shadow extents before rasterization
- verify that clipping/masking uses the image bounds, not the viewport bounds

### 4. Preserve annotation behavior

Annotation/drawing currently exports correctly. Therefore:
- do not rewrite annotation export just for consistency
- only touch shared code if required by the new composition boundary
- if shared code changes are necessary, verify annotation parity explicitly

### 5. Viewport and zoom behavior for tall scroll captures

Use adaptive initial framing:
- normal screenshots: keep centered behavior
- tall scroll captures: default to top-aligned initial view with sensible margins

Zoom/pan behavior should:
- preserve a stable, intuitive anchor
- keep the top of the capture reachable
- avoid forced recentering that hides the beginning of a tall capture

This follows common long-canvas editor behavior and is more intuitive than vertically centering very tall images.

## Testing Strategy

Add regression coverage at the level that is practical in this codebase.

Required coverage:
- normal screenshot + shadow export
- normal screenshot + crop + background export
- scroll capture + background export
- scroll capture + shadow export
- crop combined with background/shadow export
- tall scroll capture initial viewport positioning
- tall scroll capture zoom behavior preserving top reachability

Preferred testing style:
- pure geometry/composition tests for final bounds and placement
- targeted render/export tests for cases where pixel or size assertions are feasible
- viewport state tests for initial framing/zoom anchoring behavior

If full image snapshot tests are too expensive, assert intermediate composition values that directly determine correctness.

## Risks and Mitigations

### Risk: fixing export breaks annotation output
Mitigation:
- isolate composition changes to crop/background paths
- add verification around annotation behavior if shared code is touched

### Risk: preview and export still differ because they call the same helper with different inputs
Mitigation:
- centralize both calculation and the input model where possible
- compare the exact state objects used by preview vs export during implementation/debugging

### Risk: scroll captures still fail due to extreme dimensions
Mitigation:
- add dedicated tests with tall aspect ratios
- explicitly validate surface sizing and clipping extents for scroll captures

## Implementation Notes

Likely areas to inspect first:
- preview rendering code under `src/capture/editor/render.rs`
- editor state/composition data under `src/capture/editor/state.rs`
- window/canvas integration under `src/capture/editor/window/canvas.rs`
- save/export logic under `src/capture/editor/io_ops.rs`
- crop/background UI state wiring under the editor modules

The implementation should begin by mapping the current preview pipeline and export pipeline to find where geometry/effect calculations diverge.

## Acceptance Criteria

- Saved output for crop/background/shadow visually matches the preview for normal screenshots.
- Saved output for crop/background/shadow visually matches the preview for scroll captures.
- Scroll-capture exports no longer show missing background or black/distorted surrounding artifacts.
- Crop-related export remains correct when combined with background effects.
- Annotation/drawing export still matches preview.
- Tall scroll captures open top-aligned instead of being awkwardly centered.
- Zoom/pan on tall scroll captures keeps the top reachable and behaves predictably.
- Regression tests cover the identified bug classes.
