//! Automatic zoom placement derived from clicks and purposeful pointer landings.
//!
//! Recorded clicks are primary attention signals. Pointer landings remain a
//! fallback for hover-driven interfaces and recordings without click samples.

use crate::recording::editor::sidecar::PointerSidecar;

/// Zoom scale used for a single purposeful interaction.
pub const AUTO_ZOOM_SCALE: f64 = 1.5;
/// Tighter zoom used for repeated interactions with the same target.
pub const REPEATED_INTERACTION_ZOOM_SCALE: f64 = 1.8;
/// Shortest useful region after trim/segment clipping.
pub const MIN_SUGGESTED_ZOOM_SECONDS: f64 = 1.4;
/// Normal auto-zoom duration before recording-boundary clipping.
pub const MIN_AUTO_ZOOM_SECONDS: f64 = 1.8;
/// Longest auto-placed region so a long hold cannot swallow the video.
pub const MAX_SUGGESTED_ZOOM_SECONDS: f64 = 3.5;

const MAX_SCALE_ANISOTROPY: f64 = 0.03;
const STILL_RADIUS_DIAGONAL_FRACTION: f64 = 0.008;
const MIN_STILL_RADIUS_PX: f64 = 6.0;
const MAX_STILL_RADIUS_PX: f64 = 18.0;
const ARRIVAL_WINDOW_SECONDS: f64 = 0.7;
const ARRIVAL_DISTANCE_DIAGONAL_FRACTION: f64 = 0.025;
const MIN_ARRIVAL_DISTANCE_PX: f64 = 24.0;
const MAX_ARRIVAL_DISTANCE_PX: f64 = 64.0;
const MIN_ARRIVAL_SPEED_PX_PER_SECOND: f64 = 100.0;
const MIN_STABLE_SECONDS: f64 = 0.42;
const CLUSTER_MERGE_GAP_SECONDS: f64 = 0.9;
const CLUSTER_RADIUS_DIAGONAL_FRACTION: f64 = 0.018;
const MIN_CLUSTER_RADIUS_PX: f64 = 12.0;
const MAX_CLUSTER_RADIUS_PX: f64 = 40.0;
const MAX_CLUSTER_SPAN_SECONDS: f64 = 2.5;
const PRE_ROLL_SECONDS: f64 = 0.35;
const POST_ROLL_SECONDS: f64 = 1.1;
const SECONDS_PER_SUGGESTION: f64 = 6.0;
const CLICK_CONFIDENCE: f64 = 100.0;

/// A zoom region in source-time seconds with the pixel focus to zoom on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomSuggestion {
    pub start: f64,
    pub end: f64,
    /// Source time of the interaction used to place the suggestion.
    pub center_time: f64,
    /// Focus point in encoded-video pixel coordinates.
    pub center: (f64, f64),
    pub scale: f64,
    pub(crate) priority: f64,
}

#[derive(Debug, Clone, Copy)]
struct FrameSample {
    t: f64,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct Landing {
    start: f64,
    end: f64,
    center: (f64, f64),
    confidence: f64,
    is_click: bool,
}

#[derive(Debug)]
struct LandingCluster {
    landings: Vec<Landing>,
}

#[derive(Debug)]
struct ScoredSuggestion {
    suggestion: ZoomSuggestion,
    score: f64,
}

/// Build conservative zoom suggestions from this recording's pointer interactions.
///
/// Area-recording sidecars store coordinates relative to the selected region,
/// while the editor works in encoded-video pixels. Coordinates are scaled into
/// that space before detection. If the X/Y scales disagree significantly, the
/// recording and sidecar do not describe the same crop and no guesses are made.
pub fn suggest_zooms(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
    total_seconds: f64,
) -> Vec<ZoomSuggestion> {
    if !width.is_finite()
        || !height.is_finite()
        || !total_seconds.is_finite()
        || width <= 1.0
        || height <= 1.0
        || total_seconds < MIN_SUGGESTED_ZOOM_SECONDS
    {
        return Vec::new();
    }

    let Some((scale_x, scale_y)) = pointer_scale(sidecar, width, height) else {
        return Vec::new();
    };
    let samples = transformed_samples(sidecar, scale_x, scale_y, width, height, total_seconds);
    let clicks = transformed_clicks(sidecar, scale_x, scale_y, width, height, total_seconds);
    if samples.len() < 3 && clicks.is_empty() {
        return Vec::new();
    }

    let diagonal = width.hypot(height);
    let still_radius =
        (diagonal * STILL_RADIUS_DIAGONAL_FRACTION).clamp(MIN_STILL_RADIUS_PX, MAX_STILL_RADIUS_PX);
    let arrival_distance = (diagonal * ARRIVAL_DISTANCE_DIAGONAL_FRACTION)
        .clamp(MIN_ARRIVAL_DISTANCE_PX, MAX_ARRIVAL_DISTANCE_PX);
    let cluster_radius = (diagonal * CLUSTER_RADIUS_DIAGONAL_FRACTION)
        .clamp(MIN_CLUSTER_RADIUS_PX, MAX_CLUSTER_RADIUS_PX);

    let mut landings = detect_landings(&samples, still_radius, arrival_distance);
    landings.extend(clicks);
    landings.sort_by(|a, b| a.start.total_cmp(&b.start));
    let clusters = cluster_landings(landings, cluster_radius);
    let mut suggestions: Vec<ScoredSuggestion> = clusters
        .into_iter()
        .filter_map(|cluster| suggestion_for_cluster(cluster, total_seconds))
        .collect();

    let limit = ((total_seconds / SECONDS_PER_SUGGESTION).ceil() as usize).max(1);
    if suggestions.len() > limit {
        suggestions.sort_by(|a, b| b.score.total_cmp(&a.score));
        suggestions.truncate(limit);
    }
    suggestions.sort_by(|a, b| a.suggestion.start.total_cmp(&b.suggestion.start));
    suggestions
        .into_iter()
        .map(|scored| scored.suggestion)
        .collect()
}

fn pointer_scale(sidecar: &PointerSidecar, width: f64, height: f64) -> Option<(f64, f64)> {
    if !sidecar.region.is_area() {
        return Some((1.0, 1.0));
    }

    let scale_x = width / sidecar.region.w as f64;
    let scale_y = height / sidecar.region.h as f64;
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return None;
    }
    if (scale_x / scale_y - 1.0).abs() > MAX_SCALE_ANISOTROPY {
        return None;
    }
    Some((scale_x, scale_y))
}

fn transformed_samples(
    sidecar: &PointerSidecar,
    scale_x: f64,
    scale_y: f64,
    width: f64,
    height: f64,
    total_seconds: f64,
) -> Vec<FrameSample> {
    let mut samples: Vec<_> = sidecar
        .pointer
        .iter()
        .filter_map(|sample| {
            let x = sample.x * scale_x;
            let y = sample.y * scale_y;
            if !sample.t.is_finite()
                || !x.is_finite()
                || !y.is_finite()
                || sample.t < 0.0
                || sample.t > total_seconds
                || x < 0.0
                || x >= width
                || y < 0.0
                || y >= height
            {
                return None;
            }
            Some(FrameSample { t: sample.t, x, y })
        })
        .collect();
    samples.sort_by(|a, b| a.t.total_cmp(&b.t));
    samples
}

fn transformed_clicks(
    sidecar: &PointerSidecar,
    scale_x: f64,
    scale_y: f64,
    width: f64,
    height: f64,
    total_seconds: f64,
) -> Vec<Landing> {
    sidecar
        .clicks
        .iter()
        .filter_map(|click| {
            let x = click.x * scale_x;
            let y = click.y * scale_y;
            if !click.t.is_finite()
                || !x.is_finite()
                || !y.is_finite()
                || click.t < 0.0
                || click.t > total_seconds
                || x < 0.0
                || x >= width
                || y < 0.0
                || y >= height
                || !(1..=3).contains(&click.button)
            {
                return None;
            }
            Some(Landing {
                start: click.t,
                end: click.t,
                center: (x, y),
                confidence: CLICK_CONFIDENCE,
                is_click: true,
            })
        })
        .collect()
}

fn detect_landings(
    samples: &[FrameSample],
    still_radius: f64,
    min_arrival_distance: f64,
) -> Vec<Landing> {
    let mut landings = Vec::new();
    let mut index = 0;

    while index + 1 < samples.len() {
        let anchor = samples[index];
        let mut hold_end = index;
        while hold_end + 1 < samples.len()
            && distance(samples[hold_end + 1], anchor) <= still_radius
        {
            hold_end += 1;
        }

        let hold_seconds = samples[hold_end].t - anchor.t;
        if hold_seconds < MIN_STABLE_SECONDS || index == 0 {
            index += 1;
            continue;
        }

        let center = median_position(&samples[index..=hold_end]);
        let arrival_start_time = anchor.t - ARRIVAL_WINDOW_SECONDS;
        let arrival_start =
            samples[..index].partition_point(|sample| sample.t < arrival_start_time);
        let incoming = &samples[arrival_start..=index];
        let farthest = incoming
            .iter()
            .map(|sample| point_distance((sample.x, sample.y), center))
            .fold(0.0_f64, f64::max);
        let peak_speed = incoming
            .windows(2)
            .filter_map(|pair| {
                let dt = pair[1].t - pair[0].t;
                let before = point_distance((pair[0].x, pair[0].y), center);
                let after = point_distance((pair[1].x, pair[1].y), center);
                if dt <= 0.0 || after >= before {
                    return None;
                }
                Some(distance(pair[0], pair[1]) / dt)
            })
            .fold(0.0_f64, f64::max);

        if farthest < min_arrival_distance || peak_speed < MIN_ARRIVAL_SPEED_PX_PER_SECOND {
            index += 1;
            continue;
        }

        let confidence = (farthest / min_arrival_distance).min(3.0)
            + (peak_speed / MIN_ARRIVAL_SPEED_PX_PER_SECOND).min(3.0)
            + (hold_seconds / MIN_STABLE_SECONDS).min(3.0);
        landings.push(Landing {
            start: anchor.t,
            end: samples[hold_end].t,
            center,
            confidence,
            is_click: false,
        });
        index = hold_end + 1;
    }

    landings
}

fn cluster_landings(landings: Vec<Landing>, cluster_radius: f64) -> Vec<LandingCluster> {
    let mut clusters: Vec<LandingCluster> = Vec::new();
    for landing in landings {
        let should_merge = clusters.last().is_some_and(|cluster| {
            let first = cluster.landings.first().expect("cluster is non-empty");
            let last = cluster.landings.last().expect("cluster is non-empty");
            let center = median_landing_position(&cluster.landings);
            landing.start - last.end <= CLUSTER_MERGE_GAP_SECONDS
                && landing.end - first.start <= MAX_CLUSTER_SPAN_SECONDS
                && point_distance(landing.center, center) <= cluster_radius
        });
        if should_merge {
            clusters
                .last_mut()
                .expect("checked non-empty above")
                .landings
                .push(landing);
        } else {
            clusters.push(LandingCluster {
                landings: vec![landing],
            });
        }
    }
    clusters
}

fn suggestion_for_cluster(cluster: LandingCluster, total_seconds: f64) -> Option<ScoredSuggestion> {
    let first = cluster.landings.first()?;
    let last = cluster.landings.last()?;
    let clicks: Vec<_> = cluster
        .landings
        .iter()
        .filter(|landing| landing.is_click)
        .collect();
    let focus: Vec<_> = if clicks.is_empty() {
        cluster.landings.iter().collect()
    } else {
        clicks.clone()
    };
    let center = (
        median(focus.iter().map(|landing| landing.center.0).collect()),
        median(focus.iter().map(|landing| landing.center.1).collect()),
    );
    let center_time = median(focus.iter().map(|landing| landing.start).collect());

    let mut start = (first.start - PRE_ROLL_SECONDS).max(0.0);
    let mut end = (last.end + POST_ROLL_SECONDS).min(total_seconds);
    let desired_minimum = MIN_AUTO_ZOOM_SECONDS.min(total_seconds);
    if end - start < desired_minimum {
        end = (start + desired_minimum).min(total_seconds);
        start = (end - desired_minimum).max(0.0);
    }
    if end - start > MAX_SUGGESTED_ZOOM_SECONDS {
        start = (center_time - PRE_ROLL_SECONDS).max(0.0);
        end = (start + MAX_SUGGESTED_ZOOM_SECONDS).min(total_seconds);
        start = (end - MAX_SUGGESTED_ZOOM_SECONDS).max(0.0);
    }
    if end - start < MIN_SUGGESTED_ZOOM_SECONDS {
        return None;
    }

    let repeated = if clicks.is_empty() {
        cluster.landings.len() > 1
    } else {
        clicks.len() > 1
    };
    let score = cluster
        .landings
        .iter()
        .map(|landing| landing.confidence)
        .sum::<f64>()
        + if repeated { 2.0 } else { 0.0 };
    Some(ScoredSuggestion {
        suggestion: ZoomSuggestion {
            start,
            end,
            center_time,
            center,
            scale: if repeated {
                REPEATED_INTERACTION_ZOOM_SCALE
            } else {
                AUTO_ZOOM_SCALE
            },
            priority: score,
        },
        score,
    })
}

fn distance(a: FrameSample, b: FrameSample) -> f64 {
    point_distance((a.x, a.y), (b.x, b.y))
}

fn point_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn median_position(samples: &[FrameSample]) -> (f64, f64) {
    (
        median(samples.iter().map(|sample| sample.x).collect()),
        median(samples.iter().map(|sample| sample.y).collect()),
    )
}

fn median_landing_position(landings: &[Landing]) -> (f64, f64) {
    (
        median(landings.iter().map(|landing| landing.center.0).collect()),
        median(landings.iter().map(|landing| landing.center.1).collect()),
    )
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };

    const W: f64 = 1920.0;
    const H: f64 = 1080.0;

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: W as u32,
                h: H as u32,
            },
        )
    }

    fn sample(t: f64, x: f64, y: f64) -> PointerSample {
        PointerSample {
            t,
            x,
            y,
            kind: CursorKind::Default,
        }
    }

    fn add_landing(
        sidecar: &mut PointerSidecar,
        arrival_t: f64,
        from: (f64, f64),
        target: (f64, f64),
        hold_seconds: f64,
    ) {
        sidecar.pointer.extend([
            sample(arrival_t - 0.6, from.0, from.1),
            sample(
                arrival_t - 0.25,
                (from.0 + target.0) * 0.5,
                (from.1 + target.1) * 0.5,
            ),
            sample(arrival_t, target.0, target.1),
            sample(arrival_t + 0.15, target.0 + 1.0, target.1),
            sample(arrival_t + hold_seconds, target.0, target.1 + 1.0),
        ]);
    }

    #[test]
    fn initial_idle_does_not_create_a_suggestion() {
        let mut data = sidecar();
        for index in 0..20 {
            data.pointer.push(sample(index as f64 * 0.1, 400.0, 300.0));
        }
        assert!(suggest_zooms(&data, W, H, 10.0).is_empty());
    }

    #[test]
    fn passive_idle_without_a_recent_arrival_is_ignored() {
        let mut data = sidecar();
        data.pointer.extend([
            sample(0.0, 100.0, 100.0),
            sample(1.5, 700.0, 400.0),
            sample(2.0, 701.0, 401.0),
            sample(2.5, 700.0, 400.0),
        ]);
        assert!(suggest_zooms(&data, W, H, 10.0).is_empty());
    }

    #[test]
    fn move_then_hold_creates_one_useful_suggestion() {
        let mut data = sidecar();
        add_landing(&mut data, 3.0, (100.0, 100.0), (800.0, 500.0), 0.6);
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].start <= 2.65);
        assert!(suggestions[0].end - suggestions[0].start >= MIN_AUTO_ZOOM_SECONDS);
        assert_eq!(suggestions[0].center, (800.0, 500.0));
        assert_eq!(suggestions[0].scale, AUTO_ZOOM_SCALE);
    }

    #[test]
    fn long_hold_creates_one_bounded_suggestion() {
        let mut data = sidecar();
        add_landing(&mut data, 2.0, (100.0, 100.0), (900.0, 500.0), 6.0);
        let suggestions = suggest_zooms(&data, W, H, 12.0);
        assert_eq!(suggestions.len(), 1);
        assert!(suggestions[0].end - suggestions[0].start <= MAX_SUGGESTED_ZOOM_SECONDS);
    }

    #[test]
    fn area_coordinates_are_scaled_to_encoded_video_pixels() {
        let mut data = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 960,
                h: 540,
            },
        );
        add_landing(&mut data, 3.0, (50.0, 50.0), (400.0, 250.0), 0.6);
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (800.0, 500.0));
    }

    #[test]
    fn mismatched_area_and_video_aspect_ratio_is_rejected() {
        let mut data = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 1600,
                h: 1000,
            },
        );
        add_landing(&mut data, 3.0, (100.0, 100.0), (800.0, 500.0), 0.6);
        assert!(suggest_zooms(&data, W, H, 10.0).is_empty());
    }

    #[test]
    fn valid_click_alone_creates_a_suggestion() {
        let mut data = sidecar();
        data.clicks.push(ClickSample {
            t: 3.0,
            x: 800.0,
            y: 500.0,
            button: 1,
        });
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (800.0, 500.0));
        assert_eq!(suggestions[0].center_time, 3.0);
        assert_eq!(suggestions[0].scale, AUTO_ZOOM_SCALE);
        assert!(suggestions[0].start < 3.0);
        assert!(suggestions[0].end > 3.0);
    }

    #[test]
    fn area_click_coordinates_are_scaled_to_encoded_video_pixels() {
        let mut data = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 960,
                h: 540,
            },
        );
        data.clicks.push(ClickSample {
            t: 3.0,
            x: 400.0,
            y: 250.0,
            button: 1,
        });
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (800.0, 500.0));
    }

    #[test]
    fn repeated_clicks_on_the_same_target_merge() {
        let mut data = sidecar();
        data.clicks.extend([
            ClickSample {
                t: 3.0,
                x: 800.0,
                y: 500.0,
                button: 1,
            },
            ClickSample {
                t: 3.4,
                x: 802.0,
                y: 500.0,
                button: 1,
            },
        ]);
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (801.0, 500.0));
        assert_eq!(suggestions[0].scale, REPEATED_INTERACTION_ZOOM_SCALE);
    }

    #[test]
    fn invalid_clicks_are_ignored() {
        let mut data = sidecar();
        data.clicks.extend([
            ClickSample {
                t: 3.0,
                x: -1.0,
                y: 500.0,
                button: 1,
            },
            ClickSample {
                t: 4.0,
                x: 800.0,
                y: 500.0,
                button: 8,
            },
            ClickSample {
                t: f64::NAN,
                x: 800.0,
                y: 500.0,
                button: 1,
            },
        ]);
        assert!(suggest_zooms(&data, W, H, 10.0).is_empty());
    }

    #[test]
    fn click_is_preferred_over_a_landing_when_density_is_capped() {
        let mut data = sidecar();
        add_landing(&mut data, 1.5, (100.0, 100.0), (400.0, 300.0), 0.6);
        data.clicks.push(ClickSample {
            t: 4.5,
            x: 1500.0,
            y: 700.0,
            button: 1,
        });
        let suggestions = suggest_zooms(&data, W, H, 6.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (1500.0, 700.0));
        assert_eq!(suggestions[0].center_time, 4.5);
    }

    #[test]
    fn click_and_matching_landing_merge_without_inflating_scale() {
        let mut data = sidecar();
        add_landing(&mut data, 3.0, (100.0, 100.0), (800.0, 500.0), 0.6);
        data.clicks.push(ClickSample {
            t: 3.05,
            x: 801.0,
            y: 500.0,
            button: 1,
        });
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].center, (801.0, 500.0));
        assert_eq!(suggestions[0].scale, AUTO_ZOOM_SCALE);
    }

    #[test]
    fn rapid_distant_landings_do_not_merge() {
        let mut data = sidecar();
        add_landing(&mut data, 2.0, (500.0, 500.0), (200.0, 300.0), 0.45);
        add_landing(&mut data, 3.2, (200.0, 300.0), (1500.0, 700.0), 0.45);
        let suggestions = suggest_zooms(&data, W, H, 12.0);
        assert_eq!(suggestions.len(), 2);
        assert!(point_distance(suggestions[0].center, suggestions[1].center) > 1000.0);
    }

    #[test]
    fn repeated_landings_on_the_same_target_merge() {
        let mut data = sidecar();
        data.pointer.extend([
            sample(0.4, 300.0, 300.0),
            sample(0.7, 600.0, 450.0),
            sample(1.0, 900.0, 600.0),
            sample(1.5, 901.0, 600.0),
            sample(1.65, 600.0, 450.0),
            sample(1.9, 900.0, 600.0),
            sample(2.4, 899.0, 601.0),
        ]);
        let suggestions = suggest_zooms(&data, W, H, 10.0);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].scale, REPEATED_INTERACTION_ZOOM_SCALE);
        assert!(point_distance(suggestions[0].center, (900.0, 600.0)) < 2.0);
    }

    #[test]
    fn suggestion_density_is_capped() {
        let mut data = sidecar();
        for index in 0..10 {
            let t = 1.0 + index as f64 * 2.9;
            let target = if index % 2 == 0 {
                (300.0, 300.0)
            } else {
                (1500.0, 700.0)
            };
            let from = if index % 2 == 0 {
                (1500.0, 700.0)
            } else {
                (300.0, 300.0)
            };
            add_landing(&mut data, t, from, target, 0.45);
        }
        let suggestions = suggest_zooms(&data, W, H, 30.0);
        assert_eq!(suggestions.len(), 5);
        assert!(suggestions
            .windows(2)
            .all(|pair| pair[0].start <= pair[1].start));
    }

    #[test]
    fn out_of_frame_and_non_finite_samples_are_ignored() {
        let mut data = sidecar();
        data.pointer.extend([
            sample(1.0, -10.0, 300.0),
            sample(2.0, f64::NAN, 300.0),
            sample(3.0, 300.0, H + 1.0),
        ]);
        assert!(suggest_zooms(&data, W, H, 10.0).is_empty());
    }
}
