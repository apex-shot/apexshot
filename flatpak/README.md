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
flatpak-builder --user --install --force-clean .flatpak-builder/build \
  flatpak/org.apexshot.ApexShot.yml
flatpak run org.apexshot.ApexShot
```

The first build downloads the pinned runtime and module sources. Cargo dependencies come from the generated `flatpak/cargo-sources.json`; the build runs with `--offline`, `--locked`, and `CARGO_NET_OFFLINE=true`. After the first successful build, repeat with `--disable-download` to verify all sources are cached:

```bash
flatpak-builder --user --disable-download --install --force-clean .flatpak-builder/build \
  flatpak/org.apexshot.ApexShot.yml
```

Refresh Cargo source pins after changing `Cargo.lock` by running `flatpak/refresh-cargo-sources.sh` with `curl` and `uv` installed. It uses a fixed commit and checksum for the Cargo source generator. Do not hand-edit `cargo-sources.json`.

## Runtime contents and permissions

GNOME Platform 50 provides the `ffmpeg`, `ffprobe`, `pactl`, `curl`, `wget`, and `unzip` commands, PulseAudio client library, and common GStreamer plugins. Its Freedesktop 25.08 base automatically provides the `codecs-extra` runtime extension. The package builds the Qt 5 capture helper and its Wayland client plugin, adds `ximagesrc` for X11 recording, bundles `wl-copy`/`wl-paste` and `xclip`, and includes Tesseract with the English language data. It also installs the image editor's gradient, wallpaper, and thumbnail assets. The two `ocrs` models are pinned by checksum and copied into the app's private cache on launch, so OCR works on a first launch without model downloads.

The app has Wayland and fallback X11 display access, the PulseAudio socket, Pictures/Videos save access, network access for upload destinations, and narrowly allow-listed D-Bus names: `org.apexshot.ApexShot.ShellOverlay`, `org.apexshot.ApexShot.WindowList`, the tray watcher, and notifications. It does not grant access to the legacy extension names or GNOME Shell itself, and it does not request unrestricted host filesystem access or host command execution. Install and enable host extension version 8 or later in the GNOME session for extension integration; older releases do not receive these app-prefixed D-Bus requests.

Automatic System-theme matching depends on the host publishing its color-scheme preference through GTK or the Settings portal. The Ubuntu 24.04 test host's GNOME 46.2 portal stack exposes this preference when its portal backends are running. If the Settings portal is unavailable, System falls back to the runtime's GTK theme. Explicit Light and Dark choices remain available; the package does not request dconf or GNOME Shell bus access to work around a missing portal.

## Test coverage

The packaging workflow builds this manifest on pull requests and relevant pushes. For a desktop smoke test, confirm the app opens, use the real screenshot and ScreenCast portals, record and stop a short session, verify clipboard copy on the active display protocol, and check that the installed GNOME extension's window-list and overlay D-Bus methods are reachable. A headless `--help` invocation alone is not a desktop integration test.

This local manifest uses the working tree as its application source. Keep it local to development and testing; the maintainer must create a separate, independently authored manifest for any publication.
