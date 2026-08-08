# Flatpak / Portal-Only Changes

Status: in progress (plan §10.2–§10.4)  
Date: 2026-08-03  
App ID (Flatpak): `org.apexshot.ApexShot`  
App ID (native packages, unchanged): `io.github.codegoddy.apexshot`

This document describes the code and packaging changes made to support a
**portal-only Flatpak** build. Native deb/rpm/AUR builds are unchanged unless
noted.

---

## How to build

```bash
# Portal-only binary (no system Tesseract, no Qt helper)
cargo build --release --no-default-features --features flatpak

# Native binary (default — same as before)
cargo build --release
```

Flatpak packaging lives under `flatpak/`. See `flatpak/README.md`.

---

## 1. Cargo feature: `flatpak`

**Files:** `Cargo.toml`, `build.rs`

| Feature | Default? | Effect |
|---|---|---|
| `tesseract-ocr` | yes | Links system Tesseract (native packages) |
| `flatpak` | no | Portal-only mode |

`flatpak` does **not** turn off default features by itself. Always build with:

```bash
--no-default-features --features flatpak
```

### What `build.rs` does under `flatpak`

Skips compiling the Qt5/X11 C++ helper (`apexshot-capture`). That binary is not
shipped in the Flatpak and is not required at runtime in portal-only mode.

---

## 2. Runtime gate: `portal_only()`

**File:** `src/app_identity.rs`

```rust
portal_only() → true when:
  - built with --features flatpak, OR
  - FLATPAK_ID is set in the environment (real Flatpak sandbox)
```

`host_escape_blocked(what)` returns an error string when `portal_only()` is true.
Call sites use it to refuse host-only operations cleanly.

### App ID under the `flatpak` feature

| | Native | Flatpak feature |
|---|---|---|
| App ID | `io.github.codegoddy.apexshot` | `org.apexshot.ApexShot` |
| Desktop file | `…/io.github.codegoddy.apexshot.desktop` | `…/org.apexshot.ApexShot.desktop` |

Chosen to match Flathub peers (e.g. Flameshot → `org.flameshot.Flameshot`) and
because you control `apexshot.org`. Native packages keep the old ID until a
coordinated rename.

---

## 3. Host escapes blocked under portal-only

These paths **no-op or error** when `portal_only()` is true. Native builds keep
the old behavior.

| Area | File(s) | Behavior under portal-only |
|---|---|---|
| CLI `install` / `uninstall` | `src/main.rs` | Rejected with a clear error |
| Browser native-host install | `src/main.rs` | Rejected (unsupported in Flatpak v1) |
| PermissionStore mutation | `src/backend/portal_permissions.rs` | No-op (sandbox must not write portal impl store) |
| Qt helper warm-up | `src/daemon/mod.rs` | Skipped |
| Compositor hotkey install | `src/hotkeys/mod.rs` | Bail / no-op (`hyprctl`, `swaymsg`, GNOME custom keys, `gtk-launch`) |
| `wf-recorder` recording | `src/recording/mod.rs` | Disabled; ScreenCast portal + PipeWire path used instead |
| GNOME Shell dbus-send/gdbus | `src/gnome_shell.rs`, `src/gnome_integration/mod.rs` | Disabled (typed zbus IPC is a later slice) |
| In-app GNOME extension install | `src/onboarding/extensions.rs` | Opens listing URL only; no `gnome-extensions` host tool |

---

## 4. Portal / toolkit replacements

### Screenshots — XDG Screenshot portal first

**File:** `src/backend/wayland.rs`

Under portal-only:

1. **Forced first:** `org.freedesktop.portal.Screenshot`
2. **Skipped:** KDE `ScreenShot2`, wlroots `wlr-screencopy` (not available in sandbox)
3. **Fallback:** ScreenCast portal + single PipeWire frame (already existed)

Native builds keep the previous order (KDE → wlr-screencopy → portal → screencast).

### Recording — ScreenCast portal + PipeWire

**File:** `src/recording/mod.rs`

`wf-recorder` is never selected under portal-only. The existing ashpd ScreenCast
path remains the recording backend.

### Clipboard — no `wl-copy` / `xclip`

**File:** `src/utils/clipboard.rs`

| Content | Portal-only path |
|---|---|
| Text | `arboard` (already in-process) |
| Image | `arboard` after decode via `image` |
| File URI | plain URI text via `arboard` |

Native still prefers `wl-copy` / `xclip` where present.

### Notifications — D-Bus only

**File:** `src/utils/notify.rs`

Under portal-only, only `org.freedesktop.Notifications` is used. `notify-send` is
never spawned.

### Open URL / file — GIO (OpenURI portal)

**New file:** `src/utils/open.rs`

All former `xdg-open` call sites now go through:

- `utils::open::open_url` / `open_uri`
- `utils::open::open_path`

Implementation: `gio::AppInfo::launch_default_for_uri`, which uses the OpenURI
portal inside Flatpak and works off the GTK main thread.

Touched call sites include history actions, daemon open-file, cloud auth browser
launch, settings about/cloud links, onboarding, preview overlay, recording
editor “open folder”, and capture editor helpers.

### Autostart — Background portal

**File:** `src/settings/windowing.rs`

Under portal-only, “start at login” uses ashpd
`org.freedesktop.portal.Background` (`RequestBackground` with autostart +
`apexshot daemon`). It does **not** write `~/.config/autostart/*.desktop`.

Native still writes XDG autostart desktop files.

---

## 5. Telemetry default off (Flatpak)

**Files:** `src/config.rs`, `src/usage_telemetry.rs`

| Build | Default `telemetry_enabled` |
|---|---|
| Native | `true` (unchanged; still overridable) |
| `--features flatpak` | `false` (opt-in only) |

Users can still enable it in Settings. `APEXSHOT_TELEMETRY=0` still forces off.

---

## 6. OCR without system Tesseract

**Files:** `Cargo.toml`, `src/ocr/mod.rs`

- `tesseract` crate is **optional**, gated by feature `tesseract-ocr` (default on).
- Flatpak builds omit it and use the existing pure-Rust `ocrs` engine only.
- Text-region highlighter returns no boxes without Tesseract (acceptable for v1;
  ocrs boxes can be wired later).
- `ocrs` models still download at runtime into the user cache (needs network
  permission in the Flatpak).

---

## 7. Flatpak packaging skeleton

**Directory:** `flatpak/`

| File | Role |
|---|---|
| `org.apexshot.ApexShot.yml` | Manifest: GNOME Platform 49, portal finish-args, no host FS |
| `org.apexshot.ApexShot.desktop` | Desktop entry for the Flatpak app ID |
| `org.apexshot.ApexShot.metainfo.xml` | AppStream metadata |
| `README.md` | Build prerequisites, local build, Flathub checklist |

### Manifest finish-args (permissions)

- Wayland + fallback X11, IPC, DRI  
- Network (cloud + OCR models)  
- PulseAudio (recording)  
- `xdg-pictures`, `xdg-videos`  
- Talk: StatusNotifierWatcher (tray), Notifications  

**Not requested:** `--filesystem=host`, host spawning, broad home access.

### Modules

- `gtk4-layer-shell` (pinned source) — used on wlroots; harmless on GNOME  
- `apexshot` — `cargo build --release --locked --no-default-features --features flatpak`

### Still needed before Flathub submit

1. Replace `type: dir` source with a tagged release archive + sha256  
2. Generate `flatpak/cargo-sources.json` for offline Cargo builds  
3. Run full `flatpak-builder` matrix (GNOME/KDE/wlroots, portal deny, offline)  
4. Host Flathub domain verification on `apexshot.org`  
5. Decide whether to bundle OCR models and/or an ffmpeg extension  

---

## 8. What the user experiences

### Flatpak user

- Install from Flathub (once published) as `org.apexshot.ApexShot`
- First capture/record: normal portal permission dialogs (no PermissionStore hack)
- Screenshots/recordings via desktop portals
- Clipboard and notifications work without extra host tools
- Links/files open via the portal
- “Start at login” goes through the Background portal consent UI
- Telemetry off until they turn it on
- No in-app host package install, no browser native-messaging host, no host
  GNOME extension zip install (link out instead)
- OCR works via `ocrs` (models downloaded on first use)

### Native package user

- Behavior unchanged: Qt helper, Tesseract, host hotkey snippets, autostart
  desktop files, etc. still available
- Shared code paths (`utils::open`, etc.) still work; they use GIO first which
  is fine on the host too

---

## 9. Intentionally not done yet

| Item | Notes |
|---|---|
| Full native app ID rename | Flatpak-only ID for now; rename deb/rpm/AUR in one pass later |
| Typed zbus GNOME extension IPC | Extension features disabled under portal-only until this lands |
| GlobalShortcuts-only polish | Host hotkey install gated; portal daemon already exists |
| Theme via Settings portal | `gsettings` host reads still present on native; low priority in sandbox |
| Browser full-page capture | Unsupported in Flatpak v1 (document in UI/release notes) |
| `cargo-sources.json` + live `flatpak-builder` CI | Next packaging step |
| Dual-ID migration aliases | Needed when native packages switch to `org.apexshot.ApexShot` |

---

## 10. Quick reference — important symbols

| Symbol | Location | Meaning |
|---|---|---|
| `portal_only()` | `app_identity` | Sandbox / flatpak-feature gate |
| `host_escape_blocked()` | `app_identity` | Standard error for blocked host ops |
| `utils::open::*` | `utils/open.rs` | Portal-safe URL/file open |
| `--features flatpak` | Cargo | Compile-time portal-only mode |
| `tesseract-ocr` | Cargo | System Tesseract (default on native) |

Plan tracking: `APEXSHOT_DESKTOP_IMPLEMENTATION_PLAN.md` §§10.2–10.4.
