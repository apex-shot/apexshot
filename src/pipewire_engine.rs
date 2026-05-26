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

use pipewire as pw;
use pw::properties::properties;
use pw::spa;

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
}

/// Format negotiated with the compositor.
#[derive(Debug, Clone)]
pub struct NegotiatedFormat {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub framerate_num: u32,
    pub framerate_denom: u32,
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
// PipeWireStream — a minimal handle (unused for now, PipeWireCapture is primary)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub struct PipeWireStream {
    inner: Arc<Mutex<StreamInner>>,
    _stream: pw::stream::StreamRc,
}

// ---------------------------------------------------------------------------
// Supported video formats
// ---------------------------------------------------------------------------

#[allow(dead_code)]
const SUPPORTED_FORMATS: &[spa::param::video::VideoFormat] = &[
    spa::param::video::VideoFormat::BGRx,
    spa::param::video::VideoFormat::BGRA,
    spa::param::video::VideoFormat::RGBx,
    spa::param::video::VideoFormat::RGBA,
];

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
    /// Negotiated format (set by param_changed callback).
    format: Option<NegotiatedFormat>,
    /// Full raw format info for conversion.
    raw_format: Option<spa::param::video::VideoInfoRaw>,
    /// Queue of raw frame buffers (pushed by process callback).
    frames: std::collections::VecDeque<Vec<u8>>,
    /// Incremented each time a frame is dequeued publicly.
    frames_consumed: u64,
    /// Set to true when the stream enters error state.
    error: Option<String>,
    /// For single-frame mode: stop collecting after this many frames.
    max_frames: Option<u64>,
}

// ---------------------------------------------------------------------------
// PipeWireCapture
// ---------------------------------------------------------------------------

/// Full PipeWire screen capture session.
///
/// ```ignore
/// let capture = PipeWireCapture::connect(pipewire_fd, node_id, Some(1), None, None)?;
/// let frame = capture.wait_for_frame(Duration::from_secs(2))?;
/// // frame.pixels is RGBA32
/// drop(capture); // disconnects stream, tears down PipeWire
/// ```
pub struct PipeWireCapture {
    inner: Arc<Mutex<StreamInner>>,
    _thread_loop: pw::thread_loop::ThreadLoopRc,
    _context: pw::context::ContextRc,
    _core: pw::core::CoreRc,
    _stream: pw::stream::StreamRc,
}

impl PipeWireCapture {
    /// Create a new PipeWire capture session.
    ///
    /// * `pipewire_fd` — file descriptor from the portal's `OpenPipeWireRemote`.
    /// * `node_id` — PipeWire node ID from the portal's `Start` response.
    /// * `max_frames` — if `Some(n)`, stop collecting after n frames; `None` = continuous.
    /// * `width_hint`, `height_hint` — preferred resolution for format negotiation.
    pub fn connect(
        pipewire_fd: OwnedFd,
        node_id: u32,
        max_frames: Option<u64>,
        width_hint: Option<u32>,
        height_hint: Option<u32>,
    ) -> PipeWireResult<Self> {
        pw::init();

        // SAFETY: pw_thread_loop_new is always safe to call; the Rust binding
        // marks it unsafe due to the underlying C FFI.
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

        // Build format negotiation parameters (SPA pod).
        let format_pod = build_enum_format_pod(width_hint, height_hint);
        let pod = spa::pod::Pod::from_bytes(&format_pod)
            .ok_or_else(|| PipeWireError::Connect("Failed to parse format pod bytes".into()))?;
        let mut params = [pod];

        // Register callbacks before connecting.
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
                guard.format = Some(NegotiatedFormat {
                    width: w,
                    height: h,
                    stride: w * bpp,
                    framerate_num: info.framerate().num,
                    framerate_denom: info.framerate().denom,
                });
                guard.raw_format = Some(info);

                eprintln!(
                    "[pipewire] Negotiated format: {:?} {}x{} @ {}/{} fps",
                    guard.raw_format.as_ref().unwrap().format(),
                    w,
                    h,
                    guard.raw_format.as_ref().unwrap().framerate().num,
                    guard.raw_format.as_ref().unwrap().framerate().denom,
                );
            })
            .process(|_stream, inner| {
                let mut guard = match inner.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };

                // Respect max_frames limit
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

                if let Some(ref mem) = datas[0].data() {
                    if chunk_size <= mem.len() {
                        let raw_data = mem[..chunk_size].to_vec();
                        guard.frames.push_back(raw_data);
                    }
                }
                // buffer returned to stream via Drop
            })
            .register()
            .map_err(|e| PipeWireError::Connect(format!("Failed to register listener: {e}")))?;

        // Connect the stream to the compositor's node.
        stream
            .connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(|e| PipeWireError::Connect(format!("Failed to connect stream: {e}")))?;

        // Start the thread loop to begin processing PipeWire events.
        thread_loop.start();

        // Wait for format negotiation (compositor sends param_changed async).
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

        // Check for errors during negotiation.
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

    /// Get the negotiated format (only available after connection).
    pub fn format(&self) -> Option<NegotiatedFormat> {
        self.inner.lock().unwrap().format.clone()
    }

    /// Block until a frame arrives, or timeout.
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

    /// Non-blocking: return the next frame, or `None` if none available.
    pub fn try_recv_frame(&self) -> PipeWireResult<Option<PipeWireFrame>> {
        let mut guard = self.inner.lock().unwrap();

        if let Some(ref err) = guard.error {
            return Err(PipeWireError::Stream(err.clone()));
        }

        let raw_format = match guard.raw_format.as_ref() {
            Some(f) => *f,
            None => return Ok(None),
        };

        let raw = match guard.frames.pop_front() {
            Some(data) => data,
            None => return Ok(None),
        };

        guard.frames_consumed += 1;
        drop(guard);

        Ok(Some(convert_to_rgba_frame(&raw, &raw_format)))
    }

    /// Total frames dequeued so far.
    pub fn frames_consumed(&self) -> u64 {
        self.inner.lock().unwrap().frames_consumed
    }

    /// Check if the stream has encountered an error.
    pub fn has_error(&self) -> bool {
        self.inner.lock().unwrap().error.is_some()
    }

    /// Get the error message, if any.
    pub fn error_message(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }
}

// ---------------------------------------------------------------------------
// Frame format conversion
// ---------------------------------------------------------------------------

/// Convert a raw PipeWire frame buffer to RGBA32 pixels.
fn convert_to_rgba_frame(raw: &[u8], format: &spa::param::video::VideoInfoRaw) -> PipeWireFrame {
    let width = format.size().width as usize;
    let height = format.size().height as usize;
    let bpp = format_bpp(format.format()) as usize;
    let swaps_rb = format_swaps_rb(format.format());
    let stride = width * bpp;
    let row_len = width * 4; // output is always RGBA32

    let mut pixels = Vec::with_capacity(row_len * height);

    for row in 0..height {
        let src_start = row * stride;
        let src_row = &raw[src_start..src_start + width * bpp];

        for px in src_row.chunks_exact(bpp) {
            if swaps_rb {
                pixels.push(px[2]); // R ← B position
                pixels.push(px[1]); // G
                pixels.push(px[0]); // B ← R position
                pixels.push(px.get(3).copied().unwrap_or(255));
            } else {
                pixels.push(px[0]); // R
                pixels.push(px[1]); // G
                pixels.push(px[2]); // B
                pixels.push(px.get(3).copied().unwrap_or(255));
            }
        }
    }

    PipeWireFrame {
        pixels,
        width: format.size().width,
        height: format.size().height,
        stride: row_len as u32,
    }
}

// ---------------------------------------------------------------------------
// SPA pod construction
// ---------------------------------------------------------------------------

/// Build an `EnumFormat` SPA pod advertising our supported video formats.
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

/// Capture a single video frame from a PipeWire stream.
///
/// Opens a connection, grabs one frame, disconnects. Replacement for
/// `capture_single_frame_from_pipewire()`.
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
        // BGR× formats have R and B in expected byte order
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRx));
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        // RGB× formats are swapped
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
    fn test_convert_bgra_to_rgba_indirect() {
        // VideoInfoRaw can't be easily constructed in tests, but we verify
        // the format utility functions that drive the conversion logic.
        assert!(!format_swaps_rb(spa::param::video::VideoFormat::BGRA));
        assert!(format_swaps_rb(spa::param::video::VideoFormat::RGBA));
        assert_eq!(format_bpp(spa::param::video::VideoFormat::BGRA), 4);
    }
}
