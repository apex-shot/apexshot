//! OCR (Optical Character Recognition) module
//!
//! This module provides text extraction from screenshots using Tesseract OCR,
//! with clipboard integration for easy text copying.

use crate::backend::CaptureData;
use crate::capture::{capture_to_rgba_image, SaveError};
use image::RgbaImage;
use thiserror::Error;

/// Errors that can occur during OCR operations
#[derive(Debug, Error)]
pub enum OcrError {
    #[error("Tesseract initialization failed: {0}")]
    InitializationError(String),

    #[error("Tesseract not found. Please install tesseract: apt install tesseract-ocr / pacman -S tesseract")]
    TesseractNotFound,

    #[error("OCR recognition failed: {0}")]
    RecognitionError(String),

    #[error("Image processing error: {0}")]
    ImageError(String),

    #[error("Clipboard error: {0}")]
    ClipboardError(String),

    #[error("No text detected in image")]
    NoTextDetected,

    #[error("Low confidence text detected: {0}% (min: {1}%)")]
    LowConfidence(i32, i32),
}

pub type OcrResult<T> = Result<T, OcrError>;

/// OCR configuration options
#[derive(Debug, Clone)]
pub struct OcrConfig {
    /// Language(s) for OCR (e.g., "eng", "eng+fra", "eng+fra+deu")
    /// Default: "eng" (English)
    pub language: String,

    /// Minimum confidence threshold (0-100)
    /// Below this threshold, returns an error
    /// Default: 50
    pub min_confidence: i32,

    /// Whether to copy extracted text to clipboard
    /// Default: true
    pub clipboard_output: bool,

    /// Data path for Tesseract language files
    /// None uses system default
    pub datapath: Option<String>,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            language: "eng".to_string(),
            min_confidence: 50,
            clipboard_output: true,
            datapath: None,
        }
    }
}

impl OcrConfig {
    /// Create a new OCR config with the specified language
    pub fn with_language<S: Into<String>>(mut self, lang: S) -> Self {
        self.language = lang.into();
        self
    }

    /// Set the minimum confidence threshold
    pub fn with_min_confidence(mut self, confidence: i32) -> Self {
        self.min_confidence = confidence.clamp(0, 100);
        self
    }

    /// Enable or disable clipboard output
    pub fn with_clipboard(mut self, enable: bool) -> Self {
        self.clipboard_output = enable;
        self
    }

    /// Set custom Tesseract data path
    pub fn with_datapath<S: Into<String>>(mut self, path: S) -> Self {
        self.datapath = Some(path.into());
        self
    }
}

/// Result of an OCR operation
#[derive(Debug, Clone)]
pub struct OcrOutput {
    /// The extracted text
    pub text: String,

    /// Confidence score (0-100)
    pub confidence: i32,

    /// Whether text was copied to clipboard
    pub copied_to_clipboard: bool,
}

/// OCR preprocessing settings
#[derive(Debug, Clone)]
struct PreprocessConfig {
    /// Scale factor for upscaling (2.0-4.0 recommended)
    /// Tesseract works best at ~300 DPI equivalent
    scale_factor: f32,

    /// Contrast enhancement factor (1.0 = none, >1.0 = more contrast)
    contrast: f32,

    /// Apply adaptive thresholding for better text separation
    threshold: bool,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            scale_factor: 2.0, // Reduced from 3.0 for faster processing (still good for OCR)
            contrast: 1.05,    // Reduced contrast enhancement for speed
            threshold: false,  // Disabled - adaptive thresholding may hurt UI text OCR
        }
    }
}

/// Enhanced preprocessing for better OCR accuracy on UI screenshots
///
/// This performs:
/// 1. Upscaling to improve effective DPI (Tesseract prefers ~300 DPI)
/// 2. Color inversion for dark-mode UIs (Tesseract expects dark-on-light)
/// 3. Optional contrast enhancement
fn preprocess_image(image: &RgbaImage, config: &PreprocessConfig) -> Vec<u8> {
    use image::imageops::{resize, FilterType};

    // Step 1: Upscale the image for better OCR
    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * config.scale_factor) as u32;
    let new_height = (original_height as f32 * config.scale_factor) as u32;

    // Use Lanczos3 for high-quality upscaling
    let resized = resize(image, new_width, new_height, FilterType::Lanczos3);

    // Step 2: Convert to grayscale with inversion for dark mode
    // Tesseract is trained on dark text on light backgrounds (paper)
    // Most UIs are light text on dark backgrounds, so we invert
    let mut luma_data = Vec::with_capacity((new_width * new_height) as usize);

    for pixel in resized.pixels() {
        // Skip fully transparent pixels (background)
        if pixel[3] < 50 {
            luma_data.push(255); // White background
            continue;
        }

        // Standard ITU-R BT.709 luma calculation
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;

        // Invert: light text becomes dark, dark background becomes light
        let inverted = 255.0 - luma;

        // Apply mild contrast enhancement if configured
        let enhanced = if config.contrast > 1.0 {
            ((inverted - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0)
        } else {
            inverted
        };

        luma_data.push(enhanced as u8);
    }

    // Step 3: Apply adaptive thresholding if enabled
    if config.threshold {
        // Calculate local threshold using a simple moving average
        // For better results on UI text with varying backgrounds
        apply_adaptive_threshold(&mut luma_data, new_width, new_height);
    }

    luma_data
}

#[cfg(test)]
fn rgba_to_luma(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((image.width() * image.height()) as usize);
    for pixel in image.pixels() {
        // Standard ITU-R BT.709 luma calculation (alpha ignored)
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        out.push(luma.round().clamp(0.0, 255.0) as u8);
    }
    out
}

/// Apply adaptive thresholding for better text separation
///
/// This uses a simplified local threshold approach that works well for UI text
/// on varying backgrounds. Each pixel is compared to the average of its neighborhood.
fn apply_adaptive_threshold(data: &mut [u8], width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    let radius = 3usize; // Neighborhood radius

    // Create a copy for reading original values
    let original = data.to_vec();

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;

            // Calculate local average in neighborhood
            let mut sum = 0u32;
            let mut count = 0u32;

            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(h);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(w);

            for ny in y_start..y_end {
                for nx in x_start..x_end {
                    sum += original[ny * w + nx] as u32;
                    count += 1;
                }
            }

            let local_avg = (sum / count) as u8;
            let pixel = original[idx];

            // Apply threshold with a slight bias toward dark (text)
            // This helps with anti-aliased text
            let threshold = if local_avg > 128 {
                local_avg - 10
            } else {
                local_avg + 10
            };

            data[idx] = if pixel < threshold { 0 } else { 255 };
        }
    }
}

/// Extract text from a CaptureData using Tesseract OCR
///
/// # Arguments
/// * `capture` - The captured image data
/// * `config` - OCR configuration options
///
/// # Returns
/// * `OcrResult` containing the extracted text and metadata
///
/// # Example
/// ```no_run
/// use apexshot::ocr::{extract_text, OcrConfig};
/// use apexshot::backend::{CaptureData, PixelFormat};
///
/// // Create a dummy capture
/// let capture = CaptureData::new(
///     vec![0; 4], // 1x1 RGBA pixel
///     1,
///     1,
///     PixelFormat::RGBA32,
/// );
///
/// let config = OcrConfig::default()
///     .with_language("eng+fra")
///     .with_min_confidence(60);
///
/// match extract_text(&capture, &config) {
///     Ok(result) => println!("Extracted: {} (confidence: {}%)", result.text, result.confidence),
///     Err(e) => eprintln!("OCR failed: {}", e),
/// }
/// ```
pub fn extract_text(capture: &CaptureData, config: &OcrConfig) -> OcrResult<OcrOutput> {
    // Convert CaptureData to RgbaImage
    let rgba_image = capture_to_rgba_image(capture)
        .map_err(|e: SaveError| OcrError::ImageError(e.to_string()))?;

    // Apply enhanced preprocessing for better OCR accuracy
    let preprocess_config = PreprocessConfig::default();
    let luma_data = preprocess_image(&rgba_image, &preprocess_config);
    let width = (rgba_image.width() as f32 * preprocess_config.scale_factor) as i32;
    let height = (rgba_image.height() as f32 * preprocess_config.scale_factor) as i32;

    // Initialize Tesseract with optimized parameters for UI text
    let datapath = config.datapath.as_deref();
    let mut tesseract = tesseract::Tesseract::new(datapath, Some(&config.language))
        .map_err(|e| OcrError::InitializationError(e.to_string()))?
        .set_variable("tessedit_pageseg_mode", "6") // Assume a single uniform block of text
        .map_err(|e| OcrError::InitializationError(format!("Failed to set psm: {}", e)))?
        .set_variable("textord_heavy_nr", "1") // Prefer noise removal for cleaner text
        .map_err(|e| OcrError::InitializationError(format!("Failed to set noise removal: {}", e)))?
        .set_frame(
            &luma_data, width, height, 1,     // bytes_per_pixel (grayscale)
            width, // bytes_per_line (no padding)
        )
        .map_err(|e| OcrError::ImageError(format!("Failed to set frame: {}", e)))?
        .recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

    // Get the extracted text
    let text = tesseract
        .get_text()
        .map_err(|e| OcrError::RecognitionError(format!("Failed to get text: {}", e)))?;

    // Get confidence score
    let confidence = tesseract.mean_text_conf();

    // Check if any text was detected
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(OcrError::NoTextDetected);
    }

    // Check confidence threshold
    if confidence < config.min_confidence {
        return Err(OcrError::LowConfidence(confidence, config.min_confidence));
    }

    // Copy to clipboard if requested
    let mut copied_to_clipboard = false;
    if config.clipboard_output {
        if let Err(e) = copy_to_clipboard(trimmed_text) {
            eprintln!("Warning: Failed to copy to clipboard: {}", e);
        } else {
            copied_to_clipboard = true;
        }
    }

    Ok(OcrOutput {
        text: trimmed_text.to_string(),
        confidence,
        copied_to_clipboard,
    })
}

/// Copy text to the system clipboard
///
/// On Wayland, uses `wl-copy` CLI tool for reliable clipboard persistence.
/// On X11, uses the `arboard` crate.
/// Falls back to `xclip` if arboard fails.
///
/// # Arguments
/// * `text` - The text to copy
///
/// # Returns
/// * `Ok(())` if successful
/// * `Err(OcrError)` if clipboard operation failed
pub fn copy_to_clipboard(text: &str) -> OcrResult<()> {
    // Check if we're on Wayland
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        // Use wl-copy for Wayland (more reliable than arboard for this use case)
        // Use spawn() instead of output() to avoid waiting for the background process
        match std::process::Command::new("wl-copy").arg(text).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => {
                // Fall through to arboard if wl-copy fails
                eprintln!("Warning: wl-copy failed, trying arboard: {}", e);
            }
        }
    }

    // Try arboard (works on X11 and as fallback on Wayland)
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| OcrError::ClipboardError(format!("Failed to access clipboard: {}", e)))?;

    clipboard
        .set_text(text)
        .map_err(|e| OcrError::ClipboardError(format!("Failed to set clipboard text: {}", e)))?;

    Ok(())
}

/// Extract text from an image file path
///
/// Convenience function for OCR from a saved image file.
///
/// # Arguments
/// * `path` - Path to the image file
/// * `config` - OCR configuration options
///
/// # Returns
/// * `OcrResult` containing the extracted text and metadata
pub fn extract_text_from_path<P: AsRef<std::path::Path>>(
    path: P,
    config: &OcrConfig,
) -> OcrResult<OcrOutput> {
    let image = image::open(path)
        .map_err(|e| OcrError::ImageError(format!("Failed to open image: {}", e)))?;

    let rgba_image = image.to_rgba8();

    // Apply enhanced preprocessing for better OCR accuracy
    let preprocess_config = PreprocessConfig::default();
    let luma_data = preprocess_image(&rgba_image, &preprocess_config);
    let width = (rgba_image.width() as f32 * preprocess_config.scale_factor) as i32;
    let height = (rgba_image.height() as f32 * preprocess_config.scale_factor) as i32;

    // Initialize Tesseract with optimized parameters for UI text
    let datapath = config.datapath.as_deref();
    let mut tesseract = tesseract::Tesseract::new(datapath, Some(&config.language))
        .map_err(|e| OcrError::InitializationError(e.to_string()))?
        .set_variable("tessedit_pageseg_mode", "6") // Assume a single uniform block of text
        .map_err(|e| OcrError::InitializationError(format!("Failed to set psm: {}", e)))?
        .set_variable("textord_heavy_nr", "1") // Prefer noise removal for cleaner text
        .map_err(|e| OcrError::InitializationError(format!("Failed to set noise removal: {}", e)))?
        .set_frame(
            &luma_data, width, height, 1,     // bytes_per_pixel (grayscale)
            width, // bytes_per_line (no padding)
        )
        .map_err(|e| OcrError::ImageError(format!("Failed to set frame: {}", e)))?
        .recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

    // Get the extracted text
    let text = tesseract
        .get_text()
        .map_err(|e| OcrError::RecognitionError(format!("Failed to get text: {}", e)))?;

    // Get confidence score
    let confidence = tesseract.mean_text_conf();

    // Check if any text was detected
    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(OcrError::NoTextDetected);
    }

    // Check confidence threshold
    if confidence < config.min_confidence {
        return Err(OcrError::LowConfidence(confidence, config.min_confidence));
    }

    // Copy to clipboard if requested
    let mut copied_to_clipboard = false;
    if config.clipboard_output {
        if let Err(e) = copy_to_clipboard(trimmed_text) {
            eprintln!("Warning: Failed to copy to clipboard: {}", e);
        } else {
            copied_to_clipboard = true;
        }
    }

    Ok(OcrOutput {
        text: trimmed_text.to_string(),
        confidence,
        copied_to_clipboard,
    })
}

/// Bounding box for a detected text region
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Text region detected by OCR with bounding box
#[derive(Debug, Clone)]
pub struct DetectedTextRegion {
    pub bounds: BoundingBox,
    pub text: String,
    pub confidence: i32,
}

/// Extract text with bounding boxes from an image
pub fn extract_text_regions(image: &RgbaImage) -> Result<Vec<DetectedTextRegion>, OcrError> {
    extract_text_regions_tesseract(image)
}

/// Get both line-level and word-level text regions to maximize text detection.
/// Words are returned when their bounding boxes are below a minimum threshold,
/// catching small text elements like view counts that get grouped into lines.
fn extract_text_regions_tesseract(image: &RgbaImage) -> Result<Vec<DetectedTextRegion>, OcrError> {
    use std::ffi::CString;

    // Use faster preprocessing for highlighter text detection
    let preprocess_config = fast_region_preprocess_config();
    let luma_data = preprocess_image(image, &preprocess_config);
    let scale = preprocess_config.scale_factor;
    let width = (image.width() as f32 * scale) as i32;
    let height = (image.height() as f32 * scale) as i32;

    let mut api = tesseract::plumbing::TessBaseApi::create();
    let lang = CString::new("eng").map_err(|e| OcrError::InitializationError(e.to_string()))?;
    api.init_2(None, Some(&lang))
        .map_err(|e| OcrError::InitializationError(format!("Init failed: {}", e)))?;

    let psm_name = CString::new("tessedit_pageseg_mode").unwrap();
    // Use PSM 3 (default auto page segmentation) - better for scattered UI text like YouTube
    // PSM 6 assumes uniform block, misses small text below thumbnails
    let psm_value = CString::new("3").unwrap();
    api.set_variable(&psm_name, &psm_value)
        .map_err(|e| OcrError::InitializationError(format!("Set psm failed: {}", e)))?;

    // Set additional options for faster processing
    let enable_dict_name = CString::new("load_system_dawg").unwrap();
    let enable_dict_value = CString::new("0").unwrap();
    let _ = api.set_variable(&enable_dict_name, &enable_dict_value); // Ignore errors

    let freq_dawg_name = CString::new("load_freq_dawg").unwrap();
    let freq_dawg_value = CString::new("0").unwrap();
    let _ = api.set_variable(&freq_dawg_name, &freq_dawg_value);

    api.set_image(&luma_data, width, height, 1, width)
        .map_err(|e| OcrError::ImageError(format!("Set image failed: {}", e)))?;

    api.recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

    let inv_scale = 1.0 / scale;
    let mut regions = Vec::new();

    // Get full text for line-by-line extraction
    let full_text = api
        .get_utf8_text()
        .map_err(|e| OcrError::RecognitionError(format!("GetText failed: {}", e)))?;
    let text_str = full_text.as_ref().to_str().unwrap_or("");
    let text_lines: Vec<&str> = text_str.lines().collect();

    // First pass: get text line bounding boxes
    let textline_level: tesseract::plumbing::tesseract_sys::TessPageIteratorLevel = 2; // RIL_TEXTLINE
    let line_boxes = api.get_component_images_1(textline_level, 1).map_err(|e| {
        OcrError::RecognitionError(format!("GetComponentImages (lines) failed: {}", e))
    })?;

    let line_count = line_boxes.get_count();

    // Collect line regions
    let mut line_regions = Vec::new();
    for i in 0..line_count {
        if let Some(box_ref) = line_boxes.get_box_copied(i) {
            let mut x: i32 = 0;
            let mut y: i32 = 0;
            let mut w: i32 = 0;
            let mut h: i32 = 0;
            box_ref.get_geometry(Some(&mut x), Some(&mut y), Some(&mut w), Some(&mut h));

            let text = text_lines
                .get(i as usize)
                .copied()
                .unwrap_or("")
                .trim()
                .to_string();

            line_regions.push(DetectedTextRegion {
                bounds: BoundingBox {
                    x: (x as f32 * inv_scale) as i32,
                    y: (y as f32 * inv_scale) as i32,
                    width: (w as f32 * inv_scale) as i32,
                    height: (h as f32 * inv_scale) as i32,
                },
                text,
                confidence: 0,
            });
        }
    }

    // Second pass: also get word-level boxes for better small text detection
    // This catches small text elements like "1.2M views" that might be in a single line
    let word_level: tesseract::plumbing::tesseract_sys::TessPageIteratorLevel = 3; // RIL_WORD
    if let Ok(word_boxes) = api.get_component_images_1(word_level, 1) {
        let word_count = word_boxes.get_count();
        let mut word_regions = Vec::new();

        for i in 0..word_count {
            if let Some(box_ref) = word_boxes.get_box_copied(i) {
                let mut x: i32 = 0;
                let mut y: i32 = 0;
                let mut w: i32 = 0;
                let mut h: i32 = 0;
                box_ref.get_geometry(Some(&mut x), Some(&mut y), Some(&mut w), Some(&mut h));

                // Scale back to original coordinates
                let orig_x = (x as f32 * inv_scale) as i32;
                let orig_y = (y as f32 * inv_scale) as i32;
                let orig_w = (w as f32 * inv_scale) as i32;
                let orig_h = (h as f32 * inv_scale) as i32;

                // Only add word regions that are not already covered by line regions
                // This catches small text that Tesseract might group into a larger line
                let is_covered = line_regions.iter().any(|lr| {
                    lr.bounds.x <= orig_x
                        && lr.bounds.y <= orig_y
                        && (lr.bounds.x + lr.bounds.width) >= (orig_x + orig_w)
                        && (lr.bounds.y + lr.bounds.height) >= (orig_y + orig_h)
                });

                if !is_covered {
                    // Try to get text from the word iterator
                    let word_text = format!("word_{i}"); // Placeholder - actual text comes from line regions
                    word_regions.push(DetectedTextRegion {
                        bounds: BoundingBox {
                            x: orig_x,
                            y: orig_y,
                            width: orig_w,
                            height: orig_h,
                        },
                        text: word_text,
                        confidence: 0,
                    });
                }
            }
        }
        regions.extend(word_regions);
    }

    // Add all line regions
    regions.extend(line_regions);

    Ok(regions)
}

/// Fast preprocessing config optimized for text region detection (highlighter cursor sizing)
/// Uses higher upscaling to catch small text (11-12px view counts) and mild contrast
fn fast_region_preprocess_config() -> PreprocessConfig {
    PreprocessConfig {
        scale_factor: 2.5, // Higher upscale for small text detection (view counts, channel names)
        contrast: 1.15,    // Mild contrast enhancement to separate text from background
        threshold: false,  // No adaptive thresholding (hurts anti-aliased text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::PixelFormat;

    #[test]
    fn test_ocr_config_default() {
        let config = OcrConfig::default();
        assert_eq!(config.language, "eng");
        assert_eq!(config.min_confidence, 50);
        assert!(config.clipboard_output);
        assert!(config.datapath.is_none());
    }

    #[test]
    fn test_ocr_config_builder() {
        let config = OcrConfig::default()
            .with_language("eng+fra")
            .with_min_confidence(70)
            .with_clipboard(false)
            .with_datapath("/usr/share/tessdata");

        assert_eq!(config.language, "eng+fra");
        assert_eq!(config.min_confidence, 70);
        assert!(!config.clipboard_output);
        assert_eq!(config.datapath, Some("/usr/share/tessdata".to_string()));
    }

    #[test]
    fn test_min_confidence_clamping() {
        let config1 = OcrConfig::default().with_min_confidence(-10);
        assert_eq!(config1.min_confidence, 0);

        let config2 = OcrConfig::default().with_min_confidence(150);
        assert_eq!(config2.min_confidence, 100);
    }

    #[test]
    fn test_rgba_to_luma_conversion() {
        // Create a simple 2x2 RGBA image
        let image: RgbaImage = image::ImageBuffer::from_raw(
            2,
            2,
            vec![
                255, 0, 0, 255, // Red
                0, 255, 0, 255, // Green
                0, 0, 255, 255, // Blue
                255, 255, 255, 255, // White
            ],
        )
        .unwrap();

        let luma = rgba_to_luma(&image);

        // Red: 0.299*255 = 76
        assert!((luma[0] as i32 - 76).abs() < 2);
        // Green: 0.587*255 = 150
        assert!((luma[1] as i32 - 150).abs() < 2);
        // Blue: 0.114*255 = 29
        assert!((luma[2] as i32 - 29).abs() < 2);
        // White: 255
        assert_eq!(luma[3], 255);
    }

    #[test]
    fn test_rgba_to_luma_alpha_channel_ignored() {
        // Test that alpha doesn't affect luma calculation
        let image1: RgbaImage = image::ImageBuffer::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        let image2: RgbaImage = image::ImageBuffer::from_raw(1, 1, vec![255, 0, 0, 128]).unwrap();

        let luma1 = rgba_to_luma(&image1);
        let luma2 = rgba_to_luma(&image2);

        assert_eq!(luma1[0], luma2[0]);
    }

    #[test]
    fn test_extract_text_empty_capture() {
        // Create an empty 10x10 white image
        let pixels = vec![255u8; 10 * 10 * 3];
        let capture = CaptureData::new(pixels, 10, 10, PixelFormat::RGB24);

        let config = OcrConfig::default().with_clipboard(false);

        // Should fail because there's no text
        let result = extract_text(&capture, &config);
        assert!(matches!(result, Err(OcrError::NoTextDetected)));
    }
}
