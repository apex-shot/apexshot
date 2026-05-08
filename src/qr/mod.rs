//! QR code detection and decoding module
//!
//! Provides fast QR code detection using rqrr, with fallback to OCR text extraction.
//!
//! Uses rqrr's raw-byte API (`prepare_from_u8`) to avoid image crate version conflicts
//! (rqrr depends on image 0.25, apexshot uses image 0.24).

use image::RgbaImage;

/// Detect and decode QR codes in an image.
///
/// Returns `Some(decoded_text)` if a QR code is found, `None` otherwise.
pub fn detect_and_decode(image: &RgbaImage) -> Option<String> {
    let (w, h) = image.dimensions();
    let gray: Vec<u8> = image
        .pixels()
        .map(|p| {
            if p[3] < 50 {
                255u8
            } else {
                (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8
            }
        })
        .collect();

    detect_and_decode_from_gray(&gray, w, h)
}

/// Detect and decode QR codes from raw grayscale pixel data.
pub fn detect_and_decode_from_gray(data: &[u8], width: u32, height: u32) -> Option<String> {
    let w = width as usize;
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(w, height as usize, |x, y| data[y * w + x]);
    let grids = prepared.detect_grids();

    for grid in grids {
        if let Ok((_, content)) = grid.decode() {
            return Some(content);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcode::{types::Color, QrCode};

    fn render_qr(payload: &str, module_px: u32, quiet_zone_modules: u32) -> RgbaImage {
        let code = QrCode::new(payload.as_bytes()).unwrap();
        let modules = code.width() as u32;
        let size = (modules + quiet_zone_modules * 2) * module_px;
        let mut image = RgbaImage::from_pixel(size, size, image::Rgba([255, 255, 255, 255]));

        for y in 0..modules {
            for x in 0..modules {
                if code[(x as usize, y as usize)] == Color::Dark {
                    let start_x = (x + quiet_zone_modules) * module_px;
                    let start_y = (y + quiet_zone_modules) * module_px;
                    for py in start_y..start_y + module_px {
                        for px in start_x..start_x + module_px {
                            image.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
                        }
                    }
                }
            }
        }

        image
    }

    #[test]
    fn test_detects_generated_qr() {
        let payload = "https://apexshot.test/qr-smoke";
        let image = render_qr(payload, 8, 4);
        assert_eq!(detect_and_decode(&image).as_deref(), Some(payload));
    }

    #[test]
    fn test_detects_generated_qr_with_transparent_margin() {
        let payload = "apexshot:transparent-qr";
        let mut image = render_qr(payload, 6, 4);

        for y in 0..image.height() {
            for x in 0..image.width() {
                if x < 8 || y < 8 || x >= image.width() - 8 || y >= image.height() - 8 {
                    image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
                }
            }
        }

        assert_eq!(detect_and_decode(&image).as_deref(), Some(payload));
    }

    #[test]
    fn test_no_qr_on_plain_image() {
        let pixels = vec![255u8; 100 * 100 * 4];
        let image = RgbaImage::from_raw(100, 100, pixels).unwrap();
        assert!(detect_and_decode(&image).is_none());
    }

    #[test]
    fn test_no_qr_on_noise() {
        let pixels: Vec<u8> = (0..50 * 50 * 4).map(|i| (i % 256) as u8).collect();
        let image = RgbaImage::from_raw(50, 50, pixels).unwrap();
        assert!(detect_and_decode(&image).is_none());
    }
}
