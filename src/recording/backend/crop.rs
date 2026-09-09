use super::super::{RecordError, RecordResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CropMargins {
    pub(super) left: u32,
    pub(super) right: u32,
    pub(super) top: u32,
    pub(super) bottom: u32,
}

pub(super) fn compute_wayland_crop(
    stream_position: (i32, i32),
    stream_size: (i32, i32),
    selection: (i32, i32, u32, u32),
) -> Result<CropMargins, String> {
    let (stream_x, stream_y) = stream_position;
    let (stream_w, stream_h) = stream_size;
    let (sel_x, sel_y, sel_w, sel_h) = selection;

    if stream_w <= 0 || stream_h <= 0 || sel_w == 0 || sel_h == 0 {
        return Err("invalid stream or selection size".into());
    }

    let left = sel_x - stream_x;
    let top = sel_y - stream_y;
    let right = stream_w - left - sel_w as i32;
    let bottom = stream_h - top - sel_h as i32;

    // Overlay geometry and portal stream metadata can differ by a physical
    // pixel at fractional display scales. Treat that as edge rounding and
    // clip to the stream instead of abandoning the crop (which also makes
    // pointer/auto-zoom coordinates refer to the wrong frame).
    const EDGE_ROUNDING_TOLERANCE_PX: i32 = 2;
    if left < -EDGE_ROUNDING_TOLERANCE_PX
        || top < -EDGE_ROUNDING_TOLERANCE_PX
        || right < -EDGE_ROUNDING_TOLERANCE_PX
        || bottom < -EDGE_ROUNDING_TOLERANCE_PX
    {
        return Err("selection falls outside the selected monitor stream".into());
    }

    Ok(CropMargins {
        left: left.max(0) as u32,
        right: right.max(0) as u32,
        top: top.max(0) as u32,
        bottom: bottom.max(0) as u32,
    })
}

/// Resolve the stream's top-left in global coordinates.
///
/// Do not query GDK here: this runs on the recording worker thread, and GTK
/// is only safe on the daemon's main thread. Off-thread `Display::default()`
/// segfaults in libc (`tokio-rt-worker`) right after the ScreenCast portal
/// returns — which is why area recording died while fullscreen did not.
pub(super) fn resolve_wayland_stream_position(
    reported: Option<(i32, i32)>,
    _stream_size: (i32, i32),
    _selection: (i32, i32, u32, u32),
) -> (i32, i32) {
    if let Some(pos) = reported {
        return pos;
    }
    eprintln!("[recording] Stream missing position metadata; assuming (0,0) for crop");
    (0, 0)
}

/// Build a client-side crop for a pre-selected area, or `None` to record the
/// whole stream when crop math cannot be resolved (never fail the session).
pub(super) fn wayland_area_crop_or_full(
    stream_position: Option<(i32, i32)>,
    stream_size: (i32, i32),
    selection: (i32, i32, u32, u32),
) -> Option<CropMargins> {
    let position = resolve_wayland_stream_position(stream_position, stream_size, selection);
    match compute_wayland_crop(position, stream_size, selection) {
        Ok(crop) => Some(crop),
        Err(err) => {
            eprintln!(
                "[recording] Could not crop to selected region ({err}); recording the full stream instead"
            );
            None
        }
    }
}

pub(in crate::recording) fn fit_within_max_resolution(
    width: u32,
    height: u32,
    max_resolution: Option<(u32, u32)>,
) -> (u32, u32) {
    let Some((max_w, max_h)) = max_resolution else {
        return (width, height);
    };
    if width <= max_w && height <= max_h {
        return (width, height);
    }

    let scale = (max_w as f64 / width as f64).min(max_h as f64 / height as f64);
    let mut out_w = (width as f64 * scale).round().max(2.0) as u32;
    let mut out_h = (height as f64 * scale).round().max(2.0) as u32;
    out_w -= out_w % 2;
    out_h -= out_h % 2;
    (out_w.max(2), out_h.max(2))
}

pub(in crate::recording) fn wayland_video_filter(max_resolution: Option<(u32, u32)>) -> String {
    let scale = if let Some((max_w, max_h)) = max_resolution {
        format!(
            "scale=w='min(iw,{max_w})':h='min(ih,{max_h})':force_original_aspect_ratio=decrease:force_divisible_by=2:in_range=pc:out_range=tv"
        )
    } else {
        // Keep original size, but make dimensions encoder-safe for yuv420p.
        "scale=w='trunc(iw/2)*2':h='trunc(ih/2)*2':in_range=pc:out_range=tv".to_string()
    };
    format!("{scale},format=yuv420p")
}

pub(super) fn scale_crop_to_frame(
    crop: CropMargins,
    src_w: u32,
    src_h: u32,
    frame_w: u32,
    frame_h: u32,
) -> Option<CropMargins> {
    if frame_w < 2 || frame_h < 2 {
        return None;
    }
    let scaled = if src_w > 0 && src_h > 0 && (src_w != frame_w || src_h != frame_h) {
        let sx = frame_w as f64 / src_w as f64;
        let sy = frame_h as f64 / src_h as f64;
        CropMargins {
            left: (crop.left as f64 * sx).round() as u32,
            right: (crop.right as f64 * sx).round() as u32,
            top: (crop.top as f64 * sy).round() as u32,
            bottom: (crop.bottom as f64 * sy).round() as u32,
        }
    } else {
        crop
    };
    if scaled.left + scaled.right >= frame_w || scaled.top + scaled.bottom >= frame_h {
        return None;
    }
    Some(scaled)
}

pub(super) fn even_crop_output(crop: CropMargins, frame_w: u32, frame_h: u32) -> (u32, u32) {
    let mut width = frame_w.saturating_sub(crop.left + crop.right).max(2);
    let mut height = frame_h.saturating_sub(crop.top + crop.bottom).max(2);
    width &= !1;
    height &= !1;
    (width.max(2), height.max(2))
}

pub(super) fn crop_rgba_frame(
    frame: &crate::pipewire_engine::PipeWireFrame,
    crop: CropMargins,
    out_width: u32,
    out_height: u32,
) -> RecordResult<Vec<u8>> {
    if crop.left + out_width > frame.width || crop.top + out_height > frame.height {
        return Err(RecordError::GStreamerError(
            "Wayland crop exceeded frame bounds".into(),
        ));
    }

    let src_stride = frame.stride as usize;
    let row_bytes = out_width as usize * 4;
    let start_x = crop.left as usize * 4;
    let start_y = crop.top as usize;
    let mut cropped = Vec::with_capacity(row_bytes * out_height as usize);

    for y in 0..out_height as usize {
        let src_start = (start_y + y) * src_stride + start_x;
        let src_end = src_start + row_bytes;
        let row = frame.pixels.get(src_start..src_end).ok_or_else(|| {
            RecordError::GStreamerError("Wayland crop exceeded frame bounds".into())
        })?;
        cropped.extend_from_slice(row);
    }

    Ok(cropped)
}
