# ApexShot Preview Helper (GNOME Extension)

This GNOME extension keeps ApexShot's screenshot preview overlay on top of other windows during drag operations on GNOME Wayland.

## Why This Extension?

On GNOME Wayland, the capture preview overlay can get lowered by the window manager when you drag it to other applications (e.g., to drop a screenshot in Discord). This extension listens for D-Bus signals from ApexShot and applies always-on-top stacking to the preview window.

## Installation

### Method 1: Extension Manager
1. Open GNOME Extension Manager (or Extensions app)
2. Click "+" and select the `metadata.json` file from this folder
3. Enable the extension

### Method 2: Manual (Terminal)
```bash
# Create extension directory
mkdir -p ~/.local/share/gnome-shell/extensions/org.apexshot.PreviewStacking@apexshot.github.io

# Copy extension files
cp -r . ~/.local/share/gnome-shell/extensions/org.apexshot.PreviewStacking@apexshot.github.io/

# Restart GNOME Shell (Alt+F2, type "r", Enter) or log out/in
```

### Enable the Extension
```bash
gnome-extensions enable org.apexshot.PreviewStacking@apexshot.github.io
```

## Requirements
- ApexShot built with D-Bus integration (default build)
- GNOME Shell 45, 46, or 47
- D-Bus session bus available

## Troubleshooting

### Preview still doesn't stay on top
- Check that the extension is enabled: `gnome-extensions list --user`
- Check logs: `journalctl --user -f | grep apexshot`
- Make sure D-Bus signals are being sent (check with `busctl monitor --session`)

### Extension not loading
- Verify the UUID matches: check `metadata.json` uuid matches the directory name
- Try restarting GNOME Shell

## Uninstall
```bash
gnome-extensions disable org.apexshot.PreviewStacking@apexshot.github.io
rm -rf ~/.local/share/gnome-shell/extensions/org.apexshot.PreviewStacking@apexshot.github.io
```

## How It Works

1. When ApexShot captures a screenshot and shows the preview overlay, it emits a D-Bus signal `PreviewOpened` with the window ID
2. This extension listens on the session D-Bus for `org.apexshot.Preview` signals
3. When it receives the signal, it applies `make_above()` to the window and tracks it
4. If the window loses focus (e.g., during drag), the extension re-raises it
5. When the preview closes, it emits `PreviewClosed` and the extension removes constraints