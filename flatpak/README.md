# Flatpak local package

This manifest builds a local test package for `org.apexshot.ApexShot`; it is not a submission manifest. The manifest and packaging material here were created with AI assistance. A maintainer must independently author any submission manifest and disclose the assisted code or other material required by the current Flathub requirements.

## Build and install

Install Flatpak, `flatpak-builder`, `bwrap`, and `elfutils` (for `eu-strip`), add Flathub, then install the GNOME 50 runtime and matching SDK extensions:

```bash
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub \
  org.gnome.Platform//50 \
  org.gnome.Sdk//50 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08 \
  org.freedesktop.Sdk.Extension.llvm20//25.08
```

From the repository root, build and install the package for the current user:

```bash
export APEXSHOT_STUDIO_VERIFY_KEY='<official public verification key>'
export APEXSHOT_STUDIO_RELEASE_TIMESTAMP='<pinned official build marker>'
bash flatpak/prepare-local-build-config.sh
flatpak-builder --user --install --force-clean .flatpak-builder/build \
  flatpak/org.apexshot.ApexShot.yml
flatpak run org.apexshot.ApexShot
```

The public key and pinned release marker must match the official release
configuration; neither is a signing secret. The helper validates both using
the release gate and writes ignored local build inputs consumed by the
manifest. They are optional for this local manifest: CI passes the same
repository variables whenever the maintainer has configured them, and
otherwise builds a test package without Studio unlock configuration, where the
free program works and Studio final output is unavailable. A half-set pair is
rejected rather than silently ignored. A package built from this local
manifest remains a development test package, not a publication submission.

The first build downloads the pinned runtime and module sources. Cargo dependencies come from the generated `flatpak/cargo-sources.json`; the build runs with `--offline`, `--locked`, and `CARGO_NET_OFFLINE=true`. After the first successful build, repeat once with `--disable-download` and `--force-clean` to prove every source is cached rather than merely present in the build directory. That is the only rebuild that should clean the build directory:

```bash
flatpak-builder --user --disable-download --install --force-clean .flatpak-builder/build \
  flatpak/org.apexshot.ApexShot.yml
```

### Repeat builds reuse the cache

Do not delete `.flatpak-builder/` between builds, and do not pass
`--force-clean` on a routine rebuild. `flatpak-builder` keeps two caches there:

- `cache/` — the built result of each dependency module.
- `downloads/` — every module archive and every crate pinned in
  `cargo-sources.json`.

A second build therefore does not recompile Qt 5, Tesseract, Leptonica, the
GStreamer plugin, or the clipboard clients. Editing a module's definition or
its sources invalidates that module; a change to the top-level `build-options`
or a newer `flatpak-builder` can invalidate the cache more broadly, so expect
the first build after such a change to recompile the dependency modules. A
change to `src/` rebuilds only the application module.

For fast iteration on application code, add `--keep-build-dirs` and drop
`--force-clean`, so the application module's Cargo `target/` directory survives
and the next build is incremental instead of cold:

```bash
flatpak-builder --user --install --keep-build-dirs .flatpak-builder/build \
  flatpak/org.apexshot.ApexShot.yml
```

Add `--ccache` so a rebuilt C/C++ module reuses compiled objects: the SDK
already ships `ccache` (4.14 on GNOME 50) and its objects live in
`.flatpak-builder/ccache`. Without the flag the build points ccache at its
disabled directory and compiles from scratch.

Harnesses that redirect `XDG_DATA_HOME` (some agent and desktop harnesses do)
resolve `--user` to a private installation the desktop session cannot see, so
the package never appears to `flatpak run` or the launcher. Unset
`XDG_DATA_HOME`, `XDG_CACHE_HOME`, and `XDG_CONFIG_HOME` for the `flatpak` and
`flatpak-builder` commands, which then use `~/.local/share/flatpak`.

A redirected `XDG_CONFIG_HOME` bites a second time, on `flatpak run`. The
`xdg-pictures` and `xdg-videos` permissions are resolved through
`xdg-user-dir`, which reads `$XDG_CONFIG_HOME/user-dirs.dirs`; a redirected
config home has no such file, so those keywords mount nothing and the sandbox
silently lacks `~/Pictures` and `~/Videos`. The app then reports host files
that plainly exist as missing (`recording does not exist:
/home/<user>/Videos/clip.mp4`). Unset the same three variables for
`flatpak run` when launching from a harness like that, and confirm the sandbox
view before blaming the app:

```bash
env -u XDG_DATA_HOME -u XDG_CACHE_HOME -u XDG_CONFIG_HOME \
  flatpak run --command=sh org.apexshot.ApexShot -c 'ls ~/Videos'
```

### Build-tool version notes

`flatpak-builder` supplies the install libdir itself, defaulting to `/app/lib`
for an application, and passes `--libdir` to `meson setup`. Meson refuses the
option twice, so the meson modules must not also set `-Dlibdir=lib`. A
`flatpak-builder` old enough not to pass it falls back to meson's own default,
which is `lib64` inside the SDK, so confirm where the library landed whenever
the builder is upgraded:

```bash
find .flatpak-builder/build/files -name 'libgtk4-layer-shell.so.0'
```

Refresh Cargo source pins after changing `Cargo.lock` by running `flatpak/refresh-cargo-sources.sh` with `curl` and `uv` installed. It uses a fixed commit and checksum for the Cargo source generator. Do not hand-edit `cargo-sources.json`.

## Runtime contents and permissions

GNOME Platform 50 provides the `ffmpeg`, `ffprobe`, `pactl`, `curl`, `wget`, and `unzip` commands, PulseAudio client library, and common GStreamer plugins. Its Freedesktop 25.08 base automatically provides the `codecs-extra` runtime extension. The package builds the Qt 5 capture helper and its Wayland client plugin, adds `ximagesrc` for X11 recording, bundles `wl-copy`/`wl-paste` and `xclip`, and includes Tesseract with the English language data. It also installs the image editor's gradient, wallpaper, and thumbnail assets. The two `ocrs` models are pinned by checksum and copied into the app's private cache on launch, so OCR works on a first launch without model downloads.

The app has Wayland and fallback X11 display access, the PulseAudio socket, Pictures/Videos save access, network access for upload destinations, and narrowly allow-listed D-Bus names: `org.apexshot.ApexShot.ShellOverlay`, `org.apexshot.ApexShot.WindowList`, the tray watcher, and notifications. It does not grant access to the legacy extension names or GNOME Shell itself, and it does not request unrestricted host filesystem access or host command execution. Install and enable host extension version 8 or later in the GNOME session for extension integration; older releases do not receive these app-prefixed D-Bus requests.

Automatic System-theme matching depends on the host publishing its color-scheme preference through GTK or the Settings portal. The Ubuntu 24.04 test host's GNOME 46.2 portal stack exposes this preference when its portal backends are running. If the Settings portal is unavailable, System falls back to the runtime's GTK theme. Explicit Light and Dark choices remain available; the package does not request dconf or GNOME Shell bus access to work around a missing portal.

## Test coverage

The packaging workflow builds this manifest on pull requests and relevant pushes. For a desktop smoke test, confirm the app opens, use the real screenshot and ScreenCast portals, record and stop a short session, verify clipboard copy on the active display protocol, and check that the installed GNOME extension's window-list and overlay D-Bus methods are reachable. A headless `--help` invocation alone is not a desktop integration test.

This local manifest uses the working tree as its application source. Keep it local to development and testing; the maintainer must create a separate, independently authored manifest for any publication.
