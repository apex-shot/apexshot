# ApexShot Nix package

This directory holds the Nix flake package for ApexShot, built from the
source tree in this repository. It is a source build: the Rust binary comes
from `Cargo.lock`, the Qt5 capture helper is compiled by `build.rs` through
CMake, and the working-tree files (assets, locales, GNOME extension, native
host) are installed into the store path.

Status: x86_64-linux only. The package builds under `nix build` and the
installed CLI runs headless. **The desktop runtime (capture, recording, portals, GNOME
extension, browser native host) is untested on NixOS.** Treat it as a build
artifact that still needs a desktop smoke test.

## Build

Git-based flakes only see files Git tracks, so new files must be staged
before `.#` references find them. `path:` references read the working tree
directly:

```sh
nix build "path:$PWD#apexshot"            # works with untracked files
git add flake.nix flake.lock packaging/nix && nix build .#apexshot   # after staging
./result/bin/apexshot --help
```

`nix run` runs the same binary through `apps.default`.

## Using it in a NixOS configuration

Add the flake as an input (`github:apex-shot/apexshot` or a local checkout),
then install the package declaratively:

```nix
{
  inputs.apexshot.url = "github:apex-shot/apexshot";

  outputs = { self, nixpkgs, apexshot, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        {
          environment.systemPackages = [
            apexshot.packages.x86_64-linux.apexshot
          ];
        }
      ];
    };
  };
}
```

### Portal and PipeWire

ApexShot's portal capture and recording depend on host services, which this
package does not configure. Set them in your system configuration:

```nix
{ pkgs, ... }:
{
  xdg.portal = {
    enable = true;
    extraPortals = [ pkgs.xdg-desktop-portal-gnome ];
  };

  services.pipewire = {
    enable = true;
    pulse.enable = true;
  };
}
```

`xdg.portal.enable` requires at least one implementation in
`xdg.portal.extraPortals`, and the NixOS module asserts this. The GNOME desktop
module already sets `xdg.portal.enable` and adds `xdg-desktop-portal-gnome` and
`xdg-desktop-portal-gtk`, so on GNOME you only need the explicit `extraPortals`
line if you disable that module's defaults. On wlroots, Hyprland, or KDE, use the
matching portal backend instead (`xdg-desktop-portal-wlr`,
`xdg-desktop-portal-hyprland`, or `kdePackages.xdg-desktop-portal-kde` on KDE).

`pulse.enable` runs the PulseAudio compatibility server that the `pactl` and
`paplay` clients use.

### GNOME Shell extension

The package installs the ApexShot integration extension under
`share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io`.
It is installed like any other package, per the NixOS GNOME manual's shell
extensions section. Enable it in the Extensions app, or pre-seed the default
through `services.desktopManager.gnome.extraGSettingsOverrides` on
`org.gnome.shell.enabled-extensions`. The manual notes that overrides only
change defaults, so a user-changed value wins.

### Browser native messaging host

The package ships a Chrome/Chromium native messaging manifest with its `path`
rewritten to the store path of `apexshot-native-host`:

```text
share/apexshot/native-messaging-hosts/io.github.codegoddy.apexshot.json
```

Chrome reads system-wide manifests from `/etc/opt/chrome/native-messaging-hosts/`
(Google Chrome) and `/etc/chromium/native-messaging-hosts/` (Chromium), and it
requires an absolute `path` on Linux. Link the manifest there with
`environment.etc`:

```nix
{
  environment.etc = let
    manifest = "${apexshot.packages.x86_64-linux.apexshot}/share/apexshot/native-messaging-hosts/io.github.codegoddy.apexshot.json";
  in {
    "opt/chrome/native-messaging-hosts/io.github.codegoddy.apexshot.json".source = manifest;
    "chromium/native-messaging-hosts/io.github.codegoddy.apexshot.json".source = manifest;
  };
}
```

The manifest is declarative here, so do not write a second copy into your
user config.

### Fonts and OCR languages

ApexShot's UI expects the Inter font. The Debian package depends on
`fonts-inter` and the Nix package does not install fonts. Add it with
`fonts.packages = [ pkgs.inter ];`. Fontconfig falls back to the system sans
font if it is missing.

The OCR engine uses the English traineddata bundled with the package. It is
set through `TESSDATA_PREFIX` by a default in the wrapper, so you can point
`TESSDATA_PREFIX` at a directory with more languages to override it.

## What the package contains

- `bin/apexshot`, a wrapper around the Rust binary, with `APEXSHOT_CAPTURE_BIN`
  pointing at this package's `apexshot-capture`. This keeps a different
  `/usr/bin/apexshot-capture` from another native install from being picked up.
- `bin/apexshot-capture`, the Qt5 capture overlay, built with the Qt wrapper.
- `bin/apexshot-native-host`, a launcher that runs `apexshot native-host`.
- A build with the `nix` Cargo feature. In it, `apexshot install` and
  `apexshot uninstall` refuse to copy binaries into `/usr` and point back to the
  flake (the `--no-binary` and `--autostart-only` variants still run), and
  `apexshot update` returns an error. Capture, recording, and the Fedora
  recording restriction are unchanged.
- `share/apexshot/background-images`, `share/apexshot/sounds`, and the locale
  catalogues in `share/locale`, found relative to the executable.
- The `.desktop` entry, metainfo, icons, and an autostart entry for the daemon
  under `etc/xdg/autostart`. NixOS links `etc/xdg` from system packages.

The wrapper prepends a PATH containing the tools the runtime calls: `ffmpeg`
and `ffprobe`, `pactl` and `paplay` (PulseAudio), `pw-play` (PipeWire),
`aplay`, `wl-copy` and `wl-paste`, `xclip`, `notify-send`, `gsettings` and
`gdbus`, `dbus-send`, `gtk-launch`, `curl`, `wget`, `wf-recorder`, and
`pgrep`.

These are deliberately left to the host: `gnome-extensions` ships with GNOME
Shell, and the compositor helpers (`hyprctl`, `swaymsg`, `niri`), KDE's
`qdbus`, and `systemctl` belong to the desktop session. The Debian package
also doesn't depend on them.

## Verification

The derivation sets `doCheck = false`, so `nix build` does not run the Rust
test suite: the release test harness would recompile the whole dependency
graph a second time. Run the headless unit tests on a development checkout
with `cargo test --lib distro::` (no display, no portal, no VM). The package
build and `apexshot --help` CLI were checked with Nix; the Qt capture helper
was not launched because this verification ran without a desktop session.

## Sources

Verified against the pinned nixpkgs `nixos-26.05` (commit
`7c8764b7c7b09b34f632464276218ef9090eaa11`):

- `pkgs/build-support/rust/build-rust-package/default.nix`: `buildFeatures`,
  `cargoBuildFlags`, `cargoTestFlags`, and `cargoLock`.
- `pkgs/build-support/rust/hooks/cargo-build-hook.sh` and
  `cargo-check-hook.sh`: `--target` handling and check flag order.
- `pkgs/build-support/rust/hooks/rust-bindgen-hook.sh`: `rustPlatform.bindgenHook`
  sets `LIBCLANG_PATH` and `BINDGEN_EXTRA_CLANG_ARGS`.
- `pkgs/by-name/cm/cmake/setup-hook.sh` and `nixpkgs-cmake-prefix-path.patch`:
  CMake reads `NIXPKGS_CMAKE_PREFIX_PATH`, which build.rs inherits.
- `pkgs/development/libraries/qt-5/hooks/wrap-qt-apps-hook.sh`: Qt wrapper.
- `pkgs/applications/graphics/tesseract/wrapper.nix`: `tesseract5` with
  `enableLanguages`, the `TESSDATA_PREFIX` default, and the tessdata path.
- `pkgs/development/libraries/gstreamer/core/setup-hook.sh` and
  `pkgs/build-support/setup-hooks/wrap-gapps-hook/wrap-gapps-hook.sh`:
  GStreamer and GSettings paths.
- `pkgs/by-name/gd/gdk-pixbuf/setup-hook.sh` and `librsvg/package.nix`: the
  SVG loader is merged into librsvg's loaders cache.
- `pkgs/stdenv/generic/setup.sh`: `substituteInPlace --replace-fail`.
- `nixos/modules/config/system-path.nix`: `/etc/xdg` is linked.
- `nixos/modules/config/xdg/portal.nix` and
  `nixos/modules/services/desktop-managers/gnome.nix`: portal options.
- `nixos/modules/services/desktops/pipewire/pipewire.nix`: `pulse.enable`.
- `nixos/modules/services/desktop-managers/gnome.md`: shell extensions and
  GSettings overrides.
- Chrome native messaging docs, Linux manifest locations and absolute path
  requirement: https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
