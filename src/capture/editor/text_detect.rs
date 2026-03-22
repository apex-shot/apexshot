//! Text detection types for highlighter enhancement
//!
//! This module provides types for representing detected text regions in an image,
//! enabling the highlighter tool to intelligently highlight text.
//!
//! Uses ocrs for fast, accurate text detection on screenshots.

use super::types::{Point, Rect};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// ============================================================================
// Constants
// ============================================================================

/// Minimum cursor size for highlighter strokes
pub const MIN_CURSOR_SIZE: f64 = 8.0;

/// Maximum cursor size for highlighter strokes
pub const MAX_CURSOR_SIZE: f64 = 72.0;

/// Default cursor size for highlighter strokes
pub const DEFAULT_CURSOR_SIZE: f64 = 16.0;

/// Grid cell size for spatial indexing (pixels)
const GRID_CELL_SIZE: i32 = 50;

/// Extra hit padding around detected text to make text-aware highlighting
/// easier to start and stop exactly at word boundaries without requiring
/// pixel-perfect pointer placement.
const TEXT_GUIDE_MARGIN: i32 = 6;

/// Screenshots often contain very small UI labels (eg. button text).
/// Upscaling before OCR improves detection of these compact text regions.
const OCR_INPUT_SCALE: f32 = 2.0;

/// Mild local contrast boost for screenshots with text inside colored buttons,
/// chips and low-contrast controls.
const OCR_UI_CONTRAST: f32 = 1.18;

/// Default text detection model URL (ocrs - pure Rust)
const DEFAULT_DETECTION_MODEL_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";

// ============================================================================
// Helper Functions
// ============================================================================

/// Clamp a cursor size value to the valid range [MIN_CURSOR_SIZE, MAX_CURSOR_SIZE].
pub fn clamp_cursor_size(size: f64) -> f64 {
    size.clamp(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE)
}

/// Convert a pixel coordinate to a grid cell key
fn grid_key(x: i32, y: i32) -> (i32, i32) {
    (x / GRID_CELL_SIZE, y / GRID_CELL_SIZE)
}

/// Apply light screenshot-oriented preprocessing to improve detection of text
/// inside buttons and other compact UI controls.
fn preprocess_for_ui_text_detection(image: &RgbaImage) -> RgbaImage {
    let mut processed = image.clone();

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
        let boosted =
            |channel: f32| ((channel - luminance) * OCR_UI_CONTRAST + luminance).clamp(0.0, 255.0);

        pixel[0] = boosted(r).round() as u8;
        pixel[1] = boosted(g).round() as u8;
        pixel[2] = boosted(b).round() as u8;
        pixel[3] = 255;
    }

    processed
}

/// Load the text detection model from the cache or download it.
fn load_detection_model() -> Result<rten::Model, String> {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
        .join("apexshot");
    let model_path = cache_dir.join("text-detection.rten");

    if model_path.exists() {
        // Load from cache
        let data = std::fs::read(&model_path)
            .map_err(|e| format!("Failed to read cached model: {}", e))?;
        rten::Model::load(data).map_err(|e| format!("Failed to load cached detection model: {}", e))
    } else {
        // Download model
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache dir: {}", e))?;

        let output = std::process::Command::new("curl")
            .args([
                "-L",
                "-o",
                model_path.to_str().unwrap(),
                DEFAULT_DETECTION_MODEL_URL,
            ])
            .output()
            .map_err(|e| format!("Failed to download model: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to download model: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let data = std::fs::read(&model_path)
            .map_err(|e| format!("Failed to read downloaded model: {}", e))?;
        rten::Model::load(data)
            .map_err(|e| format!("Failed to load downloaded detection model: {}", e))
    }
}

// ============================================================================
// Word Region
// ============================================================================

/// A single word detected within a text region.
///
/// Represents the bounding box and text content of an individual word,
/// typically produced by OCR or text detection algorithms.
#[derive(Debug, Clone)]
pub struct WordRegion {
    /// Bounding rectangle of the word in image coordinates
    pub bounds: Rect,
    /// The text content of the word
    pub text: String,
}

// ============================================================================
// Text Region
// ============================================================================

/// A detected text region, typically a line of text.
///
/// Contains the overall bounding box, text metrics, and individual word regions.
/// Used by the highlighter tool to determine appropriate highlight regions.
#[derive(Debug, Clone)]
pub struct TextRegion {
    /// Bounding rectangle of the entire text region in image coordinates
    pub bounds: Rect,
    /// Height of the text (font size equivalent)
    pub text_height: f64,
    /// Y-coordinate of the text baseline
    pub baseline_y: f64,
    /// Individual words within this text region
    pub words: Vec<WordRegion>,
}

impl TextRegion {
    fn contains_point_with_margin(&self, point: Point, margin: i32) -> bool {
        let x = point.x as i32;
        let y = point.y as i32;
        x >= self.bounds.x - margin
            && x < self.bounds.x + self.bounds.width + margin
            && y >= self.bounds.y - margin
            && y < self.bounds.y + self.bounds.height + margin
    }

    /// Check if a point lies within this text region's bounds.
    pub fn contains_point(&self, point: Point) -> bool {
        self.contains_point_with_margin(point, 0)
    }

    /// Check if a point lies within the guided hover zone for this text region.
    pub fn contains_point_with_guide(&self, point: Point) -> bool {
        self.contains_point_with_margin(point, TEXT_GUIDE_MARGIN)
    }

    /// Check if this text region intersects with a given rectangle.
    pub fn intersects_rect(&self, rect: &Rect) -> bool {
        let self_right = self.bounds.x + self.bounds.width;
        let self_bottom = self.bounds.y + self.bounds.height;
        let other_right = rect.x + rect.width;
        let other_bottom = rect.y + rect.height;

        self.bounds.x < other_right
            && self_right > rect.x
            && self.bounds.y < other_bottom
            && self_bottom > rect.y
    }
}

// ============================================================================
// Detection Status
// ============================================================================

/// Status of text detection operation.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionStatus {
    /// Detection is pending and has not yet started
    Pending,
    /// Detection completed successfully
    Ready,
    /// Detection failed with an error message
    Failed(String),
}

// ============================================================================
// Text Detector
// ============================================================================

/// Container for detected text regions and detection status.
///
/// Manages the state of text detection, including the detected regions
/// and whether detection is pending, ready, or failed.
/// Uses spatial indexing for fast point lookup.
#[derive(Debug, Clone)]
pub struct TextDetector {
    /// Detected text regions
    regions: Vec<TextRegion>,
    /// Current detection status
    status: DetectionStatus,
    /// Spatial grid index: maps grid cell (x, y) to region indices
    /// This enables O(1) lookup for point queries instead of O(n)
    spatial_index: HashMap<(i32, i32), Vec<usize>>,
}

impl TextDetector {
    /// Create a new detector in pending state with no regions.
    pub fn new_pending() -> Self {
        Self {
            regions: Vec::new(),
            status: DetectionStatus::Pending,
            spatial_index: HashMap::new(),
        }
    }

    /// Create a detector with pre-populated regions (ready state).
    pub fn with_regions(regions: Vec<TextRegion>) -> Self {
        let mut detector = Self {
            regions,
            status: DetectionStatus::Ready,
            spatial_index: HashMap::new(),
        };
        detector.build_spatial_index();
        detector
    }

    /// Create a detector in failed state with an error message.
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            regions: Vec::new(),
            status: DetectionStatus::Failed(error.into()),
            spatial_index: HashMap::new(),
        }
    }

    /// Build spatial index from regions
    fn build_spatial_index(&mut self) {
        self.spatial_index.clear();
        for (idx, region) in self.regions.iter().enumerate() {
            let start_x = region.bounds.x / GRID_CELL_SIZE;
            let start_y = region.bounds.y / GRID_CELL_SIZE;
            let end_x = (region.bounds.x + region.bounds.width) / GRID_CELL_SIZE;
            let end_y = (region.bounds.y + region.bounds.height) / GRID_CELL_SIZE;

            for gx in start_x..=end_x {
                for gy in start_y..=end_y {
                    self.spatial_index
                        .entry((gx, gy))
                        .or_insert_with(Vec::new)
                        .push(idx);
                }
            }
        }
    }

    /// Get the current detection status.
    pub fn status(&self) -> &DetectionStatus {
        &self.status
    }

    /// Check if detection is complete and regions are available.
    pub fn is_ready(&self) -> bool {
        matches!(self.status, DetectionStatus::Ready)
    }

    /// Get the detected text regions, if detection is ready.
    pub fn regions(&self) -> &[TextRegion] {
        &self.regions
    }

    fn hit_test_point_with<F>(&self, point: Point, contains: F) -> Option<&TextRegion>
    where
        F: Fn(&TextRegion, Point) -> bool,
    {
        if !self.is_ready() {
            return None;
        }

        let cell = grid_key(point.x as i32, point.y as i32);
        if let Some(indices) = self.spatial_index.get(&cell) {
            for &idx in indices {
                if let Some(region) = self.regions.get(idx) {
                    if contains(region, point) {
                        return Some(region);
                    }
                }
            }
        }
        None
    }

    /// Find the text region containing the given point.
    /// Uses spatial indexing for O(1) average case lookup.
    pub fn hit_test_point(&self, point: Point) -> Option<&TextRegion> {
        self.hit_test_point_with(point, |region, point| region.contains_point(point))
    }

    /// Find the text region containing the given point, allowing a small guide margin
    /// around the detected text bounds to make pointer placement less fragile.
    pub fn guided_hit_test_point(&self, point: Point) -> Option<&TextRegion> {
        self.hit_test_point_with(point, |region, point| {
            region.contains_point_with_guide(point)
        })
    }

    /// Find the text height using a small guided zone around detected text bounds.
    pub fn best_text_height_at_point(&self, point: Point) -> Option<f64> {
        self.guided_hit_test_point(point)
            .map(|region| region.text_height)
    }

    /// Find all text regions intersecting the given path.
    /// Uses spatial indexing for efficient lookup.
    pub fn hit_test_path(&self, path: &[Point]) -> Vec<&TextRegion> {
        if !self.is_ready() || path.is_empty() {
            return Vec::new();
        }

        let mut cells_visited = std::collections::HashSet::new();
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;

        for (i, point) in path.iter().enumerate() {
            let cell = grid_key(point.x as i32, point.y as i32);
            cells_visited.insert(cell);

            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);

            if i > 0 {
                let prev = path[i - 1];
                let dx = point.x - prev.x;
                let dy = point.y - prev.y;
                let steps = ((dx.abs().max(dy.abs())) / GRID_CELL_SIZE as f64).ceil() as i32;
                for s in 1..=steps {
                    let t = s as f64 / steps as f64;
                    let interp_x = prev.x + dx * t;
                    let interp_y = prev.y + dy * t;
                    cells_visited.insert(grid_key(interp_x as i32, interp_y as i32));
                }
            }
        }

        let padding = GRID_CELL_SIZE as f64;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;

        let gc_min_x = (min_x as i32 / GRID_CELL_SIZE) - 1;
        let gc_min_y = (min_y as i32 / GRID_CELL_SIZE) - 1;
        let gc_max_x = (max_x as i32 / GRID_CELL_SIZE) + 1;
        let gc_max_y = (max_y as i32 / GRID_CELL_SIZE) + 1;

        for gx in gc_min_x..=gc_max_x {
            for gy in gc_min_y..=gc_max_y {
                cells_visited.insert((gx, gy));
            }
        }

        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for cell in &cells_visited {
            if let Some(indices) = self.spatial_index.get(cell) {
                for &idx in indices {
                    if seen.insert(idx) {
                        if let Some(region) = self.regions.get(idx) {
                            let path_bounds = Rect {
                                x: min_x.floor() as i32,
                                y: min_y.floor() as i32,
                                width: (max_x - min_x).ceil().max(1.0) as i32,
                                height: (max_y - min_y).ceil().max(1.0) as i32,
                            };
                            if region.intersects_rect(&path_bounds) {
                                result.push(region);
                            }
                        }
                    }
                }
            }
        }

        result.sort_by(|a, b| (a.bounds.y, a.bounds.x).cmp(&(b.bounds.y, b.bounds.x)));

        result
    }

    /// Get the text height at a given point, if a text region contains it.
    pub fn text_height_at_point(&self, point: Point) -> Option<f64> {
        self.hit_test_point(point).map(|region| region.text_height)
    }

    /// Set detection results, transitioning to ready state.
    pub fn set_results(&mut self, regions: Vec<TextRegion>) {
        self.regions = regions;
        self.build_spatial_index();
        self.status = DetectionStatus::Ready;
    }

    /// Set detection as failed with an error message.
    pub fn set_failed(&mut self, error: impl Into<String>) {
        self.regions.clear();
        self.spatial_index.clear();
        self.status = DetectionStatus::Failed(error.into());
    }
}

impl Default for TextDetector {
    fn default() -> Self {
        Self::new_pending()
    }
}

// ============================================================================
// Background Text Detection
// ============================================================================

/// Thread-safe flag for background text detection status.
#[derive(Debug, Clone)]
pub struct BackgroundTextDetection {
    /// Atomic flag indicating detection completion
    ready: Arc<AtomicBool>,
}

impl BackgroundTextDetection {
    /// Create a new background detection flag in not-ready state.
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if background detection has completed.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// Mark background detection as complete.
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    /// Get a clone of the ready flag for use in background threads.
    pub fn ready_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ready)
    }
}

impl Default for BackgroundTextDetection {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Background Detection Functions
// ============================================================================

/// Run text detection in background thread using ocrs
///
/// Returns immediately. Results are set on the provided detector.
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

/// Detect text regions using ocrs (pure Rust OCR engine)
fn detect_text_regions_ocrs(image: &RgbaImage) -> Result<Vec<TextRegion>, String> {
    use image::imageops::{resize, FilterType};
    use ocrs::{DimOrder, ImageSource, OcrEngine, OcrEngineParams};
    use rten_imageproc::{BoundingRect, RotatedRect};
    use rten_tensor::prelude::*;

    // Load detection model
    let detection_model = load_detection_model()
        .map_err(|e| format!("Failed to load ocrs detection model: {}", e))?;

    // Create OCR engine (detection only - no recognition needed for cursor sizing)
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: None, // Not needed for cursor sizing
        ..Default::default()
    })
    .map_err(|e| format!("Failed to create OCR engine: {}", e))?;

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

    // Convert image to ocrs format
    let width = scaled.width() as usize;
    let height = scaled.height() as usize;
    let rgb_data: Vec<u8> = scaled
        .pixels()
        .flat_map(|p| [p[0], p[1], p[2]]) // Convert RGBA to RGB
        .collect();

    // Create ImageSource from raw pixel data
    let image_tensor = rten_tensor::NdTensor::from_data(
        [height, width, 3],
        rgb_data
            .iter()
            .map(|&b| b as f32 / 255.0)
            .collect::<Vec<_>>(),
    );

    // Transpose to CHW format
    let chw_tensor = image_tensor.permuted([2, 0, 1]);

    let input = engine
        .prepare_input(
            ImageSource::from_tensor(chw_tensor.view(), DimOrder::Chw)
                .map_err(|e| format!("Failed to create image source: {}", e))?,
        )
        .map_err(|e| format!("Failed to prepare input: {}", e))?;

    // Detect text words
    let words: Vec<RotatedRect> = engine
        .detect_words(&input)
        .map_err(|e| format!("Failed to detect words: {}", e))?;

    if words.is_empty() {
        return Ok(Vec::new());
    }

    // Convert words to bounding boxes and sort by Y position
    let mut word_boxes: Vec<(Rect, f64)> = words
        .iter()
        .filter_map(|rotated_rect| {
            let rect = rotated_rect.bounding_rect();

            let x = (rect.left() / OCR_INPUT_SCALE).floor() as i32;
            let y = (rect.top() / OCR_INPUT_SCALE).floor() as i32;
            let right = ((rect.left() + rect.width()) / OCR_INPUT_SCALE).ceil() as i32;
            let bottom = ((rect.top() + rect.height()) / OCR_INPUT_SCALE).ceil() as i32;

            let bounds = Rect {
                x,
                y,
                width: (right - x).max(1),
                height: (bottom - y).max(1),
            };
            let center_y = bounds.y as f64 + bounds.height as f64 / 2.0;

            if bounds.width < 2 || bounds.height < 2 {
                None
            } else {
                Some((bounds, center_y))
            }
        })
        .collect();

    // Sort by Y position first, then X position
    word_boxes.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.x.cmp(&b.0.x))
    });

    // Group words into lines based on vertical proximity
    // Words on the same line have center Y values within a threshold
    let mut lines: Vec<Vec<Rect>> = Vec::new();
    let mut current_line: Vec<Rect> = Vec::new();
    let mut current_line_y: Option<f64> = None;

    for (bounds, center_y) in word_boxes {
        let is_new_line = match current_line_y {
            Some(line_y) => {
                // Group words if their vertical centers are within 50% of the word height
                let avg_height: f64 = current_line.iter().map(|r| r.height as f64).sum::<f64>()
                    / current_line.len().max(1) as f64;
                (center_y - line_y).abs() > avg_height * 0.5
            }
            None => false,
        };

        if is_new_line {
            if !current_line.is_empty() {
                lines.push(current_line);
            }
            current_line = Vec::new();
            current_line_y = Some(center_y);
        } else if current_line_y.is_none() {
            current_line_y = Some(center_y);
        }

        current_line.push(bounds);
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Convert lines to TextRegion format
    let text_regions: Vec<TextRegion> = lines
        .into_iter()
        .filter_map(|line_words| {
            if line_words.is_empty() {
                return None;
            }

            // Calculate bounding box of the entire line
            let mut min_x = f64::MAX;
            let mut min_y = f64::MAX;
            let mut max_x = f64::MIN;
            let mut max_y = f64::MIN;

            let word_regions: Vec<WordRegion> = line_words
                .into_iter()
                .map(|word_bounds| {
                    min_x = min_x.min(word_bounds.x as f64);
                    min_y = min_y.min(word_bounds.y as f64);
                    max_x = max_x.max((word_bounds.x + word_bounds.width) as f64);
                    max_y = max_y.max((word_bounds.y + word_bounds.height) as f64);

                    WordRegion {
                        bounds: word_bounds,
                        text: String::new(),
                    }
                })
                .collect();

            let bounds = Rect {
                x: min_x as i32,
                y: min_y as i32,
                width: (max_x - min_x) as i32,
                height: (max_y - min_y) as i32,
            };

            // Skip empty/tiny regions
            if bounds.width < 2 || bounds.height < 2 {
                return None;
            }

            let text_height = bounds.height as f64;
            let baseline_y = bounds.y as f64 + text_height / 2.0;

            Some(TextRegion {
                bounds,
                text_height,
                baseline_y,
                words: word_regions,
            })
        })
        .collect();

    Ok(text_regions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn text_region_contains_point() {
        let region = TextRegion {
            bounds: make_rect(10, 20, 100, 30),
            text_height: 20.0,
            baseline_y: 40.0,
            words: vec![],
        };

        assert!(region.contains_point(Point { x: 50.0, y: 35.0 }));
        assert!(region.contains_point(Point { x: 10.0, y: 20.0 }));
        assert!(!region.contains_point(Point { x: 9.0, y: 35.0 }));
        assert!(!region.contains_point(Point { x: 110.0, y: 35.0 }));
        assert!(!region.contains_point(Point { x: 50.0, y: 50.0 }));
    }

    #[test]
    fn text_region_intersects_rect() {
        let region = TextRegion {
            bounds: make_rect(10, 10, 50, 20),
            text_height: 16.0,
            baseline_y: 24.0,
            words: vec![],
        };

        assert!(region.intersects_rect(&make_rect(30, 15, 50, 50)));
        assert!(region.intersects_rect(&make_rect(20, 15, 20, 10)));
        assert!(!region.intersects_rect(&make_rect(100, 100, 50, 50)));
        assert!(!region.intersects_rect(&make_rect(60, 10, 50, 20)));
    }

    #[test]
    fn text_detector_hit_test() {
        let regions = vec![
            TextRegion {
                bounds: make_rect(0, 0, 100, 20),
                text_height: 16.0,
                baseline_y: 14.0,
                words: vec![],
            },
            TextRegion {
                bounds: make_rect(0, 30, 100, 20),
                text_height: 16.0,
                baseline_y: 44.0,
                words: vec![],
            },
        ];

        let detector = TextDetector::with_regions(regions);

        assert!(detector.is_ready());
        assert!(detector
            .hit_test_point(Point { x: 50.0, y: 10.0 })
            .is_some());
        assert!(detector
            .hit_test_point(Point { x: 50.0, y: 40.0 })
            .is_some());
        assert!(detector
            .hit_test_point(Point { x: 50.0, y: 25.0 })
            .is_none());
    }

    #[test]
    fn text_detector_hit_test_path() {
        let regions = vec![
            TextRegion {
                bounds: make_rect(0, 0, 100, 20),
                text_height: 16.0,
                baseline_y: 14.0,
                words: vec![],
            },
            TextRegion {
                bounds: make_rect(0, 50, 100, 20),
                text_height: 16.0,
                baseline_y: 64.0,
                words: vec![],
            },
        ];

        let detector = TextDetector::with_regions(regions);

        let hits =
            detector.hit_test_path(&[Point { x: 10.0, y: 10.0 }, Point { x: 90.0, y: 10.0 }]);
        assert_eq!(hits.len(), 1);

        let hits = detector.hit_test_path(&[Point { x: 50.0, y: 0.0 }, Point { x: 50.0, y: 70.0 }]);
        assert_eq!(hits.len(), 2);

        let hits =
            detector.hit_test_path(&[Point { x: 150.0, y: 10.0 }, Point { x: 200.0, y: 10.0 }]);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn clamp_cursor_size_bounds() {
        assert_eq!(clamp_cursor_size(5.0), MIN_CURSOR_SIZE);
        assert_eq!(clamp_cursor_size(100.0), MAX_CURSOR_SIZE);
        assert_eq!(clamp_cursor_size(20.0), 20.0);
        assert_eq!(clamp_cursor_size(MIN_CURSOR_SIZE), MIN_CURSOR_SIZE);
        assert_eq!(clamp_cursor_size(MAX_CURSOR_SIZE), MAX_CURSOR_SIZE);
    }

    #[test]
    fn background_text_detection_flag() {
        let detection = BackgroundTextDetection::new();
        assert!(!detection.is_ready());

        detection.mark_ready();
        assert!(detection.is_ready());

        let flag = detection.ready_flag();
        flag.store(false, Ordering::Release);
        assert!(!detection.is_ready());

        flag.store(true, Ordering::Release);
        assert!(detection.is_ready());
    }

    #[test]
    fn detection_status_transitions() {
        let mut detector = TextDetector::new_pending();
        assert!(matches!(detector.status(), DetectionStatus::Pending));
        assert!(!detector.is_ready());

        detector.set_results(vec![]);
        assert!(matches!(detector.status(), DetectionStatus::Ready));
        assert!(detector.is_ready());

        detector.set_failed("Test error");
        assert!(matches!(detector.status(), DetectionStatus::Failed(_)));
        assert!(!detector.is_ready());
    }
}
