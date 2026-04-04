# OCR Accuracy Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve text extraction accuracy across all screenshot types by fixing preprocessing, enabling modern Tesseract features, and adding deskewing.

**Architecture:** Six sequential improvements to `src/ocr/mod.rs` preprocessing pipeline and Tesseract configuration, plus deduplication of the two extraction entry points. Each task is independently testable and buildable.

**Tech Stack:** Rust, `image` crate (0.24.9), `tesseract` crate (0.15), Tesseract OCR engine

---

### Task 1: Auto-detect dark/light mode before inverting

**Problem:** `preprocess_image` always inverts luminance (line 180), breaking light-mode screenshots where dark text on white background gets inverted into exactly what Tesseract doesn't want.

**Files:**
- Modify: `src/ocr/mod.rs:146-200` (replace `preprocess_image`)
- Modify: `src/ocr/mod.rs:754-765` (add test)

- [ ] **Step 1: Add dark-mode detection function**

Add this function before `preprocess_image` in `src/ocr/mod.rs`:

```rust
/// Determine if an image is predominantly dark-mode.
///
/// Samples pixels and checks if the median luminance is below 128.
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
```

- [ ] **Step 2: Rewrite `preprocess_image` to conditionally invert**

Replace the entire `preprocess_image` function (lines 146-200) with:

```rust
/// Enhanced preprocessing for better OCR accuracy on UI screenshots
///
/// This performs:
/// 1. Upscaling to improve effective DPI (Tesseract prefers ~300 DPI)
/// 2. Auto-detect dark/light mode and invert only if dark-mode
/// 3. Optional contrast enhancement
/// 4. Optional Otsu binarization
fn preprocess_image(image: &RgbaImage, config: &PreprocessConfig) -> Vec<u8> {
    use image::imageops::{resize, FilterType};

    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * config.scale_factor) as u32;
    let new_height = (original_height as f32 * config.scale_factor) as u32;

    let resized = resize(image, new_width, new_height, FilterType::Lanczos3);

    let dark_mode = is_dark_mode(image);
    let mut luma_data = Vec::with_capacity((new_width * new_height) as usize);

    for pixel in resized.pixels() {
        if pixel[3] < 50 {
            luma_data.push(255);
            continue;
        }

        let luma = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;

        let processed = if dark_mode {
            let inverted = 255.0 - luma;
            if config.contrast > 1.0 {
                ((inverted - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0)
            } else {
                inverted
            }
        } else {
            if config.contrast > 1.0 {
                ((luma - 128.0) * config.contrast + 128.0).clamp(0.0, 255.0)
            } else {
                luma
            }
        };

        luma_data.push(processed as u8);
    }

    if config.threshold {
        apply_otsu_threshold(&mut luma_data);
    }

    luma_data
}
```

- [ ] **Step 3: Add test for dark-mode detection**

Add to the `#[cfg(test)] mod tests` section:

```rust
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
    for y in 0..100 {
        for x in 0..100 {
            let luma = if x < 50 { 40 } else { 220 };
            pixels.extend_from_slice(&[luma, luma, luma, 255]);
        }
    }
    let image = RgbaImage::from_raw(100, 100, pixels).unwrap();
    assert!(!is_dark_mode(&image));
}
```

- [ ] **Step 4: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test ocr::tests::test_is_dark_mode --no-fail-fast`
Expected: All 3 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/ocr/mod.rs
git commit -m "refactor(ocr): auto-detect dark/light mode before inverting

- Add is_dark_mode() function using median luminance sampling
- Only invert dark-mode screenshots, preserve light-mode as-is
- Add tests for dark, light, and mixed content detection"
```

---

### Task 2: Enable LSTM OCR Engine Mode and fix PSM

**Problem:** Tesseract OEM is unset (defaults to system config, often legacy mode). PSM 6 assumes a single uniform text block — wrong for screenshots with mixed layouts.

**Files:**
- Modify: `src/ocr/mod.rs:320-334` (extract_text Tesseract setup)
- Modify: `src/ocr/mod.rs:451-465` (extract_text_from_path Tesseract setup)

- [ ] **Step 1: Add OEM and PSM constants**

Add these constants near the top of `src/ocr/mod.rs`, after the imports:

```rust
/// Tesseract OCR Engine Mode — LSTM only (best accuracy for most text)
const TESS_OEM: &str = "1";

/// Tesseract Page Segmentation Mode — auto with OSD (handles mixed layouts)
const TESS_PSM: &str = "3";
```

- [ ] **Step 2: Update `extract_text` Tesseract configuration**

Replace the Tesseract initialization block in `extract_text` (lines 320-334):

```rust
    let datapath = config.datapath.as_deref();
    let mut tesseract = tesseract::Tesseract::new(datapath, Some(&config.language))
        .map_err(|e| OcrError::InitializationError(e.to_string()))?
        .set_variable("tessedit_ocr_engine_mode", TESS_OEM)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set oem: {}", e)))?
        .set_variable("tessedit_pageseg_mode", TESS_PSM)
        .map_err(|e| OcrError::InitializationError(format!("Failed to set psm: {}", e)))?
        .set_variable("textord_heavy_nr", "1")
        .map_err(|e| OcrError::InitializationError(format!("Failed to set noise removal: {}", e)))?
        .set_frame(
            &luma_data, width, height, 1,
            width,
        )
        .map_err(|e| OcrError::ImageError(format!("Failed to set frame: {}", e)))?
        .recognize()
        .map_err(|e| OcrError::RecognitionError(e.to_string()))?;
```

- [ ] **Step 3: Update `extract_text_from_path` identically**

Replace the Tesseract initialization block in `extract_text_from_path` (lines 451-465) with the exact same code as Step 2.

- [ ] **Step 4: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test ocr::tests::test_extract_text_empty_capture --no-fail-fast`
Expected: Test passes (still returns NoTextDetected for blank image)

- [ ] **Step 5: Commit**

```bash
git add src/ocr/mod.rs
git commit -m "feat(ocr): enable LSTM engine mode and auto page segmentation

- Set tessedit_ocr_engine_mode=1 (LSTM only) for better recognition
- Change PSM from 6 (uniform block) to 3 (auto with OSD) for mixed layouts
- Apply to both extract_text() and extract_text_from_path()"
```

---

### Task 3: Replace adaptive threshold with Otsu's method

**Problem:** `apply_adaptive_threshold` is O(n²) with radius=3 and disabled by default. Otsu's method is O(n), single-pass, and produces better binary separation for text.

**Files:**
- Modify: `src/ocr/mod.rs:213-259` (replace `apply_adaptive_threshold`)
- Modify: `src/ocr/mod.rs:136-144` (update `PreprocessConfig` default)
- Modify: `src/ocr/mod.rs:670-676` (update `fast_region_preprocess_config`)

- [ ] **Step 1: Replace `apply_adaptive_threshold` with Otsu's method**

Replace the entire `apply_adaptive_threshold` function (lines 213-259):

```rust
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
```

- [ ] **Step 2: Update `PreprocessConfig` to enable thresholding by default**

Change the default implementation (lines 136-144):

```rust
impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            scale_factor: 2.0,
            contrast: 1.05,
            threshold: true,  // Otsu's binarization — fast and effective
        }
    }
}
```

- [ ] **Step 3: Update `fast_region_preprocess_config`**

Change the function (lines 670-676):

```rust
fn fast_region_preprocess_config() -> PreprocessConfig {
    PreprocessConfig {
        scale_factor: 2.5,
        contrast: 1.15,
        threshold: true,  // Otsu's for better small text detection
    }
}
```

- [ ] **Step 4: Add Otsu's threshold test**

Add to the test module:

```rust
#[test]
fn test_otsu_threshold_separates_bimodal_data() {
    let mut data: Vec<u8> = (0..100).map(|_| 30u8).chain((0..100).map(|_| 220u8)).collect();
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
```

- [ ] **Step 5: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test ocr::tests::test_otsu --no-fail-fast`
Expected: Both Otsu tests pass

Run: `cargo test ocr::tests --no-fail-fast`
Expected: All OCR tests pass

- [ ] **Step 6: Commit**

```bash
git add src/ocr/mod.rs
git commit -m "refactor(ocr): replace adaptive threshold with Otsu's binarization

- Implement Otsu's method — O(n) single-pass vs O(n*r²) adaptive
- Enable thresholding by default in PreprocessConfig
- Update fast_region_preprocess_config to use Otsu's
- Add tests for bimodal and uniform data separation"
```

---

### Task 4: Add deskewing

**Problem:** Rotated screenshots (even 1-2°) significantly reduce Tesseract accuracy. No rotation detection or correction exists.

**Files:**
- Create: `src/ocr/deskew.rs`
- Modify: `src/ocr/mod.rs` (add module, call deskew in preprocessing)
- Modify: `src/lib.rs` (no change needed — ocr module is already public)

- [ ] **Step 1: Create deskew module**

Create `src/ocr/deskew.rs`:

```rust
//! Image deskewing using projection profile analysis.
//!
//! Detects text rotation angle and corrects it before OCR processing.

use image::RgbaImage;

/// Maximum rotation angle to search for (degrees).
/// Most screenshots are within ±5° if skewed at all.
const MAX_ANGLE_DEG: f64 = 5.0;

/// Angle step size for search (degrees).
const ANGLE_STEP_DEG: f64 = 0.5;

/// Detect the skew angle of text in a grayscale image.
///
/// Uses projection profile variance: when text is aligned,
/// the horizontal projection has maximum variance (sharp peaks at text lines).
pub fn detect_skew_angle(gray_data: &[u8], width: u32, height: u32) -> f64 {
    let mut best_angle = 0.0;
    let mut best_variance = 0.0f64;

    let mut angles = Vec::new();
    let mut angle = -MAX_ANGLE_DEG;
    while angle <= MAX_ANGLE_DEG {
        angles.push(angle);
        angle += ANGLE_STEP_DEG;
    }

    for &angle in &angles {
        let variance = projection_variance(gray_data, width as usize, height as usize, angle);
        if variance > best_variance {
            best_variance = variance;
            best_angle = angle;
        }
    }

    best_angle
}

/// Rotate grayscale image data by the given angle (degrees).
///
/// Returns new pixel data with the same dimensions.
/// Rotation is around the image center, with white (255) fill for empty areas.
pub fn rotate_gray(
    data: &[u8],
    width: usize,
    height: usize,
    angle_deg: f64,
) -> Vec<u8> {
    if angle_deg.abs() < 0.1 {
        return data.to_vec();
    }

    let angle_rad = angle_deg.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    let mut result = vec![255u8; width * height];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;

            let src_x = (dx * cos_a + dy * sin_a + cx).round() as isize;
            let src_y = (-dx * sin_a + dy * cos_a + cy).round() as isize;

            if src_x >= 0 && src_x < width as isize && src_y >= 0 && src_y < height as isize {
                let src_idx = (src_y as usize) * width + (src_x as usize);
                result[y * width + x] = data[src_idx];
            }
        }
    }

    result
}

/// Compute the variance of the horizontal projection profile at a given angle.
///
/// Higher variance means text lines are more clearly separated,
/// indicating better alignment.
fn projection_variance(
    data: &[u8],
    width: usize,
    height: usize,
    angle_deg: f64,
) -> f64 {
    if angle_deg.abs() < 0.1 {
        return projection_variance_straight(data, width, height);
    }

    let angle_rad = angle_deg.to_radians();
    let sin_a = angle_rad.sin();
    let cos_a = angle_rad.cos();

    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    let diag = ((width * width + height * height) as f64).sqrt() as usize;
    let mut projection = vec![0.0f64; diag];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;

            let rotated_y = (-dx * sin_a + dy * cos_a + cy).round() as isize;

            if rotated_y >= 0 && rotated_y < diag as isize {
                let pixel_val = data[y * width + x] as f64;
                projection[rotated_y as usize] += 255.0 - pixel_val;
            }
        }
    }

    variance(&projection)
}

fn projection_variance_straight(data: &[u8], width: usize, height: usize) -> f64 {
    let mut projection = vec![0.0f64; height];

    for y in 0..height {
        for x in 0..width {
            let pixel_val = data[y * width + x] as f64;
            projection[y] += 255.0 - pixel_val;
        }
    }

    variance(&projection)
}

fn variance(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let mean: f64 = values.iter().sum::<f64>() / n;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variance_uniform_values() {
        let values = vec![1.0, 1.0, 1.0, 1.0];
        assert!((variance(&values) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_variance_different_values() {
        let values = vec![0.0, 0.0, 255.0, 255.0];
        let var = variance(&values);
        assert!(var > 10000.0);
    }

    #[test]
    fn test_rotate_zero_degrees() {
        let data = vec![100u8, 200, 150, 50];
        let result = rotate_gray(&data, 2, 2, 0.0);
        assert_eq!(result, data);
    }

    #[test]
    fn test_detect_skew_angle_straight_text() {
        let mut data = vec![255u8; 100 * 100];
        for y in 20..30 {
            for x in 10..90 {
                data[y * 100 + x] = 0;
            }
        }
        for y in 50..60 {
            for x in 10..90 {
                data[y * 100 + x] = 0;
            }
        }

        let angle = detect_skew_angle(&data, 100, 100);
        assert!(angle.abs() < 1.0);
    }
}
```

- [ ] **Step 2: Add deskew module to `src/ocr/mod.rs`**

Add after the existing imports at the top of `src/ocr/mod.rs`:

```rust
pub mod deskew;
```

- [ ] **Step 3: Integrate deskewing into `preprocess_image`**

Modify `preprocess_image` to call deskew after upscaling but before grayscale conversion. Add this block after the `resize` call and before the pixel loop:

```rust
    // Deskew if rotation is detected
    let skew_angle = deskew::detect_skew_angle(
        &luma_data_for_skew,
        new_width,
        new_height,
    );
    if skew_angle.abs() > 0.5 {
        luma_data = deskew::rotate_gray(&luma_data, new_width as usize, new_height as usize, skew_angle);
    }
```

Wait — we need the grayscale data before we can detect skew. Let me restructure. Replace the full `preprocess_image` function:

```rust
fn preprocess_image(image: &RgbaImage, config: &PreprocessConfig) -> Vec<u8> {
    use image::imageops::{resize, FilterType};

    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * config.scale_factor) as u32;
    let new_height = (original_height as f32 * config.scale_factor) as u32;

    let resized = resize(image, new_width, new_height, FilterType::Lanczos3);

    let dark_mode = is_dark_mode(image);

    // First pass: convert to grayscale (no inversion yet) for skew detection
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
        luma_data = deskew::rotate_gray(&luma_data, new_width as usize, new_height as usize, skew_angle);
    }

    // Second pass: apply inversion (if dark mode) and contrast
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
```

- [ ] **Step 4: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test ocr::deskew::tests --no-fail-fast`
Expected: All deskew tests pass

Run: `cargo test ocr::tests --no-fail-fast`
Expected: All OCR tests pass

- [ ] **Step 5: Commit**

```bash
git add src/ocr/mod.rs src/ocr/deskew.rs
git commit -m "feat(ocr): add image deskewing using projection profile analysis

- Create src/ocr/deskew.rs with skew detection and rotation
- Detect angle via projection profile variance (-5° to +5° range)
- Rotate corrected image before inversion and binarization
- Add tests for variance, rotation, and straight text detection"
```

---

### Task 5: Deduplicate `extract_text` and `extract_text_from_path`

**Problem:** The two functions share ~95% identical code (preprocessing, Tesseract setup, post-processing). Changes must be applied to both, risking divergence.

**Files:**
- Modify: `src/ocr/mod.rs` (extract shared `run_tesseract` function, simplify both entry points)

- [ ] **Step 1: Create shared `run_tesseract` function**

Add this function between `preprocess_image` and `extract_text`:

```rust
/// Run Tesseract OCR on an RGBA image.
///
/// Handles preprocessing, QR detection, Tesseract setup, and result formatting.
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
```

- [ ] **Step 2: Simplify `extract_text`**

Replace the entire `extract_text` function:

```rust
pub fn extract_text(capture: &CaptureData, config: &OcrConfig) -> OcrResult<OcrOutput> {
    let rgba_image = capture_to_rgba_image(capture)
        .map_err(|e: SaveError| OcrError::ImageError(e.to_string()))?;
    run_tesseract(&rgba_image, config)
}
```

- [ ] **Step 3: Simplify `extract_text_from_path`**

Replace the entire `extract_text_from_path` function:

```rust
pub fn extract_text_from_path<P: AsRef<std::path::Path>>(
    path: P,
    config: &OcrConfig,
) -> OcrResult<OcrOutput> {
    let image = image::open(path)
        .map_err(|e| OcrError::ImageError(format!("Failed to open image: {}", e)))?;
    let rgba_image = image.to_rgba8();
    run_tesseract(&rgba_image, config)
}
```

- [ ] **Step 4: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test ocr::tests --no-fail-fast`
Expected: All tests pass (same behavior, refactored code)

- [ ] **Step 5: Commit**

```bash
git add src/ocr/mod.rs
git commit -m "refactor(ocr): deduplicate extract_text and extract_text_from_path

- Extract shared run_tesseract() function
- Both entry points now delegate to run_tesseract()
- No behavioral change — pure refactoring"
```

---

### Task 6: Final verification and integration test

**Files:**
- No new files — verify all previous changes work together

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: Compiles without errors or warnings (except existing warnings)

- [ ] **Step 2: Run all OCR tests**

Run: `cargo test ocr --no-fail-fast`
Expected: All tests pass

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit final verification**

```bash
git add -A
git commit -m "test(ocr): verify all OCR accuracy improvements integrate correctly

- All 6 improvements working together: dark-mode detection, LSTM mode,
  PSM 3, Otsu's binarization, deskewing, and deduplication
- All existing tests passing"
```

---

## Summary of Changes

| Task | What Changed | Impact |
|------|-------------|--------|
| 1 | Auto-detect dark/light mode | Fixes light-mode OCR (was broken by unconditional inversion) |
| 2 | LSTM OEM + PSM 3 | Better recognition on mixed-layout screenshots |
| 3 | Otsu's binarization | Faster + better text/background separation |
| 4 | Deskewing | Fixes rotated/skewed captures |
| 5 | Deduplication | Maintainability — single source of truth |
| 6 | Integration test | Verify everything works together |

## Files Modified

- `src/ocr/mod.rs` — all 6 tasks
- `src/ocr/deskew.rs` — new file (Task 4)

## Dependencies

No new dependencies required. All changes use existing `image` crate functionality.
