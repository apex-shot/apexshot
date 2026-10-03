//! Geometry for the two studied click effects, shared by the cursor overlay
//! renderers.
//!
//! The studied recorder exposes a nullable click effect that is either a
//! `ripple` or a `circle`. Each has a fixed visible lifetime; there is no
//! user-facing duration for them. The numbers here restate that visible-time
//! behaviour in our own terms.
//!
//! The studied ripple is a footage *warp*: an outward pull along a decaying
//! ring, a calm-zone fade, and a chromatic split. Our cursor overlay is a
//! transparent surface drawn on top of the video and never samples its pixels,
//! so [`ripple_radius_fraction`] and [`ripple_opacity`] can only stand in for
//! that warp as a drawn ring. This module does not claim parity with the warp;
//! the pull and split constants are kept so the gap stays explicit.

/// Visible lifetime of the ripple, in milliseconds.
pub const RIPPLE_VISIBLE_DURATION_MS: f64 = 1000.0;
/// Visible lifetime of the circle, in milliseconds.
pub const CIRCLE_VISIBLE_DURATION_MS: f64 = 450.0;

/// Ring band width, as a fraction of the video width.
pub const RIPPLE_BAND_01: f64 = 0.05;
/// Starting radius of the ripple ring, as a fraction of the video width.
///
/// This is the studied `pullSize01` default.
pub const RIPPLE_PULL_SIZE_01: f64 = 0.04;
/// How far the ripple ring travels outward, as a fraction of the video width.
pub const RIPPLE_RADIUS_GROWTH_01: f64 = 0.2;
/// Exponential decay of the studied ripple displacement.
pub const RIPPLE_DECAY: f64 = 3.0;
/// Angular frequency of the studied ripple displacement.
pub const RIPPLE_FREQUENCY: f64 = 9.4;
/// Peak outward displacement of the studied footage warp, as a fraction of the
/// video width. Not drawn by the overlay renderer.
pub const RIPPLE_PULL_01: f64 = 0.015;
/// Chromatic split of the studied footage warp, as a fraction of the video
/// width. Not drawn by the overlay renderer.
pub const RIPPLE_CHROMATIC_ABERRATION_01: f64 = 0.004;
/// Margin a warping effect needs, as a fraction of the video width.
pub const RIPPLE_READ_REACH_01: f64 = RIPPLE_PULL_01 + RIPPLE_CHROMATIC_ABERRATION_01 * 4.0;

/// Circle radius at rest, as a fraction of the video width.
pub const CIRCLE_RADIUS_START_01: f64 = 0.002;
/// Circle radius at the end of its life, as a fraction of the video width.
pub const CIRCLE_RADIUS_END_01: f64 = 0.03;
/// Circle rim width, as a fraction of the video width.
pub const CIRCLE_RIM_WIDTH_01: f64 = 0.0007;
/// Circle fill grey, from the studied shader's linear space.
pub const CIRCLE_FILL_GRAY_LINEAR: f64 = 0.7569;
/// Circle rim grey, from the studied shader's linear space.
pub const CIRCLE_RIM_GRAY_LINEAR: f64 = 0.0;
/// Peak rim alpha of the circle.
pub const CIRCLE_RIM_ALPHA: f64 = 0.5;
/// Overall circle alpha.
pub const CIRCLE_ALPHA: f64 = 0.6;

/// Smooth Hermite interpolation between `edge0` and `edge1`, clamped to
/// `0..=1` (the studied `ey` helper).
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if (edge1 - edge0).abs() < f64::EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Ease-out cubic, `1 - (1 - t)^3`, clamped to `0..=1`.
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Circle radius as a fraction of the video width at `progress` in `0..=1`.
///
/// The studied radius grows from [`CIRCLE_RADIUS_START_01`] to
/// [`CIRCLE_RADIUS_END_01`] on an ease-out-cubic curve.
pub fn circle_radius_fraction(progress: f64) -> f64 {
    if !(0.0..=1.0).contains(&progress) {
        return 0.0;
    }
    CIRCLE_RADIUS_START_01
        + (CIRCLE_RADIUS_END_01 - CIRCLE_RADIUS_START_01) * ease_out_cubic(progress)
}

/// Circle opacity at `progress` in `0..=1`.
///
/// The studied curve fades in over the first 15% and out from 15% to 90%, so
/// the effect is invisible at both ends and peaks near the start.
pub fn circle_opacity(progress: f64) -> f64 {
    if !(0.0..=1.0).contains(&progress) {
        return 0.0;
    }
    smoothstep(0.0, 0.15, progress) * (1.0 - smoothstep(0.15, 0.9, progress))
}

/// Ripple ring radius as a fraction of the video width at `progress` in `0..=1`.
pub fn ripple_radius_fraction(progress: f64) -> f64 {
    if !(0.0..=1.0).contains(&progress) {
        return 0.0;
    }
    RIPPLE_PULL_SIZE_01 + progress * RIPPLE_RADIUS_GROWTH_01
}

/// Signed displacement envelope of the studied ripple at `progress` in `0..=1`.
///
/// Mirrors the studied damped oscillation. The overlay renderer never samples
/// the footage, so this is kept for the drawn ring's strength and as an
/// explicit record of the unported warp.
pub fn ripple_bounce(progress: f64) -> f64 {
    if !(0.0..=1.0).contains(&progress) {
        return 0.0;
    }
    (-RIPPLE_DECAY * progress).exp() * (RIPPLE_FREQUENCY * progress).sin()
}

/// Drawn-ring strength of the ripple at `progress` in `0..=1`.
///
/// The magnitude of [`ripple_bounce`]: zero at the click, peaking early, and
/// decaying towards the end of the visible window.
pub fn ripple_opacity(progress: f64) -> f64 {
    ripple_bounce(progress).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn smoothstep_hits_both_edges_and_midpoint() {
        assert!(approx(smoothstep(0.0, 1.0, 0.0), 0.0));
        assert!(approx(smoothstep(0.0, 1.0, 1.0), 1.0));
        assert!(approx(smoothstep(0.0, 1.0, 0.5), 0.5));
        assert!(approx(smoothstep(0.0, 1.0, -1.0), 0.0));
        assert!(approx(smoothstep(0.0, 1.0, 2.0), 1.0));
    }

    #[test]
    fn circle_radius_endpoints_and_monotonicity() {
        assert!(approx(circle_radius_fraction(0.0), CIRCLE_RADIUS_START_01));
        assert!(approx(circle_radius_fraction(1.0), CIRCLE_RADIUS_END_01));
        let mut previous = circle_radius_fraction(0.0);
        for step in 1..=100 {
            let radius = circle_radius_fraction(step as f64 / 100.0);
            assert!(radius >= previous, "radius must not shrink");
            previous = radius;
        }
        // Ease-out means more than half the travel is done by the midpoint.
        let mid = circle_radius_fraction(0.5);
        let half_travel = (CIRCLE_RADIUS_START_01 + CIRCLE_RADIUS_END_01) / 2.0;
        assert!(mid > half_travel);
    }

    #[test]
    fn circle_opacity_is_zero_at_both_ends_and_peaks_early() {
        assert!(approx(circle_opacity(0.0), 0.0));
        assert!(approx(circle_opacity(1.0), 0.0));
        assert!(circle_opacity(0.15) > 0.99);
        let mut peak = 0.0;
        let mut peak_at = 0.0;
        for step in 0..=100 {
            let progress = step as f64 / 100.0;
            let opacity = circle_opacity(progress);
            if opacity > peak {
                peak = opacity;
                peak_at = progress;
            }
        }
        assert!(peak > 0.99);
        assert!((peak_at - 0.15).abs() < 0.02);
        // Invisible around the visible-duration boundary.
        assert!(circle_opacity(0.99) < 0.01);
    }

    #[test]
    fn circle_functions_are_zero_outside_the_window() {
        assert_eq!(circle_radius_fraction(-0.1), 0.0);
        assert_eq!(circle_radius_fraction(1.1), 0.0);
        assert_eq!(circle_opacity(-0.1), 0.0);
        assert_eq!(circle_opacity(1.1), 0.0);
    }

    #[test]
    fn ripple_radius_grows_to_the_studied_extent() {
        assert!(approx(ripple_radius_fraction(0.0), RIPPLE_PULL_SIZE_01));
        assert!(approx(
            ripple_radius_fraction(1.0),
            RIPPLE_PULL_SIZE_01 + RIPPLE_RADIUS_GROWTH_01
        ));
        let mut previous = ripple_radius_fraction(0.0);
        for step in 1..=100 {
            let radius = ripple_radius_fraction(step as f64 / 100.0);
            assert!(radius > previous, "radius must grow");
            previous = radius;
        }
    }

    #[test]
    fn ripple_opacity_starts_and_ends_near_zero_with_an_early_peak() {
        assert!(approx(ripple_opacity(0.0), 0.0));
        assert!(ripple_opacity(1.0) < 0.01);
        let mut peak = 0.0_f64;
        for step in 0..=100 {
            peak = peak.max(ripple_opacity(step as f64 / 100.0));
        }
        assert!(peak > 0.5);
    }

    #[test]
    fn ripple_read_reach_matches_the_studied_sum() {
        assert!(approx(RIPPLE_READ_REACH_01, 0.031));
    }
}
