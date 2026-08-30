//! Native PipeWire engine for screen capture.
//!
//! Replaces the GStreamer `pipewiresrc` pipeline with direct `libpipewire` API.
//!
//! Architecture:
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
    // PipeWire's BGRx/BGRA memory order is B,G,R,A/x. The public frame data
    // we feed to ffmpeg is always RGBA, so BGR formats need R/B swapped.
    // RGBx/RGBA are already in the desired channel order.
    matches!(
        format,
        spa::param::video::VideoFormat::BGRx | spa::param::video::VideoFormat::BGRA
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

enum PipeWireCoreSource {
    /// XDG ScreenCast portal remote (scoped graph).
    PortalFd(OwnedFd),
    /// Default session socket — KDE `zkde_screencast` / system nodes.
    DefaultSocket,
}

pub struct PipeWireCapture {
    inner: Arc<Mutex<StreamInner>>,
    // Keep the listener alive for the lifetime of the capture. Dropping it
    // unregisters PipeWire callbacks, which means format negotiation can
    // succeed but no process callbacks arrive afterwards (empty 261-byte mp4s).
    _listener: pw::stream::StreamListener<Arc<Mutex<StreamInner>>>,
    _stream: pw::stream::StreamRc,
    _core: pw::core::CoreRc,
    _context: pw::context::ContextRc,
    // Destroyed last so disconnect/stop in Drop still have a live loop.
    _thread_loop: pw::thread_loop::ThreadLoopRc,
}

fn teardown_pw_stream(thread_loop: &pw::thread_loop::ThreadLoopRc, stream: &pw::stream::StreamRc) {
    {
        let _lock = thread_loop.lock();
        let _ = stream.disconnect();
    }
    thread_loop.stop();
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        teardown_pw_stream(&self._thread_loop, &self._stream);
    }
}

impl PipeWireCapture {
    /// Connect using a portal-provided PipeWire remote FD.
    pub fn connect(
        pipewire_fd: OwnedFd,
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
    ) -> PipeWireResult<Self> {
        Self::connect_inner(
            PipeWireCoreSource::PortalFd(pipewire_fd),
            node_id,
            max_frames,
            width_hint,
            height_hint,
        )
    }

    /// Connect to the default session PipeWire socket.
    ///
    /// Used by KDE-native `zkde_screencast_unstable_v1` streams, which publish
    /// a node on the regular session graph rather than a portal-scoped remote.
    pub fn connect_default(
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
    ) -> PipeWireResult<Self> {
        Self::connect_inner(
            PipeWireCoreSource::DefaultSocket,
            node_id,
            max_frames,
            width_hint,
            height_hint,
        )
    }

    fn connect_inner(
        source: PipeWireCoreSource,
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

        // Hold the loop lock while creating objects bound to it (PipeWire rule).
        let _setup_lock = thread_loop.lock();
        let context = pw::context::ContextRc::new(&thread_loop, None)
            .map_err(|e| PipeWireError::Init(format!("Failed to create context: {e}")))?;

        let core = match source {
            PipeWireCoreSource::PortalFd(pipewire_fd) => {
                context.connect_fd_rc(pipewire_fd, None).map_err(|e| {
                    PipeWireError::Init(format!("Failed to connect core via fd: {e}"))
                })?
            }
            PipeWireCoreSource::DefaultSocket => context.connect_rc(None).map_err(|e| {
                PipeWireError::Init(format!("Failed to connect to default PipeWire socket: {e}"))
            })?,
        };

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

        let format_bytes = build_enum_format_pod(width_hint, height_hint);
        let buffers_bytes = build_shm_buffers_pod();
        let format_pod = spa::pod::Pod::from_bytes(&format_bytes)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse format pod".into()))?;
        let buffers_pod = spa::pod::Pod::from_bytes(&buffers_bytes)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse buffers pod".into()))?;
        let mut params = [format_pod, buffers_pod];

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
                let Some(pixel_data) = copy_cpu_frame(&mut datas[..], chunk_size) else {
                    return;
                };
                guard.frames.push_back(pixel_data);
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

        drop(_setup_lock);
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
                teardown_pw_stream(&thread_loop, &stream);
                return Err(PipeWireError::FormatNegotiation);
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        {
            let guard = inner.lock().unwrap();
            if let Some(ref err) = guard.error {
                teardown_pw_stream(&thread_loop, &stream);
                return Err(PipeWireError::Stream(err.clone()));
            }
            if guard.format.is_none() {
                teardown_pw_stream(&thread_loop, &stream);
                return Err(PipeWireError::FormatNegotiation);
            }
        }

        Ok(PipeWireCapture {
            inner,
            _listener,
            _stream: stream,
            _core: core,
            _context: context,
            _thread_loop: thread_loop,
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

        Ok(convert_to_rgba_frame(
            &raw,
            &raw_format,
            color_space,
            cursor,
        ))
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

fn copy_cpu_frame(datas: &mut [spa::buffer::Data], chunk_size: usize) -> Option<Vec<u8>> {
    if datas.is_empty() || chunk_size == 0 || chunk_size > 64 * 1024 * 1024 {
        return None;
    }
    let data = &mut datas[0];
    let kind = data.type_();
    if let Some(mem) = data.data() {
        if chunk_size <= mem.len() {
            return Some(mem[..chunk_size].to_vec());
        }
    }
    if kind == spa::buffer::DataType::MemFd {
        return mmap_memfd(data.fd(), chunk_size);
    }
    None
}

fn mmap_memfd(fd: i32, chunk_size: usize) -> Option<Vec<u8>> {
    if fd < 0 {
        return None;
    }
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
        return None;
    }
    let copied = unsafe { std::slice::from_raw_parts(ptr as *const u8, chunk_size).to_vec() };
    unsafe { libc::munmap(ptr, chunk_size) };
    Some(copied)
}

// ---------------------------------------------------------------------------
// Cursor metadata extraction (raw SPA buffer access)
// ---------------------------------------------------------------------------

// From pipewire spa/buffer/meta.h: SPA_META_Cursor = 5
#[allow(dead_code)]
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
#[allow(dead_code)]
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

fn rgba_copy_plan(raw_len: usize, width: usize, height: usize, bpp: usize) -> Option<usize> {
    if width == 0 || height == 0 || bpp == 0 {
        return None;
    }
    let packed = width.checked_mul(bpp)?;
    let min_len = packed.checked_mul(height)?;
    if raw_len < min_len {
        return None;
    }
    if raw_len.is_multiple_of(height) {
        let stride = raw_len / height;
        if stride >= packed {
            return Some(stride);
        }
    }
    Some(packed)
}

fn convert_to_rgba_frame(
    raw: &[u8],
    format: &spa::param::video::VideoInfoRaw,
    color_space: ColorSpace,
    cursor: Option<CursorOverlay>,
) -> Option<PipeWireFrame> {
    let width = format.size().width as usize;
    let height = format.size().height as usize;
    let bpp = format_bpp(format.format()) as usize;
    let video_format = format.format();
    let swaps_rb = format_swaps_rb(video_format);
    let has_alpha = matches!(
        video_format,
        spa::param::video::VideoFormat::BGRA | spa::param::video::VideoFormat::RGBA
    );
    let stride = rgba_copy_plan(raw.len(), width, height, bpp)?;
    let row_len = width * 4;

    let mut pixels = Vec::with_capacity(row_len * height);

    for row in 0..height {
        let src_start = row * stride;
        let src_row = raw.get(src_start..src_start + width * bpp)?;
        for px in src_row.chunks_exact(bpp) {
            if swaps_rb {
                pixels.push(px[2]);
                pixels.push(px[1]);
                pixels.push(px[0]);
                pixels.push(if has_alpha { px[3] } else { 255 });
            } else {
                pixels.push(px[0]);
                pixels.push(px[1]);
                pixels.push(px[2]);
                pixels.push(if has_alpha { px[3] } else { 255 });
            }
        }
    }

    if let Some(ref cur) = cursor {
        composite_cursor_into_frame(
            &mut pixels,
            width as u32,
            height as u32,
            row_len as u32,
            cur,
        );
    }

    Some(PipeWireFrame {
        pixels,
        width: format.size().width,
        height: format.size().height,
        stride: row_len as u32,
        cursor,
        color_space,
    })
}

// ---------------------------------------------------------------------------
// SPA pod construction
// ---------------------------------------------------------------------------

fn build_shm_buffers_pod() -> Vec<u8> {
    use pw::spa::pod::{Object, Property, Value};
    use pw::spa::utils::SpaTypes;

    let mem_ptr = spa::buffer::DataType::MemPtr.as_raw();
    let mem_fd = spa::buffer::DataType::MemFd.as_raw();
    let data_type = (1 << mem_ptr) | (1 << mem_fd);
    let obj = Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: spa::param::ParamType::Buffers.as_raw(),
        properties: vec![Property::new(
            spa_sys::SPA_PARAM_BUFFERS_dataType,
            Value::Int(data_type),
        )],
    };
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::with_capacity(256)),
        &Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner()
}

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
    capture_single_frame_with_min_frames(pipewire_fd, node_id, timeout, 1)
}

pub fn capture_single_frame_with_min_frames(
    pipewire_fd: OwnedFd,
    node_id: u32,
    timeout: Duration,
    min_frames_before_return: u64,
) -> PipeWireResult<PipeWireFrame> {
    let capture = PipeWireCapture::connect(
        pipewire_fd,
        node_id,
        Some(min_frames_before_return),
        None,
        None,
    )?;
    let deadline = Instant::now() + timeout;
    let required = min_frames_before_return.max(1);

    loop {
        if capture.frames_consumed() + capture.inner.lock().unwrap().frames.len() as u64 >= required
        {
            break;
        }
        if Instant::now() > deadline {
            return Err(PipeWireError::Timeout(timeout));
        }
        if let Some(err) = capture.error_message() {
            return Err(PipeWireError::Stream(err));
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let mut last_frame = None;
    while capture.frames_consumed() < required {
        last_frame = Some(capture.wait_for_frame(timeout)?);
    }

    last_frame.ok_or(PipeWireError::NoFrame)
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
        assert!(format_swaps_rb(spa::param::video::VideoFormat::BGRx));
        assert!(format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::RGBx));
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::RGBA));
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
    fn test_build_shm_buffers_pod_is_valid() {
        let data = build_shm_buffers_pod();
        assert!(!data.is_empty());
        assert!(spa::pod::Pod::from_bytes(&data).is_some());
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
        assert!(format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::RGBA));
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRA), 4);
    }

    #[test]
    fn rgba_copy_plan_rejects_area_crop_hint_mismatch() {
        let packed_1080p = 1920 * 1080 * 4;
        assert_eq!(rgba_copy_plan(packed_1080p, 1920, 1080, 4), Some(1920 * 4));
        assert_eq!(rgba_copy_plan(640 * 480 * 4, 1920, 1080, 4), None);
        assert_eq!(rgba_copy_plan(0, 1920, 1080, 4), None);
        let padded = 1920 * 4 + 64;
        assert_eq!(rgba_copy_plan(padded * 1080, 1920, 1080, 4), Some(padded));
    }
}
