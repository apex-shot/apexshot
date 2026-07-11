# Plan: Replace GStreamer with Native libpipewire (OBS-style)

**Historical status:** Core native PipeWire path is implemented in `src/pipewire_engine.rs` and used by Wayland recording/screenshot capture. Webcam/PiP recording support described in this plan has since been removed from ApexShot. GStreamer is retained where still needed for non-webcam recording fallback paths such as X11/GIF/audio-related pipelines. Treat remaining “current state” tables below as design history, not live inventory.

## Goal

Replace all GStreamer-based PipeWire frame consumption in the GNOME/Wayland path with
native `libpipewire` C API, following the same architecture OBS uses in
`obs-studio/plugins/linux-pipewire/pipewire.c`. **Do not touch** the wlroots/Hyprland
paths (wf-recorder, wlr-screencopy).

## Current State Summary

### Files that use GStreamer today

| File | Lines | Role |
|------|-------|------|
| `src/recording/mod.rs` | 3,554 | Main recording: video, audio, GIF, encoder selection, portal session |
| `src/backend/wayland.rs` | 792 | Single-frame screenshot capture via ScreenCast portal |
| `src/overlay/webcam.rs` | 177 | Webcam preview (GTK layer-shell overlay) |

### GStreamer dependency graph

```
src/recording/mod.rs
  ├── gstreamer, gstreamer-app, gstreamer-video  (Cargo.toml deps)
  ├── pipewire_source_pipeline() → "pipewiresrc fd=… path=…"
  ├── build_pipeline() → GStreamer pipeline strings for recording
  ├── record_gif_rust_with_commands() → GStreamer pipeline → ffmpeg stdin
  └── get_wayland_source() → ashpd ScreenCast portal (NOT GStreamer, keep this)

src/backend/wayland.rs
  ├── gstreamer, gstreamer-app, gstreamer-video
  ├── ensure_gstreamer_initialized()
  ├── pipewire_source_pipeline() → "pipewiresrc fd=… num-buffers=1"
  ├── capture_single_frame_from_pipewire() → single-frame grab via GStreamer AppSink
  └── capture_via_screencast() → portal session (KEEP) + GStreamer capture (REPLACE)

src/overlay/webcam.rs
  ├── gstreamer, gstreamer-app
  └── GStreamer pipeline: v4l2src → videoconvert → BGRA → AppSink
```

### What stays (NOT changed)

- `src/recording/mod.rs`: wf-recorder path (`record_with_wf_recorder()`) — wlroots only
- `src/backend/screencopy.rs`: wlr-screencopy native Wayland protocol capture
- `src/backend/x11.rs`: X11 capture
- `src/backend/wayland.rs`: `capture_via_screenshot_portal()`, `capture_monitor_via_native_screencopy()`
- All `ashpd` portal D-Bus code (`get_wayland_source()` portal session creation)
- The `compute_wayland_crop()` and crop logic
- Control command handling (`RecordingControlCommand`, command_rx channel)
- GNOME extension, C++ overlay, daemon D-Bus interface

## Architecture: How OBS Does It

OBS's `pipewire.c` provides a clean layered model we will replicate:

```
┌─────────────────────────────────────────────────┐
│  Screencast Portal (ashpd, already working)     │
│  CreateSession → SelectSources → Start          │
│  → OpenPipeWireRemote → pipewire_fd, node_id    │
└──────────────────────┬──────────────────────────┘
                       │ fd, node_id
┌──────────────────────▼──────────────────────────┐
│  NEW: src/pipewire_engine.rs                     │
│                                                  │
│  obs_pipewire_connect_fd(fd)                     │
│    → pw_thread_loop, pw_context, pw_core         │
│                                                  │
│  obs_pipewire_connect_stream(stream_name, node)  │
│    → pw_stream_new(), pw_stream_connect()        │
│    → format negotiation (DMA-BUF first, SHM)     │
│                                                  │
│  Callback: on_process_cb(frame)                  │
│    → extract data from spa_buffer                │
│    → DMA-BUF: fds → mmap or GPU texture (future) │
│    → SHM: memcpy from buffer->datas[i].data      │
│                                                  │
│  obs_pipewire_stream_destroy()                   │
│  obs_pipewire_destroy()                          │
└──────────────────────────────────────────────────┘
```

### Key design choices from OBS we will adopt

1. **Dedicated PipeWire thread loop** (`pw_thread_loop`) — keeps PipeWire I/O off
   the main/GTK threads, same as OBS's approach.

2. **Format negotiation via SPA pods** — build a prioritized list:
   - First: DMA-BUF formats WITH DRM modifiers (zero-copy, but complex to set up)
   - Second: SHM formats (memcpy, simpler, always works)

3. **Two-phase buffer delivery**:
   - `process_video_sync()` — DMA-BUF path (GPU buffers via fds)
   - `process_video_async()` — SHM path (CPU buffers via memcpy)
   For the initial implementation, we focus on the SHM path (always works,
   no GPU dependency). DMA-BUF can be added later.

4. **Cursor handling** via `SPA_META_Cursor` metadata (embedded cursor mode from portal).

5. **Crop handling** via `SPA_META_VideoCrop` region metadata.

## Implementation Phases

### Phase 1: New `src/pipewire_engine.rs` — Core PipeWire Engine

Create a standalone Rust module that wraps `libpipewire` and provides:

```rust
// src/pipewire_engine.rs — new file

pub struct PipeWireEngine { /* pw_thread_loop, pw_context, pw_core */ }
pub struct PipeWireStream { /* pw_stream, negotiated format, texture handle */ }
pub struct PipeWireFrame {
    pub pixels: Vec<u8>,       // RGBA32 pixel data
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub cursor_visible: bool,
    pub cursor_x: i32, cursor_y: i32,
    pub cursor_hotspot_x: i32, cursor_hotspot_y: i32,
    pub cursor_bitmap: Option<Vec<u8>>,
    pub cursor_width: u32, cursor_height: u32,
    pub crop: Option<Rect>,
}

impl PipeWireEngine {
    /// Connect to PipeWire via the fd from the portal.
    pub fn connect(pipewire_fd: OwnedFd) -> Result<Self>;
    /// Create a stream for the given PipeWire node.
    pub fn create_stream(&mut self, node_id: u32, stream_name: &str,
                         cursor_visible: bool, width_hint: Option<u32>,
                         height_hint: Option<u32>) -> Result<PipeWireStream>;
    /// Roundtrip to sync with PipeWire server.
    pub fn roundtrip(&self);
}

impl PipeWireStream {
    /// Dequeue next frame (blocking or non-blocking).
    pub fn dequeue_frame(&self) -> Option<PipeWireFrame>;
    /// Set active/inactive (pause/resume).
    pub fn set_active(&self, active: bool);
}

impl Drop for PipeWireStream { /* disconnect + destroy */ }
impl Drop for PipeWireEngine { /* teardown */ }
```

**Dependencies to add to Cargo.toml:**
- `pipewire = "0.9"` (already present, currently unused at Rust level)
- No new system deps needed beyond what's already listed

**Rust pipewire crate approach:**
The `pipewire` crate (v0.9) provides safe Rust bindings to `libpipewire`.
We'll use:
- `pipewire::main_loop::MainLoop` (or thread loop)
- `pipewire::context::Context`
- `pipewire::core::Core`
- `pipewire::stream::Stream` with `StreamListener`

Key format negotiation code (Rust adaptation of OBS's `build_format()`):

```rust
use pipewire::spa::pod::PodBuilder;
use pipewire::spa::param::video::VideoInfo;

// Build SPA format params for supported formats
fn build_format_params(stream: &PipeWireStream) -> Vec<SpaPod> {
    let formats = &[
        (spa::video::VideoFormat::BGRA, 4),  // Most common
        (spa::video::VideoFormat::RGBA, 4),
        (spa::video::VideoFormat::BGRx, 4),
        (spa::video::VideoFormat::RGBx, 4),
    ];
    // Build SPA pods with size, framerate, modifier info...
}
```

### Phase 2: Replace `capture_single_frame_from_pipewire` in `src/backend/wayland.rs`

Replace the GStreamer single-frame capture with native PipeWire:

```rust
// BEFORE (GStreamer, ~100 lines):
fn capture_single_frame_from_pipewire(node_id, pipewire_fd) -> CaptureData {
    ensure_gstreamer_initialized()?;
    let pipeline_str = format!("pipewiresrc fd={} path={} num-buffers=1 ...");
    let pipeline = gst::parse::launch(&pipeline_str)?;
    // ... AppSink dance ...
}

// AFTER (native PipeWire, ~30 lines):
fn capture_single_frame_from_pipewire(node_id, pipewire_fd) -> CaptureData {
    let mut engine = PipeWireEngine::connect(pipewire_fd)?;
    let stream = engine.create_stream(node_id, "apexshot-screenshot",
                                       false, None, None)?;
    // Wait for one frame (blocking dequeue with timeout)
    let frame = stream.dequeue_frame_timeout(Duration::from_secs(2))?;
    Ok(CaptureData::new(frame.pixels, frame.width, frame.height,
                        PixelFormat::RGBA32))
}
```

**Remove from wayland.rs:**
- `ensure_gstreamer_initialized()` function
- `pipewire_source_pipeline()` function
- GStreamer pipeline parsing and AppSink code

**Remove from Cargo.toml (after all phases):**
- `gstreamer`, `gstreamer-app`, `gstreamer-video` dependencies

### Phase 3: Replace Recording Pipeline in `src/recording/mod.rs`

This is the largest change. The current flow:

```
get_wayland_source() → returns "pipewiresrc fd=… path=…" STRING
build_pipeline() → concatenates GStreamer pipeline string
start_recording_with_commands() → gst::parse::launch() → set_state(Playing)
→ bus.iter_timed() loop → set_state(Null)
```

New flow:

```
get_wayland_source() → returns WaylandSource { node_id, pipewire_fd, crop }
                         (same portal flow, BUT it returns node_id+fd instead
                          of a GStreamer pipeline string)
build_pipeline() → returns BuiltPipeline { encoder_info, crop, audio_config }
start_recording_with_commands() →
    1. Open PipeWire stream (continuous, not single-frame)
    2. Spawn recording thread/task with tokio
    3. Main loop: dequeue PipeWire frames → encode → mux → write
```

**Recording architecture with native PipeWire:**

```
┌─────────────┐    ┌──────────────────┐    ┌──────────────┐
│ PipeWire    │───▶│ Frame processing │───▶│ FFmpeg CLI   │──▶ file.mp4/webm
│ Stream      │    │ (crop, resize,   │    │ (encode+mux) │
│ (DMA/SHM)   │    │  colorspace)     │    │ via stdin     │
└─────────────┘    └──────────────────┘    └──────────────┘
```

For video encoding, we have two options:

**Option A: FFmpeg CLI pipe (simpler, recommended for Phase 3)**
- Write raw RGBA frames to ffmpeg stdin (same as GIF path already does)
- ffmpeg handles encoding, muxing, audio mixing
- Single ffmpeg process per recording
- Pro: battle-tested, supports all codecs, handles audio mixing
- Con: extra process, pipe overhead

**Option B: Rust video encoder crate (future)**
- Use `rav1e` (AV1), `x264` crate, or `libvpx` bindings directly
- Pro: no ffmpeg dependency, more control
- Con: significant additional implementation effort

**Recommendation: Option A (ffmpeg CLI) for Phase 3**, since:
- ffmpeg is already a dependency (GIF path uses it)
- Handles video + audio mixing in one command
- Supports all formats (webm/vp9, mp4/h264, gif)
- Users already have ffmpeg installed

### Phase 4: Replace GIF Recording ✅ COMPLETE

Current GIF path: GStreamer pipeline → AppSink → ffmpeg stdin
New GIF path: PipeWire stream → raw RGBA frames → ffmpeg stdin

This is actually *simpler* because we get raw frames directly instead of
going through a GStreamer pipeline. The ffmpeg invocation stays the same.

**Implemented in:** `src/recording/mod.rs` `record_gif_wayland_native()`.
X11 GIF recording still uses GStreamer fallback (`record_gif_x11_gstreamer()`).

### Phase 5: Replace Webcam Preview (`src/overlay/webcam.rs`) ✅ COMPLETE

Current: GStreamer `v4l2src ! videoconvert ! appsink`
New: XDG Camera portal (`org.freedesktop.portal.Camera`) → native PipeWire
stream via `PipeWireCapture`. Falls back to v4l2 GStreamer when the Camera
portal is unavailable (older desktops, wlroots).

**Implemented in:** `src/overlay/webcam.rs` `start_webcam_preview()` using
Camera portal + native PipeWire. v4l2/GStreamer fallback in
`start_webcam_preview_v4l2()`.

### Phase 6: Cleanup 🔄 PARTIALLY COMPLETE

- ~~Remove `gstreamer`, `gstreamer-app`, `gstreamer-video` from Cargo.toml~~ — **retained**: 
  still used by X11 recording fallback (`record_x11_with_gstreamer()`),
  X11 GIF fallback (`record_gif_x11_gstreamer()`), and webcam v4l2 fallback.
- ~~Remove `gstreamer1.0-pipewire` from distro packaging lists~~ — **retained**:
  needed for X11 GStreamer fallback path.
- `pipewire_source_pipeline()` function in recording/mod.rs is now `#[allow(dead_code)]` —
  kept as reference but unused on Wayland.
- `ensure_gstreamer_initialized()` and GStreamer pipeline code removed from
  `src/backend/wayland.rs` ✅
- `capture_single_frame_from_pipewire()` in wayland.rs replaced with
  `crate::pipewire_engine::capture_single_frame()` ✅

## Files Modified

### New files
- `src/pipewire_engine.rs` — core PipeWire engine (~400-600 lines)

### Modified files
- `src/backend/wayland.rs` — replace `capture_single_frame_from_pipewire()`,
  remove GStreamer init/pipeline code (~100 lines removed, ~30 added)
- `src/recording/mod.rs` — replace `build_pipeline()`, recording loop,
  GIF loop, remove GStreamer pipeline handling (~600 lines changed)
- `Cargo.toml` — remove gstreamer deps (unless webcam still needs them)
- `src/distro/mod.rs` — remove `gstreamer1.0-pipewire` from package lists

### NOT modified
- `src/overlay/webcam.rs` — may keep GStreamer for v4l2 pipeline (TBD)
- `src/backend/screencopy.rs` — wlroots path, untouched
- `src/backend/x11.rs` — untouched
- `capture-overlay/` — C++ code, untouched
- `gnome-extension/` — untouched
- `src/daemon/mod.rs` — untouched (uses backend, not GStreamer directly)

## Verification Plan

### Step 1: Compile check
```bash
cargo build 2>&1 | head -50
# Should compile without GStreamer deps (or with only webcam keeping them)
```

### Step 2: Single-frame screenshot test (GNOME Wayland)
```bash
# Test the wayland.rs capture_via_screencast path
cargo test --lib backend::wayland::tests
# Manual: trigger a fullscreen screenshot via the daemon
./target/debug/apexshot daemon &
# Send capture command via D-Bus
dbus-send --session --dest=org.apexshot.Daemon --type=method_call \
  /org/apexshot/Daemon org.apexshot.Daemon.Trigger string:"capture_screen"
```

### Step 3: Single-frame area capture (crop from full screen)
```bash
# Trigger area capture
dbus-send --session --dest=org.apexshot.Daemon --type=method_call \
  /org/apexshot/Daemon org.apexshot.Daemon.Trigger string:"capture_area"
```

### Step 4: Recording test
```bash
# Start a 5-second screen recording, verify output file
./target/debug/apexshot record screen --output /tmp/test_recording.webm &
sleep 5
kill %1
ffprobe /tmp/test_recording.webm
# Should show: video stream, correct resolution, duration ~5s
```

### Step 5: GIF recording test
```bash
./target/debug/apexshot record gif --output /tmp/test_gif.gif --duration 5
file /tmp/test_gif.gif
# Should show: GIF image data, animated
```

### Step 6: Regression check — Hyprland
```bash
# Ensure wlroots path still works
HYPRLAND_INSTANCE_SIGNATURE=test cargo test recording::tests
# wf-recorder path should be completely untouched
```

### Step 7: Dependency check
```bash
# Verify system packages no longer need gstreamer1.0-pipewire
apt-cache show apexshot  # check depends/recommends
# Should NOT list gstreamer1.0-pipewire
```

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|-----------|
| `pipewire` Rust crate API incomplete for DMA-BUF | Medium | Start with SHM-only (memcpy), add DMA-BUF later |
| FFmpeg pipe stalls or deadlocks | Medium | Use tokio::io::AsyncWrite, separate stdin writer task |
| Format negotiation fails on some compositors | Low | OBS's format list works on all major compositors; copy same list |
| Audio sync with video | Medium | Use ffmpeg's built-in A/V sync; simpler than GStreamer's clock |
| GIF recording frame timing | Low | Same ffmpeg invocation, just different frame source |
| PipeWire thread safety issues | Low | Use `pw_thread_loop` + channel-based frame passing to main thread |
