# Highlighter Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve highlighter text detection accuracy, fix cursor scaling, add OCR fallback, and add loading indicator.

**Architecture:** Five sequential improvements to the highlighter's text detection pipeline (`src/capture/editor/text_detect.rs`), cursor rendering (`src/capture/editor/window/cursor.rs`), and editor state (`src/capture/editor/state.rs`). Each task is independently testable.

**Tech Stack:** Rust, `image` crate (0.24.9), `ocrs`/`rten` (pure Rust OCR), GTK4 Cairo cursors

---

### Task 1: Fix cursor width scaling — chisel-shaped highlighter cursor

**Problem:** `cursor.rs:156` hardcodes cursor width to `DEFAULT_HIGHLIGHTER_CURSOR_SIZE` (16px) regardless of detected text height. Tall text gets a skinny vertical bar cursor. Real highlighter tips are chisel-shaped — wider than tall.

**Solution:** Make the cursor wider than tall (width = height × 1.5), like a real highlighter chisel tip. Both dimensions scale with detected text height.

**Files:**
- Modify: `src/capture/editor/window/cursor.rs:150-213`

- [ ] **Step 1: Fix `create_highlighter_cursor_surface`**

Read the current function at `src/capture/editor/window/cursor.rs:150-213`. Replace the width/height calculation:

Current code (line 155-156):
```rust
    let height = clamp_cursor_size(height) * CURSOR_WIDTH_RATIO;
    let width = DEFAULT_HIGHLIGHTER_CURSOR_SIZE;
```

Replace with:
```rust
    let clamped_height = clamp_cursor_size(height);
    let height = clamped_height;
    let width = clamped_height * CURSOR_WIDTH_RATIO;
```

This makes the cursor **wider than tall** (1.5:1 ratio), like a real highlighter chisel tip. Both dimensions scale with detected text size:
- 8px text → 12×8 cursor
- 16px text → 24×16 cursor
- 32px text → 48×32 cursor

- [ ] **Step 2: Build and verify**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/capture/editor/window/cursor.rs
git commit -m "fix(editor): make highlighter cursor chisel-shaped and scale with text

- Cursor was a fixed 16px wide vertical bar regardless of text size
- Now width = height × 1.5 (chisel shape like real highlighters)
- Both dimensions scale with detected text height"
```

---

### Task 2: Share preprocessing between highlighter and OCR

**Problem:** `text_detect.rs` has its own `preprocess_for_ui_text_detection()` (contrast 1.18x, 2x scale, no dark-mode detection, no Otsu, no deskew) while `src/ocr/mod.rs` has the improved pipeline (dark-mode detection, Otsu, deskew). The highlighter's text detection doesn't benefit from the OCR improvements.

**Solution:** Create a shared preprocessing function in `src/capture/editor/preprocess.rs` that both the highlighter and OCR can use. The highlighter needs RGBA output (for ocrs), while OCR needs grayscale output (for Tesseract).

**Files:**
- Create: `src/capture/editor/preprocess.rs`
- Modify: `src/capture/editor/text_detect.rs` (use shared preprocessing)
- Modify: `src/capture/editor.rs` (add module)
- Modify: `src/ocr/mod.rs` (optional — use shared preprocessing for Tesseract path)

**Note:** The highlighter uses ocrs which needs RGBA input (CHW format), while Tesseract needs grayscale. We'll create a shared function that handles the common steps (dark-mode detection, contrast, upscaling) and returns RGBA for ocrs. The OCR module can separately convert to grayscale after.

- [ ] **Step 1: Create shared preprocessing module**

Create `src/capture/editor/preprocess.rs`:

```rust
//! Shared image preprocessing for text detection and OCR.
//!
//! Provides screenshot-oriented preprocessing that improves text detection
//! accuracy across both the highlighter (ocrs) and OCR (Tesseract) pipelines.

use image::{RgbaImage, imageops::{resize, FilterType}};

/// Mild local contrast boost for screenshots with text inside colored buttons,
/// chips and low-contrast controls.
const UI_CONTRAST: f32 = 1.18;

/// Upscale factor for better text detection of small UI labels.
const UI_SCALE: f32 = 2.0;

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

    let original_width = image.width();
    let original_height = image.height();
    let new_width = (original_width as f32 * UI_SCALE).round().max(1.0) as u32;
    let new_height = (original_height as f32 * UI_SCALE).round().max(1.0) as u32;

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
            let inverted = 255.0 - luminance;
            let boosted = |channel: f32| {
                let inv = 255.0 - channel;
                ((inv - 128.0) * UI_CONTRAST + 128.0).clamp(0.0, 255.0)
            };
            pixel[0] = boosted(r).round() as u8;
            pixel[1] = boosted(g).round() as u8;
            pixel[2] = boosted(b).round() as u8;
        } else {
            let boosted = |channel: f32| {
                ((channel - luminance) * UI_CONTRAST + luminance).clamp(0.0, 255.0)
            };
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
/// Returns a scale factor between 1.0 and 3.0.
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
    fn test_preprocess_preserves_dimensions() {
        let pixels = vec![128u8; 50 * 50 * 4];
        let image = RgbaImage::from_raw(50, 50, pixels).unwrap();
        let (processed, _) = preprocess_for_text_detection(&image);
        assert_eq!(processed.width(), 100);
        assert_eq!(processed.height(), 100);
    }
}
```

- [ ] **Step 2: Add module to `src/capture/editor.rs`**

Read `src/capture/editor.rs` to find the module declarations. Add:

```rust
pub mod preprocess;
```

- [ ] **Step 3: Update `text_detect.rs` to use shared preprocessing**

Read `text_detect.rs` to find the current `preprocess_for_ui_text_detection` function and `detect_text_regions_ocrs`. 

Replace the old preprocessing function and constants. Remove these:
- `OCR_INPUT_SCALE` constant (line 38)
- `OCR_UI_CONTRAST` constant (line 42)
- `preprocess_for_ui_text_detection` function (lines 64-92)

In `detect_text_regions_ocrs`, replace:
```rust
    let preprocessed = preprocess_for_ui_text_detection(image);

    let scaled = if (OCR_INPUT_SCALE - 1.0).abs() > f32::EPSILON {
        let scaled_width = ((preprocessed.width() as f32 * OCR_INPUT_SCALE).round() as u32).max(1);
        let scaled_height =
            ((preprocessed.height() as f32 * OCR_INPUT_SCALE).round() as u32).max(1);
        resize(
            &preprocessed,
            scaled_width,
            scaled_height,
            FilterType::CatmullRom,
        )
    } else {
        preprocessed
    };
```

With:
```rust
    use super::preprocess::{adaptive_scale_factor, preprocess_for_text_detection};

    let scale = adaptive_scale_factor(image.width(), image.height());
    let (preprocessed, _dark_mode) = preprocess_for_text_detection(image);

    let scaled_width = ((preprocessed.width() as f32 * scale).round() as u32).max(1);
    let scaled_height = ((preprocessed.height() as f32 * scale).round() as u32).max(1);
    let scaled = if (scale - 1.0).abs() > f32::EPSILON {
        resize(&preprocessed, scaled_width, scaled_height, FilterType::CatmullRom)
    } else {
        preprocessed
    };
```

Also update the coordinate scaling back — replace all uses of `OCR_INPUT_SCALE` with `scale * 2.0` (since the preprocessing already scaled 2x, and now we apply the adaptive factor on top):

Actually, let me reconsider. The preprocessing already does 2x upscale. The `detect_text_regions_ocrs` was doing another 2x on top. So the total was 4x. Let me simplify: the shared preprocessing does the upscaling, and the ocrs detection runs on that directly without a second resize.

Replace the scaling section with:
```rust
    use super::preprocess::{adaptive_scale_factor, preprocess_for_text_detection};

    let scale = adaptive_scale_factor(image.width(), image.height());
    let (scaled, _dark_mode) = preprocess_for_text_detection(image);

    // If the adaptive scale differs from the preprocessing scale (2.0),
    // do a secondary resize
    let effective_scale = scale / 2.0;
    let scaled = if (effective_scale - 1.0).abs() > f32::EPSILON {
        let w = ((scaled.width() as f32 * effective_scale).round() as u32).max(1);
        let h = ((scaled.height() as f32 * effective_scale).round() as u32).max(1);
        resize(&scaled, w, h, FilterType::CatmullRom)
    } else {
        scaled
    };

    let total_scale = scale;
```

Then replace all `/ OCR_INPUT_SCALE` with `/ total_scale` in the coordinate conversion code.

- [ ] **Step 4: Build and test**

Run: `cargo check`
Expected: Compiles without errors

Run: `cargo test capture::editor::preprocess::tests --no-fail-fast`
Expected: All 5 preprocess tests pass

- [ ] **Step 5: Commit**

```bash
git add src/capture/editor/preprocess.rs src/capture/editor.rs src/capture/editor/text_detect.rs
git commit -m "refactor(editor): share preprocessing between highlighter and OCR

- Create src/capture/editor/preprocess.rs with shared text detection preprocessing
- Add dark-mode detection and contrast boost to highlighter pipeline
- Add adaptive scaling (1.5x-2.5x based on image size) for 4K performance
- Replace hardcoded OCR_INPUT_SCALE/OCR_UI_CONTRAST constants
- Add tests for dark-mode detection, adaptive scaling, and preprocessing"
```

---

### Task 3: Add OCR fallback when ocrs fails

**Problem:** If the ocrs model download fails or detection returns empty, the highlighter silently falls back to 16px default with no indication.

**Solution:** When ocrs detection fails or returns no regions, fall back to Tesseract-based text region detection from `src/ocr/mod.rs::extract_text_regions()`.

**Files:**
- Modify: `src/capture/editor/text_detect.rs` (add fallback in `spawn_text_detection`)
- Modify: `src/capture/editor.rs` (ensure ocr module is accessible)

- [ ] **Step 1: Add Tesseract fallback to `spawn_text_detection`**

Read the current `spawn_text_detection` function in `text_detect.rs` (around lines 501-525). Replace it:

Current:
```rust
pub fn spawn_text_detection(
    image: RgbaImage,
    detector: Arc<Mutex<TextDetector>>,
    ready_flag: Arc<AtomicBool>,
) -> BackgroundTextDetection {
    let handle = BackgroundTextDetection::new();

    thread::spawn(move || {
        match detect_text_regions_ocrs(&image) {
            Ok(regions) => {
                if let Ok(mut det) = detector.lock() {
                    det.set_results(regions);
                }
            }
            Err(e) => {
                if let Ok(mut det) = detector.lock() {
                    det.set_failed(e);
                }
            }
        }
        ready_flag.store(true, Ordering::Relaxed);
    });

    handle
}
```

Replace with:
```rust
pub fn spawn_text_detection(
    image: RgbaImage,
    detector: Arc<Mutex<TextDetector>>,
    ready_flag: Arc<AtomicBool>,
) -> BackgroundTextDetection {
    let handle = BackgroundTextDetection::new();

    thread::spawn(move || {
        // Try ocrs detection first (fast, pure Rust, no system deps)
        let result = detect_text_regions_ocrs(&image);

        match result {
            Ok(regions) if !regions.is_empty() => {
                eprintln!("[text_detect] ocrs detected {} text regions", regions.len());
                if let Ok(mut det) = detector.lock() {
                    det.set_results(regions);
                }
            }
            Ok(_) => {
                eprintln!("[text_detect] ocrs found no regions, falling back to Tesseract");
                // ocrs returned empty — try Tesseract as fallback
                match crate::ocr::extract_text_regions(&image) {
                    Ok(tesseract_regions) if !tesseract_regions.is_empty() => {
                        eprintln!("[text_detect] Tesseract fallback detected {} regions", tesseract_regions.len());
                        let regions: Vec<TextRegion> = tesseract_regions
                            .into_iter()
                            .map(|r| TextRegion {
                                bounds: Rect {
                                    x: r.bounds.x,
                                    y: r.bounds.y,
                                    width: r.bounds.width,
                                    height: r.bounds.height,
                                },
                                text_height: r.bounds.height as f64,
                                baseline_y: r.bounds.y as f64 + r.bounds.height as f64 / 2.0,
                                words: Vec::new(),
                            })
                            .collect();
                        if let Ok(mut det) = detector.lock() {
                            det.set_results(regions);
                        }
                    }
                    _ => {
                        eprintln!("[text_detect] Tesseract fallback also failed");
                        if let Ok(mut det) = detector.lock() {
                            det.set_failed("No text regions detected by ocrs or Tesseract");
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[text_detect] ocrs failed: {e}, falling back to Tesseract");
                // ocrs failed — try Tesseract as fallback
                match crate::ocr::extract_text_regions(&image) {
                    Ok(tesseract_regions) if !tesseract_regions.is_empty() => {
                        eprintln!("[text_detect] Tesseract fallback detected {} regions", tesseract_regions.len());
                        let regions: Vec<TextRegion> = tesseract_regions
                            .into_iter()
                            .map(|r| TextRegion {
                                bounds: Rect {
                                    x: r.bounds.x,
                                    y: r.bounds.y,
                                    width: r.bounds.width,
                                    height: r.bounds.height,
                                },
                                text_height: r.bounds.height as f64,
                                baseline_y: r.bounds.y as f64 + r.bounds.height as f64 / 2.0,
                                words: Vec::new(),
                            })
                            .collect();
                        if let Ok(mut det) = detector.lock() {
                            det.set_results(regions);
                        }
                    }
                    _ => {
                        eprintln!("[text_detect] Tesseract fallback also failed");
                        if let Ok(mut det) = detector.lock() {
                            det.set_failed(format!("ocrs failed: {e}; Tesseract fallback also failed"));
                        }
                    }
                }
            }
        }
        ready_flag.store(true, Ordering::Relaxed);
    });

    handle
}
```

- [ ] **Step 2: Build and test**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/capture/editor/text_detect.rs
git commit -m "feat(editor): add Tesseract fallback for highlighter text detection

- When ocrs model fails or returns empty, fall back to Tesseract OCR
- Log detection source and region count for debugging
- Ensures highlighter works even without ocrs model available"
```

---

### Task 4: Add loading indicator for text detection

**Problem:** The highlighter uses a 16px default cursor until text detection completes, with no visual feedback that detection is in progress.

**Solution:** Add a "detecting" state to the cursor — show a spinning or dashed-outline cursor while text detection is loading.

**Files:**
- Modify: `src/capture/editor/window/cursor.rs` (add detecting cursor variant)
- Modify: `src/capture/editor/window/events.rs` (check detection status before setting cursor)
- Modify: `src/capture/editor/state.rs` (expose detection status)

- [ ] **Step 1: Add a "detecting" cursor variant**

Add a new function to `cursor.rs`:

```rust
/// Create a cursor that indicates text detection is in progress.
/// Shows a dashed-outline rounded rectangle at the default size.
pub fn create_highlighter_detecting_cursor() -> Option<gtk4::cairo::ImageSurface> {
    let height = DEFAULT_HIGHLIGHTER_CURSOR_SIZE * CURSOR_WIDTH_RATIO;
    let width = DEFAULT_HIGHLIGHTER_CURSOR_SIZE;

    let pad = 6.0;
    let surface_width = (width + pad * 2.0).ceil() as i32;
    let surface_height = (height + pad * 2.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;

    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let x = pad;
    let y = pad;
    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);

    fn rounded_rect(ctx: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
        ctx.new_sub_path();
        ctx.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        ctx.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        ctx.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        ctx.arc(x + r, y + r, r, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2);
        ctx.close_path();
    }

    // Dashed gray outline to indicate "loading" state
    rounded_rect(&context, x, y, width, height, radius);
    context.set_source_rgba(0.5, 0.5, 0.5, 0.8);
    context.set_line_width(2.0);
    context.set_dash(&[4.0, 4.0], 0.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    let _ = context.stroke();

    Some(surface)
}
```

- [ ] **Step 2: Update cursor selection to check detection status**

Read `update_cursor_for_position` in `cursor.rs` (around lines 298-337). Before the text-aware cursor sizing logic, check if the detector is still pending:

Add this check at the start of the `HighlighterMode::TextAware` branch:

```rust
HighlighterMode::TextAware => {
    if let Ok(detector) = state.text_detector.lock() {
        match detector.status() {
            DetectionStatus::Pending => {
                // Detection still running — show loading cursor
                if let Some(surface) = create_highlighter_detecting_cursor() {
                    let display = window.display();
                    let cursor = gdk4::Cursor::from_surface(
                        &surface, 6.0, (DEFAULT_HIGHLIGHTER_CURSOR_SIZE * CURSOR_WIDTH_RATIO) / 2.0,
                    );
                    if let Some(cursor) = cursor {
                        root.set_cursor(Some(&cursor));
                        return;
                    }
                }
            }
            DetectionStatus::Failed(_) => {
                // Detection failed — show default cursor with a question mark
                // Fall through to default sizing below
            }
            DetectionStatus::Ready => {
                // Normal text-aware sizing
                let image_point = screen_to_image_coords(point);
                if let Some(text_height) = detector.best_text_height_at_point(image_point) {
                    current_size = text_height;
                }
            }
        }
    }
}
```

- [ ] **Step 3: Build and test**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src/capture/editor/window/cursor.rs src/capture/editor/window/events.rs
git commit -m "feat(editor): add loading indicator for highlighter text detection

- Show dashed gray cursor while ocrs/Tesseract detection is running
- Detects DetectionStatus::Pending state and shows detecting cursor
- Falls back to default cursor when detection fails"
```

---

### Task 5: Final verification

**Files:** No new files — verify all changes work together.

- [ ] **Step 1: Full build**

Run: `cargo build`
Expected: Compiles without errors or new warnings

- [ ] **Step 2: Run all editor tests**

Run: `cargo test capture::editor --no-fail-fast`
Expected: All tests pass

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass (except pre-existing Tesseract data failure)

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test(editor): verify all highlighter enhancements integrate correctly

All 4 improvements working together: cursor width scaling, shared
preprocessing with adaptive scaling, Tesseract fallback, and
loading indicator for text detection."
```

---

## Summary of Changes

| Task | What Changed | Impact |
|------|-------------|--------|
| 1 | Cursor width scales with text height | Visual polish — proportional cursors |
| 2 | Shared preprocessing + adaptive scaling | Better detection accuracy + 4K performance |
| 3 | Tesseract fallback | Reliability — works without ocrs model |
| 4 | Loading indicator | UX — feedback during detection |
| 5 | Integration test | Verify everything works together |

## Files Modified

- `src/capture/editor/window/cursor.rs` — Tasks 1, 4
- `src/capture/editor/preprocess.rs` — new file (Task 2)
- `src/capture/editor/text_detect.rs` — Tasks 2, 3
- `src/capture/editor.rs` — Task 2 (module declaration)
- `src/capture/editor/window/events.rs` — Task 4 (minor)

## Dependencies

No new dependencies required.
