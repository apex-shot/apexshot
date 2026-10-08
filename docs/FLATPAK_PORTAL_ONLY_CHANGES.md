# Flatpak runtime boundaries

The filename is retained for existing links. The earlier portal-only edition
that omitted the capture helper and GNOME integration is superseded by
[FLATPAK_STRATEGY.md](FLATPAK_STRATEGY.md).

## Build and identity

```sh
cargo build --release --locked --features flatpak
cargo build --release --locked
```

The first command enables sandbox integration; the second retains native
defaults. Both build the Qt capture helper. Default Tesseract support is
retained and packaged in Flatpak; `--no-default-features` is not the normal
Flatpak packaging command.

`app_identity::portal_only()` is the existing sandbox gate. It restricts
capture authorization and host operations; it no longer means that all bundled
helpers or GNOME D-Bus calls must be disabled.

Flatpak installs the binary and helper under `/app/bin`, with its desktop file
under `/app/share/applications`. Its daemon uses
`org.apexshot.ApexShot.Daemon`, while the native daemon continues to use
`org.apexshot.Daemon`. Object paths and interfaces remain compatible.
Flatpak capture IPC sockets live under
`$XDG_RUNTIME_DIR/app/org.apexshot.ApexShot`; native socket locations are unchanged.

## Capture and recording

`capture_overlay` uses the bundled helper for ApexShot's capture controls,
selection, crosshair, and recording entry points. If a sandbox lacks the helper,
still-image requests can fall back to the desktop Screenshot selector; a missing
helper is a packaging error, not the supported release configuration.

The helper obtains still pixels through the Screenshot portal. In the sandbox,
portal rejection never falls through to `QScreen::grabWindow`. Rust's sandbox
still paths likewise do not retry native capture or open a second ScreenCast
dialog after rejection. Native backend ordering is unchanged.

Wayland video recording uses ScreenCast and the portal-authorized PipeWire FD.
Sandbox builds do not enable `wf-recorder` or experimental compositor-private
recording, even when their native opt-in environment flags are present.
Area recording crops the authorized stream inside ApexShot.

## Desktop integration

Global shortcuts use the portal rather than direct GNOME Shell accelerator
calls. The sandbox's identity comes from Flatpak, so it does not register with
the unsandboxed host Registry. The Background portal grants login autostart;
denial is returned rather than falsely reported as success.

GNOME companion APIs use typed D-Bus, scoped permissions, and a dedicated host
connection. The extension is installed and enabled on the host. Sandbox window
identities are matched independently of PID namespaces. See the testing guide
for extension installation and verification.

The sandbox talks only to `org.apexshot.ApexShot.ShellOverlay` and
`org.apexshot.ApexShot.WindowList`. Updated extensions also retain the legacy
names for native clients, but the Flatpak never grants access to those names;
this prevents a still-installed older extension from widening the sandbox's
access through GNOME Shell's shared connection.

GDK clipboard providers remain available inside Flatpak. Background copies use
bundled `wl-copy`/`xclip` clients so the selection remains served after the copy
call returns. File chooser and OpenURI operations use GTK/GIO's portal support.
`pactl` is an in-sandbox client of the explicitly permitted audio socket.

## Host operations stay outside the package

Host package installation, compositor keybinding file writes, browser manifest
registration, and native update scripts are rejected. The native messaging
protocol itself can run inside Flatpak when a host browser launches it through
the optional, explicitly installed bridge. Both CLI registration and protocol
auto-registration are guarded at the file-writing boundary.

Flatpak does not display native-release updater prompts or try to spawn a host
terminal. Updates are managed by the configured Flatpak repository/software
center. Telemetry remains off by default for the sandbox build.

The tested GNOME 46.2 portal stack exports the Settings interface when its
backends are running. Automatic `System` theme following depends on this host
capability; explicit light and dark choices remain available if the portal is
missing. Do not grant dconf access to emulate a missing portal.

## Verification

Use [FLATPAK_TESTING.md](FLATPAK_TESTING.md) for the acceptance matrix.
Compile-time checks and mock D-Bus tests do not prove capture works through a
real desktop portal. Record the tested host, runtime, display server, companion
version, consent outcome, and actual output files before declaring a release
ready.
