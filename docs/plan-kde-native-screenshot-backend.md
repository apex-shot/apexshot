# Plan: KDE Plasma Native Screenshot Backend

**Status: Implemented in tree** (`src/backend/kde_screenshot.rs`, wired from `src/backend/wayland.rs`).
Broader KDE Plasma runtime testing is still called out as development-stage in the root README.
This document is retained as design history / testing checklist.

## Goal
Add a KDE-native Wayland screenshot path that avoids the ScreenCast portal permission flow by using:

- `org.kde.KWin.ScreenShot2`

This path should be preferred on KDE Plasma Wayland for screenshot and overlay-background capture, while keeping the existing GNOME, wlroots, portal, and X11 implementations intact.

## Scope

### In scope
- Fullscreen screenshot capture on KDE Plasma Wayland
- Area capture on KDE Plasma Wayland
- Window capture on KDE Plasma Wayland
- Background capture for the Rust overlay on KDE
- Backend selection logic for KDE
- Desktop-entry authorization needed for KWin access
- Tests for KDE detection and backend selection

### Out of scope
- Replacing GNOME-specific implementations
- Removing portal or wlroots paths
- Reworking recording to use KDE-native screencast protocols

## Phase 1: KDE detection helpers

### Files
- `src/backend/wayland.rs`
- optionally `src/utils/desktop_env.rs`

### Tasks
- Add a helper such as `is_kde_wayland_session()`
- Detect:
  - `XDG_SESSION_TYPE=wayland`
  - `XDG_CURRENT_DESKTOP` contains `KDE` or `Plasma`
- Keep detection isolated so Wayland backend routing stays readable

### Done when
- ApexShot can reliably identify KDE Plasma Wayland sessions

## Phase 2: KWin ScreenShot2 backend module

### New file
- `src/backend/kde_screenshot.rs`

### Responsibilities
Wrap D-Bus calls to:
- service: `org.kde.KWin.ScreenShot2`
- object: `/org/kde/KWin/ScreenShot2`
- interface: `org.kde.KWin.ScreenShot2`

### Methods to support
- Full desktop / workspace
  - `CaptureWorkspace`
- Exact area
  - `CaptureArea(x, y, width, height, options, pipe)`
- Window selection
  - `CaptureInteractive(kind=0, options, pipe)`
- Optional later support
  - `CaptureActiveWindow`
  - `CaptureScreen`

### Implementation details
- Use `zbus`
- Pass a writable fd to KWin
- Read raw image bytes from the pipe
- Decode metadata returned in the D-Bus reply
- Convert KWin raw `QImage` output to internal `CaptureData`
- Return standard `DisplayResult<CaptureData>` values

### Options to send initially
- `include-cursor`
- `include-shadow`
- `native-resolution`
- `hide-caller-windows`

### Done when
- ApexShot can call KWin directly and receive usable screenshot data

## Phase 3: Integrate KDE backend into Wayland routing

### Files
- `src/backend/wayland.rs`

### Tasks
Prefer KDE native capture on KDE Plasma Wayland.

### Preferred order on KDE Plasma Wayland

#### Fullscreen / overlay background capture
1. KWin `ScreenShot2`
2. existing native screencopy if available
3. ScreenCast portal fallback

#### Area capture
1. KWin `CaptureArea`
2. existing full-capture + crop fallback

#### Window capture
1. KWin `CaptureInteractive(kind=0)`
2. existing portal / compositor fallback

### Important
Do not change routing for:
- GNOME
- Hyprland
- Sway
- wlroots compositors
- X11

### Done when
- KDE sessions stop going straight to ScreenCast portal for screenshot capture

## Phase 4: Overlay flow integration

### Files
- `src/capture_overlay.rs`
- `src/backend/wayland.rs`

### Tasks
Ensure KDE-native capture is used by overlay-related helpers through the Wayland backend:
- `capture_screen_for_selection_impl()`
- `capture_area_direct_impl()`
- `capture_window()`
- crosshair background capture path

### Expected behavior
- Overlay background on KDE comes from KWin native screenshot capture
- Final area/window capture uses KWin when possible
- No ScreenCast portal permission prompt for screenshot flows

### Done when
- Rust overlay screenshot flows work on KDE using the native backend

## Phase 5: Desktop-entry authorization

### Why this matters
KWin rejects unauthorized callers with errors like:
- `The process is not authorized to take a screenshot`

### Files
- `packaging/apexshot.desktop`
- `packaging/apexshot-daemon.desktop`
- dev desktop-entry generation in `src/main.rs`
- autostart desktop-entry generation in `src/settings/windowing.rs`

### Tasks
Add:
- `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`

Verify desktop identity is consistent for installed, daemon, and dev launchers.

### Done when
- Installed ApexShot presents an authorized KDE desktop identity for KWin screenshot access

## Phase 6: Error handling and fallback policy

### Files
- `src/backend/kde_screenshot.rs`
- `src/backend/wayland.rs`

### Tasks
Map KDE-native failures into clear fallbacks:
- interface missing
- access denied
- invalid area
- D-Bus transport failures
- raw-image decode failures

### Policy
- KDE native path is best-effort first on KDE Plasma Wayland
- Existing portal path remains fallback
- Other desktop paths remain unchanged

### Done when
- KDE-native failure never breaks existing capture functionality

## Phase 7: Tests

### Add tests for
- KDE Wayland detection
- KDE backend preference routing
- Non-KDE desktops not using KDE path
- invalid-area and fallback-related behavior where unit-testable
- desktop-entry metadata containing the KDE restricted-interface key

### Candidate files
- `tests/wayland_backend_test.rs`
- `tests/desktop_identity.rs`
- unit tests in `src/backend/wayland.rs`

### Done when
- KDE routing and desktop-identity regressions are covered

## Phase 8: Manual validation on Fedora KDE Plasma

### Test matrix
- Fullscreen capture
- Area capture
- Crosshair capture
- Window capture
- Multi-monitor layouts
- Fractional scaling (125%, 150%)
- Daemon-triggered capture

### Success criteria
- No ScreenCast portal permission dialog for screenshot flows
- Fullscreen, area, and window screenshot flows work
- Existing non-KDE backends still behave as before

## Current implementation status

### Implemented
- KDE Plasma Wayland session detection
- `src/backend/kde_screenshot.rs` native KWin ScreenShot2 backend
- Native Wayland routing preference for KDE screenshot flows
- Desktop-entry metadata for `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`
- Fallback handling when KWin denies authorization

### Verified on current Fedora KDE session
- KWin ScreenShot2 is present on D-Bus
- Native capture attempts reach KWin correctly
- Current dev-launched ApexShot process is still rejected with `org.kde.KWin.ScreenShot2.Error.NoAuthorized`
- Fallback to existing ScreenCast portal path still works

### Next phase
- Solve launch/desktop identity so ApexShot is actually accepted by KWin as an authorized caller
- Then re-run fullscreen, area, and window validation on the native KDE path

### Additional validation performed
- Launching the main app from a temporary desktop entry that includes `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` successfully captured a fullscreen screenshot through the existing CLI path.
- Launching the Rust overlay test binary from a temporary desktop entry with the same key still produced `org.kde.KWin.ScreenShot2.Error.NoAuthorized`.

### Interpretation
- KDE authorization is at least partially tied to desktop-entry-launched application identity.
- The main packaged app path may already be acceptable to KDE in some launch scenarios.
- Ad-hoc helper/test binaries are not authorized even when launched from a desktop entry.
- Next work should focus on routing real user-facing capture flows through the authorized main app identity rather than standalone helper binaries.

## Risks

### Authorization
The main risk is KDE restricted-interface authorization. Desktop-entry metadata must match how ApexShot is launched.

### Raw image decoding
KWin returns raw `QImage` bytes plus metadata. ApexShot must correctly handle common Qt image formats and row stride.

### Coordinate scaling
Area capture and workspace capture must behave correctly under fractional scaling and multi-monitor layouts.

## Recommended implementation order
1. Add KDE detection helper
2. Add `src/backend/kde_screenshot.rs`
3. Implement workspace capture
4. Implement exact area capture
5. Implement interactive window capture
6. Integrate KDE routing into `src/backend/wayland.rs`
7. Add desktop-entry authorization metadata
8. Add tests
9. Validate manually on Fedora KDE Plasma

## Acceptance criteria
Implementation is complete when:
- KDE Plasma Wayland screenshot flows use `org.kde.KWin.ScreenShot2`
- No ScreenCast portal permission dialog appears for screenshot capture
- Rust overlay gets its background from KDE native capture
- Fullscreen, area, and window screenshot flows work
- Existing GNOME, wlroots, portal, and X11 paths remain intact
