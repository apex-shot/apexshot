# Portal Capture Identity Design

## Goal
Ensure screenshot and recording portal requests use the main ApexShot desktop identity so GNOME can persist screencast/screenshot consent across reboots, without changing the daemon's working autostart and hotkey behavior.

## Design
- Keep daemon startup and hotkey registration behavior unchanged.
- Introduce a small helper that temporarily switches `GIO_LAUNCHED_DESKTOP_FILE` and `GIO_LAUNCHED_DESKTOP_FILE_PID` to the main desktop entry (`/usr/share/applications/io.github.codegoddy.apexshot.desktop`) for portal-driven capture operations.
- Apply the helper only around screenshot/ScreenCast portal calls in the Wayland capture backend and Wayland recording flow.
- Restore the previous environment after the portal operation completes so unrelated daemon behavior is not affected.

## Scope
In scope:
- Wayland screenshot portal requests
- Wayland ScreenCast portal requests used for screenshot/window capture fallback
- Wayland recording ScreenCast requests
- Unit tests for temporary environment override and restoration

Out of scope:
- Hotkey portal identity
- Autostart desktop file behavior
- GNOME shell integration behavior

## Error handling
- If the main desktop file is not present, fall back to a generated desktop entry for `io.github.codegoddy.apexshot` on Wayland.
- If no desktop file can be resolved, leave the environment untouched.
- Environment restoration must happen even when portal calls fail.

## Testing
- Unit test that the helper overrides the portal identity to the main app desktop entry.
- Unit test that the helper restores previous env vars on drop.
- Unit test that existing pre-set non-capture env values are preserved after restoration.

## Success criteria
- Screenshot/recording portal calls no longer inherit the autostart daemon identity.
- Daemon autostart and hotkeys remain unchanged.
- New tests pass.
