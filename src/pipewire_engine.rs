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
    /// Native 4-byte pixel data in the negotiated SPA format order
    /// (usually BGRx — see `PipeWireCapture::pix_fmt` for the ffmpeg label).
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Row stride in bytes (= width * 4 for RGBA).
    pub stride: u32,
    /// Cursor overlay metadata (from SPA_META_Cursor, when available).
    pub cursor: Option<CursorOverlay>,
    /// Color space from negotiated format.
    pub color_space: ColorSpace,
    /// Compositor capture time on the local monotonic clock, or buffer receipt
    /// time when the producer does not expose that clock domain.
    pub captured_at_us: i64,
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

// ---------------------------------------------------------------------------
// Internal shared state
// ---------------------------------------------------------------------------

const CONTINUOUS_FRAME_QUEUE_CAPACITY: usize = 1;

fn push_latest_continuous_frame(
    frames: &mut std::collections::VecDeque<Vec<u8>>,
    pixel_data: Vec<u8>,
) -> usize {
    let mut dropped = 0;
    while frames.len() >= CONTINUOUS_FRAME_QUEUE_CAPACITY {
        frames.pop_front();
        dropped += 1;
    }
    frames.push_back(pixel_data);
    dropped
}

struct StreamInner {
    format: Option<NegotiatedFormat>,
    raw_format: Option<spa::param::video::VideoInfoRaw>,
    frames: std::collections::VecDeque<Vec<u8>>,
    frame_times: std::collections::VecDeque<i64>,
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
            frame_times: std::collections::VecDeque::new(),
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
        let header_bytes = build_header_meta_pod();
        let header_pod = spa::pod::Pod::from_bytes(&header_bytes).ok_or_else(|| {
            PipeWireError::Connect("Failed to parse frame header metadata pod".into())
        })?;
        let mut params = [format_pod, buffers_pod, header_pod];

        let inner_clone = Arc::clone(&inner);
        let monotonic_header = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
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
            .process(move |_stream, inner| {
                let mut guard = match inner.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                if let Some(max) = guard.max_frames {
                    if guard.frames.len() as u64 >= max {
                        return;
                    }
                }

                let mut buffer = match CaptureBuffer::dequeue(_stream) {
                    Some(buffer) => buffer,
                    None => {
                        eprintln!("[pipewire] Out of buffers!");
                        return;
                    }
                };

                let received_at_us = gstreamer::glib::monotonic_time();
                let Some((pixel_data, header_pts)) = buffer.copy_frame() else {
                    return;
                };
                let captured_at_us = frame_capture_time_us(header_pts, received_at_us, monotonic_header);
                if guard.frames.is_empty() && guard.frames_consumed == 0 {
                    eprintln!("[pipewire] first frame clock header_pts_ns={header_pts:?} received_us={received_at_us} captured_us={captured_at_us}");
                }

                // A recording encoder can be slower than the compositor at
                // large resolutions. Never retain every full RGBA frame while
                // it catches up: a 1920x1200 frame is about 9 MiB, so an
                // unbounded queue can OOM-kill the daemon (and its tray icon)
                // within seconds. Recording needs the freshest frame, whereas
                // finite still capture must preserve every requested frame.
                if guard.max_frames.is_none() {
                    let dropped = push_latest_continuous_frame(&mut guard.frames, pixel_data);
                    for _ in 0..dropped {
                        guard.cursor_queue.pop_front();
                        guard.frame_times.pop_front();
                    }
                } else {
                    guard.frames.push_back(pixel_data);
                }
                guard.frame_times.push_back(captured_at_us);
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
        let _ = guard.cursor_queue.pop_front();
        let captured_at_us = guard
            .frame_times
            .pop_front()
            .unwrap_or_else(gstreamer::glib::monotonic_time);

        guard.frames_consumed += 1;
        drop(guard);

        // Capture buffers stay in their native layout; the old per-pixel
        // BGR→RGBA swap cost ~8ms/1080p and ~27ms/4K per frame (measured).
        // Feed native 4-byte pixels (ffmpeg `-pix_fmt` matches) and strip row
        // padding by whole rows only — usually a zero-copy move when packed.
        let width = raw_format.size().width as usize;
        let height = raw_format.size().height as usize;
        let bpp = format_bpp(raw_format.format()) as usize;
        let stride = match rgba_copy_plan(raw.len(), width, height, bpp) {
            Some(s) => s,
            None => return Ok(None),
        };
        let row_len = width * bpp;
        let pixels = if stride == row_len {
            raw
        } else {
            let mut packed = Vec::with_capacity(row_len * height);
            for row in 0..height {
                match raw.get(row * stride..row * stride + row_len) {
                    Some(r) => packed.extend_from_slice(r),
                    None => return Ok(None),
                }
            }
            packed
        };

        Ok(Some(PipeWireFrame {
            pixels,
            width: width as u32,
            height: height as u32,
            stride: row_len as u32,
            cursor: None,
            color_space,
            captured_at_us,
        }))
    }

    /// ffmpeg `-pix_fmt` matching the negotiated SPA format, so the raw pipe
    /// carries native bytes with no channel swap.
    pub fn pix_fmt(&self) -> &'static str {
        let format = self
            .inner
            .lock()
            .unwrap()
            .raw_format
            .as_ref()
            .map(|f| f.format());
        match format {
            Some(spa::param::video::VideoFormat::BGRA) => "bgra",
            Some(spa::param::video::VideoFormat::RGBx) => "rgb0",
            Some(spa::param::video::VideoFormat::RGBA) => "rgba",
            _ => "bgr0",
        }
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

struct CaptureBuffer<'a> {
    raw: std::ptr::NonNull<pw::sys::pw_buffer>,
    stream: &'a pw::stream::Stream,
}

impl<'a> CaptureBuffer<'a> {
    fn dequeue(stream: &'a pw::stream::Stream) -> Option<Self> {
        let raw = std::ptr::NonNull::new(unsafe { stream.dequeue_raw_buffer() })?;
        Some(Self { raw, stream })
    }

    fn copy_frame(&mut self) -> Option<(Vec<u8>, Option<i64>)> {
        let buffer = unsafe { self.raw.as_ref().buffer.as_mut() }?;
        unsafe { copy_spa_frame(buffer) }
    }
}

impl Drop for CaptureBuffer<'_> {
    fn drop(&mut self) {
        unsafe { self.stream.queue_raw_buffer(self.raw.as_ptr()) };
    }
}

/// Copy a dequeued buffer before returning its pixel storage to PipeWire.
///
/// # Safety
/// The buffer's metadata and data arrays must remain valid for the call.
unsafe fn copy_spa_frame(buffer: &mut spa_sys::spa_buffer) -> Option<(Vec<u8>, Option<i64>)> {
    if buffer.n_datas == 0 || buffer.datas.is_null() {
        return None;
    }
    let header = unsafe {
        spa_sys::spa_buffer_find_meta_data(
            buffer,
            spa_sys::SPA_META_Header,
            std::mem::size_of::<spa_sys::spa_meta_header>(),
        )
        .cast::<spa_sys::spa_meta_header>()
        .as_ref()
    };
    if header.is_some_and(|header| header.flags & spa_sys::SPA_META_HEADER_FLAG_CORRUPTED != 0) {
        return None;
    }
    let pts = header.map(|header| header.pts);
    let datas = unsafe {
        std::slice::from_raw_parts_mut(
            buffer.datas.cast::<spa::buffer::Data>(),
            buffer.n_datas as usize,
        )
    };
    let chunk_size = datas[0].chunk().size() as usize;
    Some((copy_cpu_frame(datas, chunk_size)?, pts))
}

fn frame_capture_time_us(pts_ns: Option<i64>, received_at_us: i64, monotonic_header: bool) -> i64 {
    match pts_ns.filter(|&pts| monotonic_header && pts > 0 && pts / 1_000 <= received_at_us) {
        Some(pts) => pts / 1_000,
        None => received_at_us,
    }
}

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

fn build_header_meta_pod() -> Vec<u8> {
    use pw::spa::pod::{Object, Property, Value};
    use pw::spa::utils::{Id, SpaTypes};

    let object = Object {
        type_: SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            Property::new(
                spa_sys::SPA_PARAM_META_type,
                Value::Id(Id(spa_sys::SPA_META_Header)),
            ),
            Property::new(
                spa_sys::SPA_PARAM_META_size,
                Value::Int(std::mem::size_of::<spa_sys::spa_meta_header>() as i32),
            ),
        ],
    };
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::with_capacity(128)),
        &Value::Object(object),
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
    fn compositor_frame_clock_uses_monotonic_header_and_falls_back_for_other_domains() {
        assert_eq!(
            frame_capture_time_us(Some(9_000_123_000), 9_050_000, true),
            9_000_123
        );
        assert_eq!(
            frame_capture_time_us(Some(9_000_123_000), 9_050_000, false),
            9_050_000
        );
        for pts in [None, Some(-1), Some(0), Some(10_000_000_000)] {
            assert_eq!(frame_capture_time_us(pts, 9_050_000, true), 9_050_000);
        }
    }

    #[test]
    fn frame_header_metadata_request_is_a_valid_pod() {
        assert!(spa::pod::Pod::from_bytes(&build_header_meta_pod()).is_some());
    }

    #[test]
    fn frame_metadata_stays_with_its_pixels_and_corrupted_buffers_are_rejected() {
        let mut pixels = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut chunk = spa_sys::spa_chunk {
            offset: 0,
            size: pixels.len() as u32,
            stride: 8,
            flags: 0,
        };
        let mut data = spa_sys::spa_data {
            type_: spa_sys::SPA_DATA_MemPtr,
            flags: 0,
            fd: -1,
            mapoffset: 0,
            maxsize: pixels.len() as u32,
            data: pixels.as_mut_ptr().cast(),
            chunk: &mut chunk,
        };
        let mut header = spa_sys::spa_meta_header {
            flags: 0,
            offset: 0,
            pts: 9_000_123_000,
            dts_offset: 0,
            seq: 1,
        };
        let mut metadata = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_Header,
            size: std::mem::size_of::<spa_sys::spa_meta_header>() as u32,
            data: (&mut header as *mut spa_sys::spa_meta_header).cast(),
        };
        let mut buffer = spa_sys::spa_buffer {
            n_metas: 1,
            n_datas: 1,
            metas: &mut metadata,
            datas: &mut data,
        };
        assert_eq!(
            unsafe { copy_spa_frame(&mut buffer) },
            Some((pixels.clone(), Some(header.pts)))
        );
        header.flags = spa_sys::SPA_META_HEADER_FLAG_CORRUPTED;
        assert_eq!(header.flags, spa_sys::SPA_META_HEADER_FLAG_CORRUPTED);
        assert!(unsafe { copy_spa_frame(&mut buffer) }.is_none());
        buffer.n_metas = 0;
        buffer.metas = std::ptr::null_mut();
        assert_eq!(
            unsafe { copy_spa_frame(&mut buffer) },
            Some((pixels.clone(), None))
        );
        buffer.n_datas = 0;
        buffer.datas = std::ptr::null_mut();
        assert!(unsafe { copy_spa_frame(&mut buffer) }.is_none());
    }

    #[test]
    fn test_format_bpp() {
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRA), 4);
        assert_eq!(format_bpp(spa::param::video::VideoFormat::RGBA), 4);
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRx), 4);
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
    fn rgba_copy_plan_rejects_area_crop_hint_mismatch() {
        let packed_1080p = 1920 * 1080 * 4;
        assert_eq!(rgba_copy_plan(packed_1080p, 1920, 1080, 4), Some(1920 * 4));
        assert_eq!(rgba_copy_plan(640 * 480 * 4, 1920, 1080, 4), None);
        assert_eq!(rgba_copy_plan(0, 1920, 1080, 4), None);
        let padded = 1920 * 4 + 64;
        assert_eq!(rgba_copy_plan(padded * 1080, 1920, 1080, 4), Some(padded));
    }

    #[test]
    fn continuous_capture_queue_keeps_only_the_freshest_full_frame() {
        let mut frames = std::collections::VecDeque::new();
        assert_eq!(push_latest_continuous_frame(&mut frames, vec![1]), 0);
        assert_eq!(push_latest_continuous_frame(&mut frames, vec![2]), 1);
        assert_eq!(push_latest_continuous_frame(&mut frames, vec![3]), 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames.front(), Some(&vec![3]));
    }
}
