# Per-Feature Export Location Design

## Goal

Split the current shared export directory into separate save locations for screenshots and video recordings, move the screenshot save path control into the Screenshots tab, and remove the unsupported desktop-icons toggle from settings.

## Current Problem

The current `export_location` setting is surfaced under the General tab and is shared by screenshots and recordings. That makes the UI misleading and prevents users from choosing different destinations for screenshot files and recorded videos. The existing `Hide desktop icons while capturing` toggle is also not supportable on GNOME and should not remain exposed.

## Proposed Changes

### Config

- Add `screenshot_export_location`
- Add `video_export_location`
- Keep legacy `export_location` only as a migration input
- When loading older configs that only have `export_location`, seed both new fields from that value

### Settings UI

- Remove the storage section from the General tab
- Remove `Hide desktop icons while capturing` from settings
- Add screenshot save location controls to the Screenshots tab
- Add video save location controls to the Recording tab

### Runtime Behavior

- Screenshot capture should use `screenshot_export_location`
- Video recording should use `video_export_location`
- Older configs should continue to work through migration

## Scope Guardrails

- No change to unrelated capture behavior
- No new per-feature paths beyond screenshots and video
- No attempt to support desktop-icon hiding on GNOME

## Verification

- `cargo check`
- Manual smoke test:
  - set different screenshot and video folders
  - save settings
  - verify screenshots go to the screenshot folder
  - verify recordings go to the video folder
