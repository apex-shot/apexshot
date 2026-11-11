//! Where the split's cover-cropped screen rests in its pane.
//!
//! The studied split shows the screen's own outer edge at rest and slides the
//! cropped picture to its other edge when the cursor, looking a short way
//! ahead, would come within a clearance of the camera (`DgH:7525-7538`,
//! `DgH:7942-7950`). The slide is eased by the screen's movement spring
//! (`B39:18148`). This module decides the target, 0 for the resting edge and
//! 1 for the other, and springs it over the composition.

#![allow(dead_code)] // Wired into the export and the preview in a following change.

use super::webcam::Rect;
use super::zoom::{self, CameraSpring};

/// The studied clearance, as a share of the screen's cover width along the
/// split (`DgH:7947`).
pub(super) const CLEARANCE_RATIO: f64 = 0.125;
/// How far ahead the cursor's bounds look, in seconds (`B39:6870`, `B39:3365`).
pub(super) const LOOKAHEAD_SECONDS: f64 = 0.35;

/// One instant's split, in stage fractions (x of the stage's width, y of its
/// height), with the screen at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SplitGeometry {
    /// The screen's pane.
    pub pane: Rect,
    /// The camera's pane.
    pub camera: Rect,
    /// The rect the whole screen is drawn into to cover `pane`, centred; only
    /// its width and height are read here.
    pub cover: Rect,
    /// Side by side (true) or stacked (false).
    pub horizontal: bool,
    /// Whether the screen comes first: left of the camera side by side, above
    /// it stacked.
    pub screen_first: bool,
}

/// Whether the studied split rests the screen against its other edge so the
/// cursor stays clear of the camera. `bounds` is the cursor's bounding box in
/// fractions of the recording, `(x0, y0, x1, y1)`.
pub(super) fn flips(geometry: SplitGeometry, bounds: (f64, f64, f64, f64)) -> bool {
    let (x0, y0, x1, y1) = bounds;
    let SplitGeometry {
        pane,
        camera,
        cover,
        horizontal,
        screen_first,
    } = geometry;
    let (pane_start, pane_size, cover_size, camera_start, camera_size, near, far) = if horizontal {
        (
            pane.x,
            pane.width,
            cover.width,
            camera.x,
            camera.width,
            x0,
            x1,
        )
    } else {
        (
            pane.y,
            pane.height,
            cover.height,
            camera.y,
            camera.height,
            y0,
            y1,
        )
    };
    if !(cover_size.is_finite() && cover_size > 0.0) {
        return false;
    }
    // The cover at rest is aligned to the screen's outer edge of the pane.
    let rest_start = if screen_first {
        pane_start
    } else {
        pane_start + pane_size - cover_size
    };
    let distance = if screen_first {
        // The camera is after the screen: the cursor's far edge against the
        // camera's near edge.
        camera_start - (rest_start + far * cover_size)
    } else {
        (rest_start + near * cover_size) - (camera_start + camera_size)
    };
    distance < CLEARANCE_RATIO * cover_size
}

/// The cursor's bounding box over `LOOKAHEAD_SECONDS` from `source_time`, in
/// fractions of the recording. `cursor` is `(t, x01, y01)` sorted by `t`.
/// Without a sample in the window it is the last position before it as a
/// point, then the first after it; `None` with no samples at all.
pub(super) fn cursor_bounds(
    cursor: &[(f64, f64, f64)],
    source_time: f64,
) -> Option<(f64, f64, f64, f64)> {
    if cursor.is_empty() || !source_time.is_finite() {
        return None;
    }
    let end = source_time + LOOKAHEAD_SECONDS;
    let first = cursor.partition_point(|sample| sample.0 < source_time);
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for &(t, x, y) in &cursor[first..] {
        if t > end {
            break;
        }
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    }
    bounds.or_else(|| {
        let (_, x, y) = if first > 0 {
            cursor[first - 1]
        } else {
            cursor[first.min(cursor.len() - 1)]
        };
        Some((x, y, x, y))
    })
}

/// The resting edge sprung over the composition, sampled once per frame the
/// way the studied spring advances.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SplitRestPlan {
    step: f64,
    values: Vec<f64>,
}

impl SplitRestPlan {
    /// Spring the target over `0..=duration` at `frame_rate`. `target` gives
    /// 0 or 1 at a composition time, or `None` to keep the previous target
    /// (the split is zoomed, or not on screen). The first target is taken at
    /// rest with no motion.
    pub(super) fn build(
        duration: f64,
        frame_rate: f64,
        spring: CameraSpring,
        mut target: impl FnMut(f64) -> Option<f64>,
    ) -> Self {
        if !(duration.is_finite() && duration >= 0.0 && frame_rate.is_finite() && frame_rate > 0.0)
        {
            return Self::default();
        }
        let step = 1.0 / frame_rate;
        let frames = (duration / step).ceil() as usize + 1;
        let mut values = Vec::with_capacity(frames);
        let mut goal = target(0.0).unwrap_or(0.0).clamp(0.0, 1.0);
        let mut value = goal;
        let mut velocity = 0.0;
        let mut precision = 1.0;
        values.push(value);
        for index in 1..frames {
            let time = index as f64 * step;
            if let Some(next) = target(time) {
                let next = next.clamp(0.0, 1.0);
                if next != goal {
                    precision = zoom::studied_precision(value, next, zoom::SCREEN_SPRING_PRECISION);
                    goal = next;
                }
            }
            (value, velocity) =
                zoom::studied_advance(value, velocity, goal, spring, precision, step * 1000.0);
            values.push(value);
        }
        Self { step, values }
    }

    /// The sprung edge at `time`: 0 rests on the screen's own edge, 1 on the
    /// other. Linear between frames, held past either end.
    pub(super) fn value_at(&self, time: f64) -> f64 {
        let Some(&last) = self.values.last() else {
            return 0.0;
        };
        if !time.is_finite() || time <= 0.0 || self.step <= 0.0 {
            return self.values[0];
        }
        let position = time / self.step;
        let index = position.floor() as usize;
        if index + 1 >= self.values.len() {
            return last;
        }
        let t = position - index as f64;
        self.values[index] + (self.values[index + 1] - self.values[index]) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side_by_side(screen_first: bool) -> SplitGeometry {
        // A 16:9 screen in a 16:9 stage, split 60/40: the cover overflows the pane.
        let (pane, camera) = if screen_first {
            (
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.6,
                    height: 1.0,
                },
                Rect {
                    x: 0.6,
                    y: 0.0,
                    width: 0.4,
                    height: 1.0,
                },
            )
        } else {
            (
                Rect {
                    x: 0.4,
                    y: 0.0,
                    width: 0.6,
                    height: 1.0,
                },
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.4,
                    height: 1.0,
                },
            )
        };
        SplitGeometry {
            pane,
            camera,
            cover: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            horizontal: true,
            screen_first,
        }
    }

    #[test]
    fn a_cursor_clear_of_the_camera_keeps_the_resting_edge() {
        assert!(!flips(side_by_side(true), (0.1, 0.5, 0.2, 0.5)));
        assert!(!flips(side_by_side(false), (0.8, 0.5, 0.9, 0.5)));
    }

    #[test]
    fn a_cursor_near_the_camera_flips_to_the_other_edge() {
        // Screen first: the camera starts at 0.6 and the cover is 1.0 wide, so
        // anything right of 0.475 is within the 0.125 clearance.
        assert!(flips(side_by_side(true), (0.3, 0.5, 0.5, 0.5)));
        assert!(!flips(side_by_side(true), (0.3, 0.5, 0.47, 0.5)));
        // Hidden under the crop entirely still flips.
        assert!(flips(side_by_side(true), (0.9, 0.5, 0.9, 0.5)));
        // Screen second: rest start = 0.4 + 0.6 - 1.0 = 0, camera ends at 0.4,
        // so anything left of 0.525 flips.
        assert!(flips(side_by_side(false), (0.5, 0.5, 0.9, 0.5)));
        assert!(!flips(side_by_side(false), (0.53, 0.5, 0.9, 0.5)));
    }

    #[test]
    fn stacked_splits_measure_down_the_frame() {
        let geometry = SplitGeometry {
            pane: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.5,
            },
            camera: Rect {
                x: 0.0,
                y: 0.5,
                width: 1.0,
                height: 0.5,
            },
            cover: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.8,
            },
            horizontal: false,
            screen_first: true,
        };
        // Camera at 0.5, cover 0.8 tall: flips when 0.8*y1 > 0.5 - 0.1 => y1 > 0.5.
        assert!(!flips(geometry, (0.5, 0.2, 0.5, 0.45)));
        assert!(flips(geometry, (0.5, 0.2, 0.5, 0.55)));
    }

    #[test]
    fn the_bounds_look_ahead_and_fall_back_to_the_last_position() {
        let cursor = [
            (0.0, 0.1, 0.1),
            (1.0, 0.2, 0.3),
            (1.2, 0.6, 0.1),
            (1.5, 0.9, 0.9),
        ];
        assert_eq!(cursor_bounds(&cursor, 1.0), Some((0.2, 0.1, 0.6, 0.3)));
        assert_eq!(cursor_bounds(&cursor, 0.5), Some((0.1, 0.1, 0.1, 0.1)));
        assert_eq!(cursor_bounds(&cursor, 3.0), Some((0.9, 0.9, 0.9, 0.9)));
        assert_eq!(cursor_bounds(&[], 1.0), None);
    }

    #[test]
    fn the_plan_springs_between_edges_and_holds() {
        let spring = zoom::CAMERA_FOLLOW_SPRING;
        let plan = SplitRestPlan::build(4.0, 60.0, spring, |t| {
            if t < 1.0 {
                Some(0.0)
            } else if t < 2.0 {
                Some(1.0)
            } else {
                None
            }
        });
        assert_eq!(plan.value_at(0.5), 0.0);
        let early = plan.value_at(1.05);
        assert!(
            early > 0.0 && early < 0.9,
            "it eases rather than jumps: {early}"
        );
        assert!(
            (plan.value_at(1.95) - 1.0).abs() < 0.02,
            "it settles on the other edge"
        );
        assert!(
            (plan.value_at(3.5) - 1.0).abs() < 1e-6,
            "None holds the last target"
        );
        assert_eq!(SplitRestPlan::default().value_at(1.0), 0.0);
    }

    #[test]
    fn the_time_mapping_finds_the_source_time() {
        let mapping = zoom::TimeMapping::new(vec![(0.0, 2.0, 5.0, 7.0), (2.0, 3.0, 10.0, 11.0)]);
        assert_eq!(mapping.source_at(1.0), Some(6.0));
        assert_eq!(mapping.source_at(2.5), Some(10.5));
        assert_eq!(mapping.source_at(3.5), None);
        assert_eq!(zoom::TimeMapping::default().source_at(4.0), Some(4.0));
    }
}
