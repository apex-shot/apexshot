//! Automatic zoom placement from recorded clicks.
//!
//! An automatic zoom is a source-time window around a click: it opens shortly
//! before the interaction and keeps running long enough to read what happened.
//! Clicks that belong to the same workflow share one window, so a burst of
//! activity becomes one shot instead of several overlapping ones. The result is
//! anchored in source time, so trimming, cutting, or retiming the composition
//! leaves the zoom on the footage it was placed for.

use crate::recording::editor::sidecar::{PointerSidecar, PressSample};

/// How far before a click its zoom opens.
pub const LEAD_SECONDS: f64 = 0.3;
/// The earliest a zoom window may open. The studied detector floors the start
/// at 1 ms rather than 0, so a click on the first frame still leaves a sliver
/// of full-frame footage ahead of the zoom.
pub const START_FLOOR_SECONDS: f64 = 0.001;
/// How long a zoom keeps running after its click.
pub const TAIL_SECONDS: f64 = 2.5;
/// Windows no further apart than this merge into one zoom.
pub const MERGE_GAP_SECONDS: f64 = 2.5;
/// Clicks this close to the end of the recording are ignored.
pub const END_IGNORE_SECONDS: f64 = 1.0;
/// A zoom never reaches the recording's final fraction of a second, where the
/// last frames are the least reliable thing to hold on.
pub const END_MARGIN_SECONDS: f64 = 0.8;
/// Zoom level an automatic zoom opens at.
pub const DEFAULT_SCALE: f64 = 2.0;
/// The studied collection's minimum zoom duration. A shorter window grows to
/// this instead of being dropped.
pub const MIN_WINDOW_SECONDS: f64 = 0.1;

/// How far the cursor must travel before a rest counts as a settle, as a
/// fraction of the frame diagonal.
///
/// Without it, a recording that opens with the pointer parked over a control
/// would zoom on a cursor that never went anywhere.
pub const SETTLE_TRAVEL_FRACTION: f64 = 0.08;
/// How far the cursor may drift while resting, as a fraction of the frame
/// diagonal.
pub const SETTLE_REST_RADIUS_FRACTION: f64 = 0.015;
/// The smallest rest radius, so a tiny captured region still has a usable
/// one.
pub const SETTLE_REST_RADIUS_MIN_PX: f64 = 6.0;
/// How long the cursor must stay inside the rest radius for a pause to count.
///
/// Samples only arrive when the cursor moves, so a rest shows up as a gap in
/// the samples; a shorter gap is usually the pointer crossing a control on its
/// way somewhere else.
pub const SETTLE_REST_SECONDS: f64 = 0.45;
/// Settles this close to the recording's start are ignored.
pub const SETTLE_START_IGNORE_SECONDS: f64 = 0.3;
/// Two settles closer than this keep the first: one pause should not become
/// two zooms.
pub const SETTLE_MIN_SPACING_SECONDS: f64 = 0.8;

/// A recorded click in source time and encoded-video pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub time: f64,
    pub position: (f64, f64),
}

/// A recorded pointer position in source time and encoded-video pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPoint {
    pub time: f64,
    pub position: (f64, f64),
}

/// An automatic zoom: a source-time window and the focus it opens on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoZoom {
    pub start: f64,
    pub end: f64,
    /// Source time of the interaction that seeded the window. Placement uses
    /// this to decide which kept footage the zoom belongs to, so a click on a
    /// cut still lands on the piece it happened over.
    pub center_time: f64,
    pub center: (f64, f64),
    pub scale: f64,
}

/// The window one click contributes, clamped around the recording's bounds.
fn window_for_click(time: f64, duration: f64) -> (f64, f64) {
    (
        (time - LEAD_SECONDS).max(START_FLOOR_SECONDS),
        (time + TAIL_SECONDS).min(duration - END_MARGIN_SECONDS),
    )
}

/// Grow a window that is shorter than the minimum instead of dropping it.
///
/// The studied collection expands a short item to its 100 ms minimum on
/// insertion. Grow the end first; at the end of the recording, shift the start
/// back so the window still keeps its minimum.
fn minimum_window(start: f64, end: f64, duration: f64) -> (f64, f64) {
    if end - start >= MIN_WINDOW_SECONDS {
        return (start, end);
    }
    let end = (start + MIN_WINDOW_SECONDS).min(duration);
    let start = start.min((end - MIN_WINDOW_SECONDS).max(0.0));
    (start, end)
}

/// Merge sorted windows that are no further apart than `gap`.
///
/// A gap of exactly `gap` still merges, so clicks two and a half seconds apart
/// share a shot rather than leaving a one-frame gap between two zooms.
fn merge_windows(mut windows: Vec<(f64, f64)>, gap: f64) -> Vec<(f64, f64)> {
    windows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(windows.len());
    for (start, end) in windows {
        match merged.last_mut() {
            Some(last) if last.1 + gap >= start => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// The studied fallback when a range has no pointer sample inside it: prefer
/// the first sample at or after the start within 500 ms, otherwise the nearest
/// recorded sample.
fn nearest_pointer(pointer: &[PointerPoint], start: f64) -> Option<(f64, f64)> {
    const SMOOTHING_LOOKAHEAD_SECONDS: f64 = 0.5;
    pointer
        .iter()
        .find(|point| {
            point.time.is_finite()
                && point.time >= start
                && point.time - start <= SMOOTHING_LOOKAHEAD_SECONDS
        })
        .or_else(|| {
            pointer
                .iter()
                .filter(|point| point.time.is_finite())
                .min_by(|left, right| {
                    (left.time - start)
                        .abs()
                        .total_cmp(&(right.time - start).abs())
                })
        })
        .map(|point| point.position)
}

/// The centre of the box every point inside a window covers.
///
/// The focus is the middle of the range the pointer covered, not the click
/// alone: a workflow that drifts between controls is framed by the whole
/// movement, which is what the camera will follow.
fn focus_within(points: impl Iterator<Item = (f64, (f64, f64))>) -> Option<(f64, f64)> {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut seen = false;
    for (_time, (x, y)) in points {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
        seen = true;
    }
    seen.then_some(((min.0 + max.0) * 0.5, (min.1 + max.1) * 0.5))
}

/// Build the automatic zooms for a recording.
///
/// Clicks seed the windows; pointer movement decides where a merged window
/// looks. A window with no usable focus falls back to the clicks that created
/// it, so a zoom is never dropped just because movement data was sparse.
pub fn automatic_zooms(clicks: &[Click], pointer: &[PointerPoint], duration: f64) -> Vec<AutoZoom> {
    if !duration.is_finite() || duration <= END_IGNORE_SECONDS + MIN_WINDOW_SECONDS {
        return Vec::new();
    }
    let cutoff = duration - END_IGNORE_SECONDS;
    let windows: Vec<(f64, f64)> = clicks
        .iter()
        .filter(|click| click.time.is_finite() && click.time >= 0.0 && click.time < cutoff)
        .map(|click| window_for_click(click.time, duration))
        .collect();
    if windows.is_empty() {
        return Vec::new();
    }

    merge_windows(windows, MERGE_GAP_SECONDS)
        .into_iter()
        .map(|(start, end)| minimum_window(start, end, duration))
        .filter_map(|(start, end)| {
            // The start floor can open a window up to 1 ms after a click that
            // sits on the first frame, so the focus lookup tolerates the same
            // millisecond to keep the seeding click inside its own window.
            let in_window = |time: f64| {
                time + START_FLOOR_SECONDS >= start && time - START_FLOOR_SECONDS <= end
            };
            let center = focus_within(
                pointer
                    .iter()
                    .filter(|p| in_window(p.time))
                    .map(|p| (p.time, p.position)),
            )
            .or_else(|| nearest_pointer(pointer, start))
            .or_else(|| {
                focus_within(
                    clicks
                        .iter()
                        .filter(|c| in_window(c.time))
                        .map(|c| (c.time, c.position)),
                )
            })?;
            // The interactions in the merged group own the window; its centre
            // in time is the footage placement attaches the zoom to, so a
            // window that spans a cut still lands on the piece the activity is
            // mostly over.
            let mut first = f64::INFINITY;
            let mut last = f64::NEG_INFINITY;
            for click in clicks.iter().filter(|click| in_window(click.time)) {
                first = first.min(click.time);
                last = last.max(click.time);
            }
            if !first.is_finite() {
                return None;
            }
            let center_time = (first + last) * 0.5;
            Some(AutoZoom {
                start,
                end,
                center_time,
                center,
                scale: DEFAULT_SCALE,
            })
        })
        .collect()
}

/// Recorded clicks mapped into encoded-video pixels.
///
/// The studied detector seeds from both button-down and button-up events. The
/// sidecar stores down events in `clicks` and the release time in `presses`, so
/// add a seed at each release. A release has no stored position of its own; use
/// the nearest pointer sample, falling back to the down click that opened the
/// press.
pub fn clicks_from_sidecar(sidecar: &PointerSidecar, width: f64, height: f64) -> Vec<Click> {
    let mut clicks: Vec<Click> = sidecar
        .clicks
        .iter()
        .map(|click| Click {
            time: click.t,
            position: sidecar.map_to_video(click.x, click.y, width, height),
        })
        .collect();

    for press in &sidecar.presses {
        if !press.up.is_finite() || press.up < press.down {
            continue;
        }
        if let Some(position) = release_position(sidecar, press, width, height) {
            clicks.push(Click {
                time: press.up,
                position,
            });
        }
    }

    clicks
}

/// Where a press came up, in encoded-video pixels.
///
/// The recorder stores the press interval but not a separate release point, so
/// read the pointer sample nearest the release. If there is no pointer sample
/// close enough, reuse the down click that opened the press.
pub(crate) fn release_position(
    sidecar: &PointerSidecar,
    press: &PressSample,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    const NEAREST_POINTER_SECONDS: f64 = 0.5;
    const PRESS_MATCH_SECONDS: f64 = 0.001;
    let nearest = sidecar
        .pointer
        .iter()
        .min_by(|left, right| {
            (left.t - press.up)
                .abs()
                .total_cmp(&(right.t - press.up).abs())
        })
        .filter(|point| (point.t - press.up).abs() <= NEAREST_POINTER_SECONDS)
        .map(|point| sidecar.map_to_video(point.x, point.y, width, height));
    if nearest.is_some() {
        return nearest;
    }

    sidecar
        .clicks
        .iter()
        .find(|click| {
            click.button == press.button && (click.t - press.down).abs() <= PRESS_MATCH_SECONDS
        })
        .map(|click| sidecar.map_to_video(click.x, click.y, width, height))
}

/// Recorded pointer samples mapped into encoded-video pixels.
pub fn pointer_from_sidecar(
    sidecar: &PointerSidecar,
    width: f64,
    height: f64,
) -> Vec<PointerPoint> {
    sidecar
        .pointer
        .iter()
        .map(|point| PointerPoint {
            time: point.t,
            position: sidecar.map_to_video(point.x, point.y, width, height),
        })
        .collect()
}

/// Distance between two recorded positions.
fn distance(left: (f64, f64), right: (f64, f64)) -> f64 {
    let dx = left.0 - right.0;
    let dy = left.1 - right.1;
    (dx * dx + dy * dy).sqrt()
}

/// The frame diagonal the settle thresholds are measured against.
///
/// The video size is the real frame. If it is missing or unusable, fall back
/// to the largest coordinates the pointer reached so the thresholds still
/// scale with the footage.
fn settle_diagonal(width: f64, height: f64, pointer: &[PointerPoint]) -> f64 {
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        return (width * width + height * height).sqrt();
    }
    let mut max = (0.0_f64, 0.0_f64);
    for point in pointer {
        max.0 = max.0.max(point.position.0);
        max.1 = max.1.max(point.position.1);
    }
    (max.0 * max.0 + max.1 * max.1).sqrt()
}

/// Derive click-like settle points from pointer movement.
///
/// Recordings from compositors that cannot report clicks still carry pointer
/// positions. People act after moving somewhere and stopping, so the moments
/// the cursor comes to rest stand in for the missing clicks. Each settle is the
/// time the rest began and the mean position of the resting samples, ready to
/// feed [`automatic_zooms`] exactly the way recorded clicks are.
///
/// Samples are expected in source-time order, as the rest of the module does.
/// A rest only counts after real travel since the previous settle (or the start
/// of the recording), and the final window [`automatic_zooms`] ignores is
/// ignored here too.
pub fn settle_points(
    pointer: &[PointerPoint],
    width: f64,
    height: f64,
    duration: f64,
) -> Vec<Click> {
    if !duration.is_finite() {
        return Vec::new();
    }
    let samples: Vec<PointerPoint> = pointer
        .iter()
        .copied()
        .filter(|point| {
            point.time.is_finite() && point.position.0.is_finite() && point.position.1.is_finite()
        })
        .collect();
    if samples.len() < 2 {
        return Vec::new();
    }

    let diagonal = settle_diagonal(width, height, &samples);
    let radius = (diagonal * SETTLE_REST_RADIUS_FRACTION).max(SETTLE_REST_RADIUS_MIN_PX);
    let travel_required = diagonal * SETTLE_TRAVEL_FRACTION;
    let cutoff = duration - END_IGNORE_SECONDS;

    // Path length up to each sample, so travel since the last settle is a
    // subtraction instead of a second walk.
    let mut travelled = Vec::with_capacity(samples.len());
    travelled.push(0.0);
    for pair in samples.windows(2) {
        let step = distance(pair[0].position, pair[1].position);
        let total = travelled.last().copied().unwrap_or(0.0) + step;
        travelled.push(total);
    }

    let mut settles = Vec::new();
    let mut settled_at = 0;
    let mut last_settle_time = f64::NEG_INFINITY;
    let mut index = 0;
    while index < samples.len() {
        // A rest is a run of samples that all stay inside the radius around
        // this one, lasting until the next sample that leaves it (or the end
        // of the recording). Sources differ in density: some only report on
        // movement, so a rest is a silent gap, while others repeat the
        // position every frame, so a rest is many jittery samples. Both are
        // measured the same way.
        let anchor = samples[index].position;
        let mut last = index;
        while last + 1 < samples.len() && distance(samples[last + 1].position, anchor) <= radius {
            last += 1;
        }
        let rest_end = samples.get(last + 1).map_or(duration, |next| next.time);
        if rest_end - samples[index].time < SETTLE_REST_SECONDS {
            index += 1;
            continue;
        }

        let start = samples[index].time;
        let travelled_since_settle = travelled[index] - travelled[settled_at];
        let ignored = start < SETTLE_START_IGNORE_SECONDS
            || start >= cutoff
            || travelled_since_settle < travel_required
            || start - last_settle_time < SETTLE_MIN_SPACING_SECONDS;
        if !ignored {
            let resting = &samples[index..=last];
            let mean = (
                resting.iter().map(|point| point.position.0).sum::<f64>() / resting.len() as f64,
                resting.iter().map(|point| point.position.1).sum::<f64>() / resting.len() as f64,
            );
            settles.push(Click {
                time: start,
                position: mean,
            });
            settled_at = index;
            last_settle_time = start;
        }

        // Every gap inside this rest belongs to the same pause.
        index = last + 1;
    }
    settles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PressSample,
    };

    fn sidecar() -> PointerSidecar {
        PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 640,
                h: 480,
            },
        )
    }

    fn click(time: f64) -> Click {
        Click {
            time,
            position: (640.0, 360.0),
        }
    }

    fn point(time: f64, x: f64, y: f64) -> PointerPoint {
        PointerPoint {
            time,
            position: (x, y),
        }
    }

    #[test]
    fn a_click_opens_shortly_before_and_runs_long_after() {
        let zooms = automatic_zooms(&[click(3.0)], &[], 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].start - 2.7).abs() < 1e-9);
        assert!((zooms[0].end - 5.5).abs() < 1e-9);
        assert!((zooms[0].scale - DEFAULT_SCALE).abs() < 1e-9);
    }

    #[test]
    fn a_short_window_grows_to_the_minimum() {
        let (start, end) = minimum_window(3.0, 3.04, 20.0);
        assert!((start - 3.0).abs() < 1e-9);
        assert!((end - MIN_WINDOW_SECONDS - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_window_at_the_end_keeps_the_minimum_by_moving_back() {
        let (start, end) = minimum_window(19.95, 20.0, 20.0);
        assert!((start - 19.9).abs() < 1e-9);
        assert!((end - 20.0).abs() < 1e-9);
    }

    #[test]
    fn nearby_clicks_become_one_zoom() {
        let zooms = automatic_zooms(&[click(3.0), click(4.8)], &[], 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].start - 2.7).abs() < 1e-9);
        assert!((zooms[0].end - 7.3).abs() < 1e-9);
    }

    #[test]
    fn the_merge_gap_includes_its_boundary() {
        // Exactly the merge distance apart still shares a shot.
        let touching = automatic_zooms(&[click(3.0), click(8.3)], &[], 20.0);
        assert_eq!(touching.len(), 1);
        // A hair further apart is two shots.
        let apart = automatic_zooms(&[click(3.0), click(8.4)], &[], 20.0);
        assert_eq!(apart.len(), 2);
    }

    #[test]
    fn far_apart_clicks_stay_separate() {
        let zooms = automatic_zooms(&[click(3.0), click(9.5)], &[], 20.0);
        assert_eq!(zooms.len(), 2);
        assert!((zooms[1].start - 9.2).abs() < 1e-9);
    }

    #[test]
    fn clicks_in_the_last_second_are_ignored() {
        let zooms = automatic_zooms(&[click(19.5)], &[], 20.0);
        assert!(zooms.is_empty());
    }

    #[test]
    fn a_zoom_never_reaches_the_last_fraction_of_a_second() {
        let zooms = automatic_zooms(&[click(18.5)], &[], 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].end - 19.2).abs() < 1e-9);
    }

    #[test]
    fn a_click_on_the_first_frame_opens_at_the_floor() {
        let zooms = automatic_zooms(&[click(0.0)], &[], 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].start - START_FLOOR_SECONDS).abs() < 1e-9);
    }

    #[test]
    fn no_clicks_makes_no_zooms() {
        assert!(automatic_zooms(&[], &[], 20.0).is_empty());
    }

    #[test]
    fn a_too_short_recording_makes_no_zooms() {
        assert!(automatic_zooms(&[click(0.5)], &[], 1.0).is_empty());
    }

    #[test]
    fn focus_is_the_middle_of_the_movement_in_the_window() {
        let pointer = [
            PointerPoint {
                time: 3.0,
                position: (100.0, 100.0),
            },
            PointerPoint {
                time: 4.0,
                position: (300.0, 200.0),
            },
        ];
        let zooms = automatic_zooms(&[click(3.0)], &pointer, 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].center.0 - 200.0).abs() < 1e-9);
        assert!((zooms[0].center.1 - 150.0).abs() < 1e-9);
    }

    #[test]
    fn focus_ignores_movement_outside_the_window() {
        let pointer = [
            PointerPoint {
                time: 1.0,
                position: (10.0, 10.0),
            },
            PointerPoint {
                time: 4.0,
                position: (300.0, 200.0),
            },
        ];
        let zooms = automatic_zooms(&[click(3.0)], &pointer, 20.0);
        assert!((zooms[0].center.0 - 300.0).abs() < 1e-9);
        assert!((zooms[0].center.1 - 200.0).abs() < 1e-9);
    }

    #[test]
    fn focus_falls_back_to_the_click_without_movement() {
        let zooms = automatic_zooms(&[click(3.0)], &[], 20.0);
        assert!((zooms[0].center.0 - 640.0).abs() < 1e-9);
        assert!((zooms[0].center.1 - 360.0).abs() < 1e-9);
    }

    #[test]
    fn focus_falls_back_to_the_nearest_pointer_sample() {
        let pointer = [
            PointerPoint {
                time: 2.6,
                position: (10.0, 10.0),
            },
            PointerPoint {
                time: 5.6,
                position: (400.0, 400.0),
            },
        ];
        // The click window is 2.7-5.5, so neither sample is inside it. The
        // studied fallback picks the sample nearest the window start.
        let zooms = automatic_zooms(&[click(3.0)], &pointer, 20.0);
        assert!((zooms[0].center.0 - 10.0).abs() < 1e-9);
        assert!((zooms[0].center.1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_press_release_adds_a_second_click_seed() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(ClickSample {
            t: 3.0,
            x: 320.0,
            y: 240.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.presses.push(PressSample {
            down: 3.0,
            up: 3.4,
            button: 1,
            dragged: false,
        });
        sidecar.pointer.push(PointerSample {
            t: 3.0,
            x: 320.0,
            y: 240.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 3.4,
            x: 340.0,
            y: 260.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });

        let clicks = clicks_from_sidecar(&sidecar, 640.0, 480.0);
        assert_eq!(clicks.len(), 2);
        assert!((clicks[1].time - 3.4).abs() < 1e-9);
        assert_eq!(clicks[1].position, (340.0, 260.0));

        let pointer = pointer_from_sidecar(&sidecar, 640.0, 480.0);
        let zooms = automatic_zooms(&clicks, &pointer, 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].start - 2.7).abs() < 1e-9);
        assert!((zooms[0].end - 5.9).abs() < 1e-9);
    }

    #[test]
    fn a_release_falls_back_to_the_down_position() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(ClickSample {
            t: 3.0,
            x: 120.0,
            y: 80.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.presses.push(PressSample {
            down: 3.0,
            up: 3.4,
            button: 1,
            dragged: false,
        });

        let clicks = clicks_from_sidecar(&sidecar, 640.0, 480.0);
        assert_eq!(clicks.len(), 2);
        assert_eq!(clicks[1].position, (120.0, 80.0));
    }

    #[test]
    fn a_release_without_a_down_click_still_seeds_a_zoom() {
        let mut sidecar = sidecar();
        sidecar.presses.push(PressSample {
            down: 0.0,
            up: 0.5,
            button: 1,
            dragged: false,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.5,
            x: 100.0,
            y: 100.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });

        let clicks = clicks_from_sidecar(&sidecar, 640.0, 480.0);
        assert_eq!(clicks.len(), 1);
        assert!((clicks[0].time - 0.5).abs() < 1e-9);
        let zooms = automatic_zooms(&clicks, &pointer_from_sidecar(&sidecar, 640.0, 480.0), 20.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].end - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_release_in_the_last_second_is_ignored() {
        let mut sidecar = sidecar();
        sidecar.clicks.push(ClickSample {
            t: 3.0,
            x: 320.0,
            y: 240.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.presses.push(PressSample {
            down: 3.0,
            up: 9.5,
            button: 1,
            dragged: false,
        });

        let clicks = clicks_from_sidecar(&sidecar, 640.0, 480.0);
        let zooms = automatic_zooms(&clicks, &[], 10.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].end - 5.5).abs() < 1e-9);
    }

    #[test]
    fn no_pointer_samples_make_no_settles() {
        assert!(settle_points(&[], 640.0, 480.0, 10.0).is_empty());
    }

    #[test]
    fn a_move_then_rest_becomes_one_settle() {
        let pointer = [
            point(0.5, 0.0, 0.0),
            point(1.0, 400.0, 300.0),
            point(1.5, 400.0, 300.0),
            point(2.3, 100.0, 100.0),
        ];
        let settles = settle_points(&pointer, 640.0, 480.0, 2.5);
        assert_eq!(settles.len(), 1);
        assert!((settles[0].time - 1.0).abs() < 1e-9);
        assert_eq!(settles[0].position, (400.0, 300.0));
    }

    #[test]
    fn jitter_while_resting_is_still_one_settle() {
        let pointer = [
            point(0.5, 0.0, 0.0),
            point(1.0, 400.0, 300.0),
            point(1.2, 402.0, 298.0),
            point(1.4, 399.0, 303.0),
            point(2.4, 401.0, 301.0),
            point(2.8, 100.0, 100.0),
        ];
        let settles = settle_points(&pointer, 640.0, 480.0, 2.9);
        assert_eq!(settles.len(), 1);
        // The rest starts when the cursor arrives, and its position is the
        // mean of the samples that stayed inside the rest radius.
        assert!((settles[0].time - 1.0).abs() < 1e-9);
        assert!((settles[0].position.0 - 400.5).abs() < 1e-9);
        assert!((settles[0].position.1 - 300.5).abs() < 1e-9);
    }

    #[test]
    fn a_rest_reported_every_frame_is_still_one_settle() {
        // Sources that repeat the cursor every frame never leave a silent
        // gap: a jittery hold at 30 samples a second must still read as rest.
        let mut pointer = vec![point(0.5, 0.0, 0.0), point(0.9, 300.0, 200.0)];
        for frame in 0..30 {
            let jitter = if frame % 2 == 0 { 1.0 } else { -1.0 };
            pointer.push(point(1.0 + frame as f64 / 30.0, 400.0 + jitter, 300.0));
        }
        // The final move lands inside the ignored last second, so the hold
        // is the only settle.
        pointer.push(point(2.2, 100.0, 100.0));
        let settles = settle_points(&pointer, 640.0, 480.0, 2.5);
        assert_eq!(settles.len(), 1);
        assert!((settles[0].time - 1.0).abs() < 1e-9);
        assert!((settles[0].position.0 - 400.0).abs() < 1e-9);
        assert!((settles[0].position.1 - 300.0).abs() < 1e-9);
    }

    #[test]
    fn slow_continuous_movement_makes_no_settles() {
        // Small steps every tenth of a second: steady motion with no pause
        // long enough to read as a rest, even though the cursor travels far.
        let pointer: Vec<PointerPoint> = (0..=20)
            .map(|step| point(step as f64 * 0.1, step as f64 * 6.0, 0.0))
            .collect();
        assert!(settle_points(&pointer, 640.0, 480.0, 2.1).is_empty());
    }

    #[test]
    fn a_rest_without_travel_makes_no_settle() {
        let pointer = [
            point(0.5, 200.0, 200.0),
            point(1.5, 200.0, 200.0),
            point(2.0, 500.0, 400.0),
        ];
        assert!(settle_points(&pointer, 640.0, 480.0, 2.1).is_empty());
    }

    #[test]
    fn two_rests_half_a_second_apart_keep_the_first() {
        let pointer = [
            point(0.5, 0.0, 0.0),
            point(1.0, 400.0, 300.0),
            point(1.5, 50.0, 50.0),
            point(2.0, 50.0, 50.0),
            point(2.4, 500.0, 400.0),
        ];
        let settles = settle_points(&pointer, 640.0, 480.0, 2.5);
        assert_eq!(settles.len(), 1);
        assert!((settles[0].time - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_rest_in_the_first_third_of_a_second_is_ignored() {
        let pointer = [
            point(0.05, 0.0, 0.0),
            point(0.2, 400.0, 300.0),
            point(0.9, 400.0, 300.0),
            point(1.8, 100.0, 100.0),
        ];
        assert!(settle_points(&pointer, 640.0, 480.0, 2.0).is_empty());
    }

    #[test]
    fn a_rest_in_the_final_ignored_window_makes_no_settle() {
        // Steady movement, then the cursor stops inside the last second that
        // `automatic_zooms` never opens a zoom in.
        let pointer: Vec<PointerPoint> = (0..=38)
            .map(|step| point(0.5 + step as f64 * 0.1, step as f64 * 2.0, 0.0))
            .collect();
        assert!(settle_points(&pointer, 640.0, 480.0, 5.0).is_empty());
    }

    #[test]
    fn a_trailing_rest_is_inferred_from_the_gap_to_the_duration() {
        let pointer = [point(0.5, 0.0, 0.0), point(0.6, 400.0, 300.0)];
        let settles = settle_points(&pointer, 640.0, 480.0, 2.0);
        assert_eq!(settles.len(), 1);
        assert!((settles[0].time - 0.6).abs() < 1e-9);
        assert_eq!(settles[0].position, (400.0, 300.0));
    }

    #[test]
    fn settle_points_feed_automatic_zooms() {
        let pointer = [
            point(0.5, 0.0, 0.0),
            point(1.0, 400.0, 300.0),
            point(1.6, 400.0, 300.0),
            point(2.9, 100.0, 100.0),
        ];
        let settles = settle_points(&pointer, 640.0, 480.0, 3.0);
        assert_eq!(settles.len(), 1);
        let zooms = automatic_zooms(&settles, &pointer, 3.0);
        assert_eq!(zooms.len(), 1);
        assert!((zooms[0].start - 0.7).abs() < 1e-9);
    }
}
