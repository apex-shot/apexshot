use super::super::{RecordError, RecordResult};
use crate::pipewire_engine::{CursorMetadataState, FrameCrop, FrameCropMetadata, PipeWireFrame};

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
    // Lanczos keeps downscaled text sharper than the default scaler. swscale
    // defaults RGB->YUV to the BT.601 matrix; force BT.709 so the encoded
    // pixels match the bt709 tags written on the output.
    let scale = if let Some((max_w, max_h)) = max_resolution {
        format!(
            "scale=w='min(iw,{max_w})':h='min(ih,{max_h})':force_original_aspect_ratio=decrease:force_divisible_by=2:flags=lanczos:in_range=pc:out_range=tv:out_color_matrix=bt709"
        )
    } else {
        // Keep original size, but make dimensions encoder-safe for yuv420p.
        "scale=w='trunc(iw/2)*2':h='trunc(ih/2)*2':flags=lanczos:in_range=pc:out_range=tv:out_color_matrix=bt709".to_string()
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
    if scaled.left.checked_add(scaled.right)? >= frame_w
        || scaled.top.checked_add(scaled.bottom)? >= frame_h
    {
        return None;
    }
    Some(scaled)
}

pub(super) fn even_crop_output(crop: CropMargins, frame_w: u32, frame_h: u32) -> (u32, u32) {
    let mut width = frame_w
        .saturating_sub(crop.left.saturating_add(crop.right))
        .max(2);
    let mut height = frame_h
        .saturating_sub(crop.top.saturating_add(crop.bottom))
        .max(2);
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
    let Some(right) = crop.left.checked_add(out_width) else {
        return Err(RecordError::GStreamerError(
            "Wayland crop exceeded frame bounds".into(),
        ));
    };
    let Some(bottom) = crop.top.checked_add(out_height) else {
        return Err(RecordError::GStreamerError(
            "Wayland crop exceeded frame bounds".into(),
        ));
    };
    if right > frame.width || bottom > frame.height {
        return Err(RecordError::GStreamerError(
            "Wayland crop exceeded frame bounds".into(),
        ));
    }

    let src_stride = frame.stride as usize;
    let row_bytes = (out_width as usize)
        .checked_mul(4)
        .ok_or_else(|| RecordError::GStreamerError("Wayland crop exceeded frame bounds".into()))?;
    let start_x = (crop.left as usize)
        .checked_mul(4)
        .ok_or_else(|| RecordError::GStreamerError("Wayland crop exceeded frame bounds".into()))?;
    let start_y = crop.top as usize;
    if src_stride < frame.width as usize * 4 {
        return Err(RecordError::GStreamerError(
            "Wayland crop exceeded frame bounds".into(),
        ));
    }
    let output_len = row_bytes
        .checked_mul(out_height as usize)
        .ok_or_else(|| RecordError::GStreamerError("Wayland crop exceeded frame bounds".into()))?;
    let mut cropped = Vec::new();
    cropped
        .try_reserve_exact(output_len)
        .map_err(|_| RecordError::GStreamerError("Wayland crop exceeded frame bounds".into()))?;

    for y in 0..out_height as usize {
        let src_start = start_y
            .checked_add(y)
            .and_then(|row| row.checked_mul(src_stride))
            .and_then(|row| row.checked_add(start_x))
            .ok_or_else(|| {
                RecordError::GStreamerError("Wayland crop exceeded frame bounds".into())
            })?;
        let src_end = src_start.checked_add(row_bytes).ok_or_else(|| {
            RecordError::GStreamerError("Wayland crop exceeded frame bounds".into())
        })?;
        let row = frame.pixels.get(src_start..src_end).ok_or_else(|| {
            RecordError::GStreamerError("Wayland crop exceeded frame bounds".into())
        })?;
        cropped.extend_from_slice(row);
    }

    Ok(cropped)
}

pub(super) fn window_frame_crop(frame: &PipeWireFrame) -> Option<FrameCrop> {
    if frame.width < 2 || frame.height < 2 {
        return None;
    }
    let crop = match frame.crop {
        FrameCropMetadata::Missing => FrameCrop {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        },
        FrameCropMetadata::Present(crop) => crop,
        FrameCropMetadata::Invalid => return None,
    };
    if crop.x < 0 || crop.y < 0 || crop.width < 2 || crop.height < 2 {
        return None;
    }
    let x = crop.x as u32;
    let y = crop.y as u32;
    let right = x.checked_add(crop.width)?;
    let bottom = y.checked_add(crop.height)?;
    let row_bytes = frame.width.checked_mul(4)?;
    if right > frame.width
        || bottom > frame.height
        || frame.stride < row_bytes
        || (frame.height - 1)
            .checked_mul(frame.stride)?
            .checked_add(row_bytes)? as usize
            > frame.pixels.len()
    {
        return None;
    }
    visible_alpha_bounds(frame, crop)
        .filter(|bounds| bounds.width >= 2 && bounds.height >= 2)
        .or(Some(crop))
}

fn visible_alpha_bounds(frame: &PipeWireFrame, crop: FrameCrop) -> Option<FrameCrop> {
    if !matches!(
        frame.video_format,
        pipewire::spa::param::video::VideoFormat::BGRA
            | pipewire::spa::param::video::VideoFormat::RGBA
    ) {
        return None;
    }

    let mut left = crop.width;
    let mut top = crop.height;
    let mut right = 0;
    let mut bottom = 0;
    let mut visible = false;
    let source_x = (crop.x as usize).checked_mul(4)?;
    let crop_bottom = (crop.y as u32).checked_add(crop.height)?;
    for y in crop.y as u32..crop_bottom {
        let row_start = (y as usize)
            .checked_mul(frame.stride as usize)?
            .checked_add(source_x)?;
        let row = frame
            .pixels
            .get(row_start..row_start.checked_add(crop.width as usize * 4)?)?;
        for x in 0..crop.width {
            if row.get((x as usize).checked_mul(4)?.checked_add(3)?)? == &0 {
                continue;
            }
            left = left.min(x);
            top = top.min(y - crop.y as u32);
            right = right.max(x);
            bottom = bottom.max(y - crop.y as u32);
            visible = true;
        }
    }
    if !visible {
        return None;
    }

    Some(FrameCrop {
        x: crop.x.checked_add(i32::try_from(left).ok()?)?,
        y: crop.y.checked_add(i32::try_from(top).ok()?)?,
        width: right.checked_sub(left)?.checked_add(1)?,
        height: bottom.checked_sub(top)?.checked_add(1)?,
    })
}

pub(super) fn window_cursor_update(
    frame: &PipeWireFrame,
    crop: FrameCrop,
    output_width: u32,
    output_height: u32,
) -> Option<Option<(f64, f64)>> {
    match frame.cursor_metadata {
        CursorMetadataState::Missing
        | CursorMetadataState::Invalid
        | CursorMetadataState::Hidden => Some(None),
        CursorMetadataState::Present(position) => {
            let crop_right = crop.x.checked_add(i32::try_from(crop.width).ok()?)?;
            let crop_bottom = crop.y.checked_add(i32::try_from(crop.height).ok()?)?;
            if position.x < crop.x
                || position.y < crop.y
                || position.x >= crop_right
                || position.y >= crop_bottom
                || output_width < 2
                || output_height < 2
            {
                return Some(None);
            }

            let scale = (output_width as f64 / crop.width as f64)
                .min(output_height as f64 / crop.height as f64);
            let scaled_width = (crop.width as f64 * scale)
                .round()
                .clamp(1.0, output_width as f64);
            let scaled_height = (crop.height as f64 * scale)
                .round()
                .clamp(1.0, output_height as f64);
            let offset_x = (output_width as f64 - scaled_width) / 2.0;
            let offset_y = (output_height as f64 - scaled_height) / 2.0;
            Some(Some((
                offset_x + (position.x - crop.x) as f64 * scaled_width / crop.width as f64,
                offset_y + (position.y - crop.y) as f64 * scaled_height / crop.height as f64,
            )))
        }
    }
}

pub(super) fn normalize_window_frame(
    frame: &PipeWireFrame,
    crop: FrameCrop,
    output_width: u32,
    output_height: u32,
) -> Option<Vec<u8>> {
    if output_width < 2 || output_height < 2 || window_frame_crop(frame)? != crop {
        return None;
    }
    let source_len = (crop.width as usize)
        .checked_mul(crop.height as usize)?
        .checked_mul(4)?;
    let mut source_pixels = Vec::new();
    source_pixels.try_reserve_exact(source_len).ok()?;
    let source_x = (crop.x as usize).checked_mul(4)?;
    for y in crop.y as usize..(crop.y as usize).checked_add(crop.height as usize)? {
        let row_start = y
            .checked_mul(frame.stride as usize)?
            .checked_add(source_x)?;
        for pixel in frame
            .pixels
            .get(row_start..row_start.checked_add(crop.width as usize * 4)?)?
            .chunks_exact(4)
        {
            match frame.video_format {
                pipewire::spa::param::video::VideoFormat::BGRA => {
                    source_pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
                pipewire::spa::param::video::VideoFormat::BGRx => {
                    source_pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
                pipewire::spa::param::video::VideoFormat::RGBA => {
                    source_pixels.extend_from_slice(pixel);
                }
                pipewire::spa::param::video::VideoFormat::RGBx => {
                    source_pixels.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
                _ => return None,
            }
        }
    }

    let source = image::RgbaImage::from_raw(crop.width, crop.height, source_pixels)?;
    if crop.width == output_width && crop.height == output_height {
        return Some(source.into_raw());
    }
    let scale =
        (output_width as f64 / crop.width as f64).min(output_height as f64 / crop.height as f64);
    let scaled_width = (crop.width as f64 * scale)
        .round()
        .clamp(1.0, output_width as f64) as u32;
    let scaled_height = (crop.height as f64 * scale)
        .round()
        .clamp(1.0, output_height as f64) as u32;
    let scaled = image::imageops::resize(
        &source,
        scaled_width,
        scaled_height,
        image::imageops::FilterType::Lanczos3,
    );
    let output =
        image::RgbaImage::from_pixel(output_width, output_height, image::Rgba([0, 0, 0, 255]));
    let mut output_pixels = output.into_raw();
    let offset_x = (output_width - scaled_width) / 2;
    let offset_y = (output_height - scaled_height) / 2;
    for y in 0..scaled_height as usize {
        let source_start = y.checked_mul(scaled_width as usize)?.checked_mul(4)?;
        let target_start = ((y + offset_y as usize)
            .checked_mul(output_width as usize)?
            .checked_add(offset_x as usize)?)
        .checked_mul(4)?;
        let row_len = (scaled_width as usize).checked_mul(4)?;
        output_pixels
            .get_mut(target_start..target_start.checked_add(row_len)?)?
            .copy_from_slice(
                scaled
                    .as_raw()
                    .get(source_start..source_start.checked_add(row_len)?)?,
            );
    }
    Some(output_pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipewire_engine::{ColorSpace, FrameCropMetadata};

    fn frame(
        width: u32,
        height: u32,
        video_format: pipewire::spa::param::video::VideoFormat,
        crop: FrameCropMetadata,
        pixel: [u8; 4],
    ) -> PipeWireFrame {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&pixel);
        }
        PipeWireFrame {
            pixels,
            width,
            height,
            stride: width * 4,
            video_format,
            crop,
            cursor: None,
            cursor_metadata: CursorMetadataState::Missing,
            color_space: ColorSpace::default(),
            captured_at_us: 0,
        }
    }

    #[test]
    fn window_crop_accepts_absent_metadata_and_rejects_invalid_bounds() {
        let full = frame(
            4,
            4,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [20, 30, 40, 255],
        );
        assert_eq!(
            window_frame_crop(&full),
            Some(FrameCrop {
                x: 0,
                y: 0,
                width: 4,
                height: 4
            })
        );
        let cropped = PipeWireFrame {
            crop: FrameCropMetadata::Present(FrameCrop {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            }),
            ..full.clone()
        };
        assert_eq!(
            window_frame_crop(&cropped),
            Some(FrameCrop {
                x: 1,
                y: 1,
                width: 2,
                height: 2
            })
        );
        for metadata in [
            FrameCropMetadata::Invalid,
            FrameCropMetadata::Present(FrameCrop {
                x: -1,
                y: 0,
                width: 2,
                height: 2,
            }),
            FrameCropMetadata::Present(FrameCrop {
                x: 3,
                y: 0,
                width: 2,
                height: 2,
            }),
            FrameCropMetadata::Present(FrameCrop {
                x: 0,
                y: 0,
                width: 0,
                height: 2,
            }),
        ] {
            let invalid = PipeWireFrame {
                crop: metadata,
                ..full.clone()
            };
            assert_eq!(window_frame_crop(&invalid), None);
        }

        let mut truncated = full;
        truncated.pixels.truncate(4);
        assert_eq!(window_frame_crop(&truncated), None);
    }

    #[test]
    fn initial_window_dimensions_skip_invalid_crop_metadata() {
        let invalid = frame(
            4,
            4,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Present(FrameCrop {
                x: 3,
                y: 3,
                width: 2,
                height: 2,
            }),
            [20, 30, 40, 255],
        );
        let valid = frame(
            5,
            3,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [20, 30, 40, 255],
        );
        let (first_frame, first_crop) = [invalid, valid]
            .into_iter()
            .find_map(|frame| window_frame_crop(&frame).map(|crop| (frame, crop)))
            .unwrap();

        assert_eq!((first_frame.width, first_frame.height), (5, 3));
        assert_eq!((first_crop.width, first_crop.height), (5, 3));
    }

    #[test]
    fn window_frames_resize_in_both_directions_and_letterbox_aspect_mismatch() {
        let small = frame(
            2,
            2,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [220, 30, 10, 255],
        );
        let enlarged =
            normalize_window_frame(&small, window_frame_crop(&small).unwrap(), 4, 4).unwrap();
        assert_eq!(enlarged.len(), 4 * 4 * 4);
        assert!(enlarged
            .chunks_exact(4)
            .all(|pixel| pixel == [220, 30, 10, 255]));

        let large = frame(
            4,
            4,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [30, 180, 40, 255],
        );
        let reduced =
            normalize_window_frame(&large, window_frame_crop(&large).unwrap(), 2, 2).unwrap();
        assert!(reduced
            .chunks_exact(4)
            .all(|pixel| pixel == [30, 180, 40, 255]));

        let wide = frame(
            4,
            2,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [240, 40, 20, 255],
        );
        let letterboxed =
            normalize_window_frame(&wide, window_frame_crop(&wide).unwrap(), 4, 4).unwrap();
        assert_eq!(&letterboxed[..16], &[0, 0, 0, 255].repeat(4));
        assert_eq!(&letterboxed[16..48], &[240, 40, 20, 255].repeat(8));
        assert_eq!(&letterboxed[48..], &[0, 0, 0, 255].repeat(4));
    }

    #[test]
    fn window_normalization_keeps_colors_consistent_across_format_renegotiation() {
        for (video_format, native_pixel, expected_pixel) in [
            (
                pipewire::spa::param::video::VideoFormat::BGRx,
                [30, 20, 10, 0],
                [10, 20, 30, 255],
            ),
            (
                pipewire::spa::param::video::VideoFormat::BGRA,
                [30, 20, 10, 70],
                [10, 20, 30, 70],
            ),
            (
                pipewire::spa::param::video::VideoFormat::RGBx,
                [10, 20, 30, 0],
                [10, 20, 30, 255],
            ),
        ] {
            let frame = frame(2, 2, video_format, FrameCropMetadata::Missing, native_pixel);
            let normalized =
                normalize_window_frame(&frame, window_frame_crop(&frame).unwrap(), 2, 2).unwrap();
            assert!(normalized
                .chunks_exact(4)
                .all(|pixel| pixel == expected_pixel));
        }
    }

    #[test]
    fn window_alpha_bounds_trim_only_fully_transparent_padding() {
        let mut window = frame(
            5,
            5,
            pipewire::spa::param::video::VideoFormat::BGRA,
            FrameCropMetadata::Missing,
            [0, 0, 0, 0],
        );
        for (x, y, pixel) in [
            (1, 1, [0, 0, 0, 32]),
            (2, 2, [0, 0, 0, 255]),
            (3, 3, [40, 30, 20, 90]),
        ] {
            let start = (y * 5 + x) * 4;
            window.pixels[start..start + 4].copy_from_slice(&pixel);
        }

        let bounds = window_frame_crop(&window).unwrap();
        assert_eq!(
            bounds,
            FrameCrop {
                x: 1,
                y: 1,
                width: 3,
                height: 3,
            }
        );
        let normalized = normalize_window_frame(&window, bounds, 3, 3).unwrap();
        assert_eq!(&normalized[..4], &[0, 0, 0, 32]);
        assert_eq!(&normalized[(4 * 4)..(4 * 4 + 4)], &[0, 0, 0, 255]);
        assert_eq!(&normalized[8 * 4..9 * 4], &[20, 30, 40, 90]);
        let opaque_black = frame(
            5,
            5,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [0, 0, 0, 255],
        );
        assert_eq!(
            window_frame_crop(&opaque_black),
            Some(FrameCrop {
                x: 0,
                y: 0,
                width: 5,
                height: 5,
            })
        );

        let x_format = frame(
            5,
            5,
            pipewire::spa::param::video::VideoFormat::BGRx,
            FrameCropMetadata::Missing,
            [0, 0, 0, 0],
        );
        assert_eq!(
            window_frame_crop(&x_format),
            window_frame_crop(&opaque_black)
        );
    }

    #[test]
    fn window_cursor_uses_crop_and_resize_transform_and_rejects_outside_positions() {
        let mut frame = frame(
            4,
            4,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Present(FrameCrop {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            }),
            [10, 20, 30, 255],
        );
        frame.cursor_metadata =
            CursorMetadataState::Present(crate::pipewire_engine::CursorPosition {
                x: 2,
                y: 2,
                hotspot_x: 3,
                hotspot_y: 4,
            });
        assert_eq!(
            window_cursor_update(&frame, window_frame_crop(&frame).unwrap(), 4, 2,),
            Some(Some((2.0, 1.0)))
        );
        frame.cursor_metadata =
            CursorMetadataState::Present(crate::pipewire_engine::CursorPosition {
                x: 0,
                y: 2,
                hotspot_x: 0,
                hotspot_y: 0,
            });
        assert_eq!(
            window_cursor_update(&frame, window_frame_crop(&frame).unwrap(), 4, 2,),
            Some(None)
        );
        frame.cursor_metadata = CursorMetadataState::Hidden;
        assert_eq!(
            window_cursor_update(&frame, window_frame_crop(&frame).unwrap(), 4, 2,),
            Some(None)
        );
        frame.cursor_metadata = CursorMetadataState::Missing;
        assert_eq!(
            window_cursor_update(&frame, window_frame_crop(&frame).unwrap(), 4, 2,),
            Some(None)
        );
    }

    #[test]
    fn window_crop_and_odd_sizes_normalize_without_affecting_area_crop_bytes() {
        let mut pixels = vec![0; 4 * 4 * 4];
        for y in 1..3 {
            for x in 1..3 {
                let start = (y * 4 + x) * 4;
                pixels[start..start + 4].copy_from_slice(&[10, 200, 30, 255]);
            }
        }
        let cropped = PipeWireFrame {
            pixels,
            width: 4,
            height: 4,
            stride: 16,
            video_format: pipewire::spa::param::video::VideoFormat::RGBA,
            crop: FrameCropMetadata::Present(FrameCrop {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            }),
            cursor: None,
            cursor_metadata: CursorMetadataState::Missing,
            color_space: ColorSpace::default(),
            captured_at_us: 0,
        };
        let bounds = window_frame_crop(&cropped).unwrap();
        assert_eq!(bounds.width, 2);
        assert_eq!(
            normalize_window_frame(&cropped, bounds, 3, 3).unwrap(),
            [10, 200, 30, 255].repeat(9)
        );

        let odd = frame(
            3,
            3,
            pipewire::spa::param::video::VideoFormat::RGBA,
            FrameCropMetadata::Missing,
            [1, 2, 3, 255],
        );
        assert_eq!(
            normalize_window_frame(&odd, window_frame_crop(&odd).unwrap(), 3, 3).unwrap(),
            [1, 2, 3, 255].repeat(9)
        );

        let area = crop_rgba_frame(
            &odd,
            CropMargins {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            1,
            3,
        )
        .unwrap();
        assert_eq!(area, [1, 2, 3, 255].repeat(3));
    }
}
