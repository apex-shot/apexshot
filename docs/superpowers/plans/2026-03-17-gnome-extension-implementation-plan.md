# GNOME Extension Integration Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement D-Bus signaling from apexshot to notify a GNOME extension when the preview overlay appears, so the extension can apply always-on-top stacking.

**Architecture:** Rust app emits `PreviewOpened(xid)` D-Bus signal via zbus. GNOME extension listens and applies GNOME-specific window stacking via MetaWindow API.

**Tech Stack:** zbus (already in Cargo.toml), GTK4 (for window XID extraction), JavaScript (GNOME extension)

---

## Chunk 1: Rust D-Bus Server Module

**Files:**
- Create: `src/gnome_integration/mod.rs` - new module for D-Bus server
- Modify: `src/lib.rs` - register new module

- [ ] **Step 1: Create src/gnome_integration/mod.rs**

```rust
use std::sync::{Arc, Mutex};
use zbus::{Connection, ObjectServer, SignalContext};
use zbus_macros::Interface;

static PREVIEW_XID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Debug)]
pub struct PreviewService;

#[Interface]
impl PreviewService {
    pub fn xid(&self) -> u32 {
        PREVIEW_XID.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct Dbusholder {
    connection: Connection,
    #[allow(dead_code)]
    service: zbus::Service,
}

impl Dbusholder {
    pub fn new() -> Result<Self, zbus::Error> {
        let connection = Connection::session()?;

        // Request the well-known name with flags
        let service = connection
            .register_object(
                "/org/apexshot/Preview",
                PreviewService,
            )
            .map(|proxy| {
                // The register_object returns a Service handle
                // We need to own the connection to keep it alive
                // Actually, let's use a different approach
                service_builder
            })
            .unwrap();
        // Actually, let me rewrite this properly - zbus API is different
        
    }
}
```

**Correction - let's write the actual implementation:**

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use zbus::{Connection, SignalContext};

static PREVIEW_XID: AtomicU32 = AtomicU32::new(0);

pub fn emit_preview_opened(xid: u32) {
    PREVIEW_XID.store(xid, Ordering::SeqCst);
    
    if let Ok(conn) = Connection::session() {
        let _ = conn.emit_signal(
            "/org/apexshot/Preview",
            "org.apexshot.Preview",
            "PreviewOpened",
            &(xid,),
        );
    }
}

pub fn emit_preview_closed() {
    let xid = PREVIEW_XID.swap(0, Ordering::SeqCst);
    if xid == 0 {
        return;
    }
    
    if let Ok(conn) = Connection::session() {
        let _ = conn.emit_signal(
            "/org/apexshot/Preview",
            "org.apexshot.Preview",
            "PreviewClosed",
            &(xid,),
        );
    }
}
```

- [ ] **Step 2: Run test to verify it compiles**

Run: `cargo check`
Expected: PASS (no errors)

- [ ] **Step 3: Commit**

```bash
git add src/gnome_integration/mod.rs
git commit -m "feat: add D-Bus signaling module for GNOME integration"
```

---

## Chunk 2: Integrate D-Bus Emits into Preview Overlay

**Files:**
- Modify: `src/capture/preview_overlay.rs:74-100` - add signal emission after window creation
- Modify: `src/capture/preview_overlay.rs` - add signal emission on window close

- [ ] **Step 1: Read preview_overlay.rs around window creation**

Find where `window` is created and `.show()` is called.

- [ ] **Step 2: Add signal emission after window is shown**

After line ~84 where window is built, add:
```rust
// Emit D-Bus signal for GNOME extension always-on-top
if let Some(surface) = window.surface() {
    #[cfg(feature = "x11")]
    {
        use gdk4x11::X11Surface;
        if let Ok(xid) = surface.clone().downcast::<X11Surface>() {
            crate::gnome_integration::emit_preview_opened(xid.xid() as u32);
        }
    }
    #[cfg(not(feature = "x11"))]
    let _ = (surface); // Wayland: emit with different method (see next chunk)
}
```

- [ ] **Step 3: Add signal emission on window close**

Find where window is destroyed (probably `window.destroy()` or similar). Add:
```rust
// Emit D-Bus signal for GNOME extension
crate::gnome_integration::emit_preview_closed();
```

- [ ] **Step 4: Run test to verify it compiles**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/capture/preview_overlay.rs
git commit -m "feat: emit D-Bus signals for preview window lifecycle"
```

---

## Chunk 3: Wayland Support (XDG-DECORATION-FALLBACK)

**Files:**
- Modify: `src/gnome_integration/mod.rs` - add Wayland window ID extraction

Note: For Wayland, we need the MetaWindow (GNOME-specific). The simplest approach is:
1. On GNOME, we can use gdkwayland to get the `gdk_wayland_surface_get_wayland_surface`
2. Or use `wl_output` name from GTK's monitor info

For now, let's use a simpler approach - just emit with `0` for Wayland and let the extension use window matching by title, or we rely on the extension using the GTK window title.

```rust
pub fn get_window_id_for_wayland() -> Option<u32> {
    // On GNOME, we could use external tool or GTK to get meta_window ID
    None // Fallback: extension will find window by title instead
}
```

- [ ] **Step 1: Add fallback for Wayland**

```rust
pub fn emit_preview_opened_any(gtk_window: &gtk4::Window) {
    // Try X11 first
    #[cfg(feature = "x11")]
    {
        if let Some(surface) = gtk_window.surface() {
            use gdk4x11::X11Surface;
            if let Ok(xid) = surface.clone().downcast::<X11Surface>() {
                return Self::emit_preview_opened(xid.xid() as u32);
            }
        }
    }
    
    // For Wayland/GNOME - emit with special marker (0) 
    // Extension will match by window title "Screenshot"
    Self::emit_preview_opened(0);
}
```

- [ ] **Step 2: Update preview_overlay.rs to use new function**

- [ ] **Step 3: Run test**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/gnome_integration/mod.rs src/capture/preview_overlay.rs
git commit -m "feat: add Wayland support for D-Bus preview signals"
```

---

## Chunk 4: Create GNOME Extension

**Files:**
- Create: `gnome-extension/metadata.json`
- Create: `gnome-extension/extension.js` - main extension code
- Create: `gnome-extension/stylesheet.css` - optional styling
- Create: `gnome-extension/README.md` - installation instructions

- [ ] **Step 1: Create gnome-extension/ directory and metadata.json**

```json
{
  "uuid": "org.apexshot.PreviewStacking@apexshot.github.io",
  "name": "ApexShot Preview Helper",
  "description": "Keeps ApexShot preview windows on top during drag operations on GNOME Wayland",
  "version": "1",
  "shell-version": ["45", "46", "47"],
  "url": "https://github.com/codegoddy/apexshot",
  "author": "ApexShot Team"
}
```

- [ ] **Step 2: Create extension.js with D-Bus listener**

```javascript
'use strict';

const { GLib, Meta, St } = imports.gi;

let _previewXids = new Set();

function _applyStackingConstraints(metaWindow) {
    // Make window always on top
    metaWindow.make_above();
    
    // Prevent focus-out from lowering the window
    metaWindow.skip_taskbar = true;
    metaWindow.skip_pager = true;
    
    // Store that this window should stay on top
    const xid = metaWindow.get_id();
    _previewXids.add(xid);
}

function _removeStackingConstraints(metaWindow) {
    metaWindow.make_above(); // Keep above
    const xid = metaWindow.get_id();
    _previewXids.delete(xid);
}

// D-Bus client for org.apexshot.Preview
let _dbusClient = null;
let _dbusWatchId = null;

function _connectToApexshot() {
    try {
        // Use GLib.DBus to connect to the session bus
        const connection = GLib.DBusConnection.get(
            GLib.BusType.SESSION,
            null
        );
        
        if (!connection) {
            return;
        }
        
        // Subscribe to signals
        connection.signal_subscribe(
            'org.apexshot.Preview',      // bus name
            'org.apexshot.Preview',      // interface
            'PreviewOpened',              // signal name
            '/org/apexshot/Preview',     // object path
            null,                         // sender (any)
            0,                            // flags
            (connection, sender, object_path, interface, signal, params) => {
                const xid = params.get_child_value(0).get_uint32();
                _onPreviewOpened(xid);
            }
        );
        
        connection.signal_subscribe(
            'org.apexshot.Preview',
            'org.apexshot.Preview',
            'PreviewClosed',
            '/org/apexshot/Preview',
            null,
            0,
            (connection, sender, object_path, interface, signal, params) => {
                const xid = params.get_child_value(0).get_uint32();
                _onPreviewClosed(xid);
            }
        );
        
    } catch (e) {
        // Extension not critical - fail silently
        log(`ApexShot Preview Helper: Could not connect to D-Bus: ${e.message}`);
    }
}

function _onPreviewOpened(xid) {
    // Find the window by XID (X11) or by title (Wayland fallback)
    const display = global.display;
    
    // Try X11 approach first
    if (xid > 0) {
        const windowActor = display.get_window_actors().find(
            w => w.get_meta_window().get_id() === xid
        );
        if (windowActor) {
            const metaWindow = windowActor.get_meta_window();
            _applyStackingConstraints(metaWindow);
            return;
        }
    }
    
    // Fallback: Find by window title "Screenshot"
    const windows = display.get_workspace(0).list_windows();
    for (const w of windows) {
        if (w.get_title() === 'Screenshot' || w.get_title()?.includes('ApexShot')) {
            _applyStackingConstraints(w);
            break;
        }
    }
}

function _onPreviewClosed(xid) {
    // Remove constraints from window
    const display = global.display;
    const windows = display.get_window_actors();
    
    for (const wa of windows) {
        const mw = wa.get_meta_window();
        if (xid > 0 && mw.get_id() === xid) {
            _removeStackingConstraints(mw);
            break;
        }
    }
}

// Also watch for focus-out events to re-raise
function _setupFocusOutHandler() {
    const display = global.display;
    display.connect('focus-out', (display, event) => {
        const focusedWindow = event.get_focused_window();
        if (focusedWindow && _previewXids.has(focusedWindow.get_id())) {
            // Re-raise the preview window
            focusedWindow.raise();
            focusedWindow.make_above();
        }
    });
}

function init() {
    _connectToApexshot();
    _setupFocusOutHandler();
}

function enable() {
    // Extension enabled
}

function disable() {
    // Clean up - remove all constraints
    const display = global.display;
    for (const wa of display.get_window_actors()) {
        const mw = wa.get_meta_window();
        if (_previewXids.has(mw.get_id())) {
            mw.delete_property('above');
        }
    }
    _previewXids.clear();
}
```

- [ ] **Step 3: Create README.md**

```markdown
# ApexShot Preview Helper (GNOME Extension)

This GNOME extension keeps ApexShot's screenshot preview overlay on top of other windows during drag operations.

## Installation

### Method 1: Extension Manager
1. Open GNOME Extensions app (or use Extension Manager)
2. Click "Install" and select the `metadata.json` file
3. Enable the extension

### Method 2: Manual (Terminal)
```bash
mkdir -p ~/.local/share/gnome-shell/extensions/org.apexshot.PreviewStacking@apexshot.github.io
cp -r . ~/.local/share/gnome-shell/extensions/org.apexshot.PreviewStacking@apexshot.github.io/
# Then enable via: gnome-extensions enable org.apexshot.PreviewStacking@apexshot.github.io
```

## Requirements
- ApexShot with D-Bus integration (build with default features)
- GNOME Shell 45, 46, or 47

## Troubleshooting
- If preview doesn't stay on top, check logs: `journalctl --user -f`
- Verify extension is enabled: `gnome-extensions list --user`
```

- [ ] **Step 4: Commit**

```bash
git add gnome-extension/
git commit -m "feat: add GNOME extension for always-on-top preview"
```

---

## Chunk 5: Testing Integration

**Files:**
- Modify: Cargo.toml - add feature flag documentation

- [ ] **Step 1: Document feature flags**

Add to Cargo.toml:
```toml
[features]
default = ["x11"]
x11 = ["gdk4x11"]
```

- [ ] **Step 2: Build and verify**

Run: `cargo build --release`
Expected: SUCCESS

- [ ] **Step 3: Manual test instructions**

```
1. Install the GNOME extension
2. Take a screenshot (e.g., apexshot capture area)
3. Verify preview appears in bottom-left
4. Start dragging preview to another window
5. Verify it stays on top (doesn't get lowered)
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "docs: add feature flags for GNOME integration"
```

---

## Summary of Deliverables

1. **Rust module** - `src/gnome_integration/mod.rs` - D-Bus signal emission
2. **Preview overlay changes** - emits signals on window open/close
3. **GNOME extension** - `gnome-extension/` - listens and applies stacking
4. **Documentation** - README for extension installation

---

## Next Steps After Implementation

1. User needs to install the GNOME extension manually
2. Test on GNOME Wayland to verify always-on-top behavior
3. Optionally: add config option in apexshot settings to control behavior
4. Optionally: expand to support editor window (with toggle)