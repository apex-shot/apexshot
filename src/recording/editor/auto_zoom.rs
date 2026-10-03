//! Automatic zoom placement from recorded clicks.
//!
//! An automatic zoom is a source-time window around a click: it opens shortly
//! before the interaction and keeps running long enough to read what happened.
//! Clicks that belong to the same workflow share one window, so a burst of
//! activity becomes one shot instead of several overlapping ones. The result is
//! anchored in source time, so trimming, cutting, or retiming the composition
//! leaves the zoom on the footage it was placed for.

use crate::recording::editor::model::MIN_ZOOM_SCALE;
use crate::recording::editor::sidecar::PointerSidecar;

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
/// Zoom level an automatic zoom opens at. Fitting only ever widens from here,
/// so the model default stays the strongest automatic zoom.
pub const DEFAULT_SCALE: f64 = 2.0;
/// Windows shorter than this are not worth a shot.
pub const MIN_WINDOW_SECONDS: f64 = 0.1;
/// Context kept around the targets a window covers, as a fraction of the
/// frame on each side. A single click still leaves a margin, and a workflow
/// that spreads across controls needs a wider shot to hold every target.
pub const TARGET_CONTEXT_FRACTION: f64 = 0.10;

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
    /// Bounding box of the targets the window has to hold, in encoded-video
    /// pixels: `(min_x, min_y, max_x, max_y)`. Placement fits the zoom's
    /// strength to this rather than always opening at the model default, so a
    /// workflow spread across several controls is not framed so tightly that
    /// some of them fall outside the shot.
    pub region: (f64, f64, f64, f64),
}

impl AutoZoom {
    /// The scale that holds this window's covered targets inside an area
    /// `bounds_w` by `bounds_h` (the effective crop), never stronger than the
    /// level the window was generated at.
    ///
    /// Returns `None` when every target cannot be held at the minimum useful
    /// zoom: the moment stays full-frame rather than clamping tighter and
    /// cutting a target off. The studied guidance is explicit that a distant
    /// pair should widen, pan, or split — never force a tighter shot.
    pub fn fitted_scale(&self, bounds_w: f64, bounds_h: f64) -> Option<f64> {
        // Fitting only ever widens, so the level the generator chose is the
        // strongest this shot may open at.
        let preferred = if self.scale.is_finite() && self.scale >= 1.0 {
            self.scale
        } else {
            DEFAULT_SCALE
        };
        if !bounds_w.is_finite() || !bounds_h.is_finite() || bounds_w <= 0.0 || bounds_h <= 0.0 {
            return Some(preferred);
        }
        let (min_x, min_y, max_x, max_y) = self.region;
        let pad_x = bounds_w * TARGET_CONTEXT_FRACTION;
        let pad_y = bounds_h * TARGET_CONTEXT_FRACTION;
        // The shot opens centred on the focus, so what has to fit is the
        // furthest target on each side of it, not the targets' total width.
        let half_w = (self.center.0 - min_x).max(max_x - self.center.0).max(0.0) + pad_x;
        let half_h = (self.center.1 - min_y).max(max_y - self.center.1).max(0.0) + pad_y;
        let region_w = 2.0 * half_w;
        let region_h = 2.0 * half_h;
        if !region_w.is_finite() || !region_h.is_finite() || region_w <= 0.0 || region_h <= 0.0 {
            return Some(preferred);
        }
        let fit = (bounds_w / region_w).min(bounds_h / region_h);
        if fit < MIN_ZOOM_SCALE {
            return None;
        }
        Some(fit.min(preferred))
    }
}

/// The window one click contributes, clamped around the recording's bounds.
fn window_for_click(time: f64, duration: f64) -> (f64, f64) {
    (
        (time - LEAD_SECONDS).max(START_FLOOR_SECONDS),
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
            // mostly over. Their bounding box is the region the shot has to
            // hold around the focus it opens on.
            let mut first = f64::INFINITY;
            let mut last = f64::NEG_INFINITY;
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for click in clicks.iter().filter(|click| in_window(click.time)) {
                first = first.min(click.time);
                last = last.max(click.time);
                if click.position.0.is_finite() && click.position.1.is_finite() {
                    min_x = min_x.min(click.position.0);
                    max_x = max_x.max(click.position.0);
                    min_y = min_y.min(click.position.1);
                    max_y = max_y.max(click.position.1);
                }
            }
            if !first.is_finite() {
                return None;
            }
            let region = if min_x.is_finite() {
                (min_x, min_y, max_x, max_y)
            } else {
                (center.0, center.1, center.0, center.1)
            };
            let center_time = (first + last) * 0.5;
            Some(AutoZoom {
                start,
                end,
                center_time,
                center,
                scale: DEFAULT_SCALE,
                region,
            })
        })
        .collect()
}

/// Recorded clicks mapped into encoded-video pixels.
pub fn clicks_from_sidecar(sidecar: &PointerSidecar, width: f64, height: f64) -> Vec<Click> {
    sidecar
        .clicks
        .iter()
        .map(|click| Click {
            time: click.t,
            position: sidecar.map_to_video(click.x, click.y, width, height),
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn click(time: f64) -> Click {
        Click {
            time,
            position: (640.0, 360.0),
        }
    }

    fn click_at(time: f64, x: f64, y: f64) -> Click {
        Click {
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
    fn the_covered_region_is_the_bounding_box_of_the_clicks() {
        let zooms = automatic_zooms(
            &[click_at(3.0, 400.0, 300.0), click_at(4.0, 1500.0, 700.0)],
            &[],
            20.0,
        );
        assert_eq!(zooms.len(), 1);
        assert_eq!(zooms[0].region, (400.0, 300.0, 1500.0, 700.0));
    }

    #[test]
    fn a_tight_target_keeps_the_model_default() {
        let zooms = automatic_zooms(&[click(3.0)], &[], 20.0);
        assert!((zooms[0].fitted_scale(1920.0, 1080.0).unwrap() - DEFAULT_SCALE).abs() < 1e-9);
    }

    #[test]
    fn a_spread_pair_widens_to_hold_both_targets() {
        let zooms = automatic_zooms(
            &[click_at(3.0, 400.0, 300.0), click_at(4.0, 1500.0, 700.0)],
            &[],
            20.0,
        );
        let scale = zooms[0].fitted_scale(1920.0, 1080.0).unwrap();
        assert!(scale < DEFAULT_SCALE, "expected a wider shot, got {scale}");
        assert!(
            scale >= MIN_ZOOM_SCALE,
            "expected a usable shot, got {scale}"
        );
    }

    #[test]
    fn targets_at_opposite_edges_force_a_full_frame() {
        let zooms = automatic_zooms(
            &[click_at(3.0, 40.0, 40.0), click_at(4.0, 1880.0, 1040.0)],
            &[],
            20.0,
        );
        // No legal zoom can hold both edges, so the moment stays full-frame.
        assert_eq!(zooms[0].fitted_scale(1920.0, 1080.0), None);
    }
}
