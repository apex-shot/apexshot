# Flatpak browser capture bridge

The Flatpak app can receive full-page captures from a **host-installed** Chrome or Chromium browser through a small native-messaging launcher installed in the user's home directory. The browser sends the PNG through native messaging; the launcher starts `org.apexshot.ApexShot` with `flatpak run`, and the app saves the imported capture inside its own sandbox. No shared host directory, `--filesystem=host`, or `flatpak-spawn --host` is needed.

This registration is a host-side operation. Do not run it from the app, from a Flatpak shell, or as root. The installer refuses to run when `FLATPAK_ID` is set or `/.flatpak-info` exists.

## Install for host browsers

Install the ApexShot Flatpak first, then run this script with the **host's** Python 3 from a checkout of the repository:

```sh
python3 scripts/install-flatpak-browser-host.py
```

By default, it registers both Chrome and Chromium. Choose only one with `--browser chrome` or `--browser chromium`. The default extension ID is `kaejmfabajnakpodjffipckmcpfpdenj`; unpacked/development extensions can supply their own validated ID:

```sh
python3 scripts/install-flatpak-browser-host.py \
  --browser chromium \
  --extension-id abcdefghijklmnopabcdefghijklmnop
```

The script writes the host launcher `~/.local/bin/apexshot-flatpak-native-host` and a per-user native-messaging manifest under the selected browser's `NativeMessagingHosts` directory. It uses `XDG_CONFIG_HOME` when set, otherwise `~/.config`. Close and reopen the browser after installation if it was already running.

## Replacing a native browser registration

The native and Flatpak apps use the same native-messaging host name, so a browser can register one implementation at a time. If a selected per-user manifest or the Flatpak bridge launcher already exists with different contents, the installer refuses to overwrite it. Inspect the reported path before opting in:

```sh
python3 scripts/install-flatpak-browser-host.py --replace
```

`--replace` switches only the selected per-user browser manifest to the Flatpak launcher. It does not remove the native app, change the native `apexshot-native-host` launcher, edit system-wide manifests, or change other browser profiles. To switch that browser back to a native installation, use the native app's host installer again. Do not delete a system package's registration files to switch this per-user bridge.

To remove this registration manually, first inspect the manifest and confirm its `path` is `~/.local/bin/apexshot-flatpak-native-host`; then remove only the selected user manifest(s) and that distinct launcher. This script does not uninstall or alter the native app.

## Flatpak-installed browsers

This script registers **host** browser profiles only. A browser installed as a Flatpak cannot see the host profile manifest or launch this host executable from its sandbox. Supporting a sandboxed browser requires that browser's supported native-messaging broker/proxy integration; this installer does not set that up and does not grant a generic host native-messaging D-Bus permission. Do not work around this limitation with broad filesystem access or by exposing a generic host proxy to the app.

Until a specific sandboxed-browser integration is supported and reviewed, there is no automatic import path from that browser into the Flatpak app. If the browser or another capture tool can save the image to a file, open that file from ApexShot with the file chooser. The chooser grants access only to the image the user selects; the current web-scroll extension does not provide a separate Flatpak download fallback.

## Protocol and security

The launcher contains only an `exec` of the absolute host `flatpak` executable. It forwards stdin/stdout directly so the native-messaging length-prefixed JSON protocol remains intact, and it does not forward the browser's origin argument or print to stdout. The manifest allows only the configured extension ID. The app import command accepts the capture payload and saves it inside the Flatpak sandbox; the bridge does not expose host filesystem paths or install anything from inside the app.

The browser's native-messaging manifest format requires a host executable path and an extension-origin allowlist; the host browser starts that executable. Native messaging requests are still untrusted input, and an extension ID allowlist is not a substitute for validating the payload. The browser's native-messaging transport also has per-message size limits, so very large full-page images can exceed what the browser will send.

References: [Chrome native messaging host manifests and protocol](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging), [Flatpak file chooser portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html), and the [Flatpak native-messaging proxy security notes](https://github.com/flatpak/xdg-native-messaging-proxy).
