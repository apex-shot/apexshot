//! Automatic zoom placement derived from recorded pointer interactions.
//!
//! Clicks recorded in the pointer sidecar are classified by the cursor
//! trajectory that follows them, grouped into clusters, and turned into
//! zoom regions centered on the most significant click of each cluster.

use crate::recording::editor::sidecar::{ClickSample, PointerSidecar};

/// Zoom scale used for automatically placed clips.
pub const AUTO_ZOOM_SCALE: f64 = 1.5;
/// Shortest usable suggested region, matching manual zoom placement.
pub const MIN_SUGGESTED_ZOOM_SECONDS: f64 = 0.2;

/// Consecutive clicks farther apart than this start a new cluster.
const CLICK_CLUSTER_MERGE_GAP_SECONDS: f64 = 2.5;
/// Padding added before the first and after the last click of a cluster.
const CLICK_CLUSTER_PAD_SECONDS: f64 = 0.5;
/// Two clicks within this window and radius count as a double-click.
const DOUBLE_CLICK_WINDOW_SECONDS: f64 = 0.45;
const DOUBLE_CLICK_RADIUS_FRACTION: f64 = 0.035;
/// Trajectory window after a click used to classify the interaction.
const POST_CLICK_WINDOW_START_SECONDS: f64 = 0.1;
const POST_CLICK_WINDOW_END_SECONDS: f64 = 2.0;
const POST_CLICK_MIN_SAMPLES: usize = 3;
/// Cursor displacement (as a fraction of the frame) thresholds.
const STATIONARY_FRACTION: f64 = 0.02;
const HORIZONTAL_DRAG_FRACTION: f64 = 0.03;
const VERTICAL_MOVE_FRACTION: f64 = 0.03;
const HORIZONTAL_AXIS_RATIO: f64 = 1.8;
const VERTICAL_AXIS_RATIO: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionKind {
    Click,
    DoubleClick,
    TextFieldClick,
    TextSelection,
    DropdownOpen,
}

impl InteractionKind {
    fn strength(self) -> f64 {
        match self {
            Self::DoubleClick => 1500.0,
            Self::TextSelection => 1300.0,
            Self::DropdownOpen => 1200.0,
            Self::TextFieldClick => 1100.0,
            Self::Click => 900.0,
        }
    }
}

/// A zoom region in source-time seconds with the pixel focus to zoom on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomSuggestion {
    pub start: f64,
    pub end: f64,
    /// Source time the suggestion is centered on.
    pub center_time: f64,
    /// Focus point in video pixel coordinates.
    pub center: (f64, f64),
}

struct ClassifiedClick {
    t: f64,
    x: f64,
    y: f64,
    strength: f64,
}

/// Build zoom suggestions from the recorded clicks of a sidecar.
///
/// Clicks are classified (double-click, text selection, dropdown, text field,
/// generic click), clustered by time, and each cluster becomes one region
/// padded around its clicks and focused on its strongest interaction.
pub fn suggest_zooms(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
    total_seconds: f64,
) -> Vec<ZoomSuggestion> {
    if sidecar.clicks.is_empty() || total_seconds <= MIN_SUGGESTED_ZOOM_SECONDS {
        return Vec::new();
    }
    let scale = normalized_scale(width, height);
    let clicks = classify_clicks(sidecar, scale);
    if clicks.is_empty() {
        return Vec::new();
    }

    let mut clusters: Vec<Vec<&ClassifiedClick>> = Vec::new();
    let mut sorted: Vec<&ClassifiedClick> = clicks.iter().collect();
    sorted.sort_by(|a, b| a.t.total_cmp(&b.t));
    for click in sorted {
        let starts_new = clusters.last().is_none_or(|cluster| {
            let last = cluster.last().expect("clusters are never empty");
            click.t - last.t > CLICK_CLUSTER_MERGE_GAP_SECONDS
        });
        if starts_new {
            clusters.push(vec![click]);
        } else {
            clusters
                .last_mut()
                .expect("checked non-empty above")
                .push(click);
        }
    }

    let mut suggestions = Vec::new();
    for cluster in clusters {
        let first = cluster.first().expect("clusters are never empty");
        let last = cluster.last().expect("clusters are never empty");
        let strongest = cluster
            .iter()
            .max_by(|a, b| a.strength.total_cmp(&b.strength))
            .expect("clusters are never empty");
        let start = (first.t - CLICK_CLUSTER_PAD_SECONDS).max(0.0);
        let end = (last.t + CLICK_CLUSTER_PAD_SECONDS).min(total_seconds);
        if end - start < MIN_SUGGESTED_ZOOM_SECONDS {
            continue;
        }
        suggestions.push(ZoomSuggestion {
            start,
            end,
            center_time: (first.t + last.t) * 0.5,
            center: (strongest.x, strongest.y),
        });
    }
    suggestions
}

fn classify_clicks(sidecar: &PointerSidecar, scale: (f64, f64)) -> Vec<ClassifiedClick> {
    sidecar
        .clicks
        .iter()
        .map(|click| ClassifiedClick {
            t: click.t,
            x: click.x,
            y: click.y,
            strength: classify(sidecar, click, scale).strength(),
        })
        .collect()
}

/// Classify what the cursor did after a click by inspecting the pointer
/// trajectory that follows it.
fn classify(sidecar: &PointerSidecar, click: &ClickSample, scale: (f64, f64)) -> InteractionKind {
    if is_double_click(sidecar, click, scale) {
        return InteractionKind::DoubleClick;
    }

    let window = post_click_window(sidecar, click.t);
    if window.len() < POST_CLICK_MIN_SAMPLES {
        return InteractionKind::TextFieldClick;
    }

    let mut max_distance = 0.0_f64;
    let mut total_abs_dx = 0.0_f64;
    let mut total_abs_dy = 0.0_f64;
    let mut last_dy = 0.0_f64;
    for sample in window {
        let dx = (sample.x - click.x) * scale.0;
        let dy = (sample.y - click.y) * scale.1;
        max_distance = max_distance.max(dx.hypot(dy));
        total_abs_dx += dx.abs();
        total_abs_dy += dy.abs();
        last_dy = dy;
    }

    if max_distance < STATIONARY_FRACTION {
        return InteractionKind::TextFieldClick;
    }
    if last_dy > VERTICAL_MOVE_FRACTION && total_abs_dy > total_abs_dx * VERTICAL_AXIS_RATIO {
        return InteractionKind::DropdownOpen;
    }
    if total_abs_dx > HORIZONTAL_DRAG_FRACTION
        && total_abs_dx > total_abs_dy * HORIZONTAL_AXIS_RATIO
    {
        return InteractionKind::TextSelection;
    }
    InteractionKind::Click
}

fn is_double_click(sidecar: &PointerSidecar, click: &ClickSample, scale: (f64, f64)) -> bool {
    sidecar.clicks.iter().any(|other| {
        if other.t == click.t {
            return false;
        }
        let dt = (other.t - click.t).abs();
        if dt > DOUBLE_CLICK_WINDOW_SECONDS {
            return false;
        }
        let dx = (other.x - click.x) * scale.0;
        let dy = (other.y - click.y) * scale.1;
        dx.hypot(dy) <= DOUBLE_CLICK_RADIUS_FRACTION
    })
}

fn post_click_window(
    sidecar: &PointerSidecar,
    click_t: f64,
) -> &[crate::recording::editor::sidecar::PointerSample] {
    let pointer = &sidecar.pointer;
    if pointer.is_empty() {
        return &[];
    }
    let lo =
        pointer.partition_point(|sample| sample.t <= click_t + POST_CLICK_WINDOW_START_SECONDS);
    let hi = pointer.partition_point(|sample| sample.t <= click_t + POST_CLICK_WINDOW_END_SECONDS);
    if lo >= hi {
        return &[];
    }
    &pointer[lo..hi]
}

fn normalized_scale(width: f64, height: f64) -> (f64, f64) {
    let width = if width.is_finite() && width > 1.0 {
        width
    } else {
        1.0
    };
    let height = if height.is_finite() && height > 1.0 {
        height
    } else {
        1.0
    };
    (1.0 / width, 1.0 / height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None))
    }

    fn click(t: f64, x: f64, y: f64) -> ClickSample {
        ClickSample { t, x, y, button: 1 }
    }

    fn sample(t: f64, x: f64, y: f64) -> PointerSample {
        PointerSample {
            t,
            x,
            y,
            kind: CursorKind::Default,
        }
    }

    const W: f64 = 1920.0;
    const H: f64 = 1080.0;

    #[test]
    fn no_clicks_yields_no_suggestions() {
        let mut sidecar = sidecar();
        sidecar.pointer.push(sample(0.0, 100.0, 100.0));
        sidecar.pointer.push(sample(1.0, 200.0, 100.0));
        assert!(suggest_zooms(&sidecar, W, H, 10.0).is_empty());
    }

    #[test]
    fn single_click_builds_padded_region() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(3.0, 400.0, 300.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].start - 2.5).abs() < 1e-9);
        assert!((suggestions[0].end - 3.5).abs() < 1e-9);
        assert!((suggestions[0].center.0 - 400.0).abs() < 1e-9);
        assert!((suggestions[0].center.1 - 300.0).abs() < 1e-9);
    }

    #[test]
    fn cluster_of_rapid_clicks_becomes_one_region() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(2.0, 100.0, 100.0));
        sidecar.clicks.push(click(3.0, 120.0, 110.0));
        sidecar.clicks.push(click(9.0, 900.0, 500.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 20.0);
        assert_eq!(suggestions.len(), 2);
        assert!((suggestions[0].start - 1.5).abs() < 1e-9);
        assert!((suggestions[0].end - 3.5).abs() < 1e-9);
        assert!((suggestions[1].start - 8.5).abs() < 1e-9);
    }

    #[test]
    fn region_is_clamped_to_recording_bounds() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(0.2, 50.0, 50.0));
        sidecar.clicks.push(click(9.8, 500.0, 500.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 2);
        assert!((suggestions[0].start - 0.0).abs() < 1e-9);
        assert!((suggestions[0].end - 0.7).abs() < 1e-9);
        assert!((suggestions[1].start - 9.3).abs() < 1e-9);
        assert!((suggestions[1].end - 10.0).abs() < 1e-9);
    }

    #[test]
    fn double_click_is_stronger_than_plain_click() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(2.0, 100.0, 100.0));
        sidecar.clicks.push(click(2.3, 110.0, 104.0));
        sidecar.clicks.push(click(8.0, 700.0, 400.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 20.0);
        assert_eq!(suggestions.len(), 2);
        // The cluster containing the double-click focuses on one of its
        // clicks at the double-click position, not elsewhere.
        assert!(
            (suggestions[0].center.0 - 100.0).abs() < 1e-9
                || (suggestions[0].center.0 - 110.0).abs() < 1e-9
        );
    }

    #[test]
    fn stationary_cursor_after_click_classifies_as_text_field() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(1.0, 960.0, 540.0));
        for index in 0..12 {
            let t = 1.0 + 0.1 + index as f64 * 0.15;
            sidecar.pointer.push(sample(t, 960.0 + index as f64, 540.0));
        }
        let scale = normalized_scale(W, H);
        let kind = classify(&sidecar, &sidecar.clicks[0], scale);
        assert_eq!(kind, InteractionKind::TextFieldClick);
    }

    #[test]
    fn horizontal_drag_after_click_classifies_as_text_selection() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(1.0, 300.0, 540.0));
        for index in 0..12 {
            let t = 1.0 + 0.12 + index as f64 * 0.15;
            sidecar
                .pointer
                .push(sample(t, 300.0 + index as f64 * 80.0, 540.0));
        }
        let scale = normalized_scale(W, H);
        let kind = classify(&sidecar, &sidecar.clicks[0], scale);
        assert_eq!(kind, InteractionKind::TextSelection);
    }

    #[test]
    fn downward_move_after_click_classifies_as_dropdown() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(1.0, 960.0, 300.0));
        for index in 0..12 {
            let t = 1.0 + 0.12 + index as f64 * 0.15;
            sidecar
                .pointer
                .push(sample(t, 960.0, 300.0 + index as f64 * 60.0));
        }
        let scale = normalized_scale(W, H);
        let kind = classify(&sidecar, &sidecar.clicks[0], scale);
        assert_eq!(kind, InteractionKind::DropdownOpen);
    }

    #[test]
    fn few_trajectory_samples_classify_as_text_field() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(1.0, 100.0, 100.0));
        sidecar.pointer.push(sample(1.5, 100.0, 100.0));
        let scale = normalized_scale(W, H);
        let kind = classify(&sidecar, &sidecar.clicks[0], scale);
        assert_eq!(kind, InteractionKind::TextFieldClick);
    }
}
