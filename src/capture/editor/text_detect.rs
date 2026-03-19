//! Text detection types for highlighter enhancement
//!
//! This module provides types for representing detected text regions in an image,
//! enabling the highlighter tool to intelligently highlight text.

use super::types::{Point, Rect};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ============================================================================
// Constants
// ============================================================================

/// Minimum cursor size for highlighter strokes
pub const MIN_CURSOR_SIZE: f64 = 8.0;

/// Maximum cursor size for highlighter strokes
pub const MAX_CURSOR_SIZE: f64 = 72.0;

/// Default cursor size for highlighter strokes
pub const DEFAULT_CURSOR_SIZE: f64 = 16.0;

// ============================================================================
// Helper Functions
// ============================================================================

/// Clamp a cursor size value to the valid range [MIN_CURSOR_SIZE, MAX_CURSOR_SIZE].
pub fn clamp_cursor_size(size: f64) -> f64 {
    size.clamp(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE)
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
    /// Check if a point lies within this text region's bounds.
    pub fn contains_point(&self, point: Point) -> bool {
        let x = point.x as i32;
        let y = point.y as i32;
        x >= self.bounds.x
            && x < self.bounds.x + self.bounds.width
            && y >= self.bounds.y
            && y < self.bounds.y + self.bounds.height
    }

    /// Check if this text region intersects with a given rectangle.
    pub fn intersects_rect(&self, rect: &Rect) -> bool {
        let self_right = self.bounds.x + self.bounds.width;
        let self_bottom = self.bounds.y + self.bounds.height;
        let other_right = rect.x + rect.width;
        let other_bottom = rect.y + rect.height;

        // No intersection if one rect is completely to the left/right or above/below the other
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
#[derive(Debug, Clone)]
pub struct TextDetector {
    /// Detected text regions
    regions: Vec<TextRegion>,
    /// Current detection status
    status: DetectionStatus,
}

impl TextDetector {
    /// Create a new detector in pending state with no regions.
    pub fn new_pending() -> Self {
        Self {
            regions: Vec::new(),
            status: DetectionStatus::Pending,
        }
    }

    /// Create a detector with pre-populated regions (ready state).
    pub fn with_regions(regions: Vec<TextRegion>) -> Self {
        Self {
            regions,
            status: DetectionStatus::Ready,
        }
    }

    /// Create a detector in failed state with an error message.
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            regions: Vec::new(),
            status: DetectionStatus::Failed(error.into()),
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
    ///
    /// Returns an empty slice if detection is not ready.
    pub fn regions(&self) -> &[TextRegion] {
        &self.regions
    }

    /// Find the text region containing the given point.
    ///
    /// Returns `None` if no region contains the point or detection is not ready.
    pub fn hit_test_point(&self, point: Point) -> Option<&TextRegion> {
        if !self.is_ready() {
            return None;
        }
        self.regions.iter().find(|region| region.contains_point(point))
    }

    /// Find all text regions intersecting the given path (represented as points).
    ///
    /// Returns an empty vector if detection is not ready.
    pub fn hit_test_path(&self, path: &[Point]) -> Vec<&TextRegion> {
        if !self.is_ready() || path.is_empty() {
            return Vec::new();
        }

        // Compute bounding box of the path
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;

        for point in path {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        // Create a rect from the path bounds (with small padding for hit testing)
        let path_bounds = Rect {
            x: min_x.floor() as i32,
            y: min_y.floor() as i32,
            width: (max_x - min_x).ceil().max(1.0) as i32,
            height: (max_y - min_y).ceil().max(1.0) as i32,
        };

        self.regions
            .iter()
            .filter(|region| region.intersects_rect(&path_bounds))
            .collect()
    }

    /// Get the text height at a given point, if a text region contains it.
    ///
    /// Returns `None` if no region contains the point or detection is not ready.
    pub fn text_height_at_point(&self, point: Point) -> Option<f64> {
        self.hit_test_point(point).map(|region| region.text_height)
    }

    /// Set detection results, transitioning to ready state.
    pub fn set_results(&mut self, regions: Vec<TextRegion>) {
        self.regions = regions;
        self.status = DetectionStatus::Ready;
    }

    /// Set detection as failed with an error message.
    pub fn set_failed(&mut self, error: impl Into<String>) {
        self.regions.clear();
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
///
/// Used to signal when background OCR/text detection has completed,
/// allowing the UI thread to check readiness without blocking.
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
    ///
    /// The returned `Arc<AtomicBool>` can be used to signal completion
    /// from a background thread.
    pub fn ready_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ready)
    }
}

impl Default for BackgroundTextDetection {
    fn default() -> Self {
        Self::new()
    }
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

        // Overlapping
        assert!(region.intersects_rect(&make_rect(30, 15, 50, 50)));
        // Inside
        assert!(region.intersects_rect(&make_rect(20, 15, 20, 10)));
        // Outside - no overlap
        assert!(!region.intersects_rect(&make_rect(100, 100, 50, 50)));
        // Adjacent but not overlapping
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
        assert!(detector.hit_test_point(Point { x: 50.0, y: 10.0 }).is_some());
        assert!(detector.hit_test_point(Point { x: 50.0, y: 40.0 }).is_some());
        assert!(detector.hit_test_point(Point { x: 50.0, y: 25.0 }).is_none());
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

        // Path intersecting first region
        let hits = detector.hit_test_path(&[
            Point { x: 10.0, y: 10.0 },
            Point { x: 90.0, y: 10.0 },
        ]);
        assert_eq!(hits.len(), 1);

        // Path intersecting both regions
        let hits = detector.hit_test_path(&[
            Point { x: 50.0, y: 0.0 },
            Point { x: 50.0, y: 70.0 },
        ]);
        assert_eq!(hits.len(), 2);

        // Path not intersecting any region
        let hits = detector.hit_test_path(&[
            Point { x: 150.0, y: 10.0 },
            Point { x: 200.0, y: 10.0 },
        ]);
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
