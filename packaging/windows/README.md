# Windows local installer

`scripts/package-windows.sh` assembles the Windows app tree and compiles it into
`ApexShot-<version>-setup.exe` with Inno Setup. Windows users install ApexShot
the way they install any other program: a single `.exe`, a per-user install
under `%LOCALAPPDATA%\Programs\ApexShot`, a Start-menu entry, an uninstaller in
Add/Remove Programs, and no elevation prompt. Linux keeps the CLI installers and
packages; this is the Windows-only counterpart.

## What a run needs

- The MSVC Rust toolchain, and GTK4 on `PATH` for `pkg-config` (the same setup a
  developer build already uses).
- Qt 5 with `windeployqt`, found from `capture-overlay/build/CMakeCache.txt`.
- Visual Studio 2022 (the Qt shell builds with MSVC).
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

## What the installed tree looks like

```
ApexShot/
  bin/            apexshot-settings.exe, apexshot-history.exe,
                  apexshot-video-editor.exe, apexshot-image-editor.exe,
                  apexshot-capture.exe, ffmpeg, ffprobe, Qt and GTK DLLs
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
