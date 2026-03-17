# Architecture Overview

## Tech Stack

| Layer | Technology | Purpose |
|-------|------------|---------|
| **Language** | Rust (primary), C++/Qt5 (overlay) | Rust for safety/business logic, Qt5 for native overlay UI |
| **Async Runtime** | Tokio | Multi-threaded async I/O, concurrent operations |
| **GUI Framework** | GTK4 + gtk4-layer-shell | Main application UI, settings window |
| **Display Servers** | X11 (x11rb), Wayland (ashpd/wayland-client) | Dual-backend screen capture |
| **Video Recording** | GStreamer (gstreamer-rs) | MP4/WebM/GIF encoding |
| **OCR** | Tesseract (tesseract-rs) | Text recognition from screenshots |
| **IPC** | DBus (zbus), Unix pipes | System integration, overlay communication |
| **Build** | Cargo + CMake | Rust + C++ Qt5 co-compilation |

## Module Map (1451 nodes, 4107 edges)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ENTRY LAYER                                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  src/main.rs (3290 lines)                                                    │
│    ├─ run_capture()     → Screenshot workflow                              │
│    ├─ run_record()      → Video recording                                  │
│    ├─ run_ocr()         → Text extraction                                  │
│    └─ run_daemon()      → Background hotkey listener                       │
│                                                                             │
│  capture-overlay/src/main.cpp (Qt5)                                         │
│    └─ Region selection overlay window                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CORE LAYER                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│  overlay (50 nodes, fan-in: 71)  - Selection geometry, UI state           │
│  config (12 nodes, fan-in: 12)  - AppConfig, settings serialization        │
│  tray (14 nodes, fan-in: 8)     - System tray integration                 │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           INTERNAL LAYER                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│  capture (343 nodes)                                                        │
│    ├─ editor/           - Post-capture image editor (crop, draw, blur)    │
│    ├─ preview_overlay   - Quick preview window                             │
│    └─ save_capture()    - File I/O, clipboard, notification               │
│                                                                             │
│  backend (69 nodes)                                                         │
│    ├─ x11.rs            - X11Backend: x11rb direct capture                 │
│    ├─ wayland.rs        - WaylandBackend: portal-based capture              │
│    └─ screencopy.rs     - wlr-screencopy protocol                         │
│                                                                             │
│  daemon (62 nodes)                                                          │
│    ├─ run_daemon_inner  - Main daemon event loop                           │
│    ├─ Portal hotkey     - XDG portal global shortcuts                      │
│    └─ GNOME shell ext   - Fallback for GNOME                               │
│                                                                             │
│  hotkeys (74 nodes)                                                         │
│    ├─ run_portal_hotkey_daemon() - Portal-based hotkeys                    │
│    └─ run_gnome_hotkey_daemon()  - GNOME extension hotkeys                │
│                                                                             │
│  recording (16 nodes)                                                       │
│    ├─ start_recording()     - GStreamer pipeline setup                    │
│    ├─ Encoder profiles: VP8, VP9, H.264, H.265, Theora, GIF                │
│    └─ stop_overlay.rs   - Recording controls overlay                       │
│                                                                             │
│  ocr (21 nodes)                                                             │
│    └─ Tesseract OCR with confidence scoring                                │
│                                                                             │
│  settings (26 nodes)                                                        │
│    └─ GTK4 settings window with behavior configuration                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

## The Bridge (FFI Layer)

**NOT raw FFI** — uses **process communication**:

```mermaid
graph TB
    subgraph Rust_Main
        A[main.rs] --> B[capture_overlay.rs]
        B --> C[find_capture_binary()]
        C --> D[Command::new]
    end
    
    subgraph IPC
        D -->|spawn| E[C++ Process]
        E -->|stdin| F[JSON request]
        F -->|stdout| G[JSON response]
    end
    
    subgraph C__Qt5
        H[main.cpp] --> I[QApplication]
        I --> J[CaptureOverlay]
        J --> K[ScreenCapture]
    end
    
    D -.->|QT_IM_MODULE| H
```

### Communication Protocol

```rust
// Rust → C++ (stdin): User requests region selection
{
  "action": "select_region",
  "display": ":0",
  "cursor_pos": {"x": 100, "y": 200}
}

// C++ → Rust (stdout): User selected region
{
  "action": "region_selected", 
  "region": {"x": 50, "y": 100, "width": 800, "height": 600}
}
```

### Build Integration (`build.rs`)

```rust
// CMake compiles Qt5 overlay, outputs to OUT_DIR
// Rust embeds path via APEXSHOT_CAPTURE_BIN_DIR
// capture_overlay.rs searches: env > exe dir > build dir > PATH
```

## Threading Model

| Component | Threading | Notes |
|-----------|-----------|-------|
| **Main** | Tokio multi-threaded | `#[tokio::main]`, handles all async I/O |
| **GTK UI** | Main thread only | GTK4 requirement; runs in `spawn_blocking` |
| **Capture** | `spawn_blocking` | X11/Wayland calls block, run on thread pool |
| **Recording** | GStreamer threads | Pipeline runs on its own thread(s) |
| **OCR** | Rayon parallel | CPU-bound image processing |
| **C++ Overlay** | Qt main loop | Blocks waiting for user input |

### Key Threading Patterns

```rust
// Daemon uses tokio for concurrent capture requests
tokio::spawn(handle_capture_screen(action_tx_clone));
tokio::task::spawn_blocking(move || handle_capture_area(state_clone));

// GTK must run on main thread
tokio::task::spawn_blocking(|| run_capture_overlay(None))

// Recording uses tokio::select! for async control flow
tokio::select! {
    _ = ctrl_c => stop_recording(),
    _ = stop_fut => {},
}
```

## Inter-Module Boundaries (Top 10)

| From | To | Call Count | Description |
|------|-----|------------|-------------|
| capture | overlay | 48 | Selection geometry, UI state |
| daemon | config | 12 | Config loading for daemon |
| hotkeys | overlay | 8 | Hotkey display updates |
| daemon | tray | 8 | Tray notifications |
| main | ocr | 8 | Text extraction |
| ocr | overlay | 8 | OCR result display |
| main | capture | 8 | Capture orchestration |
| backend | overlay | 7 | Capture preview |
| daemon | backend | 7 | Screen capture in daemon |
| daemon | hotkeys | 7 | Hotkey registration |

## Key Data Structures

```rust
// Screen capture result
pub struct CaptureData {
    pub pixels: Vec<u8>,      // Raw framebuffer
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,  // RGB24, RGBA32, BGR24, etc.
}

// Image editor state (fan-out: 63)
pub struct EditorState {
    pub selection: Option<SelectionRectF>,
    pub draw_actions: Vec<DrawAction>,
    pub selected_tool: Tool,
    // ... 63 fields total
}

// Recording configuration
pub struct RecordingConfig {
    pub output_path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    // VP8/VP9/H.264/H.265/Theora/GIF
}
```

---

*Related: [Data_Flow.md](Data_Flow.md) | [Linux_Interactions.md](Linux_Interactions.md)*
