fn crop_background(
    full: &CaptureData,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Result<CaptureData, SelectionError> {
    if w <= 0 || h <= 0 || x < 0 || y < 0 {
        return Err(SelectionError::InitError(format!(
            "Invalid crop area: {}x{} at ({}, {})",
            w, h, x, y
        )));
    }
    let x_u = x as u32;
    let y_u = y as u32;
    let w_u = w as u32;
    let h_u = h as u32;
    if x_u + w_u > full.width || y_u + h_u > full.height {
        return Err(SelectionError::InitError(format!(
            "Crop area ({}x{} at ({},{})) out of bounds for {}x{} capture",
            w, h, x, y, full.width, full.height
        )));
    }

    let bpp = full.format.bytes_per_pixel as usize;
    let stride = full.stride as usize;
    let row_len = w_u as usize * bpp;

    let mut pixels = Vec::with_capacity(row_len * h_u as usize);
    for row in 0..h_u as usize {
        let src_y = y_u as usize + row;
        let offset = src_y * stride + x_u as usize * bpp;
        pixels.extend_from_slice(&full.pixels[offset..offset + row_len]);
    }

    Ok(CaptureData::new(pixels, w_u, h_u, full.format))
}

fn save_capture_to_temp_png(capture: &CaptureData) -> Result<PathBuf, SelectionError> {
    use image::{ImageBuffer, Rgba};

    let tmp = std::env::temp_dir().join(format!(
        "apexshot_capture_{}.png",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let bytes_per_pixel = capture.format.bytes_per_pixel as usize;
    let stride = capture.stride as usize;
    let width = capture.width;
    let height = capture.height;

    let is_bgr = capture.format == PixelFormat::BGR24
        || capture.format == PixelFormat::BGR32
        || capture.format == PixelFormat::BGRA32;

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let row_start = row * stride;
        let row_end = (row_start + width as usize * bytes_per_pixel).min(capture.pixels.len());
        let row_data = &capture.pixels[row_start..row_end];
        for px in row_data.chunks(bytes_per_pixel) {
            if px.len() >= 4 {
                if is_bgr {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], px[3]]);
                }
            } else if px.len() == 3 {
                if is_bgr {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
    }

    let image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba)
        .ok_or_else(|| SelectionError::InitError("Failed to build RGBA image buffer".into()))?;

    image.save(&tmp).map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to save temporary window capture {}: {e}",
            tmp.display()
        ))
    })?;

    Ok(tmp)
}

fn load_capture_data_from_path(path: &Path) -> Result<CaptureData, SelectionError> {
    let image = image::open(path).map_err(|e| {
        SelectionError::InitError(format!(
            "Failed to load capture image from {}: {e}",
            path.display()
        ))
    })?;
    let rgba = image.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    Ok(CaptureData::new(
        rgba.into_raw(),
        width,
        height,
        PixelFormat::RGBA32,
    ))
}
