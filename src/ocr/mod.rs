//! OCR (Optical Character Recognition) module
//!
//! This module provides text extraction from screenshots using Tesseract OCR,
//! with clipboard integration for easy text copying.
//!
//! QR code detection is attempted first — if a QR code is found, it is decoded
//! and the result is returned instead of running OCR.

use crate::backend::CaptureData;
use crate::capture::{capture_to_rgba_image, SaveError};
use crate::qr;
use image::RgbaImage;
use thiserror::Error;

pub mod deskew;

/// Tesseract OCR Engine Mode — LSTM only (best accuracy for most text)
const TESS_OEM: &str = "1";

/// Tesseract Page Segmentation Mode — auto with OSD (handles mixed layouts)
const TESS_PSM: &str = "3";

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

/// Source of the extracted content
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentSource {
    /// Text extracted via OCR (Tesseract or ocrs)
    Ocr { confidence: i32 },
    /// Content decoded from a QR code
    QrCode,
}

/// Result of an OCR operation
#[derive(Debug, Clone)]
pub struct OcrOutput {
    /// The extracted text or decoded QR content
    pub text: String,

    /// How the content was obtained
    pub source: ContentSource,

    /// Whether content was copied to clipboard
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
            threshold: true,   // Otsu's binarization — fast and effective
        }
    }
}

/// Determine if an image is predominantly dark-mode.
///
/// Samples pixels and checks if the median luminance is below 100.
/// Dark-mode UIs typically have median luminance < 100.
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

/// Enhanced preprocessing for better OCR accuracy on UI screenshots
///
/// This performs:
/// 1. Upscaling to improve effective DPI (Tesseract prefers ~300 DPI)
/// 2. Skew detection and correction via projection profile analysis
/// 3. Auto-detect dark/light mode and invert only if dark-mode
/// 4. Optional contrast enhancement
/// 5. Optional adaptive thresholding
fn preprocess_image(image: &RgbaImage, config: &PreprocessConfig) -> Vec<u8> {
    use image::imageops::{resize, FilterType};

    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * config.scale_factor) as u32;
    let new_height = (original_height as f32 * config.scale_factor) as u32;

    let resized = resize(image, new_width, new_height, FilterType::Lanczos3);

    let dark_mode = is_dark_mode(image);

    // Convert to grayscale (raw, no inversion) for skew detection
    let mut luma_data = Vec::with_capacity((new_width * new_height) as usize);
    for pixel in resized.pixels() {
        if pixel[3] < 50 {
            luma_data.push(255);
            continue;
        }
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        luma_data.push(luma as u8);
    }

    // Detect and correct skew
    let skew_angle = deskew::detect_skew_angle(&luma_data, new_width, new_height);
    if skew_angle.abs() > 0.5 {
        luma_data = deskew::rotate_gray(
            &luma_data,
            new_width as usize,
            new_height as usize,
            skew_angle,
        );
    }

    // Apply dark-mode inversion and contrast enhancement
    for pixel in luma_data.iter_mut() {
        let processed = if dark_mode {
            let inverted = 255.0 - (*pixel as f32);
            if config.contrast > 1.0 {
                ((inverted - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0) as u8
            } else {
                inverted as u8
            }
        } else {
            if config.contrast > 1.0 {
                ((*pixel as f32 - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0) as u8
            } else {
                *pixel
            }
        };
        *pixel = processed;
    }

    if config.threshold {
        apply_otsu_threshold(&mut luma_data);
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

/// Apply Otsu's method for optimal global thresholding.
///
/// Finds the threshold that minimizes intra-class variance between
/// foreground (text) and background pixels. O(n) single-pass algorithm.
fn apply_otsu_threshold(data: &mut [u8]) {
    let mut histogram = [0u32; 256];
    let total = data.len() as u32;

    for &pixel in data.iter() {
        histogram[pixel as usize] += 1;
    }

    let mut sum_all: u64 = 0;
    for i in 0..256 {
        sum_all += (i as u64) * (histogram[i] as u64);
    }

    let mut sum_background: u64 = 0;
    let mut weight_background: u64 = 0;
    let mut max_variance: f64 = 0.0;
    let mut threshold: u8 = 0;

    for i in 0..256 {
        let w_b = weight_background + histogram[i] as u64;
        if w_b == 0 || w_b >= total as u64 {
            weight_background += histogram[i] as u64;
            continue;
        }

        let w_f = (total as u64) - w_b;
        sum_background += (i as u64) * (histogram[i] as u64);
        let m_b = sum_background as f64 / w_b as f64;
        let m_f = (sum_all as f64 - sum_background as f64) / w_f as f64;

        let variance = (w_b as f64) * (w_f as f64) * (m_b - m_f).powi(2);

        if variance > max_variance {
            max_variance = variance;
            threshold = i as u8;
        }

        weight_background += histogram[i] as u64;
    }

    for pixel in data.iter_mut() {
        *pixel = if *pixel > threshold { 255 } else { 0 };
    }
}

/// Run Tesseract OCR on an RGBA image.
///
/// Handles QR detection, preprocessing, Tesseract setup, and result formatting.
/// Both `extract_text` and `extract_text_from_path` delegate to this.
fn run_tesseract(rgba_image: &RgbaImage, config: &OcrConfig) -> OcrResult<OcrOutput> {
    // Try QR code detection first
    if let Some(decoded) = qr::detect_and_decode(rgba_image) {
        let mut copied_to_clipboard = false;
        if config.clipboard_output {
            if let Err(e) = copy_to_clipboard(&decoded) {
                eprintln!("Warning: Failed to copy to clipboard: {}", e);
            } else {
                copied_to_clipboard = true;
            }
        }
        return Ok(OcrOutput {
            text: decoded,
            source: ContentSource::QrCode,
            copied_to_clipboard,
        });
    }

    let preprocess_config = PreprocessConfig::default();
    let luma_data = preprocess_image(rgba_image, &preprocess_config);
    let width = (rgba_image.width() as f32 * preprocess_config.scale_factor) as i32;
    let height = (rgba_image.height() as f32 * preprocess_config.scale_factor) as i32;

    let datapath = config.datapath.as_deref();
    let mut tesseract = tesseract::Tesseract::new(datapath, Some(&config.language))
        .map_err(|e| OcrError::InitializationError(e.to_string()))?
        .set_variable("tessedit_ocr_engine_mode", TESS_OEM)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set oem: {}", e)))?
        .set_variable("tessedit_pageseg_mode", TESS_PSM)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set psm: {}", e)))?
        .set_variable("textord_heavy_nr", "1")
        .map_err(|e| OcrError::InitializationError(format!("Failed to set noise removal: {}", e)))?
        .set_frame(&luma_data, width, height, 1, width)
        .map_err(|e| OcrError::ImageError(format!("Failed to set frame: {}", e)))?
        .recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

    let text = tesseract
        .get_text()
        .map_err(|e| OcrError::RecognitionError(format!("Failed to get text: {}", e)))?;

    let confidence = tesseract.mean_text_conf();

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(OcrError::NoTextDetected);
    }

    if confidence < config.min_confidence {
        return Err(OcrError::LowConfidence(confidence, config.min_confidence));
    }

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
        source: ContentSource::Ocr { confidence },
        copied_to_clipboard,
    })
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
///     Ok(result) => println!("Extracted: {} (source: {:?})", result.text, result.source),
///     Err(e) => eprintln!("OCR failed: {}", e),
/// }
/// ```
pub fn extract_text(capture: &CaptureData, config: &OcrConfig) -> OcrResult<OcrOutput> {
    let rgba_image = capture_to_rgba_image(capture)
        .map_err(|e: SaveError| OcrError::ImageError(e.to_string()))?;
    run_tesseract(&rgba_image, config)
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
    crate::utils::clipboard::copy_text_to_clipboard(text).map_err(|e| OcrError::ClipboardError(e))
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
    run_tesseract(&rgba_image, config)
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
        threshold: true,   // Otsu's for better small text detection
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
    fn test_is_dark_mode_mixed_content() {
        let mut pixels = Vec::new();
        for _y in 0..100 {
            for x in 0..100 {
                let luma = if x < 50 { 40 } else { 220 };
                pixels.extend_from_slice(&[luma, luma, luma, 255]);
            }
        }
        let image = RgbaImage::from_raw(100, 100, pixels).unwrap();
        assert!(!is_dark_mode(&image));
    }

    #[test]
    fn test_extract_text_empty_capture() {
        // Create an empty 10x10 white image
        let pixels = vec![255u8; 10 * 10 * 3];
        let capture = CaptureData::new(pixels, 10, 10, PixelFormat::RGB24);

        let config = OcrConfig::default().with_clipboard(false);

        // Should fail because there's no meaningful text (may return NoTextDetected, LowConfidence, or InitializationError if Tesseract is unavailable)
        let result = extract_text(&capture, &config);
        assert!(
            matches!(result, Err(OcrError::NoTextDetected) | Err(OcrError::LowConfidence(_, _)) | Err(OcrError::InitializationError(_))),
            "expected NoTextDetected, LowConfidence, or InitializationError for empty image, got {:?}",
            result
        );
    }

    #[test]
    fn test_otsu_threshold_separates_bimodal_data() {
        let mut data: Vec<u8> = (0..100)
            .map(|_| 30u8)
            .chain((0..100).map(|_| 220u8))
            .collect();
        apply_otsu_threshold(&mut data);

        let dark_count = data.iter().filter(|&&p| p == 0).count();
        let light_count = data.iter().filter(|&&p| p == 255).count();
        assert_eq!(dark_count, 100);
        assert_eq!(light_count, 100);
    }

    #[test]
    fn test_otsu_threshold_uniform_data() {
        let mut data: Vec<u8> = vec![128u8; 200];
        apply_otsu_threshold(&mut data);
        assert!(data.iter().all(|&p| p == 0 || p == 255));
    }
}
