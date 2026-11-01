//! Text detection types for the text-aware highlighter.
//!
//! The detector geometry and hit-testing are the Linux editor's; the
//! recognition itself goes through Windows Runtime OCR (`super::ocr`), which
//! needs no model download. Detection runs on a worker thread and reports
//! through the shared detector, so a stale result landing after a crop or a
//! new image only ever reaches its own orphaned detector.

use super::types::{Point, Rect};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Minimum cursor size for highlighter strokes.
pub const MIN_CURSOR_SIZE: f64 = 8.0;
/// Maximum cursor size for highlighter strokes.
pub const MAX_CURSOR_SIZE: f64 = 72.0;
/// Default cursor size for highlighter strokes.
pub const DEFAULT_CURSOR_SIZE: f64 = 16.0;

/// Grid cell size for spatial indexing (pixels).
const GRID_CELL_SIZE: i32 = 50;

/// Extra hit padding around detected text to make text-aware highlighting
/// easier to start and stop exactly at word boundaries without requiring
/// pixel-perfect pointer placement.
const TEXT_GUIDE_MARGIN: i32 = 24;

/// Clamp a cursor size value to the valid range [MIN_CURSOR_SIZE, MAX_CURSOR_SIZE].
pub fn clamp_cursor_size(size: f64) -> f64 {
    size.clamp(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE)
}

fn grid_key(x: i32, y: i32) -> (i32, i32) {
    (x / GRID_CELL_SIZE, y / GRID_CELL_SIZE)
}

/// A single word detected within a text region.
#[derive(Debug, Clone)]
pub struct WordRegion {
    /// Bounding rectangle of the word in image coordinates.
    pub bounds: Rect,
    /// The text content of the word.
    pub text: String,
}

/// A detected text region, typically a line of text.
#[derive(Debug, Clone)]
pub struct TextRegion {
    /// Bounding rectangle of the entire text region in image coordinates.
    pub bounds: Rect,
    /// Height of the text (font size equivalent).
    pub text_height: f64,
    /// Y-coordinate of the text baseline.
    pub baseline_y: f64,
    /// Individual words within this text region.
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

/// Status of text detection operation.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionStatus {
    /// Detection is pending and has not yet started.
    Pending,
    /// Detection completed successfully.
    Ready,
    /// Detection failed with an error message.
    Failed(String),
}

/// Container for detected text regions and detection status, with a spatial
/// index for O(1) point lookup.
#[derive(Debug, Clone)]
pub struct TextDetector {
    regions: Vec<TextRegion>,
    status: DetectionStatus,
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

    fn build_spatial_index(&mut self) {
        self.spatial_index.clear();
        for (idx, region) in self.regions.iter().enumerate() {
            let start_x = region.bounds.x / GRID_CELL_SIZE;
            let start_y = region.bounds.y / GRID_CELL_SIZE;
            let end_x = (region.bounds.x + region.bounds.width) / GRID_CELL_SIZE;
            let end_y = (region.bounds.y + region.bounds.height) / GRID_CELL_SIZE;

            for gx in start_x..=end_x {
                for gy in start_y..=end_y {
                    self.spatial_index.entry((gx, gy)).or_default().push(idx);
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
    pub fn hit_test_point(&self, point: Point) -> Option<&TextRegion> {
        self.hit_test_point_with(point, |region, point| region.contains_point(point))
    }

    /// Find the text region containing the given point, allowing a small
    /// guide margin around the detected text bounds.
    pub fn guided_hit_test_point(&self, point: Point) -> Option<&TextRegion> {
        self.hit_test_point_with(point, |region, point| {
            region.contains_point_with_guide(point)
        })
    }

    /// Find the text height using a small guided zone around detected text bounds.
    pub fn best_text_height_at_point(&self, point: Point) -> Option<f64> {
        self.guided_hit_test_point(point)
            .map(|region| region.text_height + 8.0)
    }

    /// Find all text regions intersecting the given path.
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

        result.sort_by_key(|a| (a.bounds.y, a.bounds.x));
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

/// Thread-safe flag for background text detection status.
#[derive(Debug, Clone)]
pub struct BackgroundTextDetection {
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

/// Run text detection on a worker thread using Windows Runtime OCR.
///
/// Returns immediately. Results are set on the provided detector; when the
/// caller has swapped detectors (crop, new image) the stale result only
/// reaches the orphaned one.
pub fn spawn_text_detection(
    image: Arc<RgbaImage>,
    detector: Arc<Mutex<TextDetector>>,
    ready_flag: Arc<AtomicBool>,
) -> BackgroundTextDetection {
    let handle = BackgroundTextDetection::new();

    thread::spawn(move || {
        match super::ocr::detect_word_lines(&image) {
            Ok(lines) => {
                if let Ok(mut det) = detector.lock() {
                    det.set_results(lines.iter().map(line_to_region).collect());
                }
            }
            Err(error) => {
                if let Ok(mut det) = detector.lock() {
                    det.set_failed(error);
                }
            }
        }
        ready_flag.store(true, Ordering::Relaxed);
    });

    handle
}

/// One OCR line becomes one text region: the union of its word rects, the
/// median word height as the text size, and the lowest word edge as the
/// baseline — matching how the Linux fallback shapes Tesseract output.
fn line_to_region(line: &super::ocr::OcrLine) -> TextRegion {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut heights: Vec<f64> = Vec::with_capacity(line.words.len());
    let mut words = Vec::with_capacity(line.words.len());
    for &(x, y, w, h) in &line.words {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);
        heights.push(h as f64);
        words.push(WordRegion {
            bounds: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            text: String::new(),
        });
    }
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let text_height = heights
        .get(heights.len() / 2)
        .copied()
        .unwrap_or(0.0)
        .max(1.0);
    let width = (max_x - min_x).max(1);
    let height = (max_y - min_y).max(1);
    TextRegion {
        bounds: Rect {
            x: min_x.min(max_x),
            y: min_y.min(max_y),
            width,
            height,
        },
        text_height,
        baseline_y: max_y as f64,
        words,
    }
}

#[cfg(test)]
mod tests {
    use super::{line_to_region, TextDetector, TextRegion, WordRegion};
    use crate::capture::editor::types::{Point, Rect};

    fn region(x: i32, y: i32, w: i32, h: i32) -> TextRegion {
        TextRegion {
            bounds: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            text_height: h as f64,
            baseline_y: (y + h) as f64,
            words: vec![WordRegion {
                bounds: Rect {
                    x,
                    y,
                    width: w,
                    height: h,
                },
                text: String::new(),
            }],
        }
    }

    #[test]
    fn hit_testing_uses_exact_bounds_and_the_guide_margin() {
        let detector = TextDetector::with_regions(vec![region(10, 10, 30, 10)]);
        assert!(detector
            .hit_test_point(Point { x: 20.0, y: 15.0 })
            .is_some());
        assert!(detector
            .hit_test_point(Point { x: 20.0, y: 60.0 })
            .is_none());
        assert!(detector
            .guided_hit_test_point(Point { x: 20.0, y: 30.0 })
            .is_some());
    }

    #[test]
    fn path_hit_testing_collects_regions_the_stroke_crosses() {
        let detector = TextDetector::with_regions(vec![
            region(0, 0, 20, 10),
            region(0, 100, 20, 10),
            region(200, 200, 20, 10),
        ]);
        let path = vec![Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 110.0 }];
        assert_eq!(detector.hit_test_path(&path).len(), 2);
    }

    #[test]
    fn ocr_lines_become_union_regions_with_median_height() {
        let line = super::super::ocr::OcrLine {
            words: vec![(10, 20, 30, 12), (45, 18, 25, 16)],
        };
        let region = line_to_region(&line);
        assert_eq!(
            region.bounds,
            Rect {
                x: 10,
                y: 18,
                width: 60,
                height: 16
            }
        );
        assert_eq!(region.words.len(), 2);
        assert!(region.text_height >= 12.0);
    }
}
