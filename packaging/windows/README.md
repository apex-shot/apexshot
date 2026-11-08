# Windows local installer

`scripts/package-windows.sh` assembles the Windows app tree and compiles it into
`ApexShot-<version>-setup.exe` with Inno Setup. Windows users install ApexShot
the way they install any other program: a single `.exe`, a per-user install
under `%LOCALAPPDATA%\Programs\ApexShot`, a Start-menu entry, an uninstaller in
Add/Remove Programs, and no elevation prompt. Linux keeps the CLI installers and
packages; this is the Windows-only counterpart.

## What a run needs

- A Rust toolchain that can link the GTK4 build `pkg-config` finds: the GNU
  `x86_64-pc-windows-gnu` toolchain with the MSYS2 MINGW64 GTK4 build (what CI
  uses), or MSVC with an MSVC-built GTK4 prefix.
- Qt 5 with `windeployqt`, found from `capture-overlay/build/CMakeCache.txt` or
  `qmake`.
- Visual Studio 2022 (the Qt capture shell builds with MSVC).
- [Inno Setup](https://jrsoftware.org/isinfo.php): `winget install JRSoftware.InnoSetup`.
- An ffmpeg payload: `ffmpeg.exe`, `ffprobe.exe`, and the shared `av*`/`sw*`
  DLLs. Put it in `dist/ffmpeg/bin` or pass `--ffmpeg-dir`.

```bash
bash scripts/package-windows.sh
```

The result lands in `dist/windows/`: the staged tree, and
`ApexShot-<version>-setup.exe` beside it. `--skip-build` reuses the release
binaries and the Qt shell already in place; `--skip-installer` stops after
staging, which is the fast loop while working on the layout.

## CI and supported architectures

The `windows` job in `.github/workflows/release.yml` builds this installer on a
`windows-2022` runner for every `v*` tag. It installs the GTK4 stack from
MSYS2 MINGW64, the GNU Rust toolchain, Qt 5.15.2 (MSVC 2019 64-bit), Inno Setup,
and a staged ffmpeg payload, then uploads `ApexShot-<version>-setup.exe` as the
`windows-installer` artifact. `stage_release` validates that installer with the
other package artifacts, checksums it, and adds it to the draft release.

The package is **x86_64 only**:

- The Rust binaries target `x86_64-pc-windows-gnu`; GTK comes from MSYS2
  `MINGW64`; Qt is `win64_msvc2019_64`; the capture shell is built with
  `vcvars64`; and Inno Setup is restricted to `x64compatible`.
- 32-bit Windows is not supported: the installer refuses it and the binaries are
  64-bit.
- Native ARM64 is not built. Windows 11 ARM64 counts as `x64compatible` for Inno
  Setup, so the x86_64 installer can be installed there and the binaries run
  under x64 emulation. A native ARM64 package would need a separate ARM64
  toolchain, GTK/Qt builds, and installer.

## What the installed tree looks like

```
ApexShot/
  bin/            apexshot-settings.exe, apexshot-history.exe,
                  apexshot-video-editor.exe, apexshot-image-editor.exe,
                  apexshot-update.exe, apexshot-capture.exe,
                  ffmpeg, ffprobe, Qt and GTK DLLs
  share/apexshot/ fonts, sounds, background images
  share/          glib-2.0 schemas, icon themes, gtk-4.0 data
  lib/            gdk-pixbuf loaders
  etc/            fontconfig
```

The layout is not cosmetic. GLib, GTK, and gdk-pixbuf resolve their data as
`<dll_dir>/../share` and `<dll_dir>/../lib`, and ApexShot's own lookups resolve
`../share/apexshot` the same way, so anything moved out of `share` silently
falls back to a system font or a missing theme.

The two executables a user starts are `apexshot-capture.exe` from the Start menu
or the desktop (the tray icon, the capture menu, and the hotkeys) and
`apexshot-settings.exe`, which the installer launches once after setup, and which
the tray's Settings item opens afterwards. On a fresh install the settings binary
runs first-run onboarding and then hands over to the tray shell.

## Signing

The installer is unsigned, so SmartScreen shows "Windows protected your PC"
until the download earns reputation or the binary is signed. If a code-signing
certificate is available, sign the installer after the build:

```bash
signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 \
  /a dist/windows/ApexShot-<version>-setup.exe
```
