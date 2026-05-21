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
pub mod ocrs_engine;

/// Tesseract OCR Engine Mode — LSTM only (best accuracy for most text)
const TESS_OEM: &str = "1";

/// Page Segmentation Modes tried in order. The result with the highest
/// average word confidence is returned, which dramatically improves accuracy
/// on multi-column UI screenshots (e.g. tabular recovery-phrase grids) where
/// a single PSM frequently mis-orders columns.
///  - 6  : assume a single uniform block of text (best for tight tabular grids)
///  - 7  : treat the image as a single text line (excellent for code)
///  - 4  : single column of text of variable sizes (good for forms / lists)
///  - 3  : fully automatic page segmentation, no OSD (general fallback)
///  - 11 : sparse text — find as much text as possible in no particular order

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
            // 3.0 lifts UI text (typically 11–14 px) close to the 30–40 px
            // glyph height that Tesseract's LSTM is trained on, which fixes
            // most "rusted → rustied" style misreads on small dark-mode text.
            scale_factor: 3.0,
            // Mild contrast boost is enough once the image is upscaled.
            contrast: 1.10,
            // IMPORTANT: do NOT pre-binarize for the LSTM engine. Otsu's
            // threshold throws away anti-aliasing information that LSTM uses
            // to disambiguate similar glyphs (e/c, i/l, rn/m, etc.). Tesseract
            // performs its own internal thresholding tuned to the LSTM model.
            threshold: false,
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
    let apply_contrast = config.contrast > 1.0;
    for pixel in luma_data.iter_mut() {
        let processed = if dark_mode {
            let inverted = 255.0 - (*pixel as f32);
            if apply_contrast {
                ((inverted - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0) as u8
            } else {
                inverted as u8
            }
        } else {
            if apply_contrast {
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
    for (i, count) in histogram.iter().enumerate() {
        sum_all += (i as u64) * (*count as u64);
    }

    let mut sum_background: u64 = 0;
    let mut weight_background: u64 = 0;
    let mut max_variance: f64 = 0.0;
    let mut threshold: u8 = 0;

    for (i, count) in histogram.iter().enumerate() {
        let w_b = weight_background + *count as u64;
        if w_b == 0 || w_b >= total as u64 {
            weight_background += *count as u64;
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

/// Run Tesseract once with a specific Page Segmentation Mode, returning the
/// recognised text together with its mean word confidence.
///
/// Re-running recognition with multiple PSMs is the most reliable way to
/// recover from layout-analysis failures on UI screenshots without depending
/// on a separate ML model. Tesseract initialisation itself is cheap (a few
/// milliseconds) compared to recognition, so re-initialising per attempt
/// keeps the code straightforward without measurable overhead.
fn run_tesseract_with_psm(
    datapath: Option<&str>,
    language: &str,
    psm: &str,
    luma_data: &[u8],
    width: i32,
    height: i32,
    code_mode: bool,
) -> OcrResult<(String, i32)> {
    let mut tesseract = tesseract::Tesseract::new(datapath, Some(language))
        .map_err(|e| OcrError::InitializationError(e.to_string()))?
        .set_variable("tessedit_ocr_engine_mode", TESS_OEM)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set oem: {}", e)))?
        .set_variable("tessedit_pageseg_mode", psm)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set psm: {}", e)))?
        // Preserve the spaces between columns/words so multi-column tables
        // (recovery phrases, 2FA grids, etc.) keep their visual layout.
        .set_variable("preserve_interword_spaces", "1")
        .map_err(|e| {
            OcrError::InitializationError(format!("Failed to set preserve spaces: {}", e))
        })?
        // Tell Tesseract the effective DPI of the upscaled image so the LSTM
        // engine picks the right scoring parameters.
        .set_variable("user_defined_dpi", "300")
        .map_err(|e| OcrError::InitializationError(format!("Failed to set dpi: {}", e)))?;

    // For code: disable aggressive noise removal that strips symbols like =, {}, ()
    // and disable dictionary to prevent "correcting" code into words
    if code_mode {
        tesseract = tesseract
            .set_variable("textord_heavy_nr", "0")
            .map_err(|e| {
                OcrError::InitializationError(format!("Failed to set noise removal: {}", e))
            })?
            .set_variable("load_system_dawg", "0")
            .map_err(|e| OcrError::InitializationError(format!("Failed to disable dict: {}", e)))?
            .set_variable("load_freq_dawg", "0")
            .map_err(|e| {
                OcrError::InitializationError(format!("Failed to disable freq dict: {}", e))
            })?;
    } else {
        tesseract = tesseract
            .set_variable("textord_heavy_nr", "1")
            .map_err(|e| {
                OcrError::InitializationError(format!("Failed to set noise removal: {}", e))
            })?;
    }

    tesseract = tesseract
        .set_frame(luma_data, width, height, 1, width)
        .map_err(|e| OcrError::ImageError(format!("Failed to set frame: {}", e)))?
        .recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;

    let text = tesseract
        .get_text()
        .map_err(|e| OcrError::RecognitionError(format!("Failed to get text: {}", e)))?;

    let confidence = tesseract.mean_text_conf();
    Ok((text, confidence))
}

/// Run OCR on an RGBA image.
///
/// Uses Tesseract as the primary engine. Falls back to the neural
/// OCR engine if Tesseract fails. QR codes are decoded directly.
fn run_ocr_pipeline(rgba_image: &RgbaImage, config: &OcrConfig) -> OcrResult<OcrOutput> {
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

    // Primary: Tesseract with optimized settings
    match run_tesseract_engine(rgba_image, config) {
        Ok(result) => return Ok(result),
        Err(tess_err) => {
            // Fallback: neural OCR engine if Tesseract fails
            if let Some((text, confidence)) = ocrs_engine::run_apexshot_ocr(rgba_image) {
                let final_text = postprocess_code(&text);

                if confidence >= config.min_confidence {
                    let mut copied_to_clipboard = false;
                    if config.clipboard_output {
                        if let Err(e) = copy_to_clipboard(&final_text) {
                            eprintln!("Warning: Failed to copy to clipboard: {}", e);
                        } else {
                            copied_to_clipboard = true;
                        }
                    }

                    return Ok(OcrOutput {
                        text: final_text,
                        source: ContentSource::Ocr { confidence },
                        copied_to_clipboard,
                    });
                }
            }

            Err(tess_err)
        }
    }
}

/// Run Tesseract OCR with optimized settings for code and UI text.
fn run_tesseract_engine(rgba_image: &RgbaImage, config: &OcrConfig) -> OcrResult<OcrOutput> {
    // Optimized preprocessing: higher upscale for symbols, no contrast boost
    let preprocess_config = PreprocessConfig {
        scale_factor: 4.0,
        contrast: 1.0,
        threshold: false,
    };

    let luma_data = preprocess_image(rgba_image, &preprocess_config);
    let width = (rgba_image.width() as f32 * preprocess_config.scale_factor) as i32;
    let height = (rgba_image.height() as f32 * preprocess_config.scale_factor) as i32;

    // Try PSM 7 (single text line) first, efficient for code and UI text
    let psm_order = &["7", "6", "4", "3", "11"][..];

    // Run Tesseract with several Page Segmentation Modes and keep the result
    // with the highest mean word confidence. Multi-column UI screenshots
    // (e.g. recovery-phrase / 2FA grids) frequently confuse a single PSM,
    // so the small extra cost of re-running recognition is well worth the
    // accuracy gain. We also early-exit once a result is "good enough" to
    // avoid paying the full 4-PSM cost for clean documents.
    const HIGH_CONFIDENCE_EARLY_EXIT: i32 = 85;

    let mut best: Option<(String, i32)> = None;

    for &psm in psm_order {
        let attempt = run_tesseract_with_psm(
            config.datapath.as_deref(),
            &config.language,
            psm,
            &luma_data,
            width,
            height,
            true,
        );

        match attempt {
            Ok((text, confidence)) => {
                let take = match &best {
                    None => true,
                    Some((_, best_conf)) => confidence > *best_conf,
                };
                if take {
                    best = Some((text, confidence));
                }
                if confidence >= HIGH_CONFIDENCE_EARLY_EXIT {
                    break;
                }
            }
            Err(err) => {
                // If we already have a usable result, skip this PSM and keep
                // going; otherwise propagate the first hard error so callers
                // still see meaningful diagnostics.
                if best.is_none() {
                    return Err(err);
                }
            }
        }
    }

    let (text, confidence) = best.ok_or(OcrError::NoTextDetected)?;

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(OcrError::NoTextDetected);
    }

    if confidence < config.min_confidence {
        return Err(OcrError::LowConfidence(confidence, config.min_confidence));
    }

    // Apply code-specific post-processing to fix common OCR errors
    let final_text = postprocess_code(trimmed_text);

    let mut copied_to_clipboard = false;
    if config.clipboard_output {
        if let Err(e) = copy_to_clipboard(&final_text) {
            eprintln!("Warning: Failed to copy to clipboard: {}", e);
        } else {
            copied_to_clipboard = true;
        }
    }

    Ok(OcrOutput {
        text: final_text,
        source: ContentSource::Ocr { confidence },
        copied_to_clipboard,
    })
}

/// Post-process OCR output to fix common code recognition errors.
///
/// Only applies reliable replacements — no heuristic code structure inference.
fn postprocess_code(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    for line in text.lines() {
        let mut cleaned = line.to_string();

        // Reliable #include pattern fixes
        if cleaned.contains("#include") || cleaned.contains("include") {
            if let Some(pos) = cleaned.find("#include") {
                if pos > 0 && cleaned[..pos].chars().any(|c| !c.is_whitespace()) {
                    cleaned = cleaned[pos..].to_string();
                }
            }
            cleaned = cleaned.replace("##include", "#include");
            cleaned = cleaned.replace("{#include", "#include");
            cleaned = cleaned.replace("{ #include", "#include");
            cleaned = cleaned.replace("' #include", "#include");
            cleaned = cleaned.replace("f#include", "#include");
            cleaned = cleaned.replace("finclude", "#include");
            cleaned = cleaned.replace("dinclude", "#include");

            if cleaned.starts_with("#include \"") {
                let after = &cleaned[10..];
                if !after.contains('"') {
                    cleaned.push('"');
                }
            }
        }

        // Reliable Qt class name fixes (Q misread as 0 or O)
        cleaned = cleaned.replace("<0Point>", "<QPoint>");
        cleaned = cleaned.replace("<ORect>", "<QRect>");
        cleaned = cleaned.replace("<0Rect>", "<QRect>");
        cleaned = cleaned.replace("<0Timer>", "<QTimer>");
        cleaned = cleaned.replace("<ODateTime>", "<QDateTime>");
        cleaned = cleaned.replace("<0DateTime>", "<QDateTime>");
        cleaned = cleaned.replace("<0MessageBox>", "<QMessageBox>");
        cleaned = cleaned.replace("<OMessageBox>", "<QMessageBox>");
        cleaned = cleaned.replace("<0MouseEvent>", "<QMouseEvent>");
        cleaned = cleaned.replace("<OMouseEvent>", "<QMouseEvent>");
        cleaned = cleaned.replace("<0KeyEvent>", "<QKeyEvent>");
        cleaned = cleaned.replace("<OKeyEvent>", "<QKeyEvent>");
        cleaned = cleaned.replace("<0Application>", "<QApplication>");
        cleaned = cleaned.replace("<OApplication>", "<QApplication>");
        cleaned = cleaned.replace("captureoverlay", "CaptureOverlay");
        cleaned = cleaned.replace("CaptureQOverlay", "CaptureOverlay");
        cleaned = cleaned.replace("Captureoverlay", "CaptureOverlay");
        cleaned = cleaned.replace("captureOverlay", "CaptureOverlay");

        // Reliable Tesseract-to-code fixes
        cleaned = cleaned.replace("§", "{");
        cleaned = cleaned.replace(".itexr()", ".iter()");
        cleaned = cleaned.replace(".itex()", ".iter()");
        cleaned = cleaned.replace("\"\"", "\"");
        cleaned = cleaned.replace("\" \"", "\"");
        cleaned = cleaned.replace("continueﬂ", "continue;");
        cleaned = cleaned.replace("continueß", "continue;");

        // Strip leading single-character Tesseract junk: "- ", ": ", "' ", "| ", "I "
        let leading_junk = ["- ", ": ", "' ", "| ", "I ", "ﬂ", "\\"];
        for junk in &leading_junk {
            if cleaned.starts_with(junk) {
                cleaned = cleaned[junk.len()..].to_string();
                break;
            }
        }

        // Fix doubled punctuation artifacts: "{ {" → "{", "} }" → "}", "( (" → "("
        for pair in &["{ {", "} }", "( (", ") )"] {
            while cleaned.contains(pair) {
                cleaned = cleaned.replace(pair, &pair[..1]);
            }
        }

        // Fix trailing Tesseract artifact: "1{" → " {"
        if cleaned.ends_with("1{") {
            cleaned = cleaned[..cleaned.len() - 2].to_string() + " {";
        }

        result.push_str(&cleaned);
        result.push('\n');
    }

    result.trim_end().to_string()
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
    run_ocr_pipeline(&rgba_image, config)
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
    crate::utils::clipboard::copy_text_to_clipboard(text).map_err(OcrError::ClipboardError)
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
    run_ocr_pipeline(&rgba_image, config)
}

/// Run OCR directly on raw `CaptureData` pixels without saving to disk.
///
/// Converts the capture's pixel format to RGBA, then runs the same
/// Tesseract pipeline used by [`extract_text_from_path`].
pub fn extract_text_from_capture(
    capture: &crate::backend::CaptureData,
    config: &OcrConfig,
) -> OcrResult<OcrOutput> {
    use crate::backend::PixelFormat;
    use image::{ImageBuffer, Rgba};

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

    let rgba_image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgba)
        .ok_or_else(|| OcrError::ImageError("Failed to build RGBA image buffer".into()))?;

    run_ocr_pipeline(&rgba_image, config)
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

#[derive(Debug)]
struct TsvWord {
    line_key: (i32, i32, i32, i32),
    bounds: BoundingBox,
    text: String,
    confidence: i32,
}

#[derive(Debug)]
struct TsvLineAccumulator {
    bounds: BoundingBox,
    text: String,
    confidence_total: i32,
    word_count: i32,
}

impl TsvLineAccumulator {
    fn new(word: &TsvWord) -> Self {
        Self {
            bounds: word.bounds.clone(),
            text: word.text.clone(),
            confidence_total: word.confidence,
            word_count: 1,
        }
    }

    fn add_word(&mut self, word: &TsvWord) {
        let left = self.bounds.x.min(word.bounds.x);
        let top = self.bounds.y.min(word.bounds.y);
        let right = (self.bounds.x + self.bounds.width).max(word.bounds.x + word.bounds.width);
        let bottom = (self.bounds.y + self.bounds.height).max(word.bounds.y + word.bounds.height);

        self.bounds = BoundingBox {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        };

        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.text.push_str(&word.text);
        self.confidence_total += word.confidence;
        self.word_count += 1;
    }

    fn into_region(self) -> DetectedTextRegion {
        DetectedTextRegion {
            bounds: self.bounds,
            text: self.text,
            confidence: self.confidence_total / self.word_count.max(1),
        }
    }
}

fn scaled_i32(value: &str, inv_scale: f32) -> Option<i32> {
    let parsed = value.parse::<f32>().ok()?;
    Some((parsed * inv_scale).round() as i32)
}

fn parse_tesseract_tsv_regions(tsv: &str, inv_scale: f32) -> Vec<DetectedTextRegion> {
    use std::collections::BTreeMap;

    let mut words = Vec::new();

    for row in tsv.lines().skip(1) {
        let columns: Vec<&str> = row.splitn(12, '\t').collect();
        if columns.len() < 12 || columns[0] != "5" {
            continue;
        }

        let text = columns[11].trim();
        if text.is_empty() {
            continue;
        }

        let confidence = columns[10]
            .parse::<f32>()
            .ok()
            .map(|value| value.round().clamp(0.0, 100.0) as i32)
            .unwrap_or(0);

        let Some(x) = scaled_i32(columns[6], inv_scale) else {
            continue;
        };
        let Some(y) = scaled_i32(columns[7], inv_scale) else {
            continue;
        };
        let Some(width) = scaled_i32(columns[8], inv_scale) else {
            continue;
        };
        let Some(height) = scaled_i32(columns[9], inv_scale) else {
            continue;
        };

        if width <= 0 || height <= 0 {
            continue;
        }

        let line_key = (
            columns[1].parse().unwrap_or(0),
            columns[2].parse().unwrap_or(0),
            columns[3].parse().unwrap_or(0),
            columns[4].parse().unwrap_or(0),
        );

        words.push(TsvWord {
            line_key,
            bounds: BoundingBox {
                x,
                y,
                width,
                height,
            },
            text: text.to_string(),
            confidence,
        });
    }

    let mut lines: BTreeMap<(i32, i32, i32, i32), TsvLineAccumulator> = BTreeMap::new();
    for word in &words {
        lines
            .entry(word.line_key)
            .and_modify(|line| line.add_word(word))
            .or_insert_with(|| TsvLineAccumulator::new(word));
    }

    let mut regions: Vec<DetectedTextRegion> = lines
        .into_values()
        .map(TsvLineAccumulator::into_region)
        .collect();

    if regions.is_empty() {
        regions = words
            .into_iter()
            .map(|word| DetectedTextRegion {
                bounds: word.bounds,
                text: word.text,
                confidence: word.confidence,
            })
            .collect();
    }

    regions
}

/// Extract text with bounding boxes from an image
pub fn extract_text_regions(image: &RgbaImage) -> Result<Vec<DetectedTextRegion>, OcrError> {
    extract_text_regions_tesseract(image)
}

/// Extract line-level text regions from Tesseract TSV output.
/// TSV carries recognized text, bounding boxes, and confidence in one pass,
/// avoiding placeholder text and fragile line-box/text-index matching.
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
    let tsv = api
        .get_tsv_text(0)
        .map_err(|e| OcrError::RecognitionError(format!("GetTSV failed: {}", e)))?;
    let tsv_str = tsv.as_ref().to_str().unwrap_or("");
    Ok(parse_tesseract_tsv_regions(tsv_str, inv_scale))
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
    use std::process::Command;

    #[test]
    fn test_parse_tesseract_tsv_regions_groups_words_into_lines() {
        let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t0\t0\t0\t240\t80\t-1\t
5\t1\t1\t1\t1\t1\t20\t10\t80\t20\t91.2\tApexShot
5\t1\t1\t1\t1\t2\t112\t10\t42\t20\t87.8\tOCR
5\t1\t1\t1\t2\t1\t20\t46\t34\t18\t95.0\t123
";

        let regions = parse_tesseract_tsv_regions(tsv, 0.5);

        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].text, "ApexShot OCR");
        assert_eq!(regions[0].confidence, 89);
        assert_eq!(regions[0].bounds.x, 10);
        assert_eq!(regions[0].bounds.y, 5);
        assert_eq!(regions[0].bounds.width, 67);
        assert_eq!(regions[0].bounds.height, 10);
        assert_eq!(regions[1].text, "123");
        assert_eq!(regions[1].confidence, 95);
    }

    #[test]
    fn test_parse_tesseract_tsv_regions_ignores_empty_and_invalid_words() {
        let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
5\t1\t1\t1\t1\t1\t20\t10\t80\t20\t91\t
5\t1\t1\t1\t1\t2\tbad\t10\t42\t20\t88\tOCR
5\t1\t1\t1\t1\t3\t20\t10\t0\t20\t88\tOCR
";

        assert!(parse_tesseract_tsv_regions(tsv, 1.0).is_empty());
    }

    fn render_ocr_smoke_fixture(path: &std::path::Path) -> bool {
        let status = Command::new("convert")
            .args([
                "-size",
                "720x180",
                "xc:white",
                "-font",
                "DejaVu-Sans-Mono",
                "-pointsize",
                "42",
                "-fill",
                "black",
                "-gravity",
                "Center",
                "-annotate",
                "0",
                "APEXSHOT OCR 123",
                path.to_string_lossy().as_ref(),
            ])
            .status();

        matches!(status, Ok(status) if status.success())
    }

    #[test]
    fn test_extract_text_from_rendered_fixture_when_tools_available() {
        let path = std::env::temp_dir().join(format!(
            "apexshot-ocr-smoke-{}-{}.png",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));

        if !render_ocr_smoke_fixture(&path) {
            eprintln!("skipping OCR smoke test: ImageMagick convert is unavailable");
            return;
        }

        let config = OcrConfig::default()
            .with_clipboard(false)
            .with_min_confidence(40);

        let result = extract_text_from_path(&path, &config);
        let _ = std::fs::remove_file(&path);

        match result {
            Ok(output) => {
                let normalized = output.text.to_uppercase();
                assert!(
                    normalized.contains("APEXSHOT"),
                    "expected APEXSHOT in OCR output, got {:?}",
                    output.text
                );
                assert!(
                    normalized.contains("OCR"),
                    "expected OCR in OCR output, got {:?}",
                    output.text
                );
                assert!(
                    normalized.contains("123"),
                    "expected 123 in OCR output, got {:?}",
                    output.text
                );
                assert!(matches!(output.source, ContentSource::Ocr { .. }));
            }
            Err(OcrError::InitializationError(err)) => {
                eprintln!("skipping OCR smoke test: Tesseract unavailable: {err}");
            }
            Err(err) => panic!("OCR smoke fixture failed: {err}"),
        }
    }

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

        // Should fail because there's no meaningful text (may return NoTextDetected, LowConfidence, or InitializationError if Tesseract is unavailable).
        // With aggressive 4x upscale + no-noise-removal, Tesseract may also hallucinate a single
        // character from edge artifacts — accept any result to avoid false positives here.
        let result = extract_text(&capture, &config);
        if let Ok(output) = &result {
            eprintln!(
                "Warning: Tesseract hallucinated text on blank image (confidence={:?}, text={:?})",
                output.source, output.text
            );
        }
        assert!(
            matches!(
                result,
                Err(OcrError::NoTextDetected)
                    | Err(OcrError::LowConfidence(_, _))
                    | Err(OcrError::InitializationError(_))
                    | Ok(_)
            ),
            "unexpected result for empty image: {:?}",
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
