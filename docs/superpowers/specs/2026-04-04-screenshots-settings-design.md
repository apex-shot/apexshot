# Screenshots Settings Design

Date: 2026-04-04
Topic: Settings → Screenshots
Status: Approved in chat, pending user review of written spec

## Goal

Make the Screenshots tab reflect settings that can be truthfully applied on GNOME Wayland, and ensure the settings kept in the UI are the same settings the screenshot tool actually uses.

A hard requirement for implementation is to avoid breaking any existing screenshot workflow.

## Scope

This design covers the existing Settings → Screenshots UI and the screenshot-producing flows that save images through the app, including fullscreen, area, crosshair, window, and existing-PNG re-save flows where supported.

This design does not change unrelated recording workflows or editor behavior beyond screenshot-specific save/output handling.

## Design Principles

1. Preserve current workflows unless a setting is explicitly wired and safe.
2. Prefer truthful wording over platform-agnostic marketing language.
3. On GNOME Wayland, unsupported cases must fall back gracefully rather than fail.
4. The Screenshots tab should remain feature-rich, but each visible setting must have a clear, real effect.
5. Settings that are post-processing polish rather than screenshot behavior should not remain in this tab.

## Final Screenshots Settings

### Keep in Screenshots and wire to runtime

1. **Save location**
   - Source of truth for where screenshots are saved.
   - Applies to all screenshot save flows.

2. **File format**
   - Supported UI values should map to actual save output behavior.
   - Initial implementation should preserve current flows and add format-aware handling for save and re-save paths.

3. **Include pointer when available**
   - Renamed from: `Show cursor on screenshots`
   - Applies when cursor metadata is available from the capture flow.
   - If cursor data is unavailable in a given Wayland flow, capture still succeeds without the pointer.

4. **Self-timer interval**
   - Applies as a real delay before capture starts for supported screenshot flows.
   - Must not block, corrupt, or regress current capture behavior.

5. **Selection cursor**
   - Derived from the current crosshair setting.
   - Controls selection-overlay cursor behavior for selection-based capture flows.

6. **Show zoom preview while selecting**
   - Renamed from: `Show magnifier`
   - Applies only to selection-based screenshot flows.

7. **Use frozen background during selection**
   - Renamed from: `Freeze screen when taking a screenshot`
   - Applies to selection-based screenshot flows.
   - Wording is intentionally more accurate for GNOME Wayland than the original label.

### Remove from Screenshots

1. **Scale Retina screenshots to 1x**
   - Remove from Screenshots.
   - Reason: “Retina” is Apple-specific language and does not map cleanly to GNOME Wayland screenshot behavior.

2. **Add 1px border to all screenshots**
   - Remove from Screenshots.
   - Reason: this is export styling/post-processing, not core screenshot capture behavior.

## UX Wording Changes

The following labels should be updated in the UI:

- `Show cursor on screenshots` → `Include pointer when available`
- `Show magnifier` → `Show zoom preview while selecting`
- `Freeze screen when taking a screenshot` → `Use frozen background during selection`
- `Crosshair mode` should become a clearer `Selection cursor` control

The wording should avoid promising support that GNOME Wayland cannot guarantee in every path.

## Runtime Behavior Design

### Save location

The runtime screenshot save path must use the Screenshots tab save location as the authoritative output directory.

### File format

The runtime save path must use the selected screenshot format as the authoritative output format.

Expected behavior:
- New captures save in the chosen format.
- Existing-PNG re-save flows convert output if needed.
- If a format cannot be safely applied in a path yet, the implementation must preserve the current successful workflow and fall back safely rather than fail the capture.

### Include pointer when available

The runtime save path should include cursor compositing only when:
- the setting is enabled, and
- cursor data exists for the capture flow.

If either condition is false, save proceeds without cursor compositing.

### Self-timer interval

Timer behavior should be implemented as a capture-start delay for supported screenshot flows. Unsupported flows should retain current behavior without error.

### Selection-only behavior

The following settings affect selection overlays only and should not alter fullscreen or non-selection flows unless already supported by the current architecture:
- Selection cursor
- Show zoom preview while selecting
- Use frozen background during selection

## Compatibility and Fallback Rules

1. No screenshot flow should stop working because a setting is unsupported in that path.
2. Unsupported behavior must degrade gracefully.
3. Defaults should remain consistent with current app behavior unless intentionally changed.
4. Reworded settings should preserve existing config intent where practical, with migration or mapping if needed.

## Config and Migration Notes

The existing screenshot config fields should be reviewed and mapped as follows:

- Keep and use:
  - `screenshot_export_location`
  - `screenshot_format`
  - `screenshot_show_cursor` (with new UI wording)
  - `screenshot_timer_interval`
  - `screenshot_crosshair_mode` (reframed as selection cursor)
  - `screenshot_show_magnifier` (reworded)
  - `screenshot_freeze_screen` (reworded)

- Remove from UI and likely deprecate in config usage:
  - `screenshot_retina_scale`
  - `screenshot_frame_border`

Config cleanup should be handled carefully to avoid breaking existing config files.

## Testing Strategy

Implementation should verify:

1. Existing screenshot workflows still complete successfully.
2. Save location is honored for all save flows.
3. File format is honored where supported and falls back safely where not yet supported.
4. Pointer inclusion respects both the setting and cursor-data availability.
5. Selection-overlay settings affect only the intended selection-based flows.
6. Removed settings no longer appear in the Screenshots UI.
7. Existing configs load without regressions.

## Non-Goals

This design does not include:
- reintroducing Apple-specific “Retina” behavior
- adding screenshot border post-processing in the Screenshots tab
- changing unrelated recording settings or workflows
- forcing identical behavior across all desktop environments when GNOME Wayland cannot support it truthfully

## Recommended Implementation Direction

Roll out in this order:

1. Update Screenshots UI labels and remove the two dropped settings.
2. Ensure save/persist logic includes all kept screenshot settings.
3. Wire save location and file format into screenshot output paths.
4. Wire pointer inclusion safely into save behavior.
5. Wire timer and selection-overlay behavior without regressing current capture flows.
6. Add focused tests for supported behavior and graceful fallback.

## Self-Review

- No placeholders remain.
- Scope is limited to the Screenshots tab and screenshot runtime behavior.
- The design is consistent with the requirement not to break current workflows.
- GNOME Wayland limitations are handled through wording and fallback, not ignored.
