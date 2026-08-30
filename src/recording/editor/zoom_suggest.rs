//! Automatic zoom placement derived from recorded pointer interactions.
//!
//! GNOME's stage click hook only sees Shell chrome (the stop bar, the top
//! panel), not clicks inside the app being recorded. Detect therefore treats
//! in-frame pointer dwells as the primary signal, and only keeps sidecar
//! clicks that actually land inside the video.

use crate::recording::editor::sidecar::{ClickSample, PointerSample, PointerSidecar};

/// Zoom scale used for automatically placed clips.
pub const AUTO_ZOOM_SCALE: f64 = 1.5;
/// Tighter zoom for short, click-like pauses.
pub const CLICK_ZOOM_SCALE: f64 = 1.8;
/// Shortest usable suggested region, matching manual zoom placement.
pub const MIN_SUGGESTED_ZOOM_SECONDS: f64 = 0.2;
/// Longest auto-placed region so a cluster cannot swallow the whole video.
pub const MAX_SUGGESTED_ZOOM_SECONDS: f64 = 4.5;

/// Consecutive interactions farther apart than this start a new cluster.
const CLICK_CLUSTER_MERGE_GAP_SECONDS: f64 = 1.5;
/// Padding added before the first and after the last interaction of a cluster.
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
const MIN_DWELL_SECONDS: f64 = 0.45;
const MAX_DWELL_SECONDS: f64 = 2.6;
const START_IDLE_SECONDS: f64 = 0.08;
const CHROME_TOP_PX: f64 = 48.0;
const CHROME_END_SECONDS: f64 = 2.5;
const SHORT_DWELL_SECONDS: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionKind {
    Click,
    DoubleClick,
    TextFieldClick,
    TextSelection,
    DropdownOpen,
    Dwell,
}

impl InteractionKind {
    fn strength(self) -> f64 {
        match self {
            Self::DoubleClick => 1500.0,
            Self::TextSelection => 1300.0,
            Self::DropdownOpen => 1200.0,
            Self::TextFieldClick => 1100.0,
            Self::Click => 900.0,
            Self::Dwell => 700.0,
        }
    }

    fn zoom_scale(self, dwell_seconds: f64) -> f64 {
        match self {
            Self::DoubleClick => 2.2,
            Self::TextSelection | Self::DropdownOpen | Self::Click | Self::TextFieldClick => {
                CLICK_ZOOM_SCALE
            }
            Self::Dwell if dwell_seconds < SHORT_DWELL_SECONDS => CLICK_ZOOM_SCALE,
            Self::Dwell => AUTO_ZOOM_SCALE,
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
    /// Zoom amount derived from the interaction, not a global default.
    pub scale: f64,
}

struct ClassifiedClick {
    t: f64,
    end: f64,
    x: f64,
    y: f64,
    strength: f64,
    scale: f64,
}

/// Build zoom suggestions from this recording's pointer path.
///
/// In-frame clicks are classified (double-click, text selection, dropdown,
/// text field, generic click). Pointer dwells — pauses in the cursor path —
/// are treated as interactions too, because Wayland cannot see clicks inside
/// other windows. Each time-cluster becomes one region padded around its
/// interactions and focused on the strongest one.
pub fn suggest_zooms(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
    total_seconds: f64,
) -> Vec<ZoomSuggestion> {
    if total_seconds <= MIN_SUGGESTED_ZOOM_SECONDS {
        return Vec::new();
    }
    let norm = normalized_scale(width, height);
    let mut interactions = classify_clicks(sidecar, width, height, total_seconds, norm);
    interactions.extend(detect_dwells(sidecar, width, height, total_seconds, norm));
    if interactions.is_empty() {
        return Vec::new();
    }

    let mut clusters: Vec<Vec<&ClassifiedClick>> = Vec::new();
    let mut sorted: Vec<&ClassifiedClick> = interactions.iter().collect();
    sorted.sort_by(|a, b| a.t.total_cmp(&b.t));
    for click in sorted {
        let starts_new = clusters.last().is_none_or(|cluster| {
            let last = cluster.last().expect("clusters are never empty");
            click.t - last.end > CLICK_CLUSTER_MERGE_GAP_SECONDS
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
        let mut start = (first.t - CLICK_CLUSTER_PAD_SECONDS).max(0.0);
        let mut end = (last.end + CLICK_CLUSTER_PAD_SECONDS).min(total_seconds);
        if end - start < MIN_SUGGESTED_ZOOM_SECONDS {
            continue;
        }
        if end - start > MAX_SUGGESTED_ZOOM_SECONDS {
            let mid = (strongest.t + strongest.end) * 0.5;
            start = (mid - MAX_SUGGESTED_ZOOM_SECONDS * 0.5).max(0.0);
            end = (start + MAX_SUGGESTED_ZOOM_SECONDS).min(total_seconds);
            start = (end - MAX_SUGGESTED_ZOOM_SECONDS).max(0.0);
        }
        suggestions.push(ZoomSuggestion {
            start,
            end,
            center_time: (strongest.t + strongest.end) * 0.5,
            center: (strongest.x, strongest.y),
            scale: strongest.scale,
        });
    }
    suggestions
}

fn classify_clicks(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
    total_seconds: f64,
    scale: (f64, f64),
) -> Vec<ClassifiedClick> {
    sidecar
        .clicks
        .iter()
        .filter(|click| point_is_content(click.x, click.y, click.t, width, height, total_seconds))
        .map(|click| {
            let kind = classify(sidecar, click, scale);
            ClassifiedClick {
                t: click.t,
                end: click.t,
                x: click.x,
                y: click.y,
                strength: kind.strength(),
                scale: kind.zoom_scale(0.0),
            }
        })
        .collect()
}

fn detect_dwells(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
    total_seconds: f64,
    scale: (f64, f64),
) -> Vec<ClassifiedClick> {
    let pointer = &sidecar.pointer;
    if pointer.len() < 2 {
        return Vec::new();
    }

    let mut dwells = Vec::new();
    // Old sidecars skip still samples, so a pause is a time gap between two
    // nearby points. Do not merge those gaps into the 8ms movement around them.
    for index in 1..pointer.len() {
        let prev = &pointer[index - 1];
        let curr = &pointer[index];
        let dt = curr.t - prev.t;
        if !(MIN_DWELL_SECONDS..=MAX_DWELL_SECONDS).contains(&dt) {
            continue;
        }
        if sample_distance(prev, curr, scale) > STATIONARY_FRACTION {
            continue;
        }
        push_dwell(
            &mut dwells,
            pointer,
            index - 1,
            index + 1,
            width,
            height,
            total_seconds,
        );
    }

    // New sidecars keep a still sample every 100ms. Those pauses are dense
    // runs, not gaps. Split when the cursor leaves the run origin.
    let mut run_start = 0usize;
    for index in 1..pointer.len() {
        let dt = pointer[index].t - pointer[index - 1].t;
        let left_origin =
            sample_distance(&pointer[run_start], &pointer[index], scale) > STATIONARY_FRACTION;
        if dt >= MIN_DWELL_SECONDS || left_origin {
            push_dense_dwell(
                &mut dwells,
                pointer,
                run_start,
                index,
                width,
                height,
                total_seconds,
            );
            run_start = index;
        }
    }
    push_dense_dwell(
        &mut dwells,
        pointer,
        run_start,
        pointer.len(),
        width,
        height,
        total_seconds,
    );
    dwells
}

fn push_dense_dwell(
    dwells: &mut Vec<ClassifiedClick>,
    pointer: &[PointerSample],
    start_index: usize,
    end_exclusive: usize,
    width: f64,
    height: f64,
    total_seconds: f64,
) {
    if end_exclusive < start_index + 3 {
        return;
    }
    let run = &pointer[start_index..end_exclusive];
    if run
        .windows(2)
        .any(|pair| pair[1].t - pair[0].t >= MIN_DWELL_SECONDS)
    {
        return;
    }
    push_dwell(
        dwells,
        pointer,
        start_index,
        end_exclusive,
        width,
        height,
        total_seconds,
    );
}

fn push_dwell(
    dwells: &mut Vec<ClassifiedClick>,
    pointer: &[PointerSample],
    start_index: usize,
    end_exclusive: usize,
    width: f64,
    height: f64,
    total_seconds: f64,
) {
    if end_exclusive <= start_index + 1 {
        return;
    }
    if start_index == 0 && pointer[0].t <= START_IDLE_SECONDS {
        return;
    }
    let start = &pointer[start_index];
    let end = &pointer[end_exclusive - 1];
    let duration = end.t - start.t;
    if !(MIN_DWELL_SECONDS..=MAX_DWELL_SECONDS).contains(&duration) {
        return;
    }
    let run = &pointer[start_index..end_exclusive];
    let x = run.iter().map(|sample| sample.x).sum::<f64>() / run.len() as f64;
    let y = run.iter().map(|sample| sample.y).sum::<f64>() / run.len() as f64;
    let mid = (start.t + end.t) * 0.5;
    if !point_is_content(x, y, mid, width, height, total_seconds) {
        return;
    }
    dwells.push(ClassifiedClick {
        t: start.t,
        end: end.t,
        x,
        y,
        strength: InteractionKind::Dwell.strength() + duration * 100.0,
        scale: InteractionKind::Dwell.zoom_scale(duration),
    });
}

fn sample_distance(a: &PointerSample, b: &PointerSample, scale: (f64, f64)) -> f64 {
    let dx = (b.x - a.x) * scale.0;
    let dy = (b.y - a.y) * scale.1;
    dx.hypot(dy)
}

fn point_is_content(x: f64, y: f64, t: f64, width: f64, height: f64, total_seconds: f64) -> bool {
    if width > 1.0 && (x < 0.0 || x > width) {
        return false;
    }
    if height > 1.0 && (y < 0.0 || y > height) {
        return false;
    }
    // GNOME stage clicks land on the stop bar / top panel, not the app.
    if y < CHROME_TOP_PX && t + CHROME_END_SECONDS >= total_seconds {
        return false;
    }
    true
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

    #[test]
    fn out_of_frame_clicks_are_ignored() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(9.4, 941.0, -215.0));
        sidecar.pointer.push(sample(0.0, 100.0, 100.0));
        sidecar.pointer.push(sample(1.0, 180.0, 140.0));
        assert!(suggest_zooms(&sidecar, W, H, 10.0).is_empty());
    }

    #[test]
    fn stop_bar_click_near_the_end_is_ignored() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(8.7, 1471.0, 16.0));
        assert!(suggest_zooms(&sidecar, W, H, 10.0).is_empty());
    }

    #[test]
    fn pointer_dwell_becomes_a_zoom_without_clicks() {
        let mut sidecar = sidecar();
        sidecar.pointer.push(sample(3.0, 400.0, 300.0));
        sidecar.pointer.push(sample(4.2, 402.0, 301.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].start - 2.5).abs() < 1e-9);
        assert!((suggestions[0].end - 4.7).abs() < 1e-9);
        assert!((suggestions[0].center.0 - 401.0).abs() < 1.0);
        assert!((suggestions[0].center.1 - 300.5).abs() < 1.0);
        assert!((suggestions[0].scale - AUTO_ZOOM_SCALE).abs() < 1e-9);
    }

    #[test]
    fn short_dwell_uses_a_tighter_zoom() {
        let mut sidecar = sidecar();
        sidecar.pointer.push(sample(2.0, 800.0, 400.0));
        sidecar.pointer.push(sample(2.6, 801.0, 401.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].scale - CLICK_ZOOM_SCALE).abs() < 1e-9);
        assert!((suggestions[0].end - suggestions[0].start - 1.6).abs() < 1e-9);
    }

    #[test]
    fn dense_still_samples_become_one_dwell() {
        let mut sidecar = sidecar();
        for index in 0..13 {
            sidecar
                .pointer
                .push(sample(2.0 + index as f64 * 0.1, 960.0, 540.0));
        }
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].start - 1.5).abs() < 1e-9);
        assert!((suggestions[0].end - 3.7).abs() < 1e-9);
        assert!((suggestions[0].scale - AUTO_ZOOM_SCALE).abs() < 1e-9);
    }

    #[test]
    fn start_idle_is_not_a_dwell() {
        let mut sidecar = sidecar();
        sidecar.pointer.push(sample(0.0, 100.0, 100.0));
        sidecar.pointer.push(sample(1.8, 101.0, 100.0));
        assert!(suggest_zooms(&sidecar, W, H, 10.0).is_empty());
    }

    #[test]
    fn different_pointer_paths_produce_different_zoom_times() {
        let mut first = sidecar();
        first.pointer.push(sample(2.0, 300.0, 400.0));
        first.pointer.push(sample(3.4, 301.0, 401.0));
        let mut second = sidecar();
        second.pointer.push(sample(6.4, 900.0, 500.0));
        second.pointer.push(sample(7.0, 901.0, 502.0));
        let a = suggest_zooms(&first, W, H, 10.0);
        let b = suggest_zooms(&second, W, H, 10.0);
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert!((a[0].start - b[0].start).abs() > 3.0);
        assert!((a[0].end - a[0].start - (b[0].end - b[0].start)).abs() > 0.5);
        assert!((a[0].center.0 - b[0].center.0).abs() > 100.0);
        assert!((a[0].scale - b[0].scale).abs() > 0.1);
    }

    #[test]
    fn in_frame_click_still_places_a_padded_region() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(click(3.0, 400.0, 300.0));
        let suggestions = suggest_zooms(&sidecar, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].scale - CLICK_ZOOM_SCALE).abs() < 1e-9);
    }
}
