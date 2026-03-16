# Data Flow

## High-Level Data Paths

```mermaid
sequenceDiagram
    participant U as User
    participant M as main.rs
    participant O as overlay.rs
    participant CO as capture_overlay
    participant B as backend
    participant E as editor
    participant S as save
    participant R as recording

    Note over U,R: Screenshot Workflow
    U->>M: Click capture
    M->>CO: spawn Qt5 overlay
    CO->>U: Show region selector
    U->>CO: Select region
    CO-->>M: {x, y, w, h}
    M->>B: capture_screen(x,y,w,h)
    B-->>M: CaptureData{pixels}
    M->>E: Show editor
    E->>M: Apply edits
    M->>S: save_capture()
    S->>S: PNG encoding (rayon)
    S->>S: clipboard/notify/open
    
    Note over U,R: Recording Workflow
    U->>M: Click record
    M->>R: start_recording()
    R->>R: GStreamer pipeline
    R-->>M: Stream to file
    U->>M: Click stop
    M->>R: stop_recording()
    
    Note over U,R: OCR Workflow
    U->>M: OCR command
    M->>B: capture_region()
    B-->>M: CaptureData
    M->>M: tesseract::recognize()
    M->>M: Copy to clipboard
```

## Capture Pipeline Detail

```mermaid
flowchart LR
    subgraph Input
        A[User Click] --> B[Region Selection]
    end
    
    subgraph Backend_Selection
        B --> C{X11 or Wayland?}
        C -->|X11| D[x11rb::get_image]
        C -->|Wayland| E[ashpd portal]
        D --> F[X11 shared memory]
        E --> G[dma-buf/SC protocol]
    end
    
    subgraph Processing
        F --> H[CaptureData]
        G --> H
        H --> I{pixels}
        I -->|editor| J[image crate]
        I -->|save| K[PNG encode]
        I -->|ocr| L[tesseract]
    end
    
    subgraph Output
        J --> M[Preview window]
        K --> N[File/Clipboard]
        L --> O[Text]
    end
```

## Cross-Process Communication (Rust ↔ Qt5)

### Finding the Binary

```rust
// Priority order in capture_overlay.rs:
1. APEXSHOT_CAPTURE_BIN env variable      // Manual override
2. Same directory as running exe          // Installed bundles  
3. Build-time OUT_DIR (build.rs)          // cargo build
4. target/{debug,release}/                // cargo run
5. PATH                                    // System lookup
```

### IPC Protocol

```rust
// stdin: Qt5 app runs with stdin=null (no input)
// stdout: JSON responses
// stderr: Qt logging (inherited)

// Exit codes:
0 = Success (region selected or captured)
1 = Cancelled (user pressed Escape)
2 = Error (no display, permission denied, etc.)
```

### Example Session

```bash
# Rust spawns:
Command::new("apexshot-capture")
    .env("QT_IM_MODULE", "compose")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .arg("--mode").arg("overlay")
    .arg("--display").arg(":0")

# Qt5 returns:
{"action":"region_selected","region":{"x":100,"y":200,"width":800,"height":600}}

# Or for fullscreen capture:
{"action":"captured","path":"/tmp/apexshot_XXXX.png","width":1920,"height":1080}
```

## Memory Management

| Stage | Type | Manager | Notes |
|-------|------|---------|-------|
| **X11 Framebuffer** | X11 shm | X server | Shared memory segment |
| **Wayland Buffer** | dmabuf | Compositor | File descriptor passing |
| **CaptureData** | `Vec<u8>` | Rust allocator | Owned pixels |
| **Image Processing** | `image::RgbaImage` | image crate | Stack-allocated for small |
| **GStreamer** | GstBuffer | C library | Refcounted |
| **OCR** | `Vec<u8>` + FFI | tesseract-rs | C++ allocation |
| **File Output** | `Vec<u8>` | rayon parallel | Parallel PNG encoding |

## Backend Abstraction

```rust
// Two implementations, one trait interface
pub trait DisplayBackend {
    fn capture_screen(&self, area: Rect) -> DisplayResult<CaptureData>;
    fn capture_area(&self, x: i32, y: i32, w: u32, h: u32) -> DisplayResult<CaptureData>;
    fn supported_formats(&self) -> &[PixelFormat];
}

// X11: Direct framebuffer access
pub struct X11Backend { /* x11rb::Connection */ }

// Wayland: Portal-based capture
pub struct WaylandBackend { /* wayland + ashpd proxies */ }
```

## Editor Data Flow

```mermaid
flowchart TB
    subgraph Input
        A[CaptureData] --> B[RgbaImage]
    end
    
    subgraph Editor_State
        B --> C[EditorState]
        C --> D[SelectionRectF]
        C --> E[DrawActions]
        C --> F[EffectLayers]
    end
    
    subgraph Tools
        D --> G[CropTool]
        E --> H[DrawTool]
        E --> I[BlurTool]
        E --> J[TextTool]
    end
    
    subgraph Output
        G --> K[RgbaImage]
        H --> K
        I --> K
        J --> K
        K --> L[PNG/Save]
    end
```

## Key Serialization Points

| From | To | Format |
|------|-----|--------|
| Config file | AppConfig | YAML |
| CLI args | SaveConfig | JSON (internal) |
| Qt5 stdout | SelectionResult | JSON |
| Editor state | File | PNG (embedded metadata) |
| OCR result | Clipboard | Plain text |

---

*Related: [Architecture.md](Architecture.md) | [Linux_Interactions.md](Linux_Interactions.md)*
