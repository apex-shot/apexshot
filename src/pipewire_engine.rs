//! Native PipeWire engine for screen capture — OBS-style.
//!
//! Replaces the GStreamer `pipewiresrc` pipeline with direct `libpipewire` API.
//!
//! Architecture (mirrors OBS's `plugins/linux-pipewire/pipewire.c`):
//!
//! 1. `PipeWireCapture` wraps the full PipeWire connection lifecycle:
//!    `ThreadLoopRc` → `ContextRc` → `CoreRc` → `StreamRc`.
//!    All PipeWire operations run on the dedicated thread loop.
//!
//! 2. Frames arrive via the `process` callback on the PipeWire thread.
//!    They are extracted (SHM memcpy) and pushed into a `VecDeque` behind
//!    an `Arc<Mutex<>>` for consumption on the application thread.
//!
//! 3. Format negotiation: we advertise a priority list of video formats
//!    (BGRx, BGRA, RGBx, RGBA) and accept whatever the compositor picks.
//!    Color space (BT.601/BT.709/RGB, full/limited range) is also negotiated.

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
// libspa-sys for raw SPA buffer metadata access (cursor).
use libspa_sys as spa_sys;

use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single video frame extracted from a PipeWire stream.
#[derive(Debug, Clone)]
pub struct PipeWireFrame {
    /// RGBA32 pixel data (always converted to RGBA regardless of source format).
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Row stride in bytes (= width * 4 for RGBA).
    pub stride: u32,
    /// Cursor overlay metadata (from SPA_META_Cursor, when available).
    pub cursor: Option<CursorOverlay>,
    /// Color space from negotiated format.
    pub color_space: ColorSpace,
}

/// Cursor bitmap and position extracted from PipeWire buffer metadata.
#[derive(Debug, Clone)]
pub struct CursorOverlay {
    /// RGBA pixel data for the cursor image.
    pub bitmap: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Screen position of the cursor (top-left of the bitmap).
    pub x: i32,
    pub y: i32,
    /// Hotspot offset within the bitmap (the click point).
    pub hotspot_x: i32,
    pub hotspot_y: i32,
}

/// Color space information from the negotiated video format.
#[derive(Debug, Clone, Copy)]
pub struct ColorSpace {
    /// SPA video color range: 1 = full (0-255), 2 = limited (16-235).
    pub range: u32,
    /// SPA video color matrix: 1 = RGB, 2 = BT.601, 3 = BT.709.
    pub matrix: u32,
}

impl Default for ColorSpace {
    fn default() -> Self {
        Self {
            range: 1,
            matrix: 1,
        }
    }
}

impl ColorSpace {
    /// Human-readable label for the color matrix.
    pub fn matrix_label(&self) -> &'static str {
        match self.matrix {
            1 => "RGB",
            2 => "BT.601",
            3 => "BT.709",
            _ => "unknown",
        }
    }

    /// Human-readable label for the color range.
    pub fn range_label(&self) -> &'static str {
        match self.range {
            1 => "full (0-255)",
            2 => "limited (16-235)",
            _ => "unknown",
        }
    }
}

/// Format negotiated with the compositor.
#[derive(Debug, Clone)]
pub struct NegotiatedFormat {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub framerate_num: u32,
    pub framerate_denom: u32,
    pub color_space: ColorSpace,
}

/// Errors from the PipeWire engine.
#[derive(Debug, thiserror::Error)]
pub enum PipeWireError {
    #[error("PipeWire initialization failed: {0}")]
    Init(String),

    #[error("Failed to connect stream: {0}")]
    Connect(String),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Frame timeout: no frame received within {0:?}")]
    Timeout(Duration),

    #[error("Format negotiation failed")]
    FormatNegotiation,

    #[error("No frame available")]
    NoFrame,
}

pub type PipeWireResult<T> = Result<T, PipeWireError>;

// ---------------------------------------------------------------------------
// Supported video formats
// ---------------------------------------------------------------------------

fn format_bpp(format: spa::param::video::VideoFormat) -> u32 {
    match format {
        spa::param::video::VideoFormat::BGRA
        | spa::param::video::VideoFormat::RGBA
        | spa::param::video::VideoFormat::BGRx
        | spa::param::video::VideoFormat::RGBx => 4,
        _ => 4,
    }
}

fn format_swaps_rb(format: spa::param::video::VideoFormat) -> bool {
    matches!(
        format,
        spa::param::video::VideoFormat::RGBx | spa::param::video::VideoFormat::RGBA
    )
}

// ---------------------------------------------------------------------------
// Internal shared state
// ---------------------------------------------------------------------------

struct StreamInner {
    format: Option<NegotiatedFormat>,
    raw_format: Option<spa::param::video::VideoInfoRaw>,
    frames: std::collections::VecDeque<Vec<u8>>,
    /// Cursor overlays corresponding to frames (paired by queue position).
    cursor_queue: std::collections::VecDeque<CursorOverlay>,
    frames_consumed: u64,
    error: Option<String>,
    max_frames: Option<u64>,
}

// ---------------------------------------------------------------------------
// PipeWireCapture
// ---------------------------------------------------------------------------

pub struct PipeWireCapture {
    inner: Arc<Mutex<StreamInner>>,
    _thread_loop: pw::thread_loop::ThreadLoopRc,
    _context: pw::context::ContextRc,
    _core: pw::core::CoreRc,
    _stream: pw::stream::StreamRc,
}

impl PipeWireCapture {
    pub fn connect(
        pipewire_fd: OwnedFd,
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
    ) -> PipeWireResult<Self> {
        pw::init();

        // SAFETY: pw_thread_loop_new is always safe to call; binding uses unsafe for C FFI.
        let thread_loop = unsafe {
            pw::thread_loop::ThreadLoopRc::new(Some("apexshot-pw"), None)
                .map_err(|e| PipeWireError::Init(format!("Failed to create thread loop: {e}")))?
        };

        let context = pw::context::ContextRc::new(&thread_loop, None)
            .map_err(|e| PipeWireError::Init(format!("Failed to create context: {e}")))?;

        let core = context
            .connect_fd_rc(pipewire_fd, None)
            .map_err(|e| PipeWireError::Init(format!("Failed to connect core via fd: {e}")))?;

        let inner = Arc::new(Mutex::new(StreamInner {
            format: None,
            raw_format: None,
            frames: std::collections::VecDeque::new(),
            cursor_queue: std::collections::VecDeque::new(),
            frames_consumed: 0,
            error: None,
            max_frames,
        }));

        let stream = pw::stream::StreamRc::new(
            core.clone(),
            "apexshot-screen-capture",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .map_err(|e| PipeWireError::Connect(format!("Failed to create stream: {e}")))?;

        // Build format pod.
        let pod_data = build_enum_format_pod(width_hint, height_hint);
        let pod = spa::pod::Pod::from_bytes(&pod_data)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse format pod".into()))?;
        // pod is &Pod; create an array of &Pod for the connect call.
        let mut params = [pod];

        let inner_clone = Arc::clone(&inner);
        let _listener = stream
            .add_local_listener_with_user_data(inner_clone)
            .state_changed(|_stream, inner, old, new| {
                if let pw::stream::StreamState::Error(msg) = &new {
                    if let Ok(mut guard) = inner.lock() {
                        guard.error = Some(msg.clone());
                    }
                }
                eprintln!("[pipewire] Stream state: {:?} -> {:?}", old, new);
            })
            .param_changed(|_stream, inner, id, param| {
                let Some(param) = param else { return };
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let (media_type, media_subtype) =
                    match spa::param::format_utils::parse_format(param) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                if media_type != spa::param::format::MediaType::Video
                    || media_subtype != spa::param::format::MediaSubtype::Raw
                {
                    return;
                }

                let mut info = spa::param::video::VideoInfoRaw::default();
                if info.parse(param).is_err() {
                    eprintln!("[pipewire] Failed to parse video format");
                    return;
                }

                let mut guard = inner.lock().unwrap();
                let w = info.size().width;
                let h = info.size().height;
                let bpp = format_bpp(info.format());
                let cs = ColorSpace {
                    range: info.color_range(),
                    matrix: info.color_matrix(),
                };
                guard.format = Some(NegotiatedFormat {
                    width: w,
                    height: h,
                    stride: w * bpp,
                    framerate_num: info.framerate().num,
                    framerate_denom: info.framerate().denom,
                    color_space: cs,
                });
                guard.raw_format = Some(info);

                eprintln!(
                    "[pipewire] Negotiated format: {:?} {}x{} @ {}/{} fps, color: {} {}",
                    guard.raw_format.as_ref().unwrap().format(),
                    w,
                    h,
                    guard.raw_format.as_ref().unwrap().framerate().num,
                    guard.raw_format.as_ref().unwrap().framerate().denom,
                    cs.matrix_label(),
                    cs.range_label(),
                );
            })
            .process(|_stream, inner| {
                let mut guard = match inner.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                if let Some(max) = guard.max_frames {
                    if guard.frames.len() as u64 >= max {
                        return;
                    }
                }

                let mut buffer = match _stream.dequeue_buffer() {
                    Some(b) => b,
                    None => {
                        eprintln!("[pipewire] Out of buffers!");
                        return;
                    }
                };

                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let chunk_size = datas[0].chunk().size() as usize;
                if chunk_size == 0 {
                    return;
                }

                // Try DMA-BUF first (zero-copy from GPU), fall back to SHM.
                let pixel_data = if datas[0].type_() == spa::buffer::DataType::DmaBuf {
                    read_dmabuf_frame(datas, chunk_size)
                } else if let Some(ref mem) = datas[0].data() {
                    if chunk_size <= mem.len() {
                        Some(mem[..chunk_size].to_vec())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let Some(pixel_data) = pixel_data else { return };
                guard.frames.push_back(pixel_data);

                // Extract SPA_META_Cursor from the raw spa_buffer.
                // The compositor sends this when CursorMode::Metadata is used.
                // SAFETY: Buffer is alive during this callback.
                let cursor = unsafe { extract_cursor_metadata(&buffer) };
                if let Some(cur) = cursor {
                    guard.cursor_queue.push_back(cur);
                }
                // buffer returned to stream via Drop
            })
            .register()
            .map_err(|e| PipeWireError::Connect(format!("Failed to register listener: {e}")))?;

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(|e| PipeWireError::Connect(format!("Failed to connect stream: {e}")))?;

        thread_loop.start();

        let start = Instant::now();
        loop {
            {
                let guard = inner.lock().unwrap();
                if guard.format.is_some() || guard.error.is_some() {
                    break;
                }
            }
            if Instant::now().duration_since(start) > Duration::from_secs(5) {
                return Err(PipeWireError::FormatNegotiation);
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        {
            let guard = inner.lock().unwrap();
            if let Some(ref err) = guard.error {
                return Err(PipeWireError::Stream(err.clone()));
            }
            if guard.format.is_none() {
                return Err(PipeWireError::FormatNegotiation);
            }
        }

        Ok(PipeWireCapture {
            inner,
            _thread_loop: thread_loop,
            _context: context,
            _core: core,
            _stream: stream,
        })
    }

    pub fn format(&self) -> Option<NegotiatedFormat> {
        self.inner.lock().unwrap().format.clone()
    }

    pub fn wait_for_frame(&self, timeout: Duration) -> PipeWireResult<PipeWireFrame> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = self.try_recv_frame()? {
                return Ok(frame);
            }
            if Instant::now() > deadline {
                return Err(PipeWireError::Timeout(timeout));
            }
            {
                let guard = self.inner.lock().unwrap();
                if let Some(ref err) = guard.error {
                    return Err(PipeWireError::Stream(err.clone()));
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn try_recv_frame(&self) -> PipeWireResult<Option<PipeWireFrame>> {
        let mut guard = self.inner.lock().unwrap();

        if let Some(ref err) = guard.error {
            return Err(PipeWireError::Stream(err.clone()));
        }

        let raw_format = match guard.raw_format.as_ref() {
            Some(f) => *f,
            None => return Ok(None),
        };

        let color_space = guard
            .format
            .as_ref()
            .map(|f| f.color_space)
            .unwrap_or_default();

        let raw = match guard.frames.pop_front() {
            Some(data) => data,
            None => return Ok(None),
        };
        let cursor = guard.cursor_queue.pop_front();

        guard.frames_consumed += 1;
        drop(guard);

        Ok(Some(convert_to_rgba_frame(
            &raw,
            &raw_format,
            color_space,
            cursor,
        )))
    }

    pub fn frames_consumed(&self) -> u64 {
        self.inner.lock().unwrap().frames_consumed
    }

    pub fn has_error(&self) -> bool {
        self.inner.lock().unwrap().error.is_some()
    }

    pub fn error_message(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }
}

// ---------------------------------------------------------------------------
// DMA-BUF frame reading (zero-copy from GPU memory)
// ---------------------------------------------------------------------------

/// Read pixel data from a DMA-BUF buffer by mmap'ing the file descriptor.
/// DMA-BUF buffers arrive as file descriptors pointing to GPU memory.
/// mmap'ing them reads directly from GPU without a CPU copy through SHM.
fn read_dmabuf_frame(datas: &[spa::buffer::Data], chunk_size: usize) -> Option<Vec<u8>> {
    if datas.is_empty() {
        return None;
    }

    let fd = datas[0].fd();
    if fd < 0 {
        return None;
    }

    // mmap the DMA-BUF fd to read GPU memory.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            chunk_size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        eprintln!("[pipewire] DMA-BUF mmap failed, falling back to SHM");
        return None;
    }

    // Copy out before unmapping.
    let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, chunk_size).to_vec() };
    unsafe { libc::munmap(ptr, chunk_size) };

    Some(data)
}

// ---------------------------------------------------------------------------
// Cursor metadata extraction (raw SPA buffer access)
// ---------------------------------------------------------------------------

// From pipewire spa/buffer/meta.h: SPA_META_Cursor = 5
const SPA_META_CURSOR: u32 = 5;

// TODO: SPA_META_SyncTimeline = 9, SPA_DATA_SyncObj = 5.
// These require PipeWire ≥ 1.2.0 and updated libspa-sys bindings.
// When the Rust pipewire crate updates:
//   1. Build SPA_PARAM_Buffers pod with dataType=1<<SPA_DATA_DmaBuf,
//      metaType=1<<SPA_META_SyncTimeline
//   2. Build SPA_PARAM_Meta pod for SPA_META_SyncTimeline
//   3. Pass both via pw_stream_update_params() after negotiate
//   4. In DMA-BUF path: check for extra SpaData::SyncObj datas,
//      poll() on acquire fd, signal release fd after processing

/// Extract SPA_META_Cursor from a PipeWire buffer.
///
/// Uses raw FFI to access the internal spa_buffer and call
/// `spa_buffer_find_meta_data`. The safe `pipewire` crate does not expose
/// this, so we reach through the `Buffer` struct's internal pointer.
///
/// # Safety
/// `buffer` must be a valid, alive PipeWire buffer.
unsafe fn extract_cursor_metadata(buffer: &pw::buffer::Buffer) -> Option<CursorOverlay> {
    // Buffer layout: { buf: NonNull<pw_sys::pw_buffer>, stream: &Stream }
    // NonNull<T> is repr(transparent) over *const T, so offset 0 is the raw pointer.
    let pw_buf: *const pw::sys::pw_buffer =
        *(buffer as *const pw::buffer::Buffer as *const *const pw::sys::pw_buffer);

    if pw_buf.is_null() {
        return None;
    }
    let spa_buf: *mut spa_sys::spa_buffer = (*pw_buf).buffer;
    if spa_buf.is_null() {
        return None;
    }

    let cursor_meta = spa_sys::spa_buffer_find_meta_data(
        spa_buf,
        SPA_META_CURSOR,
        std::mem::size_of::<spa_sys::spa_meta_cursor>(),
    );

    if cursor_meta.is_null() {
        return None;
    }

    let cursor: &spa_sys::spa_meta_cursor = &*cursor_meta.cast::<spa_sys::spa_meta_cursor>();
    let bitmap_offset = cursor.bitmap_offset;
    if bitmap_offset == 0 {
        return None;
    }

    let bitmap_ptr =
        (cursor_meta as *const u8).add(bitmap_offset as usize) as *const spa_sys::spa_meta_bitmap;
    let bitmap: &spa_sys::spa_meta_bitmap = &*bitmap_ptr;

    let bw = bitmap.size.width;
    let bh = bitmap.size.height;
    if bw == 0 || bh == 0 {
        return None;
    }

    let bitmap_data_ptr = bitmap_ptr.add(1) as *const u8;
    let bitmap_bytes = (bw * bh * 4) as usize;
    let bitmap_pixels = std::slice::from_raw_parts(bitmap_data_ptr, bitmap_bytes).to_vec();

    // Convert BGRA cursor bitmap to RGBA.
    let mut rgba = bitmap_pixels;
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2); // B↔R
    }

    Some(CursorOverlay {
        bitmap: rgba,
        width: bw,
        height: bh,
        x: cursor.position.x,
        y: cursor.position.y,
        hotspot_x: cursor.hotspot.x,
        hotspot_y: cursor.hotspot.y,
    })
}

/// Alpha-blend a cursor bitmap into frame pixels at the correct position.
fn composite_cursor_into_frame(
    pixels: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    stride: u32,
    cursor: &CursorOverlay,
) {
    let cx = cursor.x - cursor.hotspot_x;
    let cy = cursor.y - cursor.hotspot_y;

    let start_x = cx.max(0) as u32;
    let start_y = cy.max(0) as u32;
    let end_x = (cx + cursor.width as i32).min(frame_width as i32).max(0) as u32;
    let end_y = (cy + cursor.height as i32).min(frame_height as i32).max(0) as u32;

    for py in start_y..end_y {
        let cur_row = (py - start_y) as usize;
        let frame_row = py as usize;

        for px in start_x..end_x {
            let cur_col = (px - start_x) as usize;
            let cur_idx = (cur_row * cursor.width as usize + cur_col) * 4;
            let frame_idx = frame_row * stride as usize + px as usize * 4;

            let ca = cursor.bitmap[cur_idx + 3] as f32 / 255.0;
            let ca_inv = 1.0 - ca;

            pixels[frame_idx] =
                (cursor.bitmap[cur_idx] as f32 * ca + pixels[frame_idx] as f32 * ca_inv) as u8;
            pixels[frame_idx + 1] = (cursor.bitmap[cur_idx + 1] as f32 * ca
                + pixels[frame_idx + 1] as f32 * ca_inv) as u8;
            pixels[frame_idx + 2] = (cursor.bitmap[cur_idx + 2] as f32 * ca
                + pixels[frame_idx + 2] as f32 * ca_inv) as u8;
            pixels[frame_idx + 3] = 255;
        }
    }
}

// ---------------------------------------------------------------------------
// Frame format conversion
// ---------------------------------------------------------------------------

fn convert_to_rgba_frame(
    raw: &[u8],
    format: &spa::param::video::VideoInfoRaw,
    color_space: ColorSpace,
    cursor: Option<CursorOverlay>,
) -> PipeWireFrame {
    let width = format.size().width as usize;
    let height = format.size().height as usize;
    let bpp = format_bpp(format.format()) as usize;
    let swaps_rb = format_swaps_rb(format.format());
    let stride = width * bpp;
    let row_len = width * 4;

    let mut pixels = Vec::with_capacity(row_len * height);

    for row in 0..height {
        let src_start = row * stride;
        let src_row = &raw[src_start..src_start + width * bpp];
        for px in src_row.chunks_exact(bpp) {
            if swaps_rb {
                pixels.push(px[2]);
                pixels.push(px[1]);
                pixels.push(px[0]);
                pixels.push(px.get(3).copied().unwrap_or(255));
            } else {
                pixels.push(px[0]);
                pixels.push(px[1]);
                pixels.push(px[2]);
                pixels.push(px.get(3).copied().unwrap_or(255));
            }
        }
    }

    // Composite cursor into frame if metadata was available.
    if let Some(ref cur) = cursor {
        composite_cursor_into_frame(
            &mut pixels,
            width as u32,
            height as u32,
            row_len as u32,
            cur,
        );
    }

    PipeWireFrame {
        pixels,
        width: format.size().width,
        height: format.size().height,
        stride: row_len as u32,
        cursor,
        color_space,
    }
}

// ---------------------------------------------------------------------------
// SPA pod construction
// ---------------------------------------------------------------------------

fn build_enum_format_pod(width_hint: Option<u32>, height_hint: Option<u32>) -> Vec<u8> {
    use pw::spa::pod::Value;
    use pw::spa::utils::{Fraction, Rectangle, SpaTypes};

    let w = width_hint.unwrap_or(1920);
    let h = height_hint.unwrap_or(1080);

    let obj = pw::spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA
        ),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle {
                width: w,
                height: h
            },
            Rectangle {
                width: 1,
                height: 1
            },
            Rectangle {
                width: 8192,
                height: 4320
            }
        ),
        pw::spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 60, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 360, denom: 1 }
        ),
    );

    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::with_capacity(1024)),
        &Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner()
}

// ---------------------------------------------------------------------------
// Convenience: single-frame capture
// ---------------------------------------------------------------------------

pub fn capture_single_frame(
    pipewire_fd: OwnedFd,
    node_id: u32,
    timeout: Duration,
) -> PipeWireResult<PipeWireFrame> {
    let capture = PipeWireCapture::connect(pipewire_fd, node_id, Some(1), None, None)?;
    capture.wait_for_frame(timeout)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bpp() {
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRA), 4);
        assert_eq!(format_bpp(spa::param::video::VideoFormat::RGBA), 4);
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRx), 4);
    }

    #[test]
    fn test_format_swaps_rb() {
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRx));
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        assert!(format_swaps_rb(spa::param::video::VideoFormat::RGBx));
        assert!(format_swaps_rb(spa::param::video::VideoFormat::RGBA));
    }

    #[test]
    fn test_build_enum_format_pod_is_valid() {
        let data = build_enum_format_pod(Some(1920), Some(1080));
        assert!(!data.is_empty());
        let pod = spa::pod::Pod::from_bytes(&data);
        assert!(pod.is_some());
    }

    #[test]
    fn test_build_enum_format_pod_no_hint() {
        let data = build_enum_format_pod(None, None);
        assert!(!data.is_empty());
        let pod = spa::pod::Pod::from_bytes(&data);
        assert!(pod.is_some());
    }

    #[test]
    fn test_color_space_defaults() {
        let cs = ColorSpace::default();
        assert_eq!(cs.range, 1);
        assert_eq!(cs.matrix, 1);
        assert_eq!(cs.matrix_label(), "RGB");
        assert_eq!(cs.range_label(), "full (0-255)");
    }

    #[test]
    fn test_color_space_labels() {
        assert_eq!(
            ColorSpace {
                range: 2,
                matrix: 3
            }
            .matrix_label(),
            "BT.709"
        );
        assert_eq!(
            ColorSpace {
                range: 2,
                matrix: 3
            }
            .range_label(),
            "limited (16-235)"
        );
    }

    #[test]
    fn test_convert_bgra_to_rgba_indirect() {
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        assert!(format_swaps_rb(spa::param::video::VideoFormat::RGBA));
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRA), 4);
    }
}
