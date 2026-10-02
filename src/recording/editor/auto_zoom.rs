//! Automatic zoom placement from recorded clicks.
//!
//! An automatic zoom is a source-time window around a click: it opens shortly
//! before the interaction and keeps running long enough to read what happened.
//! Clicks that belong to the same workflow share one window, so a burst of
//! activity becomes one shot instead of several overlapping ones. The result is
//! anchored in source time, so trimming, cutting, or retiming the composition
//! leaves the zoom on the footage it was placed for.

/// How far before a click its zoom opens.
pub const LEAD_SECONDS: f64 = 0.3;
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
/// Windows shorter than this are not worth a shot.
pub const MIN_WINDOW_SECONDS: f64 = 0.1;

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
    pub center: (f64, f64),
    pub scale: f64,
}

/// The window one click contributes, clamped around the recording's bounds.
fn window_for_click(time: f64, duration: f64) -> (f64, f64) {
    (
        (time - LEAD_SECONDS).max(0.0),
        (time + TAIL_SECONDS).min(duration - END_MARGIN_SECONDS),
    )
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
        .filter(|(start, end)| end - start >= MIN_WINDOW_SECONDS)
        .collect();
    if windows.is_empty() {
        return Vec::new();
    }

    merge_windows(windows, MERGE_GAP_SECONDS)
        .into_iter()
        .filter_map(|(start, end)| {
            let in_window = |time: f64| time + 1e-9 >= start && time - 1e-9 <= end;
            let center = focus_within(
                pointer
                    .iter()
                    .filter(|p| in_window(p.time))
                    .map(|p| (p.time, p.position)),
            )
            .or_else(|| {
                focus_within(
                    clicks
                        .iter()
                        .filter(|c| in_window(c.time))
                        .map(|c| (c.time, c.position)),
                )
            })?;
            Some(AutoZoom {
                start,
                end,
                center,
                scale: DEFAULT_SCALE,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(time: f64) -> Click {
        Click {
            time,
            position: (640.0, 360.0),
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
}
