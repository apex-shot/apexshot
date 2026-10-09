# Flatpak support strategy

Date: 2026-10-08

This replaces the paused, feature-reduced Flatpak policy from August 2026.
The goal is one ApexShot product with sandbox-appropriate integration, while
native packages retain their current behavior. Building a Flatpak is not proof
that every desktop supports every feature; release readiness requires the
runtime checks in [FLATPAK_TESTING.md](FLATPAK_TESTING.md).

## Keep the app, change the integration boundary

The image editor, video editor, annotations, export, history, cloud sharing,
OCR, QR detection, capture controls, and clipboard pipeline remain shared.
The Flatpak bundles the Qt capture helper, Tesseract, and the command-line
clients needed inside the sandbox instead of disabling these features.

Native packages keep their existing application ID, capture backends,
compositor configuration, extension installer, and package updater.
Flatpak has its own application ID, data directories, daemon name, and
capture-helper sockets so it does not take over a native installation.

| Boundary | Native package | Flatpak |
| --- | --- | --- |
| Application ID | `io.github.codegoddy.apexshot` | `org.apexshot.ApexShot` |
| Daemon bus name | `org.apexshot.Daemon` | `org.apexshot.ApexShot.Daemon` |
| Qt capture controls | Existing helper | Bundled helper |
| Still-image authorization | Existing native/portal routes | Screenshot portal |
| Wayland recording | Existing backend selection | ScreenCast portal and restricted PipeWire FD |
| Global shortcuts | Existing desktop integration | GlobalShortcuts portal, where the host implements it |
| Login autostart | Existing XDG desktop entry | Background portal consent |
| Default saving | Existing configured locations | Pictures/Videos; other paths through the file chooser |
| GNOME companion | Existing host install | Separately installed on the host, narrow D-Bus API |
| Browser companion | Existing native host | Optional host-managed Flatpak launcher for host browsers |
| Package updates | Existing native updater | Host Flatpak/software-center updates |

## Do not bypass the sandbox

Do not request the whole session/system bus, host spawning, host/home filesystem
access, or the unrestricted host PipeWire socket. A denied Screenshot request
must not retry a direct Qt screen grab. Recording uses the PipeWire remote
authorized by the ScreenCast portal, not a compositor-private capture path.

Normal bundled clients such as `pactl` and `wl-copy` run inside the sandbox;
they are not host escapes. Their audio/display connections still depend on
the explicit Flatpak permissions and the host compositor's support.

## GNOME integration is optional and host-managed

The GNOME extension remains outside Flatpak because it runs inside GNOME
Shell. Flatpak must not write the host extension directory, invoke host
extension tools, or report that opening a download installed the extension.
Onboarding detects an enabled companion by its D-Bus service and explains
host installation when it is absent.

The companion's exported APIs must use a dedicated D-Bus connection. Sharing
GNOME Shell's connection would make a seemingly narrow Flatpak `talk-name`
permission also expose other services owned by that connection. Window matching
must use the application identity and expected window role/title, not assume
that a sandbox PID equals the host PID.

Flatpak uses `org.apexshot.ApexShot.ShellOverlay` and
`org.apexshot.ApexShot.WindowList`, available only on the dedicated connection
in the updated companion. It never grants the legacy companion names. An older
installed extension is therefore unavailable to Flatpak rather than implicitly
exposing GNOME Shell. Native clients retain their legacy names.

The app must remain useful without the companion. GNOME-owned masks, pointer
tracking, window enumeration/focus, and preview stacking require the companion;
they are not capabilities supplied by the Screenshot portal itself.

## Limits to state explicitly

Portal selection and permission dialogs belong to the host desktop. Global
shortcuts, autostart, window selection, and remote-input scrolling depend on its
portal implementation. A permission denial is not a reason to request broader
access. Where remote-input scrolling is unavailable, manual scroll assistance
is the fallback.

The sandbox does not alter the host notification settings for recording's
Do Not Disturb option. Native packages retain that behavior. Direct compositor
configuration, host package installation, and native updater execution also
remain native-only operations.

The tested GNOME 46.2 portal stack exposes the host color-scheme preference
through the Settings portal when its backends are running. Automatic `System`
theme following depends on that capability; if the portal is unavailable,
users can select light or dark explicitly. A missing portal is not a reason
to grant direct dconf or unrestricted session-bus access.

Host-installed Chrome/Chromium can use the explicit bridge described in
[FLATPAK_BROWSER_BRIDGE.md](FLATPAK_BROWSER_BRIDGE.md). Flatpak-installed browsers
have a separate native-messaging boundary and are not covered by that bridge.
Do not claim browser parity without testing the browser's packaging too.

## Distribution and publication

[flatpak/README.md](../flatpak/README.md) describes the local build/install path.
The development manifest is not a Flathub submission: it uses the working tree
and includes AI-assisted packaging work. Current Flathub requirements prohibit
AI-assisted manifests and require disclosure of affected generated app material.
A maintainer must independently author the submission manifest and follow the
current policy before publication.

Publication additionally requires a tagged/checksummed app source, all build
dependencies declared for offline builds, metadata validation, supported
runtimes, and an honest desktop/architecture test matrix. No install command
should imply the app already exists on Flathub.

## Primary references

- [Flatpak sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
- [Portal D-Bus design constraints](https://flatpak.github.io/xdg-desktop-portal/docs/design-considerations.html)
- [Screenshot portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html)
- [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
- [Background portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Background.html)
- [Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
