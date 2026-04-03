# General shortcuts cleanup and wiring design

## Goal
Remove misleading shortcut rows that are not wanted and make the remaining general shortcut rows perform real daemon actions.

## Scope
Remove only these shortcut rows from the Settings > Shortcuts UI:
- Toggle Desktop Icons
- Pin to the Screen

Keep and wire these rows so they work as real hotkeys:
- Open File
- Open From Clipboard
- Restore Recently Closed File
- Hide/Show Overlays

## Behavior
- Open File: open the most recently captured or saved file.
- Open From Clipboard: import/open an image from the clipboard in ApexShot. If the clipboard does not contain an image, show an error notification.
- Restore Recently Closed File: restore the most recently closed floating overlay/file view.
- Hide/Show Overlays: toggle ApexShot overlay visibility.

## Architecture
- Shortcuts UI removes the two unwanted rows but keeps the four supported rows.
- Settings save/load continues to persist the four supported shortcuts.
- Hotkey config generation adds real bindings for the four supported actions.
- Daemon action mapping gains dedicated actions for the four supported shortcuts.
- Daemon runtime reuses existing "last capture" behavior where available and adds minimal state/hooks only for restore/toggle paths that are currently missing.

## Validation
- Shortcuts UI test verifies removed rows are absent and kept rows remain.
- Hotkey config test verifies supported general shortcuts are emitted.
- Daemon action mapping tests verify new binding names/args map to daemon actions.
- Runtime/manual verification confirms each shortcut performs the requested action.
