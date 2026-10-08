# Flatpak acceptance checks

Run these against a disposable desktop VM, not an existing user's capture
configuration. Build/install instructions are in [flatpak/README.md](../flatpak/README.md).
This is a test plan, not a claim that every row has passed.

## GNOME companion installation

Flatpak does not install the extension into the host. For a development test,
package the extension from the same checkout as the app, then run these commands
in a host terminal:

```sh
gnome-extensions pack gnome-extension --force \
  --extra-source=cursor-classifier.js \
  --extra-source=press-tracker.js \
  --extra-source=shell-overlay.js \
  --extra-source=window-list.js \
  --extra-source=preview-stacking.js \
  --extra-source=daemon-ownership.js
gnome-extensions install --force \
  apexshot-gnome-integration@apexshot.github.io.shell-extension.zip
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

Use a GNOME version listed in `gnome-extension/metadata.json`. A new Wayland
extension installation may require logging out and back in. Do not install
an unpublished development extension into a real user's session merely to
make a package test pass.

Release users install the separately published `apexshot-gnome-integration.zip`
from ApexShot's releases with the host `gnome-extensions` tool. A released older
extension does not contain unreleased sandbox changes; test the matching source
archive until a compatible extension release exists.

## Required behavior matrix

| Check | Acceptance |
| --- | --- |
| Offline build | All crates and module sources declared; build commands require no network |
| Install and foreground launch | Onboarding/settings, image editor, and video editor render |
| Screenshot consent | First request prompts when required; approved capture produces a valid image |
| Screenshot refusal | No image and no direct capture/second permission prompt after cancel/deny |
| Capture controls | Area, crosshair, fullscreen, Quick Capture, OCR/QR, and GUI window selection work or state a desktop capability limit |
| GNOME companion enabled | Mask, preview positioning/stacking, window picker, pointer/click tracking work inside Flatpak |
| GNOME companion disabled | App remains usable; optional integrations fail safely without claiming installation |
| D-Bus isolation | Companion names have a different owner connection from `org.gnome.Shell`; sandbox cannot call Shell Eval |
| Native coexistence | Starting Flatpak does not replace the native daemon or use its capture-helper sockets |
| Recording consent | Authorized ScreenCast produces decodable video with the chosen source/crop |
| Recording controls | Countdown, stop/save, pause/resume/restart, pointer sidecar, and editor playback/export remain coherent |
| Audio | Mic and speaker selection/metering work through the allowed socket; output contains requested audio |
| Clipboard | Text, PNG, and saved-file references paste into another app, including after the capture worker returns |
| Files | Default Pictures/Videos save works; outside paths use file chooser grants, including reopen/export |
| Shortcuts | Granted bindings activate; unsupported/conflicting/denied bindings do not disable tray/foreground capture |
| Background | Autostart consent and revocation are reflected honestly; denial leaves foreground use intact |
| Browser bridge | Host browser ping/import produces framed protocol replies and opens the image; installer refuses existing native registration by default |
| Offline launch | Settings/editors and bundled OCR remain useful without cloud/model downloads |
| System theme | System follows the host color scheme published by GTK or the Settings portal. If both are unavailable, use the runtime theme; explicit light/dark selections remain usable without dconf access. |
| Native regression | Default-feature tests and relevant lints remain green; native desktop identities/backends remain unchanged |

Record results separately for GNOME Wayland, GNOME X11, KDE Wayland, and a
wlroots desktop with its portal backend. Portal features differ across these
hosts; one VM pass does not establish all-desktop parity. Test x86_64 and
aarch64 before advertising both architectures.

For native coexistence, compare D-Bus owners for `org.apexshot.Daemon` and
`org.apexshot.ApexShot.Daemon` and verify their separate saved/configuration paths.
Do not grant the whole session bus to get a failing companion call to pass.

## Local evidence

Keep screenshots and redacted test logs outside the source tree unless they are
intentional public release documentation. Never publish user screens, audio,
browser URLs, cloud credentials, or portal restore tokens as test evidence.
