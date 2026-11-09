//! The studied cursor travel path: a damped spring chasing the recorded path.
//!
//! The reference we studied does not move the cursor sprite straight to each
//! recorded point. It runs a spring on the pointer stream, so the cursor eases
//! toward a point, trails it by the spring's own lag, and comes to rest exactly
//! on it. The spring presets below are the reference's cursor animation
//! presets, and the default is the soft spring the reference uses when no
//! preset is chosen.
//!
//! This module implements that motion law. The reference's lead/speed planner,
//! which raises the lead around clicks, drags, hover and cursor-image changes,
//! is not ported yet, so the target is the recorded path itself; the editor's
//! speed setting scales the spring the way the reference's speed schedule does.

use super::sidecar::PointerSample;

/// The reference interpolates between two recorded points only when they are
/// no further apart than this; longer gaps hold the cursor in place.
const PATH_INTERPOLATION_GAP_MS: f64 = 40.0;
/// The reference integrates the spring in four-millisecond steps.
const SPRING_SUBSTEP_MS: f64 = 4.0;
/// Velocity and distance below which the reference snaps the spring to rest.
const REST_EPSILON: f64 = 0.01;
/// The reference's longest cursor lead.
const MAX_LEAD_MS: f64 = 600.0;
/// How far back the evaluation restarts from rest. The reference plans from
/// the spring's settle window before the segment it draws, never from the
/// recording's first frame.
const PLAN_WINDOW_MS: f64 = 2100.0;

/// A spring, in the reference's units: stiffness and damping are per second,
/// mass in arbitrary units, and positions are pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f64,
    pub damping: f64,
    pub mass: f64,
}

impl SpringConfig {
    /// The reference's default cursor spring, used when no preset is chosen.
    pub const NONE: Self = Self {
        stiffness: 125.0,
        damping: 12.0,
        mass: 1.5,
    };
    /// The reference's "Rapid" cursor animation preset.
    pub const RAPID: Self = Self {
        stiffness: 530.0,
        damping: 40.0,
        mass: 1.0,
    };
    /// The reference's "Medium" cursor animation preset.
    pub const MEDIUM: Self = Self {
        stiffness: 340.0,
        damping: 60.0,
        mass: 3.0,
    };
    /// The reference's "Smooth" cursor animation preset.
    pub const SMOOTH: Self = Self {
        stiffness: 470.0,
        damping: 70.0,
        mass: 3.0,
    };

    /// The preset the editor's smoothing slider selects. The slider's bands
    /// walk the reference's cursor presets from its default spring to its
    /// softest one, so an existing project keeps a sensible setting.
    pub fn for_smooth(smooth: f64) -> Self {
        let smooth = if smooth.is_finite() {
            smooth.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if smooth < 0.25 {
            Self::NONE
        } else if smooth < 0.5 {
            Self::RAPID
        } else if smooth < 0.75 {
            Self::MEDIUM
        } else {
            Self::SMOOTH
        }
    }

    /// The lag the spring itself introduces, in milliseconds. The reference
    /// uses it to seed its lead search; without the planner it is the lead.
    pub fn lag_ms(self) -> f64 {
        if self.stiffness <= 0.0 || !self.stiffness.is_finite() {
            return 0.0;
        }
        (self.damping / self.stiffness * 1000.0).clamp(0.0, MAX_LEAD_MS)
    }

    fn speed_scaled(self, speed: f64) -> Self {
        let speed = if speed.is_finite() {
            speed.clamp(0.25, 4.0)
        } else {
            1.0
        };
        Self {
            stiffness: self.stiffness * speed * speed,
            damping: self.damping * speed,
            mass: self.mass,
        }
    }
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::NONE
    }
}

/// The recorded path position at `time_ms`, before the spring: interpolated
/// between close points, held across long gaps and between events.
fn path_at(samples: &[PointerSample], time_ms: f64) -> (f64, f64) {
    let Some(first) = samples.first() else {
        return (0.0, 0.0);
    };
    let first_ms = first.t * 1000.0;
    if time_ms <= first_ms {
        return (first.x, first.y);
    }
    let last = &samples[samples.len() - 1];
    if time_ms >= last.t * 1000.0 {
        return (last.x, last.y);
    }
    // The last sample at or before `time_ms`, which is the segment being held
    // or traversed.
    let mut low = 0;
    let mut high = samples.len();
    while low < high {
        let middle = (low + high) / 2;
        if samples[middle].t * 1000.0 <= time_ms {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let index = low - 1;
    let from = &samples[index];
    let Some(to) = samples.get(index + 1) else {
        return (from.x, from.y);
    };
    let span_ms = (to.t - from.t) * 1000.0;
    if span_ms <= 0.0 || span_ms > PATH_INTERPOLATION_GAP_MS {
        return (from.x, from.y);
    }
    let amount = ((time_ms - from.t * 1000.0) / span_ms).clamp(0.0, 1.0);
    (
        from.x + (to.x - from.x) * amount,
        from.y + (to.y - from.y) * amount,
    )
}

/// The cursor position at `seconds`, with the spring trailing the recorded
/// path by its own lag. The reference plans a lead only around sync points
/// (clicks, drags, hover and cursor-image changes), so plain movement travels
/// on the path with no lead at all.
pub fn position_at(
    samples: &[PointerSample],
    seconds: f64,
    spring: SpringConfig,
    speed: f64,
) -> (f64, f64) {
    let Some(first) = samples.first() else {
        return (0.0, 0.0);
    };
    let spring = spring.speed_scaled(speed);
    let time_ms = seconds * 1000.0;
    let first_ms = first.t * 1000.0;
    if time_ms <= first_ms {
        return (first.x, first.y);
    }
    let mut clock = (time_ms - PLAN_WINDOW_MS).max(first_ms);
    let (mut x, mut y) = path_at(samples, clock);
    let (mut velocity_x, mut velocity_y) = (0.0, 0.0);
    let mass = spring.mass.max(0.0);
    let omega = if mass > 0.0 {
        (spring.stiffness / mass).max(0.0).sqrt()
    } else {
        0.0
    };
    let critical = 2.0 * (mass * spring.stiffness).max(0.0).sqrt();
    while clock < time_ms {
        let step_ms = (time_ms - clock).min(SPRING_SUBSTEP_MS);
        let step = step_ms / 1000.0;
        let (target_x, target_y) = path_at(samples, clock);
        if mass == 0.0 {
            // A massless spring snaps to the target, as in the reference.
            x = target_x;
            y = target_y;
        } else {
            let dx = target_x - x;
            let dy = target_y - y;
            let damping_x = if velocity_x * dx > 0.0 && velocity_x.abs() > dx.abs() * omega {
                critical
            } else {
                spring.damping
            };
            let damping_y = if velocity_y * dy > 0.0 && velocity_y.abs() > dy.abs() * omega {
                critical
            } else {
                spring.damping
            };
            velocity_x += (dx * spring.stiffness - velocity_x * damping_x) / mass * step;
            velocity_y += (dy * spring.stiffness - velocity_y * damping_y) / mass * step;
            x += velocity_x * step;
            y += velocity_y * step;
            if velocity_x.abs() < REST_EPSILON && (x - target_x).abs() < REST_EPSILON {
                x = target_x;
                velocity_x = 0.0;
            }
            if velocity_y.abs() < REST_EPSILON && (y - target_y).abs() < REST_EPSILON {
                y = target_y;
                velocity_y = 0.0;
            }
        }
        clock += step_ms;
    }
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::sidecar::CursorKind;

    fn samples(points: &[(f64, f64, f64)]) -> Vec<PointerSample> {
        points
            .iter()
            .map(|&(t, x, y)| PointerSample {
                t,
                x,
                y,
                kind: CursorKind::Default,
                cursor_id: None,
            })
            .collect()
    }

    #[test]
    fn the_spring_trails_a_step_and_lands_on_it() {
        let stream = samples(&[(0.0, 0.0, 0.0), (0.2, 100.0, 0.0)]);
        let (mid, _) = position_at(&stream, 0.25, SpringConfig::NONE, 1.0);
        assert!(mid > 0.0 && mid < 100.0, "the cursor should trail the step");
        let (settled, _) = position_at(&stream, 4.0, SpringConfig::NONE, 1.0);
        assert!((settled - 100.0).abs() < 0.01, "the spring should land");
    }

    #[test]
    fn a_long_gap_holds_until_the_move_is_due() {
        let stream = samples(&[(0.0, 0.0, 0.0), (2.0, 100.0, 0.0)]);
        let (early, _) = position_at(&stream, 1.4, SpringConfig::NONE, 1.0);
        assert!(early < 1.0, "the cursor must not drift toward a late move");
        let (late, _) = position_at(&stream, 5.0, SpringConfig::NONE, 1.0);
        assert!((late - 100.0).abs() < 0.01);
    }

    #[test]
    fn a_dense_stream_is_followed_smoothly() {
        let dense: Vec<(f64, f64, f64)> = (0..=40)
            .map(|index| (index as f64 * 0.008, index as f64 * 2.0, 0.0))
            .collect();
        let stream = samples(&dense);
        let (x, _) = position_at(&stream, 0.16, SpringConfig::NONE, 1.0);
        assert!(
            x > 8.0 && x < 40.0,
            "the spring should be moving along the path"
        );
        let (landed, _) = position_at(&stream, 3.0, SpringConfig::NONE, 1.0);
        assert!(
            (landed - 80.0).abs() < 0.01,
            "the cursor should reach the end"
        );
    }

    #[test]
    fn a_faster_speed_tightens_the_follow() {
        let stream = samples(&[(0.0, 0.0, 0.0), (0.2, 100.0, 0.0)]);
        let slow = position_at(&stream, 0.25, SpringConfig::RAPID, 1.0).0;
        let fast = position_at(&stream, 0.25, SpringConfig::RAPID, 2.0).0;
        assert!(fast > slow, "speed should shorten the lag");
    }

    #[test]
    fn the_smoothing_slider_walks_the_presets() {
        assert_eq!(SpringConfig::for_smooth(0.1), SpringConfig::NONE);
        assert_eq!(SpringConfig::for_smooth(0.4), SpringConfig::RAPID);
        assert_eq!(SpringConfig::for_smooth(0.6), SpringConfig::MEDIUM);
        assert_eq!(SpringConfig::for_smooth(0.9), SpringConfig::SMOOTH);
    }

    #[test]
    fn the_default_spring_is_the_reference_default() {
        assert_eq!(SpringConfig::default(), SpringConfig::NONE);
        assert!((SpringConfig::NONE.lag_ms() - 96.0).abs() < 1e-9);
    }
}
