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
    /// Row stride in bytes (= width * 4 for the normalized frame bytes).
    pub stride: u32,
    /// SPA format that applied when this frame was dequeued.
    pub video_format: spa::param::video::VideoFormat,
    /// Crop metadata copied from this frame's SPA buffer.
    pub crop: FrameCropMetadata,
    /// Cursor bitmap metadata when this buffer carries a valid bitmap.
    pub cursor: Option<CursorOverlay>,
    /// Cursor validity and hotspot position for this buffer, independent of
    /// whether the bitmap changed.
    pub cursor_metadata: CursorMetadataState,
    /// Color space from negotiated format.
    pub color_space: ColorSpace,
    /// Compositor capture time on the local monotonic clock, or buffer receipt
    /// time when the producer does not expose that clock domain.
    pub captured_at_us: i64,
}

/// Crop rectangle from one SPA video buffer, relative to that frame's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCrop {
    /// Horizontal offset from the frame's left edge.
    pub x: i32,
    /// Vertical offset from the frame's top edge.
    pub y: i32,
    /// Crop width in pixels.
    pub width: u32,
    /// Crop height in pixels.
    pub height: u32,
}

/// SPA crop metadata state copied from a video buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCropMetadata {
    /// The buffer did not carry SPA crop metadata.
    Missing,
    /// The buffer carried a crop rectangle; consumers must validate its bounds.
    Present(FrameCrop),
    /// The buffer carried malformed or incomplete crop metadata.
    Invalid,
}

/// Cursor bitmap and position extracted from PipeWire buffer metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorOverlay {
    /// RGBA pixel data for the cursor image.
    pub bitmap: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Screen position of the bitmap's top-left corner.
    pub x: i32,
    pub y: i32,
    /// Hotspot offset within the bitmap (the click point).
    pub hotspot_x: i32,
    pub hotspot_y: i32,
}

/// Cursor hotspot position copied from a SPA cursor metadata update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    /// Hotspot position in the captured stream's pixel coordinates.
    pub x: i32,
    /// Hotspot position in the captured stream's pixel coordinates.
    pub y: i32,
    /// Hotspot offset within a newly supplied bitmap, when present.
    pub hotspot_x: i32,
    /// Hotspot offset within a newly supplied bitmap, when present.
    pub hotspot_y: i32,
}

/// Cursor metadata state for one SPA video buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMetadataState {
    /// No cursor metadata was attached to this buffer.
    Missing,
    /// The metadata header was incomplete or malformed.
    Invalid,
    /// The producer explicitly reported no active cursor (id 0).
    Hidden,
    /// A valid cursor hotspot position was attached.
    Present(CursorPosition),
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
    /// SPA pixel format negotiated for this stream format.
    pub video_format: spa::param::video::VideoFormat,
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

fn video_format_preference(prefer_alpha: bool) -> [spa::param::video::VideoFormat; 4] {
    if prefer_alpha {
        [
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx,
        ]
    } else {
        [
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
        ]
    }
}

// ---------------------------------------------------------------------------
// Internal shared state
// ---------------------------------------------------------------------------

const CONTINUOUS_FRAME_QUEUE_CAPACITY: usize = 1;

fn push_latest_continuous_frame<T>(frames: &mut std::collections::VecDeque<T>, frame: T) -> usize {
    let mut dropped = 0;
    while frames.len() >= CONTINUOUS_FRAME_QUEUE_CAPACITY {
        frames.pop_front();
        dropped += 1;
    }
    frames.push_back(frame);
    dropped
}

struct QueuedFrame {
    pixels: Vec<u8>,
    format: spa::param::video::VideoInfoRaw,
    stride: i32,
    crop: FrameCropMetadata,
    cursor: Option<CursorOverlay>,
    cursor_metadata: CursorMetadataState,
    captured_at_us: i64,
}

struct StreamInner {
    format: Option<NegotiatedFormat>,
    raw_format: Option<spa::param::video::VideoInfoRaw>,
    frames: std::collections::VecDeque<QueuedFrame>,
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
            false,
            false,
        )
    }

    /// Connect to a portal window stream, preferring an alpha-bearing format
    /// and optionally requesting per-buffer cursor metadata.
    pub fn connect_window(
        pipewire_fd: OwnedFd,
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
        cursor_metadata: bool,
    ) -> PipeWireResult<Self> {
        Self::connect_inner(
            PipeWireCoreSource::PortalFd(pipewire_fd),
            node_id,
            max_frames,
            width_hint,
            height_hint,
            true,
            cursor_metadata,
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
            false,
            false,
        )
    }

    fn connect_inner(
        source: PipeWireCoreSource,
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
        prefer_alpha: bool,
        cursor_metadata: bool,
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

        let format_bytes = build_enum_format_pod_with_alpha(width_hint, height_hint, prefer_alpha);
        let buffers_bytes = build_shm_buffers_pod();
        let format_pod = spa::pod::Pod::from_bytes(&format_bytes)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse format pod".into()))?;
        let buffers_pod = spa::pod::Pod::from_bytes(&buffers_bytes)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse buffers pod".into()))?;
        let header_bytes = build_header_meta_pod();
        let header_pod = spa::pod::Pod::from_bytes(&header_bytes).ok_or_else(|| {
            PipeWireError::Connect("Failed to parse frame header metadata pod".into())
        })?;
        let crop_bytes = build_crop_meta_pod();
        let crop_pod = spa::pod::Pod::from_bytes(&crop_bytes).ok_or_else(|| {
            PipeWireError::Connect("Failed to parse frame crop metadata pod".into())
        })?;
        let cursor_bytes = cursor_metadata.then(build_cursor_meta_pod);
        let mut params = vec![format_pod, buffers_pod, header_pod, crop_pod];
        if let Some(cursor_bytes) = cursor_bytes.as_ref() {
            let cursor_pod = spa::pod::Pod::from_bytes(cursor_bytes).ok_or_else(|| {
                PipeWireError::Connect("Failed to parse frame cursor metadata pod".into())
            })?;
            params.push(cursor_pod);
        }

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
                    video_format: info.format(),
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
                let Some(raw_format) = guard.raw_format else {
                    return;
                };
                let Some((
                    pixel_data,
                    stride,
                    crop,
                    cursor,
                    cursor_metadata,
                    header_pts,
                )) = buffer.copy_frame()
                else {
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
                let frame = QueuedFrame {
                    pixels: pixel_data,
                    format: raw_format,
                    stride,
                    crop,
                    cursor,
                    cursor_metadata,
                    captured_at_us,
                };
                if guard.max_frames.is_none() {
                    push_latest_continuous_frame(&mut guard.frames, frame);
                } else {
                    guard.frames.push_back(frame);
                }
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

        let validation_error = {
            let guard = inner.lock().unwrap();
            if let Some(ref err) = guard.error {
                Some(PipeWireError::Stream(err.clone()))
            } else if guard.format.is_none() {
                Some(PipeWireError::FormatNegotiation)
            } else {
                None
            }
        };
        if let Some(error) = validation_error {
            teardown_pw_stream(&thread_loop, &stream);
            return Err(error);
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

        let queued = match guard.frames.pop_front() {
            Some(frame) => frame,
            None => return Ok(None),
        };
        guard.frames_consumed += 1;
        drop(guard);

        // Capture buffers stay in their native layout; the old per-pixel
        // BGR→RGBA swap cost ~8ms/1080p and ~27ms/4K per frame (measured).
        // Feed native 4-byte pixels (ffmpeg `-pix_fmt` matches) and strip row
        // padding by whole rows only — usually a zero-copy move when packed.
        let width = queued.format.size().width;
        let height = queued.format.size().height;
        let bpp = format_bpp(queued.format.format()) as usize;
        let Some(pixels) = normalize_frame_rows(
            queued.pixels,
            queued.stride,
            width as usize,
            height as usize,
            bpp,
        ) else {
            return Ok(None);
        };

        Ok(Some(PipeWireFrame {
            pixels,
            width,
            height,
            stride: width.saturating_mul(bpp as u32),
            video_format: queued.format.format(),
            crop: queued.crop,
            cursor: queued.cursor,
            cursor_metadata: queued.cursor_metadata,
            color_space: ColorSpace {
                range: queued.format.color_range(),
                matrix: queued.format.color_matrix(),
            },
            captured_at_us: queued.captured_at_us,
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
        format.map(pix_fmt_for_format).unwrap_or("bgr0")
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

/// Return the ffmpeg pixel-format label matching a native SPA video format.
pub(crate) fn pix_fmt_for_format(format: spa::param::video::VideoFormat) -> &'static str {
    match format {
        spa::param::video::VideoFormat::BGRA => "bgra",
        spa::param::video::VideoFormat::RGBx => "rgb0",
        spa::param::video::VideoFormat::RGBA => "rgba",
        _ => "bgr0",
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

    fn copy_frame(
        &mut self,
    ) -> Option<(
        Vec<u8>,
        i32,
        FrameCropMetadata,
        Option<CursorOverlay>,
        CursorMetadataState,
        Option<i64>,
    )> {
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
unsafe fn copy_spa_frame(
    buffer: &mut spa_sys::spa_buffer,
) -> Option<(
    Vec<u8>,
    i32,
    FrameCropMetadata,
    Option<CursorOverlay>,
    CursorMetadataState,
    Option<i64>,
)> {
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
    let crop = unsafe { copy_spa_crop(buffer) };
    let (cursor_metadata, cursor) = unsafe { copy_spa_cursor(buffer) };
    let datas = unsafe {
        std::slice::from_raw_parts_mut(
            buffer.datas.cast::<spa::buffer::Data>(),
            buffer.n_datas as usize,
        )
    };
    let raw_chunk = unsafe { datas[0].as_raw().chunk.as_ref() }?;
    let chunk_size = raw_chunk.size as usize;
    let chunk_offset = raw_chunk.offset as usize;
    let chunk_stride = raw_chunk.stride;
    Some((
        copy_cpu_frame(&mut datas[0], chunk_offset, chunk_size)?,
        chunk_stride,
        crop,
        cursor,
        cursor_metadata,
        pts,
    ))
}

unsafe fn copy_spa_crop(buffer: &spa_sys::spa_buffer) -> FrameCropMetadata {
    let metadata = unsafe { spa_sys::spa_buffer_find_meta(buffer, spa_sys::SPA_META_VideoCrop) };
    let Some(metadata) = (unsafe { metadata.as_ref() }) else {
        return FrameCropMetadata::Missing;
    };
    if metadata.size < std::mem::size_of::<spa_sys::spa_meta_region>() as u32
        || metadata.data.is_null()
    {
        return FrameCropMetadata::Invalid;
    }
    let region = unsafe { &*metadata.data.cast::<spa_sys::spa_meta_region>() };
    FrameCropMetadata::Present(FrameCrop {
        x: region.region.position.x,
        y: region.region.position.y,
        width: region.region.size.width,
        height: region.region.size.height,
    })
}

fn frame_capture_time_us(pts_ns: Option<i64>, received_at_us: i64, monotonic_header: bool) -> i64 {
    match pts_ns.filter(|&pts| monotonic_header && pts > 0 && pts / 1_000 <= received_at_us) {
        Some(pts) => pts / 1_000,
        None => received_at_us,
    }
}

fn copy_cpu_frame(
    data: &mut spa::buffer::Data,
    chunk_offset: usize,
    chunk_size: usize,
) -> Option<Vec<u8>> {
    const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
    if chunk_size == 0 || chunk_size > MAX_FRAME_BYTES {
        return None;
    }
    let kind = data.type_();
    let max_size = data.as_raw().maxsize as usize;
    if max_size == 0 || chunk_size > max_size {
        return None;
    }
    if let Some(mem) = data.data() {
        return copy_circular_chunk(mem, chunk_offset, chunk_size);
    }
    if kind == spa::buffer::DataType::MemFd {
        let map_offset = data.as_raw().mapoffset as usize;
        let mapped = mmap_memfd(data.fd(), max_size, map_offset)?;
        return copy_circular_chunk(&mapped, chunk_offset, chunk_size);
    }
    None
}

fn copy_circular_chunk(data: &[u8], chunk_offset: usize, chunk_size: usize) -> Option<Vec<u8>> {
    if data.is_empty() || chunk_size > data.len() {
        return None;
    }
    let offset = chunk_offset % data.len();
    let first_len = chunk_size.min(data.len() - offset);
    let mut copied = Vec::with_capacity(chunk_size);
    copied.extend_from_slice(data.get(offset..offset + first_len)?);
    if first_len < chunk_size {
        copied.extend_from_slice(data.get(..chunk_size - first_len)?);
    }
    Some(copied)
}

fn mmap_memfd(fd: i32, map_size: usize, map_offset: usize) -> Option<Vec<u8>> {
    if fd < 0 || map_size == 0 || map_size > 64 * 1024 * 1024 {
        return None;
    }
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            map_offset as libc::off_t,
        )
    };
    if ptr == libc::MAP_FAILED {
        return None;
    }
    let copied = unsafe { std::slice::from_raw_parts(ptr as *const u8, map_size).to_vec() };
    unsafe { libc::munmap(ptr, map_size) };
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

/// Copy cursor metadata while the producer's SPA buffer is still dequeued.
///
/// # Safety
/// The SPA buffer's metadata array and referenced payloads must remain valid
/// for the call.
unsafe fn copy_spa_cursor(
    buffer: &spa_sys::spa_buffer,
) -> (CursorMetadataState, Option<CursorOverlay>) {
    let metadata = unsafe { spa_sys::spa_buffer_find_meta(buffer, SPA_META_CURSOR) };
    let Some(metadata) = (unsafe { metadata.as_ref() }) else {
        return (CursorMetadataState::Missing, None);
    };
    if metadata.data.is_null()
        || metadata.size < std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32
    {
        return (CursorMetadataState::Invalid, None);
    }

    let cursor = unsafe { &*metadata.data.cast::<spa_sys::spa_meta_cursor>() };
    if cursor.id == 0 {
        return (CursorMetadataState::Hidden, None);
    }
    let position = CursorPosition {
        x: cursor.position.x,
        y: cursor.position.y,
        hotspot_x: cursor.hotspot.x,
        hotspot_y: cursor.hotspot.y,
    };
    let state = CursorMetadataState::Present(position);
    let cursor_offset = cursor.bitmap_offset as usize;
    let metadata_len = metadata.size as usize;
    let Some(bitmap_header_end) =
        cursor_offset.checked_add(std::mem::size_of::<spa_sys::spa_meta_bitmap>())
    else {
        return (state, None);
    };
    if cursor_offset < std::mem::size_of::<spa_sys::spa_meta_cursor>()
        || bitmap_header_end > metadata_len
    {
        return (state, None);
    }

    let bitmap = unsafe {
        &*metadata
            .data
            .cast::<u8>()
            .add(cursor_offset)
            .cast::<spa_sys::spa_meta_bitmap>()
    };
    if bitmap.format != spa::param::video::VideoFormat::RGBA.as_raw()
        || bitmap.offset < std::mem::size_of::<spa_sys::spa_meta_bitmap>() as u32
        || bitmap.stride <= 0
    {
        return (state, None);
    }
    let width = bitmap.size.width;
    let height = bitmap.size.height;
    let Some(row_bytes) = (width as usize).checked_mul(4) else {
        return (state, None);
    };
    let stride = bitmap.stride as usize;
    let Some(pixel_bytes) = stride.checked_mul(height as usize) else {
        return (state, None);
    };
    let Some(pixel_start) = cursor_offset.checked_add(bitmap.offset as usize) else {
        return (state, None);
    };
    let Some(pixel_end) = pixel_start.checked_add(pixel_bytes) else {
        return (state, None);
    };
    if width == 0
        || height == 0
        || stride < row_bytes
        || pixel_bytes > 64 * 1024 * 1024
        || pixel_end > metadata_len
    {
        return (state, None);
    }
    let Some(x) = position.x.checked_sub(position.hotspot_x) else {
        return (state, None);
    };
    let Some(y) = position.y.checked_sub(position.hotspot_y) else {
        return (state, None);
    };
    let Some(output_len) = row_bytes.checked_mul(height as usize) else {
        return (state, None);
    };
    let pixel_data = unsafe { metadata.data.cast::<u8>().add(pixel_start) };
    let mut rgba = Vec::new();
    if rgba.try_reserve_exact(output_len).is_err() {
        return (state, None);
    }
    for row in 0..height as usize {
        let Some(offset) = row.checked_mul(stride) else {
            return (state, None);
        };
        let source = unsafe { std::slice::from_raw_parts(pixel_data.add(offset), row_bytes) };
        rgba.extend_from_slice(source);
    }

    (
        state,
        Some(CursorOverlay {
            bitmap: rgba,
            width,
            height,
            x,
            y,
            hotspot_x: position.hotspot_x,
            hotspot_y: position.hotspot_y,
        }),
    )
}

// ---------------------------------------------------------------------------
// Frame format conversion
// ---------------------------------------------------------------------------

#[cfg(test)]
fn rgba_copy_plan(raw_len: usize, width: usize, height: usize, bpp: usize) -> Option<usize> {
    frame_copy_plan(raw_len, 0, width, height, bpp).map(|(stride, _)| stride)
}

fn frame_copy_plan(
    raw_len: usize,
    frame_stride: i32,
    width: usize,
    height: usize,
    bpp: usize,
) -> Option<(usize, bool)> {
    if width == 0 || height == 0 || bpp == 0 {
        return None;
    }
    let packed = width.checked_mul(bpp)?;
    let reverse_rows = frame_stride < 0;
    let stride = if frame_stride == 0 {
        if raw_len.is_multiple_of(height) && raw_len / height >= packed {
            raw_len / height
        } else {
            packed
        }
    } else {
        frame_stride.unsigned_abs() as usize
    };
    if stride < packed {
        return None;
    }
    let required = stride
        .checked_mul(height.saturating_sub(1))?
        .checked_add(packed)?;
    (raw_len >= required).then_some((stride, reverse_rows))
}

fn normalize_frame_rows(
    mut raw: Vec<u8>,
    frame_stride: i32,
    width: usize,
    height: usize,
    bpp: usize,
) -> Option<Vec<u8>> {
    let (stride, reverse_rows) = frame_copy_plan(raw.len(), frame_stride, width, height, bpp)?;
    let row_len = width.checked_mul(bpp)?;
    let output_len = row_len.checked_mul(height)?;
    if stride == row_len && !reverse_rows {
        raw.truncate(output_len);
        return Some(raw);
    }
    let mut packed = Vec::new();
    packed.try_reserve_exact(output_len).ok()?;
    for row in 0..height {
        let source_row = if reverse_rows { height - row - 1 } else { row };
        let start = source_row.checked_mul(stride)?;
        packed.extend_from_slice(raw.get(start..start.checked_add(row_len)?)?);
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
    build_frame_meta_pod(
        spa_sys::SPA_META_Header,
        std::mem::size_of::<spa_sys::spa_meta_header>(),
    )
}

fn build_crop_meta_pod() -> Vec<u8> {
    build_frame_meta_pod(
        spa_sys::SPA_META_VideoCrop,
        std::mem::size_of::<spa_sys::spa_meta_region>(),
    )
}

fn build_cursor_meta_pod() -> Vec<u8> {
    const CURSOR_BITMAP_SIDE: usize = 384;
    let size = std::mem::size_of::<spa_sys::spa_meta_cursor>()
        + std::mem::size_of::<spa_sys::spa_meta_bitmap>()
        + CURSOR_BITMAP_SIDE * CURSOR_BITMAP_SIDE * 4;
    build_frame_meta_pod(SPA_META_CURSOR, size)
}

fn build_frame_meta_pod(meta_type: u32, size: usize) -> Vec<u8> {
    use pw::spa::pod::{Object, Property, Value};
    use pw::spa::utils::{Id, SpaTypes};

    let object = Object {
        type_: SpaTypes::ObjectParamMeta.as_raw(),
        id: spa::param::ParamType::Meta.as_raw(),
        properties: vec![
            Property::new(spa_sys::SPA_PARAM_META_type, Value::Id(Id(meta_type))),
            Property::new(spa_sys::SPA_PARAM_META_size, Value::Int(size as i32)),
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

#[cfg(test)]
fn build_enum_format_pod(width_hint: Option<u32>, height_hint: Option<u32>) -> Vec<u8> {
    build_enum_format_pod_with_alpha(width_hint, height_hint, false)
}

fn build_enum_format_pod_with_alpha(
    width_hint: Option<u32>,
    height_hint: Option<u32>,
    prefer_alpha: bool,
) -> Vec<u8> {
    use pw::spa::pod::Value;
    use pw::spa::utils::{Fraction, Rectangle, SpaTypes};

    let w = width_hint.unwrap_or(1920);
    let h = height_hint.unwrap_or(1080);
    let formats = video_format_preference(prefer_alpha);

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
            formats[0],
            formats[1],
            formats[2],
            formats[3]
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

/// Capture one still frame with pixels normalized to RGBA.
pub fn capture_single_frame(
    pipewire_fd: OwnedFd,
    node_id: u32,
    timeout: Duration,
) -> PipeWireResult<PipeWireFrame> {
    capture_single_frame_with_min_frames(pipewire_fd, node_id, timeout, 1)
}

/// Capture an RGBA still after receiving the requested number of fresh frames.
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

    let mut frame = last_frame.ok_or(PipeWireError::NoFrame)?;
    convert_still_pixels_to_rgba(&mut frame.pixels, pix_fmt_for_format(frame.video_format));
    Ok(frame)
}

fn convert_still_pixels_to_rgba(pixels: &mut [u8], format: &str) {
    let swap_red_blue = matches!(format, "bgr0" | "bgra");
    let opaque = matches!(format, "bgr0" | "rgb0");
    if !swap_red_blue && !opaque {
        return;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        if swap_red_blue {
            pixel.swap(0, 2);
        }
        if opaque {
            pixel[3] = 255;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cursor_metadata<R>(
        cursor: spa_sys::spa_meta_cursor,
        bitmap: Option<(spa_sys::spa_meta_bitmap, Vec<u8>)>,
        f: impl FnOnce(&spa_sys::spa_buffer) -> R,
    ) -> R {
        let cursor_size = std::mem::size_of::<spa_sys::spa_meta_cursor>();
        let bitmap_end = bitmap
            .as_ref()
            .and_then(|(bitmap, bytes)| {
                let payload_end = (bitmap.offset as usize).checked_add(bytes.len())?;
                (cursor.bitmap_offset as usize)
                    .checked_add(std::mem::size_of::<spa_sys::spa_meta_bitmap>().max(payload_end))
            })
            .unwrap_or(cursor_size);
        let metadata_size = bitmap_end.max(cursor_size);
        let mut storage = vec![0u64; metadata_size.div_ceil(std::mem::size_of::<u64>())];
        let base = storage.as_mut_ptr().cast::<u8>();
        unsafe {
            std::ptr::write(base.cast::<spa_sys::spa_meta_cursor>(), cursor);
            if let Some((bitmap, bytes)) = bitmap {
                let bitmap_start = cursor.bitmap_offset as usize;
                let bitmap_header_end =
                    bitmap_start.saturating_add(std::mem::size_of::<spa_sys::spa_meta_bitmap>());
                if bitmap_header_end <= metadata_size {
                    let bitmap_ptr = base.add(bitmap_start).cast::<spa_sys::spa_meta_bitmap>();
                    std::ptr::write(bitmap_ptr, bitmap);
                    let pixel_start = bitmap_start.saturating_add(bitmap.offset as usize);
                    let pixel_end = pixel_start.saturating_add(bytes.len());
                    if pixel_end <= metadata_size {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            base.add(pixel_start),
                            bytes.len(),
                        );
                    }
                }
            }
        }
        let mut metadata = spa_sys::spa_meta {
            type_: SPA_META_CURSOR,
            size: metadata_size as u32,
            data: base.cast(),
        };
        let buffer = spa_sys::spa_buffer {
            n_metas: 1,
            n_datas: 0,
            metas: &mut metadata,
            datas: std::ptr::null_mut(),
        };
        f(&buffer)
    }

    fn cursor_header(id: u32, bitmap_offset: u32) -> spa_sys::spa_meta_cursor {
        spa_sys::spa_meta_cursor {
            id,
            flags: 0,
            position: spa_sys::spa_point { x: 30, y: 40 },
            hotspot: spa_sys::spa_point { x: 2, y: 3 },
            bitmap_offset,
        }
    }

    #[test]
    fn cursor_metadata_distinguishes_missing_hidden_and_position_only_updates() {
        let missing = spa_sys::spa_buffer {
            n_metas: 0,
            n_datas: 0,
            metas: std::ptr::null_mut(),
            datas: std::ptr::null_mut(),
        };
        assert_eq!(
            unsafe { copy_spa_cursor(&missing) },
            (CursorMetadataState::Missing, None)
        );
        with_cursor_metadata(cursor_header(0, 0), None, |buffer| {
            assert_eq!(
                unsafe { copy_spa_cursor(buffer) },
                (CursorMetadataState::Hidden, None)
            );
        });
        with_cursor_metadata(cursor_header(7, 0), None, |buffer| {
            assert_eq!(
                unsafe { copy_spa_cursor(buffer) },
                (
                    CursorMetadataState::Present(CursorPosition {
                        x: 30,
                        y: 40,
                        hotspot_x: 2,
                        hotspot_y: 3,
                    }),
                    None,
                )
            );
        });
        with_cursor_metadata(cursor_header(7, u32::MAX), None, |buffer| {
            let (state, overlay) = unsafe { copy_spa_cursor(buffer) };
            assert!(matches!(state, CursorMetadataState::Present(_)));
            assert!(overlay.is_none());
        });
    }

    #[test]
    fn cursor_bitmap_copy_checks_bounds_stride_and_hotspot_origin() {
        let bitmap = spa_sys::spa_meta_bitmap {
            format: spa::param::video::VideoFormat::RGBA.as_raw(),
            size: spa_sys::spa_rectangle {
                width: 1,
                height: 2,
            },
            stride: 8,
            offset: std::mem::size_of::<spa_sys::spa_meta_bitmap>() as u32,
        };
        with_cursor_metadata(
            cursor_header(1, std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32),
            Some((
                bitmap,
                vec![1, 2, 3, 4, 90, 91, 92, 93, 5, 6, 7, 8, 80, 81, 82, 83],
            )),
            |buffer| {
                let (state, overlay) = unsafe { copy_spa_cursor(buffer) };
                assert_eq!(
                    state,
                    CursorMetadataState::Present(CursorPosition {
                        x: 30,
                        y: 40,
                        hotspot_x: 2,
                        hotspot_y: 3,
                    })
                );
                let overlay = overlay.unwrap();
                assert_eq!((overlay.width, overlay.height), (1, 2));
                assert_eq!((overlay.x, overlay.y), (28, 37));
                assert_eq!(overlay.bitmap, vec![1, 2, 3, 4, 5, 6, 7, 8]);
            },
        );

        let malformed = spa_sys::spa_meta_bitmap {
            offset: 4_096,
            ..bitmap
        };
        with_cursor_metadata(
            cursor_header(1, std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32),
            Some((malformed, Vec::new())),
            |buffer| {
                let (state, overlay) = unsafe { copy_spa_cursor(buffer) };
                assert!(matches!(state, CursorMetadataState::Present(_)));
                assert!(overlay.is_none());
            },
        );

        with_cursor_metadata(
            cursor_header(1, std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32),
            Some((
                spa_sys::spa_meta_bitmap {
                    stride: 3,
                    ..bitmap
                },
                vec![1, 2, 3, 4, 5, 6, 7, 8],
            )),
            |buffer| {
                let (state, overlay) = unsafe { copy_spa_cursor(buffer) };
                assert!(matches!(state, CursorMetadataState::Present(_)));
                assert!(overlay.is_none());
            },
        );
    }

    #[test]
    fn still_frames_convert_blue_first_formats_without_losing_alpha() {
        let mut bgrx = vec![10, 20, 30, 0, 40, 50, 60, 99];
        convert_still_pixels_to_rgba(&mut bgrx, "bgr0");
        assert_eq!(bgrx, vec![30, 20, 10, 255, 60, 50, 40, 255]);

        let mut bgra = vec![10, 20, 30, 70, 40, 50, 60, 80];
        convert_still_pixels_to_rgba(&mut bgra, "bgra");
        assert_eq!(bgra, vec![30, 20, 10, 70, 60, 50, 40, 80]);
    }

    #[test]
    fn still_frames_preserve_red_first_colors_and_ignore_padding_alpha() {
        let mut rgbx = vec![10, 20, 30, 0];
        convert_still_pixels_to_rgba(&mut rgbx, "rgb0");
        assert_eq!(rgbx, vec![10, 20, 30, 255]);

        let mut rgba = vec![10, 20, 30, 70];
        convert_still_pixels_to_rgba(&mut rgba, "rgba");
        assert_eq!(rgba, vec![10, 20, 30, 70]);
    }

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
        assert!(spa::pod::Pod::from_bytes(&build_crop_meta_pod()).is_some());
        assert!(spa::pod::Pod::from_bytes(&build_cursor_meta_pod()).is_some());
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
            Some((
                pixels.clone(),
                8,
                FrameCropMetadata::Missing,
                None,
                CursorMetadataState::Missing,
                Some(header.pts)
            ))
        );
        header.flags = spa_sys::SPA_META_HEADER_FLAG_CORRUPTED;
        assert_eq!(header.flags, spa_sys::SPA_META_HEADER_FLAG_CORRUPTED);
        assert!(unsafe { copy_spa_frame(&mut buffer) }.is_none());
        buffer.n_metas = 0;
        buffer.metas = std::ptr::null_mut();
        assert_eq!(
            unsafe { copy_spa_frame(&mut buffer) },
            Some((
                pixels.clone(),
                8,
                FrameCropMetadata::Missing,
                None,
                CursorMetadataState::Missing,
                None
            ))
        );
        buffer.n_datas = 0;
        buffer.datas = std::ptr::null_mut();
        assert!(unsafe { copy_spa_frame(&mut buffer) }.is_none());
    }

    #[test]
    fn crop_metadata_is_copied_or_rejected_without_guessing() {
        let mut region = spa_sys::spa_meta_region {
            region: spa_sys::spa_region {
                position: spa_sys::spa_point { x: 3, y: 5 },
                size: spa_sys::spa_rectangle {
                    width: 640,
                    height: 480,
                },
            },
        };
        let mut short_metadata = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_VideoCrop,
            size: 0,
            data: (&mut region as *mut spa_sys::spa_meta_region).cast(),
        };
        let mut null_data_metadata = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_VideoCrop,
            size: std::mem::size_of::<spa_sys::spa_meta_region>() as u32,
            data: std::ptr::null_mut(),
        };
        let mut metadata = spa_sys::spa_meta {
            type_: spa_sys::SPA_META_VideoCrop,
            size: std::mem::size_of::<spa_sys::spa_meta_region>() as u32,
            data: (&mut region as *mut spa_sys::spa_meta_region).cast(),
        };
        let metadata_buffer = |metadata: &mut spa_sys::spa_meta| spa_sys::spa_buffer {
            n_metas: 1,
            n_datas: 0,
            metas: metadata,
            datas: std::ptr::null_mut(),
        };
        let buffer = metadata_buffer(&mut metadata);
        assert_eq!(
            unsafe { copy_spa_crop(&buffer) },
            FrameCropMetadata::Present(FrameCrop {
                x: 3,
                y: 5,
                width: 640,
                height: 480,
            })
        );

        let short_buffer = metadata_buffer(&mut short_metadata);
        assert_eq!(
            unsafe { copy_spa_crop(&short_buffer) },
            FrameCropMetadata::Invalid
        );
        let null_data_buffer = metadata_buffer(&mut null_data_metadata);
        assert_eq!(
            unsafe { copy_spa_crop(&null_data_buffer) },
            FrameCropMetadata::Invalid
        );
        let missing_buffer = spa_sys::spa_buffer {
            n_metas: 0,
            n_datas: 0,
            metas: std::ptr::null_mut(),
            datas: std::ptr::null_mut(),
        };
        assert_eq!(
            unsafe { copy_spa_crop(&missing_buffer) },
            FrameCropMetadata::Missing
        );
    }

    #[test]
    fn queued_frames_keep_format_stride_and_crop_metadata_together() {
        let mut bgra = spa::param::video::VideoInfoRaw::default();
        bgra.set_format(spa::param::video::VideoFormat::BGRA);
        bgra.set_size(spa::utils::Rectangle {
            width: 2,
            height: 2,
        });
        let mut rgba = spa::param::video::VideoInfoRaw::default();
        rgba.set_format(spa::param::video::VideoFormat::RGBA);
        rgba.set_size(spa::utils::Rectangle {
            width: 3,
            height: 2,
        });
        let crop = FrameCropMetadata::Present(FrameCrop {
            x: 1,
            y: 0,
            width: 2,
            height: 2,
        });
        let frames = [
            QueuedFrame {
                pixels: vec![1; 16],
                format: bgra,
                stride: 8,
                crop,
                cursor: None,
                cursor_metadata: CursorMetadataState::Missing,
                captured_at_us: 1,
            },
            QueuedFrame {
                pixels: vec![2; 28],
                format: rgba,
                stride: 14,
                crop: FrameCropMetadata::Missing,
                cursor: None,
                cursor_metadata: CursorMetadataState::Missing,
                captured_at_us: 2,
            },
        ];

        assert_eq!(frames[0].pixels, vec![1; 16]);
        assert_eq!(
            frames[0].format.format(),
            spa::param::video::VideoFormat::BGRA
        );
        assert_eq!(frames[0].stride, 8);
        assert_eq!(frames[0].crop, crop);
        assert_eq!(frames[1].pixels, vec![2; 28]);
        assert_eq!(
            frames[1].format.format(),
            spa::param::video::VideoFormat::RGBA
        );
        assert_eq!(frames[1].stride, 14);
        assert_eq!(frames[1].crop, FrameCropMetadata::Missing);
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
    fn window_stream_prefers_premultiplied_bgra_before_fallback_formats() {
        assert_eq!(
            video_format_preference(true),
            [
                spa::param::video::VideoFormat::BGRA,
                spa::param::video::VideoFormat::RGBA,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::RGBx,
            ]
        );
        assert!(spa::pod::Pod::from_bytes(&build_enum_format_pod_with_alpha(
            Some(1920),
            Some(1080),
            true,
        ))
        .is_some());
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
    fn frame_rows_respect_padded_and_negative_strides() {
        let padded = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 90, 91, 92, 93, 9, 10, 11, 12, 13, 14, 15, 16,
        ];
        assert_eq!(
            normalize_frame_rows(padded.clone(), 12, 2, 2, 4),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        );
        assert_eq!(
            normalize_frame_rows(
                vec![9, 10, 11, 12, 13, 14, 15, 16, 1, 2, 3, 4, 5, 6, 7, 8],
                -8,
                2,
                2,
                4
            ),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        );
        assert_eq!(
            normalize_frame_rows(padded[..16].to_vec(), 7, 2, 2, 4),
            None
        );
    }

    #[test]
    fn packed_frame_rows_keep_the_original_allocation() {
        let raw = (0..17).collect::<Vec<u8>>();
        let original_ptr = raw.as_ptr();
        let packed = normalize_frame_rows(raw, 8, 2, 2, 4).unwrap();

        assert_eq!(packed.as_ptr(), original_ptr);
        assert_eq!(packed, (0..16).collect::<Vec<u8>>());
    }

    #[test]
    fn circular_buffer_copy_respects_offset_and_wraps_safely() {
        assert_eq!(
            copy_circular_chunk(&[0, 1, 2, 3, 4], 1, 3),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            copy_circular_chunk(&[0, 1, 2, 3, 4], 4, 3),
            Some(vec![4, 0, 1])
        );
        assert_eq!(copy_circular_chunk(&[0, 1], 0, 3), None);
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
