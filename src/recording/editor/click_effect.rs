//! Geometry for the two studied click effects, shared by the cursor overlay
//! renderers.
//!
//! The studied recorder exposes a nullable click effect that is either a
//! `ripple` or a `circle`. Each has a fixed visible lifetime; there is no
//! user-facing duration for them. The numbers here restate that visible-time
//! behaviour in our own terms.
//!
//! The studied ripple is a footage *warp*: an outward pull along a decaying
//! ring, a calm-zone fade, and a chromatic split. The pull is ported: the
//! composite export runs [`ripple_pull_px`]'s maths as a GPU fragment shader
//! (see `gst_warp`) that samples the footage at `pixel - pull`, so the visible
//! band *is* the displaced region and no per-frame map is materialised.
//! [`ripple_radius_fraction`] and [`ripple_opacity`] remain only as the
//! drawn-ring fallback for paths that cannot run the warp (see
//! `cursor_export`).
//! The live video preview samples its current footage with the same prepared
//! pull before compositing the cursor, so it no longer uses that ring stand-in.
//!
//! The chromatic split is **not** implemented. The studied `split` separates
//! the colour channels along the ring; reproducing it in the single shader
//! pass would need a second and third full-resolution pass (one per channel),
//! which is not worth the cost for a fringe the pull already carries. The
//! split constants and formula are kept in [`ripple_split_px`] so the gap
//! stays explicit rather than faked.

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

/// Signed displacement envelope at `age_ms` into the ripple's life.
///
/// The studied state function is `exp(-(age/1000) * 3) *
/// sin((age/1000) * 9.4)`, and it is zero outside `0..=1000` ms. This is the
/// same curve as [`ripple_bounce`], addressed in milliseconds so the warp can
/// use the studied time base directly.
pub fn ripple_bounce_at_ms(age_ms: f64) -> f64 {
    if !(0.0..=RIPPLE_VISIBLE_DURATION_MS).contains(&age_ms) {
        return 0.0;
    }
    let progress = age_ms / RIPPLE_VISIBLE_DURATION_MS;
    (-RIPPLE_DECAY * progress).exp() * (RIPPLE_FREQUENCY * progress).sin()
}

/// Radius of the ripple ring at `age_ms`, in video pixels.
///
/// The studied radius grows from `pullSize01 * videoWidth` by
/// `(age/1000) * videoWidth * 0.2`, and is zero outside `0..=1000` ms.
pub fn ripple_radius_px(age_ms: f64, pull_size_01: f64, video_width: f64) -> f64 {
    if !(0.0..=RIPPLE_VISIBLE_DURATION_MS).contains(&age_ms) {
        return 0.0;
    }
    let progress = age_ms / RIPPLE_VISIBLE_DURATION_MS;
    (pull_size_01 + progress * RIPPLE_RADIUS_GROWTH_01) * video_width
}

/// Ring envelope of the studied ripple: `exp(-((distance - radius)/band)^2 *
/// 4)`, where `band = videoWidth * 0.05`. The caller skips pixels further than
/// `band * 3` from the ring, so this is only evaluated on the band.
pub fn ripple_ring(distance: f64, radius: f64, band: f64) -> f64 {
    let ring_distance = (distance - radius) / band;
    (-(ring_distance * ring_distance) * 4.0).exp()
}

/// Calm-zone fade of the studied ripple.
///
/// When the calm-zone radius (`pullSize01 * videoWidth`) is larger than half a
/// pixel, the displacement fades out over the inner 35% of that radius; below
/// half a pixel there is no calm zone and the fade is 1.
pub fn ripple_calm_zone(distance: f64, pull_size_px: f64) -> f64 {
    if pull_size_px > 0.5 {
        smoothstep(pull_size_px * 0.35, pull_size_px, distance)
    } else {
        1.0
    }
}

/// One ripple's frame-constant state, shared by scalar and image samplers.
///
/// Preparing a click once avoids recalculating its radius and damped bounce
/// for every pixel. The per-pixel terms retain the export displacement maths.
/// A conservative bounding box skips clearly unaffected pixels before the
/// unchanged radial test, leaving floating-point boundary decisions to it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreparedRipple {
    click_x: f64,
    click_y: f64,
    bounce: f64,
    radius: f64,
    band: f64,
    outer_bound: f64,
    pull_size_px: f64,
    video_width: f64,
}

impl PreparedRipple {
    /// Prepare a source-space click at one age, in encoded pixels and ms.
    pub(crate) fn new(
        click_x: f64,
        click_y: f64,
        age_ms: f64,
        pull_size_01: f64,
        video_width: f64,
    ) -> Self {
        let radius = ripple_radius_px(age_ms, pull_size_01, video_width);
        let band = video_width * RIPPLE_BAND_01;
        Self {
            click_x,
            click_y,
            bounce: ripple_bounce_at_ms(age_ms),
            radius,
            band,
            outer_bound: (radius + band * 3.0) * (1.0 + f64::EPSILON * 4.0),
            pull_size_px: pull_size_01 * video_width,
            video_width,
        }
    }

    fn terms(&self, px: f64, py: f64) -> Option<(f64, f64, f64, f64)> {
        if self.bounce == 0.0 {
            return None;
        }
        let rel_x = px - self.click_x;
        let rel_y = py - self.click_y;
        if (rel_x.abs() > self.outer_bound || rel_y.abs() > self.outer_bound)
            && !rel_x.is_nan()
            && !rel_y.is_nan()
        {
            return None;
        }
        let distance = (rel_x * rel_x + rel_y * rel_y).sqrt();
        if (distance - self.radius).abs() > self.band * 3.0 {
            return None;
        }
        let ring = ripple_ring(distance, self.radius, self.band);
        if distance == 0.0 {
            return None;
        }
        let calm_zone = ripple_calm_zone(distance, self.pull_size_px);
        Some((ring, calm_zone, rel_x / distance, rel_y / distance))
    }

    /// Conservative source-x bounds of possible displacement on one source row.
    ///
    /// The slightly expanded outer radius preserves the original radial test's
    /// floating-point boundary decisions. Indeterminate bounds cover the full
    /// row rather than omitting any potentially displaced pixels.
    pub(crate) fn row_bounds(&self, source_y: f64) -> Option<(f64, f64)> {
        if self.bounce == 0.0 {
            return None;
        }
        let dy = (source_y - self.click_y).abs();
        if dy > self.outer_bound {
            return None;
        }
        if !dy.is_finite() || !self.outer_bound.is_finite() || self.outer_bound <= 0.0 {
            return Some((f64::NEG_INFINITY, f64::INFINITY));
        }
        let ratio = dy / self.outer_bound;
        let extent = self.outer_bound * (1.0 - ratio * ratio).max(0.0).sqrt();
        Some((
            (self.click_x - extent).next_down(),
            (self.click_x + extent).next_up(),
        ))
    }

    /// Signed source-pixel pull; sample the original frame at `pixel - pull`.
    pub(crate) fn pull_px(&self, px: f64, py: f64) -> (f64, f64) {
        match self.terms(px, py) {
            Some((ring, calm_zone, dir_x, dir_y)) => {
                let scalar = self.bounce * ring * self.video_width * RIPPLE_PULL_01 * calm_zone;
                (dir_x * scalar, dir_y * scalar)
            }
            None => (0.0, 0.0),
        }
    }

    fn split_px(&self, px: f64, py: f64) -> (f64, f64) {
        match self.terms(px, py) {
            Some((ring, calm_zone, dir_x, dir_y)) => {
                let scalar = self.bounce.abs()
                    * ring
                    * (1.0 - ring)
                    * 4.0
                    * self.video_width
                    * RIPPLE_CHROMATIC_ABERRATION_01
                    * calm_zone;
                (dir_x * scalar, dir_y * scalar)
            }
            None => (0.0, 0.0),
        }
    }
}

/// The studied footage pull at `age_ms` for a destination pixel, in video
/// pixels.
///
/// The studied caller samples the footage at `pixel - pull`, so a remap map
/// writes `x - pull_x` and `y - pull_y` as the source coordinate. The vector
/// points along the ring's outward direction and its sign is the studied
/// decaying bounce, so the displacement grows and reverses over the life of
/// the effect.
pub fn ripple_pull_px(
    px: f64,
    py: f64,
    click_x: f64,
    click_y: f64,
    age_ms: f64,
    pull_size_01: f64,
    video_width: f64,
) -> (f64, f64) {
    PreparedRipple::new(click_x, click_y, age_ms, pull_size_01, video_width).pull_px(px, py)
}

/// The studied chromatic split at `age_ms` for a destination pixel, in video
/// pixels.
///
/// Recorded for completeness only: the split is not applied by the export (see
/// the module doc). The studied formula is `direction * |bounce| * ring *
/// (1 - ring) * 4 * videoWidth * CHROMATIC_ABERRATION_01 * calmZoneFade`.
pub fn ripple_split_px(
    px: f64,
    py: f64,
    click_x: f64,
    click_y: f64,
    age_ms: f64,
    pull_size_01: f64,
    video_width: f64,
) -> (f64, f64) {
    PreparedRipple::new(click_x, click_y, age_ms, pull_size_01, video_width).split_px(px, py)
}

/// Read margin the studied renderer keeps around a warping effect, in video
/// pixels: `ceil(max(videoWidth * readReach01, 16))`.
///
/// Our composite applies the warp to the full source frame before the crop and
/// zoom, so the margin is the whole source rather than a padded texture; the
/// value is recorded so the gap stays explicit.
pub fn ripple_read_margin_px(video_width: f64) -> f64 {
    (video_width * RIPPLE_READ_REACH_01).max(16.0).ceil()
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

    #[test]
    fn ripple_bounce_at_ms_matches_the_progress_curve_and_bounds() {
        for step in 0..=20 {
            let progress = step as f64 / 20.0;
            assert!(approx(
                ripple_bounce_at_ms(progress * RIPPLE_VISIBLE_DURATION_MS),
                ripple_bounce(progress)
            ));
        }
        // The studied state is defined on the closed window and zero outside.
        assert!(approx(ripple_bounce_at_ms(-0.001), 0.0));
        assert!(approx(ripple_bounce_at_ms(1000.001), 0.0));
        // At the very start the sine is zero, so there is no displacement.
        assert!(approx(ripple_bounce_at_ms(0.0), 0.0));
        // The end of the closed window is the tail of the decaying sine, not
        // an exact zero; the renderer drops the effect one tick earlier.
        let end = ripple_bounce_at_ms(RIPPLE_VISIBLE_DURATION_MS);
        assert!(end.abs() < 0.01 && end != 0.0);
    }

    #[test]
    fn ripple_radius_px_grows_with_age_and_resolution() {
        assert!(approx(ripple_radius_px(0.0, 0.04, 1000.0), 40.0));
        assert!(approx(
            ripple_radius_px(1000.0, 0.04, 1000.0),
            (0.04 + RIPPLE_RADIUS_GROWTH_01) * 1000.0
        ));
        // The radius is in video pixels, so a wider video travels further.
        assert!(approx(
            ripple_radius_px(500.0, 0.04, 2000.0),
            ripple_radius_px(500.0, 0.04, 1000.0) * 2.0
        ));
        assert_eq!(ripple_radius_px(-1.0, 0.04, 1000.0), 0.0);
        assert_eq!(ripple_radius_px(1000.001, 0.04, 1000.0), 0.0);
    }

    #[test]
    fn ripple_ring_peaks_on_the_ring_and_decays_with_the_band() {
        let band = 50.0;
        let radius = 60.0;
        assert!(approx(ripple_ring(radius, radius, band), 1.0));
        assert!(approx(
            ripple_ring(radius + band, radius, band),
            (-4.0_f64).exp()
        ));
        assert!(approx(
            ripple_ring(radius - band, radius, band),
            (-4.0_f64).exp()
        ));
        assert!(ripple_ring(radius + band * 3.0, radius, band) < 1e-15);
    }

    #[test]
    fn ripple_calm_zone_fades_inside_the_pull_radius() {
        // A calm-zone radius above half a pixel fades over its inner 35%.
        assert!(approx(ripple_calm_zone(14.0, 40.0), 0.0));
        assert!(approx(ripple_calm_zone(40.0, 40.0), 1.0));
        assert!(ripple_calm_zone(27.0, 40.0) > 0.0);
        assert!(ripple_calm_zone(27.0, 40.0) < 1.0);
        // Below half a pixel the calm zone does not exist.
        assert!(approx(ripple_calm_zone(0.0, 0.4), 1.0));
    }

    #[test]
    fn ripple_pull_is_zero_at_the_click_outside_the_band_and_at_the_ends() {
        let (w, pull_size) = (1000.0, 0.04);
        // Exactly on the click the studied shader early-returns.
        assert_eq!(
            ripple_pull_px(100.0, 100.0, 100.0, 100.0, 100.0, pull_size, w),
            (0.0, 0.0)
        );
        // Far outside the ring band there is no pull.
        assert_eq!(
            ripple_pull_px(900.0, 100.0, 100.0, 100.0, 100.0, pull_size, w),
            (0.0, 0.0)
        );
        // At the click instant the bounce is zero.
        assert_eq!(
            ripple_pull_px(160.0, 100.0, 100.0, 100.0, 0.0, pull_size, w),
            (0.0, 0.0)
        );
        // Past the visible lifetime there is no state at all.
        assert_eq!(
            ripple_pull_px(160.0, 100.0, 100.0, 100.0, 1000.001, pull_size, w),
            (0.0, 0.0)
        );
    }

    #[test]
    fn ripple_pull_matches_the_studied_scalar_and_direction() {
        let (w, pull_size) = (1000.0, 0.04);
        let age = 100.0;
        let radius = ripple_radius_px(age, pull_size, w);
        let bounce = ripple_bounce_at_ms(age);
        // A pixel directly to the right of the click, on the ring.
        let (right_x, right_y) =
            ripple_pull_px(100.0 + radius, 100.0, 100.0, 100.0, age, pull_size, w);
        assert!(approx(right_x, bounce * w * RIPPLE_PULL_01));
        assert!(approx(right_y, 0.0));
        // A pixel above the click pulls upward instead.
        let (up_x, up_y) = ripple_pull_px(100.0, 100.0 + radius, 100.0, 100.0, age, pull_size, w);
        assert!(approx(up_x, 0.0));
        assert!(approx(up_y, bounce * w * RIPPLE_PULL_01));
        // Resolution scaling: the same fractional pull is twice the pixels.
        let (wide, _) = ripple_pull_px(
            200.0 + radius * 2.0,
            200.0,
            200.0,
            200.0,
            age,
            pull_size,
            2000.0,
        );
        assert!(approx(wide, right_x * 2.0));
    }

    #[test]
    fn ripple_split_records_the_unapplied_chromatic_term() {
        let (w, pull_size) = (1000.0, 0.04);
        let age = 100.0;
        let radius = ripple_radius_px(age, pull_size, w);
        let band = w * RIPPLE_BAND_01;
        // On the ring the split term is zero because ring * (1 - ring) is.
        assert!(approx(
            ripple_split_px(100.0 + radius, 100.0, 100.0, 100.0, age, pull_size, w).0,
            0.0
        ));
        // Half a band off the ring the split is non-zero and chromatic.
        let (split_x, _) = ripple_split_px(
            100.0 + radius + band * 0.5,
            100.0,
            100.0,
            100.0,
            age,
            pull_size,
            w,
        );
        assert!(split_x > 0.0);
    }

    #[test]
    fn ripple_read_margin_has_a_sixteen_pixel_floor() {
        assert!(approx(ripple_read_margin_px(100.0), 16.0));
        assert!(approx(ripple_read_margin_px(1000.0), 31.0));
    }
}
