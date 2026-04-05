# Crosshair Performance and Selector Startup Design

**Date:** 2026-04-05

## Goal
Reduce crosshair lag during hover/move on GNOME Wayland and restore screenshot selector startup speed regressions introduced during the screenshots-settings work.

## Problem Summary
Two regressions are present:

1. **Crosshair interaction lag**: in crosshair capture mode, mouse movement triggers unconditional repaint of the full-screen overlay on every move. This is most noticeable when hovering over active/open windows and when targeting small regions.
2. **Selector startup slowdown**: recent screenshots-settings changes introduced extra work in the selector launch path, causing area/crosshair startup to feel significantly slower than earlier revisions.

## Root Cause Summary
### Crosshair lag
In `capture-overlay/src/CaptureOverlay_Events.cpp`, the crosshair-mode branch of `mouseMoveEvent()` calls `update()` on every move. In `capture-overlay/src/CaptureOverlay_Drawing.cpp`, `paintEvent()` redraws the entire overlay in crosshair mode, including the full-screen background pixmap and guide lines.

This means every pointer movement repaints the full-screen surface instead of only the regions affected by the old and new cursor/selection state.

### Startup regression
Recent branch work added screenshot-selection settings propagation and optional overlay behaviors. The regression window includes commits such as:
- `1591a05 feat: wire screenshot selection settings into overlay launch`
- `dca1632 feat: render zoom preview during screenshot selection`

The fix should focus on identifying extra work now performed before the overlay is visible and deferring or skipping any nonessential initialization unless the relevant setting is actively enabled.

## Recommended Approach
Use a targeted hot-path fix first:
- keep existing user-facing crosshair behavior
- optimize repaint invalidation to the minimum affected regions
- trace startup overhead introduced by recent settings-related changes
- defer or avoid noncritical initialization before first paint/show

This is preferred over removing behavior, because it preserves the intended UX while directly addressing the hottest paths.

## Design

### 1. Crosshair repaint optimization
Add bounded invalidation for crosshair mode:
- track previous crosshair paint state (pointer position, bubble rect, selection rect)
- compute dirty regions covering:
  - old horizontal/vertical guide lines
  - new horizontal/vertical guide lines
  - old and new info bubble rects
  - old and new selection rects while dragging
- replace unconditional full-window `update()` calls in crosshair mode with targeted `update(QRect)` / `update(QRegion)` calls
- avoid redundant cursor changes when the cursor shape is already correct

This keeps the current crosshair visuals but reduces work per mouse move from full-screen repaint to local repaint.

### 2. Startup-path regression fix
Audit the current selector launch path against the pre-regression behavior:
- compare `capture-overlay/src/main.cpp`, `src/capture_overlay.rs`, and related settings plumbing to the pre-settings commits
- identify work done before overlay visibility that was not previously required
- defer optional setup unless enabled and needed for the current overlay mode
- ensure crosshair mode does not pay initialization costs for disabled or unrelated features

Likely suspects include:
- settings propagation/setup done eagerly for all modes
- preview or freeze-background related initialization
- startup-time preparation for features not used in crosshair mode

### 3. Non-goals for this patch
- redesigning the crosshair UI
- reintroducing new zoom-preview behavior in the same patch
- changing portal/backend capture architecture beyond the startup regression fix

## Testing Strategy
### Automated
Add focused regression coverage where practical for launch-path argument construction and mode-specific initialization behavior.

### Manual
Verify on GNOME Wayland:
- crosshair movement feels smooth over active/open windows
- small-target selection is responsive
- selector startup is close to pre-regression speed
- crosshair capture still completes correctly
- area capture and related selector flows are not regressed

## Risks
- dirty-region math could leave stale pixels if old/new paint bounds are incomplete
- deferring initialization could accidentally skip required state for certain capture modes

Mitigation:
- keep the optimization narrowly scoped to crosshair mode first
- verify old/new regions conservatively with small padding
- compare launch behavior against pre-regression commits before simplifying startup work

## Success Criteria
- crosshair mode no longer visibly lags during pointer movement over windows
- startup delay is materially reduced relative to the current branch state
- existing area/crosshair capture behavior remains intact
