# Fedora/KDE Overlay + Preview Progress

_Last updated: 2026-06-23_

## Goal
Stabilize ApexShot behavior for Fedora KDE Plasma and related Linux desktop paths without breaking:
- GNOME extension always-on-top behavior
- preview reopen behavior (`show-last-preview`)
- existing daemon preview management
- working Arch wlroots Rust overlay path

---

## What has been done so far

### 1. KDE native screenshot backend added
Implemented a new backend:
- `src/backend/kde_screenshot.rs`

This backend talks to:
- `org.kde.KWin.ScreenShot2`

Supported work added for:
- workspace capture
- area capture
- interactive window capture

### Current status of KDE native backend
- The backend is implemented.
- It can reach KWin correctly.
- On this Fedora KDE session, KWin returns authorization errors for helper/test binaries.
- The code now falls back cleanly when KDE native authorization is missing.

### Important product decision
For the actual screenshot UI path on Fedora KDE, we are **not depending on this backend right now** because the working user path is already the C++ overlay + portal capture flow.

---

### 2. Desktop-entry authorization metadata added
Added KDE restricted-interface metadata to desktop entries and generated launchers.

Updated:
- `packaging/apexshot.desktop`
- `packaging/apexshot-daemon.desktop`
- dev desktop-entry generation in `src/main.rs`
- autostart desktop-entry generation in `src/settings/windowing.rs`

Added key:
- `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`

---

### 3. Overlay routing policy changed
Overlay routing was refined so the Rust GTK layer-shell selector is now reserved only for the platform where it is known to work well.

#### Current intended routing

##### Rust overlay path
Use Rust GTK selector only for:
- Arch-based distro
- wlroots-style compositor
- especially Hyprland / Sway

##### C++ overlay path
Use C++ overlay for:
- Fedora KDE
- Ubuntu GNOME
- Arch GNOME
- KDE generally
- non-wlroots desktops
- other non-Arch paths

Updated:
- `src/capture_overlay.rs`

Tests updated accordingly.

---

### 4. Fedora KDE real-world validation done
Validated the actual user-facing screenshot path on Fedora KDE.

#### Confirmed working
- `capture crosshair`
- `capture area`

These use:
- **C++ overlay for selection**
- **XDG Screenshot portal for actual image capture/crop**

This matches the Ubuntu-like behavior we want for Fedora KDE.

---

### 5. Crosshair screenshot UI-remnant bug fixed
A bug was observed where remnants of the crosshair UI sometimes appeared in the final screenshot.

Likely cause:
- screenshot requested too soon after overlay hide/unmap on Wayland

Fix applied:
- added a small post-hide Wayland delay before final capture in the C++ overlay flow

Updated:
- `capture-overlay/src/main.cpp`

#### Result
Crosshair remnants issue appears fixed in testing.

---

### 6. Preview-overlay delay investigated
Investigated why the floating preview appears later even though the file is saved quickly.

#### Findings
The screenshot file is saved quickly, but the preview appears later because:
1. a new `apexshot preview <path>` subprocess is spawned
2. that subprocess initializes GTK
3. the preview window is built and presented
4. preview image decode/texture creation happens after that

#### Instrumentation added
Added timing logs in preview startup to measure:
- when `window.present()` happens
- when preview texture becomes ready
- when deferred preview actions initialize

Updated:
- `src/capture/preview_overlay.rs`

#### Observed timing
Recent measurement on this machine/session:
- `window.present()` around **1880-1900 ms**
- synchronous texture ready around **1945-1965 ms**
- deferred actions init around **2020 ms**

#### Interpretation
The dominant delay is **before** image decode finishes.
The preview texture itself is relatively fast.
The bigger bottleneck is still the preview subprocess startup / GTK preview window setup path.

---

### 7. Preview startup work reduced safely
To improve preview startup without breaking behavior, the following safe refactors were done:

#### a. Shared preview launcher helper
Added:
- `src/preview_launch.rs`

This centralizes preview subprocess launching and fallback behavior.

#### b. Preserved subprocess architecture
The preview is still launched in its own process so we do **not** break:
- GTK isolation after capture overlay closes
- GNOME extension tracking behavior
- daemon preview single-instance behavior
- reopen/restore preview behavior

#### c. Faster image path attempted
Preview loading now tries a fast GTK-native synchronous texture load first before the slower fallback decode path.

#### d. Deferred non-critical preview actions
Some preview actions were moved to an idle callback so they no longer block the earliest possible preview startup.

This was done specifically to avoid breaking:
- preview interactivity
- drag/drop
- edit button behavior
- close behavior
- pinned/unpinned logic

---

## What remains

### 1. Main preview startup bottleneck is still not fully solved
Even after safe improvements, preview appearance is still slower than desired.

#### Current understanding
The biggest remaining delay is likely in one or more of:
- subprocess process startup
- GTK application startup
- preview window/widget construction
- layer-shell / positioning setup
- other pre-present GTK work

#### Next recommended step
Add more granular timing around:
- preview subprocess entry
- GTK app creation
- `connect_activate`
- start of `setup_preview_window`
- layer-shell positioning call
- widget construction sections
- first draw / first frame mapped

This will identify exactly which block consumes most of the ~1.9s.

---

### 2. Decide whether KDE native backend stays experimental or becomes active later
Right now Fedora KDE works through the C++ + portal flow, so there is no user-facing blocker.

Open question:
- keep `org.kde.KWin.ScreenShot2` backend as experimental future work
- or later try to fully authorize the real app identity and use it as an optimization

This is optional for current Fedora KDE screenshot functionality.

---

### 3. Clean up temporary preview timing instrumentation later
Once the preview bottleneck is understood and fixed, the temporary timing logs should be removed or gated behind a debug flag.

Current debug logs are in:
- `src/capture/preview_overlay.rs`

---

## Files touched so far

### New files
- `docs/plan-kde-native-screenshot-backend.md`
- `docs/progress-fedora-kde-overlay-and-preview.md`
- `src/backend/kde_screenshot.rs`
- `src/preview_launch.rs`

### Updated files
- `capture-overlay/src/main.cpp`
- `packaging/apexshot.desktop`
- `packaging/apexshot-daemon.desktop`
- `src/backend/mod.rs`
- `src/backend/wayland.rs`
- `src/capture/preview_overlay.rs`
- `src/capture_overlay.rs`
- `src/lib.rs`
- `src/main.rs`
- `src/pipewire_engine.rs`
- `src/settings/windowing.rs`
- `src/daemon/mod.rs`
- `tests/desktop_identity.rs`

---

## Current stable conclusions

### Fedora KDE screenshot path
Use:
- **C++ overlay**
- **portal-based actual screenshot capture**

This path is currently working.

### Arch Hyprland/Sway path
Use:
- **Rust GTK layer-shell overlay**

### Crosshair remnant bug
- fixed in current testing

### Preview delay
- partially investigated
- not fully solved yet
- biggest bottleneck appears to be preview startup before first present
