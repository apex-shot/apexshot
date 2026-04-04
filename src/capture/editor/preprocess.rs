//! Shared image preprocessing for text detection and OCR.
//!
//! Provides screenshot-oriented preprocessing that improves text detection
//! accuracy across both the highlighter (ocrs) and OCR (Tesseract) pipelines.

use image::{
    imageops::{resize, FilterType},
    RgbaImage,
};

/// Mild local contrast boost for screenshots with text inside colored buttons,
/// chips and low-contrast controls.
const UI_CONTRAST: f32 = 1.18;

/// Base upscale factor for better text detection of small UI labels.
const BASE_SCALE: f32 = 2.0;

/// Preprocess a screenshot for text detection.
///
/// This performs:
/// 1. Upscaling to improve detection of small text
/// 2. Dark-mode detection and inversion (Tesseract expects dark-on-light)
/// 3. Contrast enhancement for low-contrast UI text
/// 4. Transparent pixel handling (fill with white)
///
/// Returns the preprocessed RGBA image and whether dark mode was detected.
pub fn preprocess_for_text_detection(image: &RgbaImage) -> (RgbaImage, bool) {
    let dark_mode = is_dark_mode(image);
    let scale = BASE_SCALE;

    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * scale).round().max(1.0) as u32;
    let new_height = (original_height as f32 * scale).round().max(1.0) as u32;

    let mut processed = resize(image, new_width, new_height, FilterType::CatmullRom);

    for pixel in processed.pixels_mut() {
        let alpha = pixel[3] as f32 / 255.0;
        if alpha < 0.05 {
            pixel[0] = 255;
            pixel[1] = 255;
            pixel[2] = 255;
            pixel[3] = 255;
            continue;
        }

        let r = pixel[0] as f32;
        let g = pixel[1] as f32;
        let b = pixel[2] as f32;

        let luminance = 0.299 * r + 0.587 * g + 0.114 * b;

        if dark_mode {
            let boosted = |channel: f32| {
                let inverted = 255.0 - channel;
                ((inverted - 128.0) * UI_CONTRAST + 128.0).clamp(0.0, 255.0)
            };
            pixel[0] = boosted(r).round() as u8;
            pixel[1] = boosted(g).round() as u8;
            pixel[2] = boosted(b).round() as u8;
        } else {
            let boosted =
                |channel: f32| ((channel - luminance) * UI_CONTRAST + luminance).clamp(0.0, 255.0);
            pixel[0] = boosted(r).round() as u8;
            pixel[1] = boosted(g).round() as u8;
            pixel[2] = boosted(b).round() as u8;
        }

        pixel[3] = 255;
    }

    (processed, dark_mode)
}

/// Determine if an image is predominantly dark-mode.
///
/// Samples pixels and checks if the median luminance is below 100.
fn is_dark_mode(image: &RgbaImage) -> bool {
    let mut samples = Vec::new();
    let step = (image.width() * image.height()).max(1000) / 1000;
    let step = step.max(1);

    for (i, pixel) in image.pixels().enumerate() {
        if i % step as usize != 0 {
            continue;
        }
        if pixel[3] < 50 {
            continue;
        }
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        samples.push(luma);
    }

    if samples.is_empty() {
        return false;
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = samples[samples.len() / 2];
    median < 100.0
}

/// Compute the scale factor for text detection based on image size.
///
/// Large images (e.g., 4K) need less upscaling. Small images need more.
/// Returns a scale factor between 1.5 and 2.5.
pub fn adaptive_scale_factor(width: u32, height: u32) -> f32 {
    let pixels = (width * height) as f64;
    let megapixels = pixels / 1_000_000.0;

    if megapixels > 8.0 {
        1.5
    } else if megapixels > 4.0 {
        1.75
    } else if megapixels > 2.0 {
        2.0
    } else {
        2.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dark_mode_detects_dark_image() {
        let mut pixels = Vec::new();
        for _ in 0..100 * 100 {
            pixels.extend_from_slice(&[30, 30, 30, 255]);
        }
        let image = RgbaImage::from_raw(100, 100, pixels).unwrap();
        assert!(is_dark_mode(&image));
    }

    #[test]
    fn test_is_dark_mode_detects_light_image() {
        let mut pixels = Vec::new();
        for _ in 0..100 * 100 {
            pixels.extend_from_slice(&[240, 240, 240, 255]);
        }
        let image = RgbaImage::from_raw(100, 100, pixels).unwrap();
        assert!(!is_dark_mode(&image));
    }

    #[test]
    fn test_adaptive_scale_small_image() {
        assert!((adaptive_scale_factor(800, 600) - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_adaptive_scale_4k_image() {
        assert!((adaptive_scale_factor(3840, 2160) - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_preprocess_preserves_scale_ratio() {
        let pixels = vec![128u8; 50 * 50 * 4];
        let image = RgbaImage::from_raw(50, 50, pixels).unwrap();
        let (processed, _) = preprocess_for_text_detection(&image);
        assert_eq!(processed.width(), 100);
        assert_eq!(processed.height(), 100);
    }
}
