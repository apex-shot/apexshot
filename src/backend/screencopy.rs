//! Direct Wayland screencopy capture via `zwlr_screencopy_manager_v1`.
//!
//! This speaks the `wlr-screencopy` Wayland protocol directly over the
//! compositor socket.  No D-Bus, no portal daemon, no screen-sharing popup.
//! Latency is ~50 ms.
//!
//! Supported compositors: Sway, Hyprland, Niri, river, and any other
//! wlroots-based compositor.  Also works on KDE Plasma ≥ 6.3 which ships
//! its own `zwlr_screencopy_manager_v1` implementation.
//!
//! Falls back gracefully: if the compositor does not advertise
//! `zwlr_screencopy_manager_v1` in the global registry, `capture()` returns
//! `None` and the caller can try the next tier.

use std::collections::BTreeMap;
use wayland_client::{
    protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

use super::{CaptureData, DisplayError, DisplayResult, PixelFormat};

// ─────────────────────────────────────────────────────────────────────────────
// Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt a direct wlr-screencopy capture of the preferred output.
///
/// Without a preference, captures the output with the smallest logical origin
/// (legacy behaviour for single-monitor and left-most multi-monitor layouts).
///
/// Returns `Ok(Some(data))` on success, `Ok(None)` if the compositor does
/// not support `zwlr_screencopy_manager_v1`, or `Err(_)` on a hard failure.
pub fn capture() -> DisplayResult<Option<CaptureData>> {
    capture_preferred(None)
}

/// Capture the Wayland output whose logical origin is closest to `(x, y)`.
///
/// Used by the multi-monitor picker so freeze matches the selected display.
pub fn capture_at(x: i32, y: i32) -> DisplayResult<Option<CaptureData>> {
    capture_preferred(Some((x, y)))
}

fn capture_preferred(prefer_origin: Option<(i32, i32)>) -> DisplayResult<Option<CaptureData>> {
    // Connect to the running Wayland compositor.
    let conn = Connection::connect_to_env().map_err(|e| {
        DisplayError::InitializationError(format!("Could not connect to Wayland display: {e}"))
    })?;

    let mut state = AppState::default();
    let mut event_queue: EventQueue<AppState> = conn.new_event_queue();
    let qh = event_queue.handle();

    // Bind globals (registry listener populates state.globals).
    let display = conn.display();
    display.get_registry(&qh, ());

    // One roundtrip: the compositor sends us all global advertisements.
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| DisplayError::InitializationError(format!("Wayland roundtrip failed: {e}")))?;

    // Need geometry events before we can rank outputs by origin.
    event_queue
        .roundtrip(&mut state)
        .map_err(|e| DisplayError::InitializationError(format!("Wayland roundtrip failed: {e}")))?;

    // If screencopy is not available, signal the caller to try the next tier.
    let manager = match state.screencopy_manager.take() {
        Some(m) => m,
        None => return Ok(None),
    };

    if state.outputs.is_empty() {
        return Err(DisplayError::CaptureError(
            "No Wayland outputs found".into(),
        ));
    }

    let output_order = state
        .outputs
        .iter()
        .map(|output| output.id().protocol_id())
        .collect::<Vec<_>>();
    let output_id = select_output_id(
        &state.output_infos,
        &output_order,
        state.xdg_output_manager.is_some(),
        prefer_origin,
    )
    .map_err(|error| match error {
        OutputSelectionError::NoOutputs => {
            DisplayError::CaptureError("No Wayland outputs found".into())
        }
        OutputSelectionError::AmbiguousOrigin => DisplayError::UnsupportedBackend(
            "Multiple Wayland outputs share the requested logical origin".into(),
        ),
        OutputSelectionError::MissingLogicalPosition => DisplayError::UnsupportedBackend(
            "The compositor did not report logical positions for every output".into(),
        ),
    })?;
    let output = state
        .outputs
        .iter()
        .find(|output| output.id().protocol_id() == output_id)
        .cloned()
        .ok_or_else(|| DisplayError::CaptureError("Selected Wayland output disappeared".into()))?;

    // Snapshot the selected output's logical origin and scale before we move
    // `state` fields further down. These are used to correctly crop
    // the physical-pixel image when the caller supplies logical-area
    // coordinates (see CaptureData::output_origin_* / output_scale).
    let output_info = state.output_infos.get(&output_id).ok_or_else(|| {
        DisplayError::CaptureError("Selected Wayland output metadata disappeared".into())
    })?;
    let (output_origin_x, output_origin_y) = output_info.effective_origin();
    let output_scale = output_info.scale;

    if let Some((px, py)) = prefer_origin {
        eprintln!(
            "[screencopy] preferred origin=({}, {}) → output origin=({}, {}) scale={}",
            px, py, output_origin_x, output_origin_y, output_scale
        );
    }

    let shm = state
        .shm
        .take()
        .ok_or_else(|| DisplayError::CaptureError("Compositor did not advertise wl_shm".into()))?;

    // Request a frame for the full output (overlay_cursor = 0 → no cursor).
    let frame = manager.capture_output(0, &output, &qh, ());

    // Flush so the compositor receives the request.
    event_queue
        .flush()
        .map_err(|e| DisplayError::CaptureError(format!("Wayland flush failed: {e}")))?;

    // The compositor will send a `buffer` event describing the frame format,
    // followed by `buffer_done`.  We loop until we know the dimensions.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|e| DisplayError::CaptureError(format!("Wayland dispatch failed: {e}")))?;

        if state.frame_info.is_some() || state.frame_failed {
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err(DisplayError::CaptureError(
                "Timed out waiting for screencopy buffer event".into(),
            ));
        }
    }

    if state.frame_failed {
        return Err(DisplayError::CaptureError(
            "Compositor rejected screencopy frame".into(),
        ));
    }

    let info = state.frame_info.take().ok_or_else(|| {
        DisplayError::CaptureError("No frame info received from compositor".into())
    })?;

    // Create a shared-memory pool big enough for one RGBA frame.
    let stride = info.stride;
    let size = (stride * info.height) as usize;

    let shm_file = create_shm_file(size)
        .map_err(|e| DisplayError::CaptureError(format!("Failed to create shm file: {e}")))?;

    // Memory-map the file so we can read pixels after the compositor writes them.
    let raw_fd = shm_file.as_raw_fd();
    let mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            raw_fd,
            0,
        )
    };
    if mmap == libc::MAP_FAILED {
        return Err(DisplayError::CaptureError("mmap failed".into()));
    }

    // Build the wl_shm_pool → wl_buffer chain.
    // SAFETY: raw_fd is valid for the lifetime of shm_file which outlives pool.
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
    let pool = shm.create_pool(borrowed_fd, size as i32, &qh, ());
    let buffer = pool.create_buffer(
        0,
        info.width as i32,
        info.height as i32,
        stride as i32,
        info.format,
        &qh,
        (),
    );

    // Tell the frame to copy into our buffer.
    frame.copy(&buffer);
    event_queue
        .flush()
        .map_err(|e| DisplayError::CaptureError(format!("Wayland flush after copy: {e}")))?;

    // Wait for the compositor to signal `ready` (or `failed`).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|e| DisplayError::CaptureError(format!("Wayland dispatch error: {e}")))?;

        if state.frame_ready || state.frame_failed {
            break;
        }
        if std::time::Instant::now() > deadline {
            unsafe { libc::munmap(mmap, size) };
            return Err(DisplayError::CaptureError(
                "Timed out waiting for screencopy ready event".into(),
            ));
        }
    }

    if state.frame_failed {
        unsafe { libc::munmap(mmap, size) };
        return Err(DisplayError::CaptureError(
            "Compositor signalled screencopy failure".into(),
        ));
    }

    // Copy pixels out of shared memory before unmapping.
    let pixels_slice = unsafe { std::slice::from_raw_parts(mmap as *const u8, size) };

    // wl_shm XRGB8888 is stored as B G R X in little-endian memory.
    // We convert to RGBA32 (R G B A) so the rest of the pipeline is uniform.
    let pixel_format = info.format;
    let pixels = convert_to_rgba(pixels_slice, info.width, info.height, stride, pixel_format);

    unsafe { libc::munmap(mmap, size) };

    // Clean up Wayland objects (best-effort; compositor reclaims them anyway
    // when the connection drops, but explicit destroy is polite).
    buffer.destroy();
    pool.destroy();
    frame.destroy();

    Ok(Some(CaptureData {
        pixels,
        width: info.width,
        height: info.height,
        stride: info.width * 4,
        format: PixelFormat::RGBA32,
        cursor: None,
        output_origin_x,
        output_origin_y,
        output_scale,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Pixel format conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a raw wl_shm buffer to packed RGBA32 bytes.
///
/// Handles the two most common formats the compositor will negotiate:
/// - `XRGB8888` (wl_shm::Format::Xrgb8888) — stored as B G R X
/// - `ARGB8888` (wl_shm::Format::Argb8888) — stored as B G R A
///
/// All other formats fall back to a transparent black row (should not occur
/// in practice because we only accept shm buffers the compositor offers).
fn convert_to_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    format: wl_shm::Format,
) -> Vec<u8> {
    let row_len = width as usize * 4;
    let mut out = vec![0u8; row_len * height as usize];

    for row in 0..height as usize {
        let src_row = &src[row * stride as usize..row * stride as usize + row_len];
        let dst_row = &mut out[row * row_len..row * row_len + row_len];

        match format {
            // XRGB8888 / ARGB8888 on little-endian: bytes in memory are B G R X/A
            wl_shm::Format::Xrgb8888 => {
                for (src_px, dst_px) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    dst_px[0] = src_px[2]; // R ← B slot (little-endian)
                    dst_px[1] = src_px[1]; // G
                    dst_px[2] = src_px[0]; // B ← R slot
                    dst_px[3] = 255; // A (fully opaque)
                }
            }
            wl_shm::Format::Argb8888 => {
                for (src_px, dst_px) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    dst_px[0] = src_px[2]; // R
                    dst_px[1] = src_px[1]; // G
                    dst_px[2] = src_px[0]; // B
                    dst_px[3] = src_px[3]; // A
                }
            }
            // BGR / BGRA variants (some compositors prefer these)
            wl_shm::Format::Bgr888 => {
                for (i, src_px) in src_row.chunks_exact(3).enumerate() {
                    let d = &mut dst_row[i * 4..i * 4 + 4];
                    d[0] = src_px[2]; // R
                    d[1] = src_px[1]; // G
                    d[2] = src_px[0]; // B
                    d[3] = 255;
                }
            }
            wl_shm::Format::Bgra8888 => {
                for (src_px, dst_px) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    dst_px[0] = src_px[2]; // R
                    dst_px[1] = src_px[1]; // G
                    dst_px[2] = src_px[0]; // B
                    dst_px[3] = src_px[3]; // A
                }
            }
            // Fallback: leave row as transparent black
            _ => {}
        }
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared memory helpers
// ─────────────────────────────────────────────────────────────────────────────

use std::os::unix::io::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// Create an anonymous file in shared memory large enough for `size` bytes.
/// Uses `memfd_create` on Linux (no temp file, no filesystem entry).
fn create_shm_file(size: usize) -> Result<OwnedFd, std::io::Error> {
    // memfd_create(2): anonymous memory file, kernel cleans it up automatically.
    let name = c"apexshot-screencopy";
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Extend to the required size.
    let ret = unsafe { libc::ftruncate(fd, size as libc::off_t) };
    if ret < 0 {
        unsafe { libc::close(fd) };
        return Err(std::io::Error::last_os_error());
    }

    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

// ─────────────────────────────────────────────────────────────────────────────
// State machine for the Wayland event loop
// ─────────────────────────────────────────────────────────────────────────────

/// Information received from the compositor via the `buffer` event.
#[derive(Debug)]
struct FrameInfo {
    width: u32,
    height: u32,
    stride: u32,
    format: wl_shm::Format,
}

#[derive(Clone, Debug)]
struct OutputInfo {
    geometry_origin: (i32, i32),
    logical_position: Option<(i32, i32)>,
    scale: i32,
}

impl OutputInfo {
    fn new() -> Self {
        Self {
            geometry_origin: (0, 0),
            logical_position: None,
            scale: 1,
        }
    }

    fn effective_origin(&self) -> (i32, i32) {
        self.logical_position.unwrap_or(self.geometry_origin)
    }

    fn update_geometry_origin(&mut self, origin: (i32, i32)) {
        self.geometry_origin = origin;
    }

    fn update_logical_position(&mut self, position: (i32, i32)) {
        self.logical_position = Some(position);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputSelectionError {
    NoOutputs,
    AmbiguousOrigin,
    MissingLogicalPosition,
}

fn output_distance_squared(left: (i32, i32), right: (i32, i32)) -> i128 {
    let dx = i128::from(left.0) - i128::from(right.0);
    let dy = i128::from(left.1) - i128::from(right.1);
    dx * dx + dy * dy
}

fn select_output_id(
    output_infos: &BTreeMap<u32, OutputInfo>,
    output_order: &[u32],
    xdg_output_available: bool,
    prefer_origin: Option<(i32, i32)>,
) -> Result<u32, OutputSelectionError> {
    if output_order.is_empty() {
        return Err(OutputSelectionError::NoOutputs);
    }
    let Some(prefer_origin) = prefer_origin else {
        return output_order
            .iter()
            .filter_map(|id| output_infos.get(id).map(|info| (*id, info)))
            .min_by_key(|(_, info)| info.effective_origin())
            .map(|(id, _)| id)
            .ok_or(OutputSelectionError::NoOutputs);
    };
    if xdg_output_available
        && output_order.len() > 1
        && output_order.iter().any(|id| {
            output_infos
                .get(id)
                .is_none_or(|info| info.logical_position.is_none())
        })
    {
        return Err(OutputSelectionError::MissingLogicalPosition);
    }

    let ranked = output_order
        .iter()
        .filter_map(|id| output_infos.get(id).map(|info| (*id, info)))
        .map(|(id, info)| {
            (
                output_distance_squared(info.effective_origin(), prefer_origin),
                id,
                info.effective_origin(),
            )
        })
        .collect::<Vec<_>>();
    let closest_distance = ranked
        .iter()
        .map(|(distance, _, _)| *distance)
        .min()
        .ok_or(OutputSelectionError::NoOutputs)?;
    let nearest = ranked
        .iter()
        .filter(|(distance, _, _)| *distance == closest_distance)
        .map(|(_, id, origin)| (*id, *origin))
        .collect::<Vec<_>>();
    let has_duplicate_nearest_origin = nearest.iter().enumerate().any(|(index, (_, origin))| {
        nearest[..index]
            .iter()
            .any(|(_, previous)| previous == origin)
    });
    if has_duplicate_nearest_origin {
        return Err(OutputSelectionError::AmbiguousOrigin);
    }
    ranked
        .into_iter()
        .min_by_key(|(distance, _, origin)| (*distance, origin.0, origin.1))
        .map(|(_, id, _)| id)
        .ok_or(OutputSelectionError::NoOutputs)
}

#[derive(Default)]
struct AppState {
    /// Bound screencopy manager (None if compositor lacks the protocol).
    screencopy_manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    /// All advertised wl_output globals.
    outputs: Vec<wl_output::WlOutput>,
    output_infos: BTreeMap<u32, OutputInfo>,
    xdg_output_manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    xdg_outputs: Vec<zxdg_output_v1::ZxdgOutputV1>,
    /// Bound wl_shm.
    shm: Option<wl_shm::WlShm>,
    /// Frame buffer parameters received from the compositor.
    frame_info: Option<FrameInfo>,
    /// Set to true when the compositor sends the `ready` event.
    frame_ready: bool,
    /// Set to true when the compositor sends the `failed` event.
    frame_failed: bool,
}

impl AppState {
    fn output_info_mut(&mut self, id: u32) -> &mut OutputInfo {
        self.output_infos.entry(id).or_insert_with(OutputInfo::new)
    }
}

// ─── Registry dispatcher ────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_screencopy_manager_v1" => {
                    let mgr = registry
                        .bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(
                            name,
                            version.min(3),
                            qh,
                            (),
                        );
                    state.screencopy_manager = Some(mgr);
                }
                "zxdg_output_manager_v1" if state.xdg_output_manager.is_none() => {
                    let manager = registry
                        .bind::<zxdg_output_manager_v1::ZxdgOutputManagerV1, _, _>(
                            name,
                            version.min(3),
                            qh,
                            (),
                        );
                    let xdg_outputs = state
                        .outputs
                        .iter()
                        .map(|output| {
                            let id = output.id().protocol_id();
                            manager.get_xdg_output(output, qh, id)
                        })
                        .collect::<Vec<_>>();
                    state.xdg_outputs.extend(xdg_outputs);
                    state.xdg_output_manager = Some(manager);
                }
                "wl_output" => {
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                    let id = output.id().protocol_id();
                    state.output_info_mut(id);
                    if let Some(manager) = &state.xdg_output_manager {
                        state
                            .xdg_outputs
                            .push(manager.get_xdg_output(&output, qh, id));
                    }
                    state.outputs.push(output);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());
                    state.shm = Some(shm);
                }
                _ => {}
            }
        }
    }
}

// ─── wl_output dispatcher (we don't need output events, just the object) ────

impl Dispatch<wl_output::WlOutput, ()> for AppState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = output.id().protocol_id();
        match event {
            wl_output::Event::Geometry { x, y, .. } => {
                state.output_info_mut(id).update_geometry_origin((x, y));
            }
            wl_output::Event::Scale { factor } => {
                state.output_info_mut(id).scale = factor.max(1);
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output_id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalPosition { x, y } = event {
            state
                .output_info_mut(*output_id)
                .update_logical_position((x, y));
        }
    }
}

// ─── wl_shm dispatchers ─────────────────────────────────────────────────────

impl Dispatch<wl_shm::WlShm, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ─── zwlr_screencopy_manager_v1 dispatcher ──────────────────────────────────

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Manager emits no events.
    }
}

// ─── zwlr_screencopy_frame_v1 dispatcher ────────────────────────────────────

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // The compositor tells us the buffer parameters we must use.
            zwlr_screencopy_frame_v1::Event::Buffer {
                format: wayland_client::WEnum::Value(fmt),
                width,
                height,
                stride,
            } if state.frame_info.is_none() => {
                // Only store if this looks like a useful shm format.
                // Prefer XRGB8888 / ARGB8888; accept anything the
                // compositor offers — convert_to_rgba handles it.
                state.frame_info = Some(FrameInfo {
                    width,
                    height,
                    stride,
                    format: fmt,
                });
            }
            zwlr_screencopy_frame_v1::Event::Buffer { .. } => {}
            // v3+ compositors send this after all Buffer events are done.
            // We don't need special handling — frame_info is already set.
            zwlr_screencopy_frame_v1::Event::BufferDone => {}

            // Frame is ready to read.
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                state.frame_ready = true;
            }

            // Capture failed (e.g. output disappeared).
            zwlr_screencopy_frame_v1::Event::Failed => {
                state.frame_failed = true;
            }

            // Flags (e.g. y_invert) — we ignore for now; compositors
            // that set y_invert are extremely rare in practice.
            zwlr_screencopy_frame_v1::Event::Flags { .. } => {}

            // Damage regions (copy_with_damage only) — not used here.
            zwlr_screencopy_frame_v1::Event::Damage { .. } => {}

            // linux-dmabuf hints — we use shm only, so ignore.
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf { .. } => {}

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_info(
        geometry_origin: (i32, i32),
        logical_position: Option<(i32, i32)>,
        scale: i32,
    ) -> OutputInfo {
        OutputInfo {
            geometry_origin,
            logical_position,
            scale,
        }
    }

    #[test]
    fn xdg_output_positions_select_outputs_when_core_origins_collide() {
        let infos = BTreeMap::from([
            (1, output_info((0, 0), Some((1280, 0)), 1)),
            (2, output_info((0, 0), Some((0, 0)), 1)),
        ]);
        let order = [1, 2];

        assert_eq!(select_output_id(&infos, &order, true, Some((0, 0))), Ok(2));
        assert_eq!(
            select_output_id(&infos, &order, true, Some((1280, 0))),
            Ok(1)
        );
    }

    #[test]
    fn later_core_geometry_does_not_overwrite_xdg_logical_position() {
        let mut info = OutputInfo::new();
        info.update_logical_position((1536, 0));
        info.update_geometry_origin((0, 0));
        assert_eq!(info.effective_origin(), (1536, 0));

        info.update_geometry_origin((0, 0));
        assert_eq!(info.effective_origin(), (1536, 0));
    }

    #[test]
    fn explicit_capture_rejects_duplicate_fallback_origins() {
        let infos = BTreeMap::from([
            (1, output_info((0, 0), None, 1)),
            (2, output_info((0, 0), None, 1)),
            (3, output_info((2, 0), None, 1)),
        ]);
        let order = [1, 2, 3];

        assert_eq!(
            select_output_id(&infos, &order, false, Some((1, 0))),
            Err(OutputSelectionError::AmbiguousOrigin)
        );
        assert_eq!(select_output_id(&infos, &order, false, None), Ok(1));
    }

    #[test]
    fn negative_logical_positions_remain_unscaled_in_fractional_layouts() {
        let infos = BTreeMap::from([
            (1, output_info((0, 0), Some((-1536, 240)), 1)),
            (2, output_info((0, 0), Some((0, 0)), 2)),
        ]);
        let order = [1, 2];

        assert_eq!(
            select_output_id(&infos, &order, true, Some((-1536, 240))),
            Ok(1)
        );
        assert_eq!(select_output_id(&infos, &order, true, Some((0, 0))), Ok(2));
    }

    #[test]
    fn incomplete_xdg_positions_do_not_fall_back_for_explicit_multimonitor_capture() {
        let infos = BTreeMap::from([
            (1, output_info((0, 0), Some((0, 0)), 1)),
            (2, output_info((0, 0), None, 1)),
        ]);
        let order = [1, 2];

        assert_eq!(
            select_output_id(&infos, &order, true, Some((0, 0))),
            Err(OutputSelectionError::MissingLogicalPosition)
        );
    }
}
