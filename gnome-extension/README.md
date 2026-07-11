# ApexShot GNOME Helper (GNOME Extension)

This GNOME extension supports ApexShot GNOME Wayland integrations:

- keeping ApexShot screenshot preview / editor windows on top during drag operations
- drawing a shell-managed dimmed recording mask around the selected recording area
- shell-side recording controls (pause / stop / timer) via D-Bus

## Why This Extension?

On GNOME Wayland, the capture preview overlay can get lowered by the window manager when you drag it to other applications (e.g., to drop a screenshot in Discord). This extension listens for D-Bus signals from ApexShot and applies always-on-top stacking to the preview window.

For recording, ApexShot cannot reliably keep a live dimmed fullscreen mask above the desktop using normal GTK windows. The extension exposes a small D-Bus API that lets ApexShot ask GNOME Shell itself to render the recording mask.

## Installation

### Method 1: Extension Manager
1. Open GNOME Extension Manager (or Extensions app)
2. Click "+" and select the `metadata.json` file from this folder
3. Enable the extension

### Method 2: Manual (Terminal)
```bash
# Create extension directory
mkdir -p ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io

# Copy extension files
cp -r . ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/

# Restart GNOME Shell (Alt+F2, type "r", Enter) or log out/in
```

### Enable the Extension
```bash
gnome-extensions enable apexshot-gnome-integration@apexshot.github.io
```

## Requirements
- ApexShot built with D-Bus integration (default build)
- GNOME Shell 45–50 (see `metadata.json` `shell-version`)
- D-Bus session bus available

## Troubleshooting

### Preview still doesn't stay on top
- Check that the extension is enabled: `gnome-extensions list --user`
- Check logs: `journalctl --user -f | grep apexshot`
- Make sure D-Bus signals are being sent (check with `busctl monitor --session`)

### Recording mask does not appear
- Check that the extension is enabled and reloaded
- Check that ApexShot is running on GNOME Wayland
- Monitor the session bus for `org.apexshot.ShellOverlay`
- Make sure the ApexShot daemon log does not show a shell-mask fallback message

### Extension not loading
- Verify the UUID matches: check `metadata.json` uuid matches the directory name
- Try restarting GNOME Shell

## Uninstall
```bash
gnome-extensions disable apexshot-gnome-integration@apexshot.github.io
rm -rf ~/.local/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
```

## How It Works

1. For screenshot previews and annotate editor windows, ApexShot emits `TrackedWindowOpened` / `TrackedWindowClosed` signals on `org.apexshot.TrackedWindow`
2. The extension tracks matching ApexShot windows and keeps them above other windows while they are active
3. For recording masks, ApexShot calls `ShowMask(x, y, width, height)` on `org.apexshot.ShellOverlay`
4. The extension creates shell-managed dim regions around the selected recording area
5. When recording ends or errors out, ApexShot calls `HideMask()` and the extension removes the mask
