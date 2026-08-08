use super::super::types::Rect;
use image::RgbaImage;

fn flatten_pixel_over_white(pixel: image::Rgba<u8>) -> image::Rgba<u8> {
    let alpha = pixel[3] as u32;
    if alpha == 255 {
        return pixel;
    }

    let blend =
        |channel: u8| -> u8 { ((channel as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8 };

    image::Rgba([blend(pixel[0]), blend(pixel[1]), blend(pixel[2]), 255])
}
const BLUR_PERFORMANCE_THRESHOLD: usize = 400 * 400; // 400x400 pixels
const BLUR_DOWNSAMPLE_FACTOR: usize = 4;
pub fn apply_blur_rect(image: &mut RgbaImage, rect: Rect, radius: f64, preserve_alpha: bool) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if radius <= 0.0 {
        return;
    }

    let image_width = image.width() as usize;
    let image_height = image.height() as usize;
    let rect_width = rect.width as usize;
    let rect_height = rect.height as usize;
    let area = rect_width * rect_height;

    // For large regions, use downsampled blur (never fall back to pixelation).
    // The downsampled path is robust and always produces a real blur.
    if area > BLUR_PERFORMANCE_THRESHOLD {
        apply_blur_rect_downsampled(image, rect, radius, preserve_alpha);
        return;
    }

    let radius = radius.max(1.0) as usize;

    // Use separable box blur for better memory efficiency
    // This uses O(max(width, height)) memory instead of O(width * height)

    let x0 = rect.x.max(0) as usize;
    let y0 = rect.y.max(0) as usize;
    let x1 = (rect.x + rect.width).min(image_width as i32) as usize;
    let y1 = (rect.y + rect.height).min(image_height as i32) as usize;

    if x1 <= x0 || y1 <= y0 {
        return;
    }

    // Expand the working area by the radius to include blur sampling
    let sample_x0 = x0.saturating_sub(radius);
    let sample_y0 = y0.saturating_sub(radius);
    let sample_x1 = (x1 + radius).min(image_width);
    let sample_y1 = (y1 + radius).min(image_height);

    let work_width = sample_x1 - sample_x0;
    let work_height = sample_y1 - sample_y0;

    if work_width == 0 || work_height == 0 {
        return;
    }

    // Extract working region
    let mut work_buffer: Vec<[u8; 4]> = Vec::with_capacity(work_width * work_height);
    for y in sample_y0..sample_y1 {
        for x in sample_x0..sample_x1 {
            work_buffer.push(image.get_pixel(x as u32, y as u32).0);
        }
    }

    box_blur_buffer(&mut work_buffer, work_width, work_height, radius);

    // Write back only the rect area (not the expanded sample area)
    for y in y0..y1 {
        for x in x0..x1 {
            let work_x = x - sample_x0;
            let work_y = y - sample_y0;
            let mut pixel = work_buffer[work_y * work_width + work_x];
            if !preserve_alpha {
                // Prevent alpha artifacts from making the checkerboard show through.
                pixel[3] = 255;
            }
            image.put_pixel(x as u32, y as u32, image::Rgba(pixel));
        }
    }
}
fn apply_blur_rect_downsampled(
    image: &mut RgbaImage,
    rect: Rect,
    radius: f64,
    preserve_alpha: bool,
) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if radius <= 0.0 {
        return;
    }

    // Inspired by the Qt overlay approach (downsample + blur + upsample).
    // This is resilient and guarantees we fill the entire target rect (no checkerboard gaps).
    let factor = BLUR_DOWNSAMPLE_FACTOR.max(2) as u32;
    let small_w = (image.width() / factor).max(1);
    let small_h = (image.height() / factor).max(1);

    // 1) Downsample full image
    let mut small = image::imageops::resize(
        image,
        small_w,
        small_h,
        image::imageops::FilterType::Triangle,
    );

    // 2) Blur the corresponding small rect
    let sr = Rect {
        x: (rect.x as f64 / factor as f64).floor() as i32,
        y: (rect.y as f64 / factor as f64).floor() as i32,
        width: ((rect.width as f64) / factor as f64).ceil() as i32,
        height: ((rect.height as f64) / factor as f64).ceil() as i32,
    };

    if let Some(sr) = sr.clamp_to(small.width(), small.height()) {
        let small_radius = (radius / factor as f64).max(1.0) as usize;
        apply_blur_rect_to_buffer(&mut small, sr, small_radius);

        // 3) Extract the blurred small region, then upsample it exactly to the original rect size
        let src_x0 = sr.x.max(0) as u32;
        let src_y0 = sr.y.max(0) as u32;
        let src_w = sr.width.max(1) as u32;
        let src_h = sr.height.max(1) as u32;

        let cropped = image::imageops::crop_imm(&small, src_x0, src_y0, src_w, src_h).to_image();
        let up = image::imageops::resize(
            &cropped,
            rect.width.max(1) as u32,
            rect.height.max(1) as u32,
            image::imageops::FilterType::Triangle,
        );

        // 4) Write back (always fills entire rect)
        let x0 = rect.x.max(0) as u32;
        let y0 = rect.y.max(0) as u32;
        for y in 0..rect.height.max(1) as u32 {
            for x in 0..rect.width.max(1) as u32 {
                let px = x0 + x;
                let py = y0 + y;
                if px < image.width() && py < image.height() {
                    let mut p = *up.get_pixel(x, y);
                    if !preserve_alpha {
                        // Prevent any alpha artifacts from making the checkerboard show through.
                        p[3] = 255;
                    }
                    image.put_pixel(px, py, p);
                }
            }
        }
    }
}
pub(super) fn apply_blur_rect_to_buffer(image: &mut image::RgbaImage, rect: Rect, radius: usize) {
    let (img_width, img_height) = image.dimensions();
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x + rect.width).min(img_width as i32) as u32;
    let y1 = (rect.y + rect.height).min(img_height as i32) as u32;

    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let radius = radius.max(1);
    let sample_x0 = x0.saturating_sub(radius as u32);
    let sample_y0 = y0.saturating_sub(radius as u32);
    let sample_x1 = ((x1 as i32) + radius as i32).min(img_width as i32) as u32;
    let sample_y1 = ((y1 as i32) + radius as i32).min(img_height as i32) as u32;

    let work_width = (sample_x1 - sample_x0) as usize;
    let work_height = (sample_y1 - sample_y0) as usize;

    if work_width == 0 || work_height == 0 {
        return;
    }

    let mut work_buffer: Vec<[u8; 4]> = Vec::with_capacity(work_width * work_height);
    for y in sample_y0..sample_y1 {
        for x in sample_x0..sample_x1 {
            let p = image.get_pixel(x, y);
            work_buffer.push([p[0], p[1], p[2], p[3]]);
        }
    }

    box_blur_buffer(&mut work_buffer, work_width, work_height, radius);

    for y in y0..y1 {
        for x in x0..x1 {
            let work_x = (x - sample_x0) as usize;
            let work_y = (y - sample_y0) as usize;
            image.put_pixel(x, y, image::Rgba(work_buffer[work_y * work_width + work_x]));
        }
    }
}
fn box_blur_buffer(buffer: &mut [[u8; 4]], width: usize, height: usize, radius: usize) {
    if width == 0 || height == 0 {
        return;
    }

    let mut temp = vec![[0u8; 4]; width * height];
    let mut prefix = vec![[0u32; 4]; width.max(height) + 1];

    for y in 0..height {
        prefix[0] = [0; 4];
        let row_start = y * width;
        for x in 0..width {
            let pixel = buffer[row_start + x];
            prefix[x + 1] = [
                prefix[x][0] + pixel[0] as u32,
                prefix[x][1] + pixel[1] as u32,
                prefix[x][2] + pixel[2] as u32,
                prefix[x][3] + pixel[3] as u32,
            ];
        }

        for x in 0..width {
            let left = x.saturating_sub(radius);
            let right = (x + radius + 1).min(width);
            let count = (right - left) as u32;
            temp[row_start + x] = [
                ((prefix[right][0] - prefix[left][0]) / count) as u8,
                ((prefix[right][1] - prefix[left][1]) / count) as u8,
                ((prefix[right][2] - prefix[left][2]) / count) as u8,
                ((prefix[right][3] - prefix[left][3]) / count) as u8,
            ];
        }
    }

    for x in 0..width {
        prefix[0] = [0; 4];
        for y in 0..height {
            let pixel = temp[y * width + x];
            prefix[y + 1] = [
                prefix[y][0] + pixel[0] as u32,
                prefix[y][1] + pixel[1] as u32,
                prefix[y][2] + pixel[2] as u32,
                prefix[y][3] + pixel[3] as u32,
            ];
        }

        for y in 0..height {
            let top = y.saturating_sub(radius);
            let bottom = (y + radius + 1).min(height);
            let count = (bottom - top) as u32;
            buffer[y * width + x] = [
                ((prefix[bottom][0] - prefix[top][0]) / count) as u8,
                ((prefix[bottom][1] - prefix[top][1]) / count) as u8,
                ((prefix[bottom][2] - prefix[top][2]) / count) as u8,
                ((prefix[bottom][3] - prefix[top][3]) / count) as u8,
            ];
        }
    }
}
pub fn apply_censor_rect(image: &mut RgbaImage, rect: Rect, block_size: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if block_size <= 0.0 {
        return;
    }

    let block = block_size as i32;
    let max_y = rect.y + rect.height;
    let max_x = rect.x + rect.width;

    // For large regions, use a more memory-efficient approach
    // by reading directly from the image instead of cloning
    let mut by = rect.y;
    while by < max_y {
        let block_height = (max_y - by).min(block);

        let mut bx = rect.x;
        while bx < max_x {
            let block_width = (max_x - bx).min(block);

            let mut r_sum: u32 = 0;
            let mut g_sum: u32 = 0;
            let mut b_sum: u32 = 0;
            let mut a_sum: u32 = 0;
            let mut count: u32 = 0;

            // Read directly from image - no clone needed
            for y in by..(by + block_height) {
                for x in bx..(bx + block_width) {
                    if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
                        let p = image.get_pixel(x as u32, y as u32);
                        r_sum += p[0] as u32;
                        g_sum += p[1] as u32;
                        b_sum += p[2] as u32;
                        a_sum += p[3] as u32;
                        count += 1;
                    }
                }
            }

            if let (Some(r), Some(g), Some(b), Some(a)) = (
                r_sum.checked_div(count),
                g_sum.checked_div(count),
                b_sum.checked_div(count),
                a_sum.checked_div(count),
            ) {
                let color = image::Rgba([r as u8, g as u8, b as u8, a as u8]);

                for y in by..(by + block_height) {
                    for x in bx..(bx + block_width) {
                        if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32
                        {
                            image.put_pixel(x as u32, y as u32, color);
                        }
                    }
                }
            }

            bx += block;
        }

        by += block;
    }
}
/// Apply blur to a rectangular region.
pub fn apply_hybrid_blur(image: &mut RgbaImage, rect: Rect, amount: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if rect.width < 2 || rect.height < 2 {
        return;
    }

    let normalized = (amount / 25.0).clamp(0.0, 1.0);
    let blur_radius = (1.2 + normalized * 7.8).max(1.2);
    let passes = if amount > 17.0 {
        3
    } else if amount > 8.0 {
        2
    } else {
        1
    };

    let pad = blur_radius.ceil() as u32;
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let iw = image.width();
    let ih = image.height();

    // Expand the crop by the blur radius so edge pixels sample correctly
    let sx = x0.saturating_sub(pad);
    let sy = y0.saturating_sub(pad);
    let sw = (rect.width as u32 + pad * 2).min(iw - sx);
    let sh = (rect.height as u32 + pad * 2).min(ih - sy);

    let mut work = image::imageops::crop_imm(image, sx, sy, sw, sh).to_image();

    for y in 0..work.height() {
        for x in 0..work.width() {
            let pixel = flatten_pixel_over_white(*work.get_pixel(x, y));
            work.put_pixel(x, y, pixel);
        }
    }

    let inner = Rect {
        x: (x0 - sx) as i32,
        y: (y0 - sy) as i32,
        width: rect.width.min(image.width() as i32 - x0 as i32),
        height: rect.height.min(image.height() as i32 - y0 as i32),
    };

    for _ in 0..passes {
        apply_blur_rect(&mut work, inner, blur_radius, true);
    }

    for y in inner.y..(inner.y + inner.height) {
        for x in inner.x..(inner.x + inner.width) {
            let mut p = *work.get_pixel(x as u32, y as u32);
            p[3] = 255;
            image.put_pixel(sx + x as u32, sy + y as u32, p);
        }
    }
}
/// Apply blackout effect to a rectangular region (solid black fill).
pub fn apply_blackout_rect(image: &mut RgbaImage, rect: &Rect) {
    let x = rect.x.max(0) as u32;
    let y = rect.y.max(0) as u32;
    let width = rect.width as u32;
    let height = rect.height as u32;

    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px < image.width() && py < image.height() {
                image.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
}
pub fn apply_focus_rect(image: &mut RgbaImage, rect: Rect, intensity: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    let image_width = image.width();
    let image_height = image.height();
    if image_width == 0 || image_height == 0 {
        return;
    }

    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x + rect.width).max(0) as u32;
    let y1 = (rect.y + rect.height).max(0) as u32;

    darken_region(image, 0, 0, image_width, y0, intensity);
    darken_region(image, 0, y1, image_width, image_height, intensity);
    darken_region(image, 0, y0, x0, y1, intensity);
    darken_region(image, x1, y0, image_width, y1, intensity);
}
fn darken_region(
    image: &mut RgbaImage,
    x_start: u32,
    y_start: u32,
    x_end: u32,
    y_end: u32,
    intensity: f64,
) {
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    let keep_ratio = (1.0 - (intensity / 100.0).clamp(0.10, 0.90)).clamp(0.10, 0.90);

    for y in y_start..y_end {
        for x in x_start..x_end {
            let pixel = image.get_pixel_mut(x, y);
            pixel[0] = (pixel[0] as f64 * keep_ratio).round() as u8;
            pixel[1] = (pixel[1] as f64 * keep_ratio).round() as u8;
            pixel[2] = (pixel[2] as f64 * keep_ratio).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::types::Rect;
    use super::*;

    #[test]
    fn hybrid_blur_flattens_transparency_to_canvas_backdrop() {
        let mut image = RgbaImage::new(20, 20);
        for y in 0..20 {
            for x in 0..20 {
                let pixel = if (6..14).contains(&x) && (6..14).contains(&y) {
                    image::Rgba([255, 255, 255, 255])
                } else {
                    image::Rgba([0, 0, 0, 0])
                };
                image.put_pixel(x, y, pixel);
            }
        }

        apply_hybrid_blur(
            &mut image,
            Rect {
                x: 2,
                y: 2,
                width: 16,
                height: 16,
            },
            20.0,
        );

        let corner = *image.get_pixel(2, 2);
        assert!(
            corner[3] == 255 && corner[0] > 220 && corner[1] > 220 && corner[2] > 220,
            "transparent pixels should flatten to the visible white canvas, got {corner:?}"
        );

        let edge = *image.get_pixel(6, 6);
        assert!(
            edge[3] == 255 && edge[0] > 220 && edge[1] > 220 && edge[2] > 220,
            "blurred white content should not pick up transparent black, got {edge:?}"
        );
    }

    #[test]
    fn downsampled_blur_buffer_updates_columns_across_rect() {
        let mut image = RgbaImage::new(24, 12);
        for y in 0..12 {
            for x in 0..24 {
                let value = if x < 12 { 0 } else { 255 };
                image.put_pixel(x, y, image::Rgba([value, value, value, 255]));
            }
        }

        apply_blur_rect_to_buffer(
            &mut image,
            Rect {
                x: 4,
                y: 2,
                width: 16,
                height: 8,
            },
            2,
        );

        let left_of_edge = *image.get_pixel(10, 6);
        let right_of_edge = *image.get_pixel(13, 6);
        let outside = *image.get_pixel(0, 0);

        assert!(
            left_of_edge[0] > 0,
            "left side near edge should receive blurred bright pixels, got {left_of_edge:?}"
        );
        assert!(
            right_of_edge[0] < 255,
            "right side near edge should receive blurred dark pixels, got {right_of_edge:?}"
        );
        assert_eq!(outside, image::Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn focus_effect_uses_configurable_intensity() {
        let rect = Rect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        };
        let mut low = RgbaImage::from_pixel(8, 8, image::Rgba([200, 180, 160, 255]));
        let mut high = low.clone();

        apply_focus_rect(&mut low, rect, 20.0);
        apply_focus_rect(&mut high, rect, 80.0);

        let low_outside = low.get_pixel(0, 0);
        let high_outside = high.get_pixel(0, 0);
        let inside = high.get_pixel(3, 3);

        assert!(high_outside[0] < low_outside[0]);
        assert!(high_outside[1] < low_outside[1]);
        assert!(high_outside[2] < low_outside[2]);
        assert_eq!(*inside, image::Rgba([200, 180, 160, 255]));
    }
}
