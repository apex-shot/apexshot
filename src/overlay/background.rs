use super::api::SelectionError;
use crate::backend::{CaptureData, PixelFormat};
use image::RgbaImage;
use rayon::prelude::*;

#[derive(Clone)]
pub(crate) struct BackgroundFrame {
    /// Full-resolution original screenshot surface.
    pub(crate) surface: gtk4::cairo::ImageSurface,
    /// Downsampled + blurred surface used for the toolbar frosted-glass effect.
    /// Built at 1/4 resolution so the blur is fast but visually strong.
    pub(crate) toolbar_blur_surface: gtk4::cairo::ImageSurface,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

pub(crate) fn rgba_to_cairo_argb_bytes(image: &RgbaImage, stride: usize) -> Vec<u8> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let raw = image.as_raw();
    let row_src_len = width * 4;

    // Allocate output buffer; rows may be wider than src due to Cairo stride padding.
    let mut out = vec![0u8; stride * height];

    // Split output into per-row chunks and process in parallel with rayon.
    // Each row is independent so there are no data races.
    out.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, dst_row)| {
            let src_start = y * row_src_len;
            let src_row = &raw[src_start..src_start + row_src_len];

            // Fast path: screenshots are always fully opaque (a == 255).
            // Avoid the per-pixel branch and just swap R↔B in-place.
            let all_opaque = src_row.chunks_exact(4).all(|p| p[3] == 255);

            if all_opaque {
                for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    // RGBA → Cairo ARGB (BGRA in memory, little-endian)
                    dst[0] = src[2]; // B
                    dst[1] = src[1]; // G
                    dst[2] = src[0]; // R
                    dst[3] = 255; // A
                }
            } else {
                // General path: handle transparent / semi-transparent pixels.
                for (src, dst) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                    let a = src[3];
                    if a == 0 {
                        dst[0] = 0;
                        dst[1] = 0;
                        dst[2] = 0;
                        dst[3] = 0;
                    } else {
                        let alpha = a as u16;
                        let premul = |c: u8| -> u8 { ((c as u16 * alpha + 127) / 255) as u8 };
                        dst[0] = premul(src[2]); // B
                        dst[1] = premul(src[1]); // G
                        dst[2] = premul(src[0]); // R
                        dst[3] = a;
                    }
                }
            }
        });

    out
}

pub(crate) fn background_frame_from_image(
    image: &RgbaImage,
) -> Result<BackgroundFrame, SelectionError> {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(SelectionError::InitError(
            "Cannot select from an empty screenshot".into(),
        ));
    }

    // Pre-compute Cairo strides (cheap, no allocation).
    let stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(width)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Toolbar blur: 1/4-resolution downsample + Gaussian blur.
    // Nearest filter is ~5x faster than Triangle; quality is invisible after
    // the blur pass and when scaled back up to screen size.
    let small_w = (width / 4).max(1);
    let small_h = (height / 4).max(1);
    let blur_stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(small_w)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Build both pixel buffers in parallel: the full-res ARGB conversion and
    // the downsample+blur are independent, so we run them on separate threads.
    let (full_data, blur_data) = rayon::join(
        || rgba_to_cairo_argb_bytes(image, stride),
        || {
            let small = image::imageops::resize(
                image,
                small_w,
                small_h,
                image::imageops::FilterType::Nearest, // fast; quality invisible after blur
            );
            let blurred = image::imageops::blur(&small, 8.0);
            rgba_to_cairo_argb_bytes(&blurred, blur_stride)
        },
    );

    // Wrap both buffers in Cairo ImageSurfaces (cheap — just takes ownership).
    let surface = gtk4::cairo::ImageSurface::create_for_data(
        full_data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    let toolbar_blur_surface = gtk4::cairo::ImageSurface::create_for_data(
        blur_data,
        gtk4::cairo::Format::ARgb32,
        small_w as i32,
        small_h as i32,
        blur_stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    Ok(BackgroundFrame {
        surface,
        toolbar_blur_surface,
        width: width as i32,
        height: height as i32,
    })
}

pub(crate) fn paint_surface_fullscreen(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    surface_w: i32,
    surface_h: i32,
    screen_width: f64,
    screen_height: f64,
) {
    let _ = context.save();
    context.scale(
        screen_width / surface_w.max(1) as f64,
        screen_height / surface_h.max(1) as f64,
    );
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

/// Paint a surface scaled to fill the full screen, but clipped to `clip_rect`.
/// The clip is applied in screen coordinates before the scale transform.
pub(crate) fn paint_surface_clipped(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    surface_w: i32,
    surface_h: i32,
    screen_width: f64,
    screen_height: f64,
    clip_x: f64,
    clip_y: f64,
    clip_w: f64,
    clip_h: f64,
) {
    let _ = context.save();
    // Clip in screen-space first, then scale into image-space.
    context.rectangle(clip_x, clip_y, clip_w, clip_h);
    context.clip();
    context.scale(
        screen_width / surface_w.max(1) as f64,
        screen_height / surface_h.max(1) as f64,
    );
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

/// Draw the overlay (dark background + clear selection rectangle)
pub(crate) fn capture_to_cairo_argb_bytes(capture: &CaptureData, stride: usize) -> Vec<u8> {
    let width = capture.width as usize;
    let height = capture.height as usize;
    let src_stride = capture.stride as usize;
    let fmt = capture.format;
    let pixels = &capture.pixels;

    let mut out = vec![0u8; stride * height];

    out.par_chunks_mut(stride)
        .enumerate()
        .for_each(|(y, dst_row)| {
            let src_row_start = y * src_stride;
            for x in 0..width {
                let si = src_row_start + x * fmt.bytes_per_pixel as usize;
                let di = x * 4;
                // Map any supported pixel format to Cairo ARGB (BGRA in memory).
                let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                    (pixels[si], pixels[si + 1], pixels[si + 2])
                } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                    (pixels[si + 2], pixels[si + 1], pixels[si])
                } else if fmt == PixelFormat::RGB24 {
                    (pixels[si], pixels[si + 1], pixels[si + 2])
                } else {
                    // BGR24
                    (pixels[si + 2], pixels[si + 1], pixels[si])
                };
                dst_row[di] = b;
                dst_row[di + 1] = g;
                dst_row[di + 2] = r;
                dst_row[di + 3] = 255; // screenshots are always opaque
            }
        });

    out
}

/// Build a `BackgroundFrame` directly from raw `CaptureData`.
///
/// Skips the `RgbaImage` intermediate entirely — one fewer full-resolution
/// allocation and copy compared to `background_frame_from_image`.
pub(crate) fn background_frame_from_capture(
    capture: &CaptureData,
) -> Result<BackgroundFrame, SelectionError> {
    let width = capture.width;
    let height = capture.height;
    if width == 0 || height == 0 {
        return Err(SelectionError::InitError(
            "Cannot select from an empty screenshot".into(),
        ));
    }

    let stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(width)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    let small_w = (width / 4).max(1);
    let small_h = (height / 4).max(1);
    let blur_stride = gtk4::cairo::Format::ARgb32
        .stride_for_width(small_w)
        .map_err(|e| SelectionError::InitError(e.to_string()))? as usize;

    // Build full-res ARGB buffer and the blur buffer in parallel.
    let (full_data, blur_data) = rayon::join(
        || capture_to_cairo_argb_bytes(capture, stride),
        || {
            // Build a tiny RgbaImage just for the blur (cheap at 1/4 size).
            let row_len = width as usize * 4;
            let mut rgba_pixels: Vec<u8> = Vec::with_capacity(row_len * height as usize);
            let src_stride = capture.stride as usize;
            let fmt = capture.format;
            let pixels = &capture.pixels;
            for y in 0..height as usize {
                let src_row_start = y * src_stride;
                for x in 0..width as usize {
                    let si = src_row_start + x * fmt.bytes_per_pixel as usize;
                    let (r, g, b) = if fmt == PixelFormat::RGBA32 || fmt == PixelFormat::RGB32 {
                        (pixels[si], pixels[si + 1], pixels[si + 2])
                    } else if fmt == PixelFormat::BGRA32 || fmt == PixelFormat::BGR32 {
                        (pixels[si + 2], pixels[si + 1], pixels[si])
                    } else if fmt == PixelFormat::RGB24 {
                        (pixels[si], pixels[si + 1], pixels[si + 2])
                    } else {
                        (pixels[si + 2], pixels[si + 1], pixels[si])
                    };
                    rgba_pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
            let small_rgba: RgbaImage = image::ImageBuffer::from_raw(width, height, rgba_pixels)
                .expect("pixel buffer size mismatch");
            let small = image::imageops::resize(
                &small_rgba,
                small_w,
                small_h,
                image::imageops::FilterType::Nearest,
            );
            let blurred = image::imageops::blur(&small, 8.0);
            rgba_to_cairo_argb_bytes(&blurred, blur_stride)
        },
    );

    let surface = gtk4::cairo::ImageSurface::create_for_data(
        full_data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    let toolbar_blur_surface = gtk4::cairo::ImageSurface::create_for_data(
        blur_data,
        gtk4::cairo::Format::ARgb32,
        small_w as i32,
        small_h as i32,
        blur_stride as i32,
    )
    .map_err(|e| SelectionError::InitError(e.to_string()))?;

    Ok(BackgroundFrame {
        surface,
        toolbar_blur_surface,
        width: width as i32,
        height: height as i32,
    })
}
