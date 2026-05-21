//! ApexShot's native OCR engine powered by neural text detection
//!
//! This module provides high-accuracy OCR using our own ML pipeline:
//! - Neural network text detection (segmentation model)
//! - Geometric layout analysis (max-empty-rectangles algorithm)
//! - CRNN text recognition with CTC decoding (beam search for code)
//!
//! Models are cached locally after first download.

use anyhow::Context;
use image::{GrayImage, ImageBuffer, Luma, RgbaImage};
use ocrs::{DecodeMethod, DimOrder, ImageSource, OcrEngine, OcrEngineParams};
use rten_imageproc::BoundingRect;
use rten_tensor::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// URL for the text detection model
const DETECTION_MODEL_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";

/// URL for the text recognition model
const RECOGNITION_MODEL_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// Cache directory for OCR models
fn model_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("ocr-models")
}

/// Ensure the model cache directory exists
fn ensure_cache_dir() -> std::io::Result<()> {
    let dir = model_cache_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// Get the cached path for a model URL
fn cached_model_path(url: &str) -> PathBuf {
    let filename = url.split('/').last().unwrap_or("model.rten");
    model_cache_dir().join(filename)
}

/// Download a model from URL to cache if not already present
fn ensure_model_cached(url: &str) -> anyhow::Result<PathBuf> {
    let cache_path = cached_model_path(url);

    if cache_path.exists() {
        return Ok(cache_path);
    }

    ensure_cache_dir().context("Failed to create model cache directory")?;

    eprintln!("[apexshot-ocr] Downloading model from {}...", url);

    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .context("Failed to download model")?;

    let status = response.status();
    if status != 200 {
        anyhow::bail!("Model download failed: HTTP {}", status);
    }

    let mut file = fs::File::create(&cache_path)
        .with_context(|| format!("Failed to create cache file: {}", cache_path.display()))?;

    let mut reader = response.into_reader();
    std::io::copy(&mut reader, &mut file).context("Failed to write model to cache")?;

    eprintln!("[apexshot-ocr] Model cached to {}", cache_path.display());
    Ok(cache_path)
}

/// Global OCR engine instance (lazy-initialized)
static OCR_ENGINE: OnceLock<Option<OcrEngine>> = OnceLock::new();

/// Initialize the OCR engine, loading models from cache or downloading them
fn init_ocr_engine() -> anyhow::Result<Option<OcrEngine>> {
    let detection_path =
        ensure_model_cached(DETECTION_MODEL_URL).context("Failed to cache detection model")?;
    let recognition_path =
        ensure_model_cached(RECOGNITION_MODEL_URL).context("Failed to cache recognition model")?;

    let detection_model =
        rten::Model::load_file(&detection_path).context("Failed to load detection model")?;

    let recognition_model =
        rten::Model::load_file(&recognition_path).context("Failed to load recognition model")?;

    // Use beam search for better accuracy on code with many symbols
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        decode_method: DecodeMethod::BeamSearch { width: 100 },
        // Restrict to code-relevant characters to prevent spurious outputs
        allowed_chars: Some(
            "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t".to_string()
        ),
        ..Default::default()
    })
    .context("Failed to initialize OCR engine")?;

    Ok(Some(engine))
}

/// Get or initialize the global OCR engine
fn get_ocr_engine() -> Option<&'static OcrEngine> {
    OCR_ENGINE
        .get_or_init(|| init_ocr_engine().ok().flatten())
        .as_ref()
}

/// Preprocess image for better code recognition:
/// 1. Convert to grayscale
/// 2. Upscale 2x to ensure small symbols exceed detection min_area
/// 3. Binarize (black text on white background) for maximum contrast
fn preprocess_for_code(image: &RgbaImage) -> RgbaImage {
    use image::imageops::{resize, FilterType};

    let (width, height) = image.dimensions();
    let scale = 2.0;
    let new_width = (width as f32 * scale) as u32;
    let new_height = (height as f32 * scale) as u32;

    // Resize with Lanczos for sharp edges
    let resized = resize(image, new_width, new_height, FilterType::Lanczos3);

    // Convert to grayscale and binarize
    let gray: GrayImage = ImageBuffer::from_fn(new_width, new_height, |x, y| {
        let pixel = resized.get_pixel(x, y);
        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        // Simple threshold: dark text becomes black, light becomes white
        let value = if luma < 128.0 { 0 } else { 255 };
        Luma([value])
    });

    // Convert back to RGBA for ocrs
    RgbaImage::from_fn(new_width, new_height, |x, y| {
        let g = gray.get_pixel(x, y)[0];
        image::Rgba([g, g, g, 255])
    })
}

/// Convert RgbaImage to HWC tensor for ocrs
fn rgba_to_hwc_tensor(image: &RgbaImage) -> rten_tensor::NdTensor<u8, 3> {
    let (width, height) = image.dimensions();
    let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);

    for pixel in image.pixels() {
        rgb_data.push(pixel[0]); // R
        rgb_data.push(pixel[1]); // G
        rgb_data.push(pixel[2]); // B
    }

    rten_tensor::NdTensor::from_data([height as usize, width as usize, 3], rgb_data)
}

/// Track the leftmost X coordinate to reconstruct indentation
struct IndentationTracker {
    base_x: Option<i32>,
}

impl IndentationTracker {
    fn new() -> Self {
        Self { base_x: None }
    }

    /// Update tracker with first line's X position
    fn set_base(&mut self, x: i32) {
        if self.base_x.is_none() {
            self.base_x = Some(x);
        }
    }

    /// Calculate indentation level (number of spaces)
    fn get_indent(&self, x: i32) -> usize {
        if let Some(base) = self.base_x {
            // Assume 4 spaces per indent level, estimate from X offset
            let offset = (x - base).max(0);
            // Rough estimate: ~10 pixels per space at 2x scale
            (offset / 10).max(0) as usize
        } else {
            0
        }
    }
}

/// Run OCR using our neural engine
///
/// Returns the extracted text and confidence score (always 80+ for ocrs since
/// it doesn't provide per-word confidence like Tesseract)
pub fn run_apexshot_ocr(rgba_image: &RgbaImage) -> Option<(String, i32)> {
    let engine = get_ocr_engine()?;

    // Preprocess for better code symbol detection
    let processed = preprocess_for_code(rgba_image);

    let tensor = rgba_to_hwc_tensor(&processed);
    let img_source = ImageSource::from_tensor(tensor.view(), DimOrder::Hwc).ok()?;

    let input = engine.prepare_input(img_source).ok()?;

    // Use full pipeline to get word-level coordinates for indentation
    let word_rects = match engine.detect_words(&input) {
        Ok(rects) => rects,
        Err(_) => return None,
    };

    if word_rects.is_empty() {
        return None;
    }

    let line_rects = engine.find_text_lines(&input, &word_rects);
    if line_rects.is_empty() {
        return None;
    }

    let line_texts = match engine.recognize_text(&input, &line_rects) {
        Ok(texts) => texts,
        Err(_) => return None,
    };

    // Reconstruct text with proper indentation based on word positions
    let mut result_lines = Vec::new();
    let mut indent_tracker = IndentationTracker::new();

    for (line_idx, line_opt) in line_texts.iter().enumerate() {
        if let Some(line) = line_opt {
            let line_text = line.to_string();
            if line_text.trim().is_empty() {
                continue;
            }

            // Get the X position of the first word for indentation
            if let Some(first_word) = line_rects.get(line_idx).and_then(|words| words.first()) {
                let x = first_word.bounding_rect().left() as i32;
                indent_tracker.set_base(x);

                // Add indentation spaces
                let indent = indent_tracker.get_indent(x);
                let indent_str = " ".repeat(indent);
                result_lines.push(format!("{}{}", indent_str, line_text.trim_start()));
            } else {
                result_lines.push(line_text);
            }
        }
    }

    if result_lines.is_empty() {
        return None;
    }

    let text = result_lines.join("\n");
    Some((text, 85))
}

/// Reset the OCR engine (useful for testing or after model updates)
pub fn reset_ocr_engine() {
    // Note: OnceLock cannot be reset, so this is a no-op for now
    // In a future version we could use a RwLock instead
}
