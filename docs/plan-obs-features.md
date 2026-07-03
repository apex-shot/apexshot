# Plan: OBS Feature Adoption for ApexShot

**Historical status:** Webcam/PiP recording support described in this plan has since been removed from ApexShot. Current recording support covers screen capture, GIF export, mic/speaker audio, countdown, controls, and editor workflows.

## Overview

Five features from OBS's `linux-pipewire` plugin to adopt, ordered by impact-to-effort ratio.

---

## Feature 1: SPA_META_Cursor Overlay (est. 50 lines, 1 hour)

### What it does
Instead of relying on the compositor to embed the cursor into the frame
(`CursorMode::Embedded`), parse `SPA_META_Cursor` metadata from each PipeWire
buffer and composite the cursor bitmap ourselves.

### OBS reference
`obs-studio/plugins/linux-pipewire/pipewire.c:889-920`

### What changes

**`src/pipewire_engine.rs`:**

1. Add fields to `PipeWireFrame`:
```rust
pub struct PipeWireFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    // NEW:
    pub cursor_visible: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub cursor_hotspot_x: i32,
    pub cursor_hotspot_y: i32,
    pub cursor_bitmap: Option<Vec<u8>>,   // RGBA cursor pixels
    pub cursor_width: u32,
    pub cursor_height: u32,
}
```

2. In the `process` callback, after extracting frame data:
```rust
// Find SPA_META_Cursor on the buffer
// Use spa::meta::cursor utilities to extract position, hotspot, bitmap
// PipeWire's SPA_META_Cursor contains:
//   - position (x, y)
//   - hotspot (x, y)  
//   - bitmap_offset → spa_meta_bitmap { format, width, height, offset }
```

3. Add `composite_cursor(&mut self)` to `PipeWireFrame` that alpha-blends the
   cursor bitmap at the correct position (accounting for hotspot).

**`src/recording/mod.rs`:**

4. Call `frame.composite_cursor()` before writing to ffmpeg stdin in both
   `record_wayland_with_ffmpeg_sync()` and `record_gif_wayland_native()`.

**Portal change:**

5. Switch from `CursorMode::Embedded` to `CursorMode::Metadata` in
   `get_wayland_source()` so the compositor sends cursor as metadata
   instead of compositing it into the frame.

### Verification
- Start recording, move the mouse rapidly. Cursor should appear at correct
  position with correct hotspot (click point, not top-left of icon).
- Test with different cursor themes (Breeze, Adwaita, DMZ).

---

## Feature 2: Color Space Handling (est. 30 lines, 30 min)

### What it does
Negotiate BT.601/BT.709 color matrix and full/partial color range with the
compositor, and tag the output accordingly.

### OBS reference
`obs-studio/plugins/linux-pipewire/pipewire.c:615-655`

### What changes

**`src/pipewire_engine.rs`:**

1. Add to `NegotiatedFormat`:
```rust
pub struct NegotiatedFormat {
    // existing fields...
    pub color_matrix: ColorMatrix,  // BT601, BT709, RGB
    pub color_range: ColorRange,    // Full (0-255), Partial (16-235)
}
```

2. Add `SPA_FORMAT_VIDEO_colorMatrix` and `SPA_FORMAT_VIDEO_colorRange` properties
   to the format negotiation pod in `build_enum_format_pod()`. Advertise:
   - Preferred: `BT709` + `Full`
   - Also accept: `BT601`, `RGB`, `Partial`

3. In `param_changed` callback, parse these from the negotiated format and store.

**`src/recording/mod.rs`:**

4. Pass color metadata to ffmpeg via `-color_primaries`, `-color_trc`,
   `-colorspace` flags on the ffmpeg command line.

### Verification
- Record on a display with non-sRGB color profile. Verify colors match.
- Check ffprobe output: `ffprobe -show_streams output.webm | grep color`

---

## Feature 3: DMA-BUF Zero-Copy + Format Modifier Negotiation (est. 250 lines, 3-4 hours)

### What it does
Instead of SHM (memcpy through CPU), import DMA-BUF file descriptors directly
with `mmap`. Query GPU for supported DRM format modifiers, build a prioritized
format list, and fall back gracefully. On failure, remove the offending modifier
and renegotiate.

### OBS reference
- `pipewire.c:420-530` — `init_format_info_sync()`, modifier querying
- `pipewire.c:545-600` — `remove_modifier_from_format()`, `renegotiate_format()`
- `pipewire.c:740-830` — DMA-BUF buffer processing (`process_video_sync`)

### What changes

**`src/pipewire_engine.rs`:**

1. Add `BufferType` enum to `PipeWireCapture::connect()`:
```rust
pub enum BufferType {
    ShmOnly,           // current behavior
    DmaBufPreferred,   // try DMA-BUF, fall back to SHM
}
```

2. Add `FormatInfo` struct tracking supported formats + modifiers:
```rust
struct FormatInfo {
    spa_format: spa::param::video::VideoFormat,
    drm_format: u32,         // DRM fourcc
    modifiers: Vec<u64>,     // supported DRM modifiers
}
```

3. In `build_enum_format_pod()`, when `BufferType::DmaBufPreferred`:
   - Build format list in two passes:
     - First: each format WITH each supported modifier (as `SPA_FORMAT_VIDEO_modifier` pods)
     - Second: each format WITHOUT modifiers (SHM fallback)
   - OBS builds a `SPA_CHOICE_Enum` of modifiers per format

4. Query DRM for supported formats/modifiers. On Linux, use:
   ```rust
   // Query /dev/dri/renderD128 for format modifier support
   // Use drm-rs or raw ioctls: DRM_IOCTL_MODE_GETPLANERESOURCES
   ```
   Simplified approach: query via `libdrm` syscalls or use a hardcoded
   common-modifier list (LINEAR is always supported).

5. Add DMA-BUF path in `process` callback alongside existing SHM path:
```rust
if datas[0].type_() == spa::buffer::DataType::DmaBuf {
    let fd = datas[0].fd();  // DMA-BUF file descriptor
    let size = datas[0].chunk().size();
    // mmap the DMA-BUF fd
    let ptr = unsafe {
        libc::mmap(ptr::null_mut(), size, PROT_READ, MAP_SHARED, fd, 0)
    };
    // Read pixels directly from mmap'd GPU memory
    let data = unsafe { slice::from_raw_parts(ptr as *const u8, size) };
    let raw = data[..size].to_vec();
    unsafe { libc::munmap(ptr, size) };
}
```

6. Add renegotiation support:
   - If DMA-BUF texture creation fails (mmap fails), call
     `remove_modifier_from_format()` and signal renegotiation via
     `pw_loop_signal_event()`.
   - OBS uses `pw_loop_add_event()` for this; we use a simple flag +
     reconnect approach for simplicity.

**`Cargo.toml`:**

7. Add `libc` (already present) — no new deps needed for mmap.

### Verification
- Record with `APEXSHOT_PIPEWIRE_DMABUF=1` env var. Check CPU usage vs SHM path.
- Test on GNOME (Mutter), KDE (KWin), Hyprland — each has different modifier sets.
- Force SHM fallback: set invalid modifiers, verify it falls back.

---

## Feature 4: Camera Portal for Webcam (est. 400 lines, 4-5 hours)

### What it does
Replace direct v4l2 access (`gst v4l2src`) with `org.freedesktop.portal.Camera`
D-Bus interface. The portal provides a PipeWire stream — the same architecture
as ScreenCast. This is the proper Wayland security model and works in Flatpak.

### OBS reference
`obs-studio/plugins/linux-pipewire/camera-portal.c` (1352 lines)

### What changes

**New file: `src/camera_portal.rs`:**

1. D-Bus client for `org.freedesktop.portal.Camera`:
```rust
// Same pattern as ScreenCast portal in recording/mod.rs
use ashpd::desktop::camera::Camera;

pub struct CameraSource {
    pipewire_fd: OwnedFd,
    node_id: u32,
    capture: PipeWireCapture,
    width: u32,
    height: u32,
}

impl CameraSource {
    /// Open the camera portal, request access, get PipeWire stream.
    pub async fn open() -> Result<Self> {
        let camera = Camera::new().await?;
        let session = camera.create_session().await?;
        camera.access_camera(&session).await?.response().await?;
        let fd = camera.open_pipe_wire_remote(&session).await?;
        let streams = /* parse streams from response */;
        // ... same PipeWireCapture::connect() pattern as screen recording
    }
}
```

2. Note: `ashpd` crate may not have `camera` module yet. Check version.
   If not available, use raw `zbus` D-Bus calls against the camera portal.

**`src/overlay/webcam.rs`:**

3. Replace `start_webcam_preview()` GStreamer pipeline with:
```rust
pub fn start_webcam_preview(device: i32, flip: bool) -> Option<WebcamPreview> {
    // Open camera portal
    let camera = block_on(CameraSource::open())?;
    // Spawn frame capture thread using PipeWireCapture
    // Same frame extraction pattern as recording
}
```

4. Keep `enumerate_webcam_devices()` for device listing, but the actual
   capture goes through the portal's PipeWire stream.

**`Cargo.toml`:**

5. If `ashpd` supports camera: no new deps. Otherwise: raw `zbus` calls
   (zbus already present).

### Verification
- Enable webcam in recording overlay. Verify camera preview appears.
- Test with multiple camera devices. Test camera hotplug.
- Verify it works in GNOME, KDE, and (if portal available) Hyprland.

---

## Feature 5: Explicit GPU Sync (est. 150 lines, 2-3 hours)

### What it does
Use `SPA_META_SyncTimeline` with DRM syncobjs for proper GPU timeline
synchronization between compositor and recorder. Requires PipeWire ≥ 1.2.0.

### OBS reference
`obs-studio/plugins/linux-pipewire/pipewire.c:574-578, 755-790, 957-1040`

### Prerequisites
- Feature 3 (DMA-BUF) must be implemented first — sync only applies to DMA-BUF.
- PipeWire ≥ 1.2.0 on the system.

### What changes

**`src/pipewire_engine.rs`:**

1. Version-check PipeWire server at connect time:
```rust
fn check_pipewire_version() -> Option<(u32, u32, u32)> {
    // Parse pw_get_library_version() or server info
}
```

2. If PipeWire ≥ 1.2.0 and `BufferType::DmaBufPreferred`:
   - Add `SPA_PARAM_BUFFERS_dataType: 1 << SPA_DATA_DmaBuf` with
     `SPA_PARAM_BUFFERS_metaType: 1 << SPA_META_SyncTimeline` to params
   - Add `SPA_META_SyncTimeline` param meta to the stream

3. In the DMA-BUF frame processing path:
   - Look for two extra `SPA_DATA_SyncObj` datas in the buffer
     (after the DMA-BUF planes)
   - Extract acquire/release syncobj fds and timeline points
   - Before reading the frame: `drmSyncobjWait(acquire_fd, acquire_point, timeout)`
   - After processing: `drmSyncobjTimelineSignal(release_fd, release_point)`

4. Simplified approach without libdrm: use `sync_file` / `sync_fence` instead of
   raw DRM syncobjs:
   ```rust
   // Before reading DMA-BUF data
   let acquire_fd = buffer->datas[planes].fd;
   // Create a sync_file from the syncobj fd
   // poll() on the fd to wait for the compositor to finish rendering
   let mut pollfd = libc::pollfd { fd: acquire_fd, events: libc::POLLIN, revents: 0 };
   libc::poll(&mut pollfd, 1, 1000); // 1s timeout
   // Now safe to read the DMA-BUF
   ```

   This is simpler than full DRM syncobj support and works on any kernel
   with `CONFIG_SYNC_FILE` (enabled by default on all distros).

### Verification
- Only testable on PipeWire ≥ 1.2.0 (Ubuntu 24.10+, Fedora 40+).
- Record high-fps video (60fps). Check for frame tearing or judder.
- Compare frame timestamp consistency with/without sync.

---

## Implementation Order & Dependency Graph

```
Feature 2 (Color space)     ← independent, do first
       ↓
Feature 1 (Cursor overlay)  ← independent, do second
       ↓
Feature 3 (DMA-BUF)         ← independent, do third
       ↓
Feature 5 (GPU sync)        ← depends on Feature 3 (DMA-BUF)
       ↓
Feature 4 (Camera portal)   ← independent, do last (largest change)
```

## Files Modified Per Feature

| Feature | `pipewire_engine.rs` | `recording/mod.rs` | `webcam.rs` | New files |
|---------|---------------------|-------------------|-------------|-----------|
| 1. Cursor | +30 | +15 | — | — |
| 2. Color | +20 | +10 | — | — |
| 3. DMA-BUF | +200 | +15 | — | — |
| 4. Camera | — | — | ~100 replaced | `camera_portal.rs` (+300) |
| 5. GPU sync | +120 | — | — | — |

## Total Estimate

| **UPDATE:** Features 1 and 2 completed.

---

## Feature 6: VAAPI Hardware Encoding (est. 30 lines, 30 min)

### What it does
Detect VAAPI-capable GPUs (Intel, AMD) and configure the ffmpeg CLI to use
`h264_vaapi` or `av1_vaapi` encoders instead of software VP9/x264. OBS uses
the same approach through `libavcodec`'s VAAPI backend.

### OBS reference
`obs-studio/plugins/obs-ffmpeg/obs-ffmpeg-vaapi.c` — rate control modes (CBR,
CQP, VBR, QVBR), global_quality, profile selection.

### What changes

**`src/recording/mod.rs`:**

1. Add to `RecordingConfig`:
```rust
pub hw_encoder: Option<HwEncoder>,  // None = software, Some(Vaapi)
```

2. In `record_wayland_with_ffmpeg_sync()`, when `hw_encoder == Some(Vaapi)`:
   - Add `-vaapi_device /dev/dri/renderD128` to ffmpeg args
   - Add `-vf 'format=nv12,hwupload'` for GPU upload
   - Switch `-c:v` from `libx264`/`libvpx` to `h264_vaapi`/`av1_vaapi`

3. Detect VAAPI device at startup:
```rust
fn detect_vaapi_device() -> Option<String> {
    // Check /dev/dri/renderD128 exists and is readable
    // Or use `vainfo` / libdrm to enumerate devices
}
```

### Verification
- Record with `APEXSHOT_HW_ENCODER=vaapi`. Check `ffprobe` shows VAAPI-encoded stream.
- Compare CPU usage vs software encoding — should be 5-10x lower.

---

## Feature 7: Encoder Quality Presets from OBS (est. 50 lines, 45 min)

### What it does
Replace apexshot's hardcoded encoder properties with OBS's battle-tested
defaults. OBS has tuned these across millions of streams.

### OBS reference
`obs-studio/plugins/obs-ffmpeg/obs-ffmpeg-video-encoders.c:59-107`

### What changes

**`src/recording/mod.rs`:**

1. Update `video_encoder_props()` to match OBS defaults:

| Encoder | OBS default | Current apexshot |
|---------|------------|-----------------|
| x264 | `veryfast` preset, CRF 23, main profile | `medium` preset, CQP 14, film tune |
| VP9 | CQ 30, deadline good, cpu-used 0 | CQ 10, deadline 10000, cpu-used 4 |
| VP8 | CQ 10, deadline good | CQ 10, deadline 8, cpu-used 4 |

2. Add a `quality` slider to the recording UI that maps to CQ/CRF values.

### Verification
- Record test clip with new presets. Compare file size and quality with old.
- Check `ffprobe` shows correct encoder parameters.

---

## Feature 8: Direct PulseAudio Capture (est. 150 lines, 2 hours)

### What it does
Use `libpulse` directly instead of ffmpeg's `-f pulse` input for audio capture.
Gives proper device enumeration, format selection, and latency control — same
approach OBS uses.

### OBS reference
`obs-studio/plugins/linux-pulseaudio/pulse-input.c` — `pa_simple_new()` with
format negotiation (`pa_sample_format_t` → obs audio format), channel mapping,
speaker layout detection.

### What changes

**New file: `src/audio_capture.rs`:**

1. Wrap `libpulse-sys` (or `libpulse-binding` crate) for PulseAudio access:
```rust
pub struct PulseAudioSource {
    device: String,
    sample_rate: u32,
    channels: u8,
    format: PulseAudioFormat,
}

impl PulseAudioSource {
    pub fn default_mic() -> Option<Self>;
    pub fn default_speaker_monitor() -> Option<Self>;
    pub fn enumerate_devices() -> Vec<DeviceInfo>;
    pub fn capture_samples(&self) -> impl Iterator<Item = Vec<u8>>;
}
```

**`src/recording/mod.rs`:**

2. Replace ffmpeg `-f pulse` audio input with captured samples written to
   ffmpeg's second stdin pipe, or keep ffmpeg pulse input with better
   device selection flags.

**`Cargo.toml`:**

3. Add `libpulse-binding` or `libpulse-sys` crate.

### Verification
- Record with mic + speaker. Verify both audio streams present.
- Test device enumeration with multiple microphones.

---

## Updated Implementation Order

```
✅ Feature 2 (Color space)       — done
✅ Feature 1 (Cursor overlay)    — done
⬜ Feature 6 (VAAPI HW encoding) — next, highest impact-to-effort
⬜ Feature 7 (Encoder presets)   — quick win after VAAPI
⬜ Feature 3 (DMA-BUF)           — larger change
⬜ Feature 5 (GPU sync)          — depends on DMA-BUF
⬜ Feature 8 (PulseAudio)        — independent
⬜ Feature 4 (Camera portal)     — largest change
```

## Updated Files Modified

| Feature | `pipewire_engine.rs` | `recording/mod.rs` | `webcam.rs` | New files |
|---------|---------------------|-------------------|-------------|-----------|
| ✅ 1. Cursor | +80 | +5 | — | — |
| ✅ 2. Color | +30 | +5 | — | — |
| 3. DMA-BUF | +200 | +15 | — | — |
| 4. Camera | — | — | ~100 replaced | `camera_portal.rs` |
| 5. GPU sync | +120 | — | — | — |
| 6. VAAPI | — | +30 | — | — |
| 7. Presets | — | +50 | — | — |
| 8. PulseAudio | — | +50 | — | `audio_capture.rs` |

## Total Estimate (updated)

| Lines of code | ~1,100 new/changed |
|---------------|--------------------|
| Files changed | 4-5 |
| New files | 2-3 |
| Total effort | ~18 hours |
