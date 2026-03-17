# GNOME Extension Integration for Always-On-Top Preview

**Date:** 2026-03-17  
**Status:** Draft

## 1. Problem Statement

On GNOME Wayland, the capture preview overlay fails to stay on top of other windows. The current `gtk4_layer_shell` approach doesn't provide reliable always-on-top behavior. When users drag the screenshot to other applications (e.g., Discord), the preview gets lowered by GNOME's window manager.

## 2. Solution Overview

Use a GNOME extension as a "D-Bus client" that listens for signals from the apexshot app. When the app creates the preview window, it emits a D-Bus signal with the window ID. The extension immediately applies GNOME-specific stacking constraints to keep that window on top.

## 3. Architecture

```
┌─────────────────────┐     PreviewOpened(xid)      ┌─────────────────────┐
│   apexshot (Rust)   │ ───────────────────────────→│  GNOME Extension    │
│   - zbus server     │     D-Bus signal             │  - D-Bus listener  │
│   - emits signal    │                               │  - MetaWindow API  │
└─────────────────────┘                               └─────────────────────┘
        │                                                     │
        │ Shows preview window                              │ Applies:
        │ emits PreviewOpened                                │ - StackingOrder.FOCUSED
        ▼                                                     │ - skip-focus-out
                                                              ▼
                                                    ┌─────────────────────┐
                                                    │ Window stays on top │
                                                    └─────────────────────┘
```

## 4. D-Bus Interface

### Bus Details
- **Bus Type:** Session Bus
- **Service Name:** `org.apexshot.Preview`
- **Object Path:** `/org/apexshot/Preview`

### Signals

#### `PreviewOpened`
Emitted when the preview window is created and visible.

| Field    | Type   | Description                |
|----------|--------|----------------------------|
| `xid`    | `u32`  | X11 window ID (X11)        |
| `window_id` | `u32` | Wayland surface ID (Wayland) |

#### `PreviewClosed`
Emitted when the preview window is destroyed/closed.

| Field | Type   | Description                |
|-------|--------|----------------------------|
| `xid` | `u32`  | The same ID from PreviewOpened |

### Method

#### `GetVersion`
Returns the protocol version for compatibility checking.

| Returns | Type   | Description              |
|---------|--------|--------------------------|
| version | `u32`  | Current version (1)      |

## 5. Implementation: Rust App Side

### Dependencies
```toml
[dependencies]
zbus = "4"
```

### Code Changes
1. Create a `zbus` server that owns `org.apexshot.Preview`
2. In `preview_overlay.rs::setup_preview_window()`, after window is shown:
   - Get window's XID (X11) or meta_window (Wayland)
   - Emit `PreviewOpened` signal with the ID
3. On window close/destroy, emit `PreviewClosed`
4. Wrap in feature flag `gnome-integration` for optional build

### X11 Window ID Extraction
```rust
// For GTK4 on X11
use gdk4x11::X11Surface;
let xid = window.surface()?.downcast::<X11Surface>()?.xid();
```

### Wayland Window ID Extraction
```rust
// For GTK4 on Wayland (via GNOME MetaWindow)
use gtk4::glib::Object;
let meta_window = window.meta_window(); // MetaWindow from gdkwayland
```

## 6. Implementation: GNOME Extension Side

### Files Structure
```
gnome-extension/
├── extension.js          # Main extension entry point
├── dbus.js               # D-Bus client wrapper
├── metadata.json         # GNOME extension metadata
└── stylesheet.css        # Optional styling
```

### metadata.json
```json
{
  "uuid": "org.apexshot.PreviewStacking@apexshot.github.io",
  "name": "ApexShot Preview Helper",
  "description": "Keeps ApexShot preview windows on top during drag operations",
  "version": "1",
  "shell-version": ["45", "46", "47"],
  "url": "https://github.com/codegoddy/apexshot"
}
```

### D-Bus Client (dbus.js)
- Connect to `org.apexshot.Preview` on session bus
- Subscribe to `PreviewOpened` signal
- On signal: find window by ID, apply stacking constraints
- Subscribe to `PreviewClosed`: remove constraints

### Window Stacking API
```javascript
// Using GNOME MetaWindow API
const metaWindow = global.display.get_window_actors().find(
  w => w.get_meta_window().get_id() === xid
)?.get_meta_window();

if (metaWindow) {
  // Move to FOCUSED layer
  metaWindow.get_group().raise(metaWindow);
  
  // Make it "unstealable" - ignore focus-out
  metaWindow.make_above();
  metaWindow.skip_taskbar = true;
  metaWindow.skip_pager = true;
}
```

### Drag-Drop Protection
The extension should set a flag to prevent GNOME from lowering the window when focus is lost:

```javascript
// In extension.js - watch for focus-out events
metaWindow.connect('focus-out', () => {
  if (this._previewWindows.has(xid)) {
    // Re-raise the window
    metaWindow.raise();
    metaWindow.make_above();
  }
});
```

## 7. Fallback Behavior

### If Extension Not Installed
The Rust app should detect if the D-Bus service exists:
1. On startup, try to acquire `org.apexshot.Preview` name
2. If failed (extension not running), continue without D-Bus
3. Log a debug message: "GNOME extension not detected, using standard positioning"

### No Signal Emission
If layer-shell works, still emit signal for consistency (optional).

## 8. Error Handling

- **D-Bus connection failed:** Log warning, continue with standard positioning
- **Window ID not found:** Extension logs debug, does nothing
- **Extension disabled:** App works normally without it

## 9. Testing

### Unit Tests (Rust)
- Mock D-Bus server emission
- Test signal serialization

### Integration Tests
1. Install extension manually
2. Take screenshot
3. Verify preview appears on top
4. Switch workspaces - preview should remain visible
5. Start dragging to another app - preview stays on top

### Manual Test Checklist
- [ ] Preview appears on top of other windows
- [ ] Preview survives workspace switch
- [ ] Drag-and-drop works without preview lowering
- [ ] Closing preview removes constraints
- [ ] Works on GNOME 45, 46, 47

## 10. Security Considerations

- D-Bus uses session bus (user-level isolation)
- No privileged operations required
- Extension only modifies window it creates via signal

## 11. Future Extensions

- **Editor window support:** Add `EditorOpened` signal for editor window always-on-top (optional toggle in settings)
- **Multi-monitor:** Extend signal to include monitor ID for correct positioning
- **Configurable behavior:** Let users choose via app settings

## 12. Out of Scope

- Editor window always-on-top (handled as standard window)
- KWin, Plasma integration (future work)
- Snap/flatpak sandbox restrictions (may need D-Bus policy configuration)