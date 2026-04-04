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
