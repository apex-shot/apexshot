# Windows Support Plan

Status: in progress. The audit, the verification build, the Windows capture
backends, the Quick Access shell, and the Rust settings surface are done.
Recording follows its own plan in [WINDOWS_RECORDING_PLAN.md](WINDOWS_RECORDING_PLAN.md).

## Goal

Make ApexShot run on Windows without changing Linux behaviour. Linux stays the
primary target and must remain bit-for-bit identical in look, feel, and code
paths after every step here.

## The invariant

Every PR in this series must leave these green:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --lib
cmake -S capture-overlay -B capture-overlay/build && cmake --build capture-overlay/build
```

Plus a manual pass on GNOME Wayland and X11: area capture, window capture,
recording countdown, and scroll capture. If a change cannot be justified as
"Linux output unchanged", it does not belong in this series.

## What the audit found

`capture-overlay/` is 34 files and 10,434 lines. It is the only UI in the tree
that is plausibly portable, because it is Qt Widgets rather than GTK.

| Measure | Lines | Share |
|---|---|---|
| No Linux API referenced at all | 5,325 | 51% |
| Contains an X11 / Wayland / D-Bus reference | 5,109 | 49% |

That 49% overstates the work: it counts whole files containing any match, so a
791-line file with 120 lines of X11 scroll injection lands entirely in the
platform column.

### Verified build, not just a reading

The portable set was compiled on Windows/MSVC before writing this plan
(out-of-tree CMake project, repository untouched):

- MSVC 19.44 (VS Build Tools 2022), Qt 5.15.2 `msvc2019_64`, CMake 4.4.4,
  Ninja, ccache-compatible `sccache` 0.18.0.
- **19 of 23 translation units compile unmodified.** Only four files fail, and
  only for missing headers: `CaptureOverlay.cpp`, `CaptureOverlay_Scroll.cpp`,
  `CaptureOverlay_Window.cpp` (all `<X11/...>`) and `main.cpp` (`<unistd.h>`,
  `<fcntl.h>`).
- Cold build 21 compilations; wiping the objects and rebuilding is 20 sccache
  hits in **4.9 s**.

### Three findings that change the plan

1. **`Qt5::DBus` ships for Windows.** `Qt5DBus.lib` and its CMake config are
   present in the official Qt 5.15.2 package, so every D-Bus call in the
   overlay compiles and links there. D-Bus is a *runtime* problem (no session
   bus), not a build blocker. This removes five files from the compile blocker
   list and demotes the daemon-IPC seam.
2. **`Sandbox.h` spreads D-Bus across the UI.** Nine files include it, including
   pure-UI ones (`CaptureModeToolbar.cpp`, `MonitorPicker.cpp`,
   `CaptureOverlay_RecordingSettingsDrawing.cpp`). It is the single header that
   makes the toolbar depend on the daemon transport, so it is the first thing
   to split.
3. **`M_PI` is not defined by MSVC** without `_USE_MATH_DEFINES`. Six sites:
   `CaptureOverlay_DrawingPrimitives.cpp` (2), `RecordingControlsWindow.cpp`
   (3), and one more. A one-line fix per file, but it fails the build outright.

Also confirmed: **`Qt5::X11Extras` is linked but never used.** No `QX11Info`,
`xcb_*`, or `X11Extras` reference exists anywhere in the sources.

## Seam inventory

| # | Seam | Where | Linux-only lines | Kind | Windows replacement |
|---|---|---|---|---|---|
| S1 | Screen capture | `ScreenCapture.cpp/.h` | 481 | compile + runtime | Windows.Graphics.Capture, DXGI fallback |
| S2 | Window enumeration + focus | `CaptureOverlay_Window.cpp` | ~250 of 609 | compile | `EnumWindows` + DWM cloaked probing |
| S3 | Work area / geometry | `CaptureOverlay.cpp` X11 helpers | ~130 of 541 | compile | `EnumDisplayMonitors`, `GetMonitorInfo`, per-monitor DPI |
| S4 | Scroll injection | `CaptureOverlay_Scroll.cpp` | ~120 of 791 | compile | `SendInput` with `MOUSEEVENTF_WHEEL` |
| S5 | Daemon IPC | 5 files, ~15 call sites | ~200 | **runtime only** | Named pipe, same JSON-RPC shape |
| S6 | Entry point + worker mode | `main.cpp` | ~200 of 1,490 | compile | `wWinMain`, no `dup2`; drop worker mode |
| S7 | Build system | `CMakeLists.txt` | - | compile | Conditional `find_package` + split source lists |

Net compile work is roughly **700-800 lines across four files**, not the
1,200-1,800 the static read suggested. Everything else is runtime behaviour.

## Layout

Platform code gets its own folders. Portable code never gains a `_windows` copy.

```
capture-overlay/src/
  platform/
    platform.h              # CaptureSource, WindowList, WorkArea,
                            # PointerWheel, ShellLink
    linux/
      screen_capture.cpp
      window_list.cpp
      work_area.cpp
      pointer_wheel.cpp
      shell_link.cpp
    windows/
      screen_capture.cpp
      window_list.cpp
      work_area.cpp
      pointer_wheel.cpp
      shell_link.cpp
```

Rules:

- Identical filenames per platform, so a missing implementation is a link error
  rather than a silent no-op.
- No `#ifdef` inside portable files after the seam PRs land. The build picks the
  platform source list instead.
- Linux files move, they are not rewritten. Moves happen in their own PR so
  review diffs stay mechanical.

## PR sequence

Titles match the shape of merged subjects on `main`: plain, sentence case,
imperative, no type prefix.

1. **Make the capture overlay compile on MSVC.** Define the missing math
   constant, drop the unused X11Extras link, and split `Sandbox.h` so the UI no
   longer includes the daemon transport. No behaviour change on Linux; this PR
   is provable by the verification build above.
2. **Split the capture overlay's source lists by platform.** CMake only.
   Portable, Linux, and Windows lists; no file moves.
3. **Move the overlay's Linux platform code into folders.** Mechanical `git mv`
   of S1-S4 into `platform/linux/`, sources updated. Review diff should be
   almost entirely renames.
4. **Add the Windows capture, window list, work-area, and input backends.** New
   code under `platform/windows/`, WGC-based. No Linux file changes.
5. **Scope the Rust dependencies to the Linux target.** Target-scoped dependency
   tables and a platform switch in `build.rs`. This is the PR that makes
   `cargo check --target x86_64-pc-windows-msvc` possible, and the highest-risk
   step for Linux, so it lands alone.

Runtime work that is not a build dependency, and can follow: the daemon
transport (S5), clipboard, global hotkeys (`RegisterHotKey`), notifications,
and whatever replaces the GTK settings surface.

## Local Windows toolchain

Installed for the verification build, reusable for every later step:

| Piece | Version / path |
|---|---|
| Compiler | MSVC 19.44, VS Build Tools 2022 |
| Qt | 5.15.2 `msvc2019_64` → `C:\Qt\5.15.2\msvc2019_64` |
| CMake | 4.4.4 |
| Ninja | 1.13 (WinGet package path) |
| Cache | `sccache` 0.18.0 → `SCCACHE_DIR=C:\dev\.cache\sccache`, 20 GB |

`RUSTC_WRAPPER` is set to `sccache.exe`, so Rust builds are cached too.
The out-of-tree verification project lives in the session temp directory, not
in this repository.

Note: use `/Z7` or a Release configuration with sccache. Debug `/Zi` makes
parallel `cl.exe` runs collide on `vc140.pdb` (C1041).

## Settings surface

`windows-ui/` is a small Rust crate that builds `apexshot-settings.exe` with
GTK4. It includes the Linux settings pages through `#[path]` rather than
duplicating them, and supplies the Windows-only pieces (DPI handling, font
registration, login autostart, Cloud/Studio facades, and the window shell).

- The window is borderless with the app's own title bar, rounded corners, and a
  single outline.
- The full page list is present: General, Shortcuts, Quick Access, Recording,
  Screenshots, Annotate, Cloud, Advanced, Studio, About. Pages whose runtime is
  not implemented on Windows say so at the top and still persist their config.
- Theme (System/Light/Dark) follows the Windows app theme for System and
  applies immediately to the settings window.
- Capture sounds are enabled: `assets/sounds/*.wav` are played by the C++ shell
  through `PlaySoundW` when a capture freezes.
- Saves write the usual YAML config plus `%APPDATA%/apexshot/settings.json`.
  The C++ shell reads that JSON (`settings_store.cpp`) so the two processes
  agree without a YAML dependency in Qt.

Build (MSVC GTK4 from the gvsbuild release archive):

```powershell
. C:\dev\tools\devshell.ps1
$env:PKG_CONFIG_PATH = 'C:/gtk/lib/pkgconfig'
$env:PKG_CONFIG = 'C:\gtk\bin\pkg-config.exe'
$env:PATH = "C:\gtk\bin;$env:PATH"
cargo build --manifest-path windows-ui/Cargo.toml
```

The Qt capture shell launches the resulting executable from the tray's
**Settings...** action, preferring `APEXSHOT_SETTINGS`, then a sibling
`apexshot-settings.exe`.

## Deliberately out of scope

- The GTK image editor, preview overlay, and onboarding. The settings surface
  is the first Rust UI on Windows; the others need their own port.
- GNOME extension, Flatpak, and Nix packaging.
- Recording internals, which follow their own plan.

## Open decisions

1. **Worker mode.** Drop it on Windows, or reimplement it over a named pipe.
   Dropping is cheaper and only costs the batch capture path.
2. **Qt 5 or Qt 6** for the Windows build only. Qt 5 matches the repo today;
   Qt 6 is the longer-lived choice. Mixing versions across platforms is fine as
   long as the portable sources avoid version-specific APIs.

Settled: MSVC over MinGW (already installed, and the Rust toolchain is
`stable-x86_64-pc-windows-msvc`), Qt 5.15.2 over Qt 6 for the first cut.
