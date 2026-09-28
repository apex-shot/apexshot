// Still-image rendering for the video editor's background fills.
//
// Preview and export must agree on what a gradient or a rounded card looks
// like, so both render from the functions here rather than each describing
// the background in their own terms. ffmpeg's `gradients` filter is not used:
// it distributes stops evenly (silently dropping a dragged position) and
// animates by default. Export writes these stills to disk and loops them.

use super::{GradientKind, GradientStop, VideoGradient};

/// A gradient rasterized into 8-bit RGB. Small enough to hand to both Cairo
/// (preview) and the `image` crate (export).
pub struct GradientBitmap {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB, `width * height * 3` bytes.
    pub pixels: Vec<u8>,
}

impl GradientBitmap {
    pub fn pixel(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let index = ((y * self.width + x) * 3) as usize;
        (
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
        )
    }
}

/// Sample `gradient` across a `width` x `height` box.
///
/// Stops are honored at their exact positions, so a dragged stop survives to
/// the exported file. Beyond the first and last stop the end colors extend,
/// which is what a gradient does everywhere else in the app.
pub fn render_gradient(gradient: &VideoGradient, width: u32, height: u32) -> GradientBitmap {
    let width = width.max(1);
    let height = height.max(1);
    let stops = gradient.draw_stops();
    let ((x0, y0), (x1, y1)) = gradient.endpoints(width as f64, height as f64);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length_sq = dx * dx + dy * dy;
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    // The farthest corner from the centre, so a radial ramp reaches every
    // pixel with its last stop.
    let radial_span = cx.hypot(cy).max(f64::EPSILON);
    let angular_offset = gradient.angle_degrees.to_radians();

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 3];
    for y in 0..height {
        for x in 0..width {
            let px = x as f64;
            let py = y as f64;
            let t = match gradient.kind {
                GradientKind::Linear => {
                    // Project the pixel onto the gradient line, normalized to
                    // 0..=1.
                    if length_sq > f64::EPSILON {
                        (((px - x0) * dx + (py - y0) * dy) / length_sq).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }
                GradientKind::Radial => ((px - cx).hypot(py - cy) / radial_span).clamp(0.0, 1.0),
                GradientKind::Angular => {
                    // Screen coordinates put +y down, so atan2 already runs
                    // clockwise from east, matching the angle's convention.
                    let theta = (py - cy).atan2(px - cx);
                    (theta - angular_offset).rem_euclid(std::f64::consts::TAU)
                        / std::f64::consts::TAU
                }
                // Figma's diamond metric: distance is the larger axis, so the
                // color bands are squares rather than the radial's circles.
                GradientKind::Diamond => {
                    let dx = (px - cx).abs() / cx.max(f64::EPSILON);
                    let dy = (py - cy).abs() / cy.max(f64::EPSILON);
                    dx.max(dy).clamp(0.0, 1.0)
                }
            };
            let (r, g, b) = sample_stops(&stops, t);
            let index = ((y * width + x) * 3) as usize;
            pixels[index] = r;
            pixels[index + 1] = g;
            pixels[index + 2] = b;
        }
    }
    GradientBitmap {
        width,
        height,
        pixels,
    }
}

/// The color the ramp shows at `t` (0 = first stop, 1 = last), without
/// rasterizing. Callers seeding a stop with "the color already under it" use
/// this, so they do not have to guess a sample position from a bitmap.
pub fn sample_color_at(gradient: &VideoGradient, t: f64) -> (u8, u8, u8) {
    sample_stops(&gradient.draw_stops(), t.clamp(0.0, 1.0))
}

/// Color at `t` along the stop list, holding the end colors beyond the ends.
///
/// Blending happens in premultiplied alpha and the result is already
/// composited over the canvas's black backdrop, so a stop fading out fades
/// toward black rather than toward a washed-out gray.
fn sample_stops(stops: &[GradientStop], t: f64) -> (u8, u8, u8) {
    let first = match stops.first() {
        Some(stop) => stop,
        None => return (0, 0, 0),
    };
    let last = stops.last().unwrap_or(first);
    if t <= first.position {
        return over_black(first);
    }
    if t >= last.position {
        return over_black(last);
    }
    for pair in stops.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if t >= a.position && t <= b.position {
            let span = b.position - a.position;
            let local = if span > f64::EPSILON {
                (t - a.position) / span
            } else {
                0.0
            };
            let mix = |from: u8, to: u8| {
                let from = from as f64 * a.a as f64 / 255.0;
                let to = to as f64 * b.a as f64 / 255.0;
                (from + (to - from) * local).round().clamp(0.0, 255.0) as u8
            };
            return (mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b));
        }
    }
    over_black(last)
}

/// A stop's color flattened onto the black backdrop by its own alpha.
fn over_black(stop: &GradientStop) -> (u8, u8, u8) {
    let alpha = stop.a as f64 / 255.0;
    (
        (stop.r as f64 * alpha).round() as u8,
        (stop.g as f64 * alpha).round() as u8,
        (stop.b as f64 * alpha).round() as u8,
    )
}

/// A grayscale rounded-rect mask, white inside and black outside, with a
/// one-pixel anti-aliased edge.
///
/// ffmpeg's `alphamerge` copies the mask's luma straight into the frame's
/// alpha channel, and it errors outright if the mask is not exactly the
/// frame's size — so this is always rendered at the video rect, never the
/// canvas.
pub fn render_rounded_mask(width: u32, height: u32, radius: f64) -> GradientBitmap {
    let width = width.max(1);
    let height = height.max(1);
    let radius = radius.clamp(0.0, width.min(height) as f64 / 2.0);
    let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
    let (hw, hh) = (width as f64 / 2.0, height as f64 / 2.0);

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 3];
    for y in 0..height {
        for x in 0..width {
            // Rounded-rect signed distance. `outside` measures the corner
            // region only; the `min(max(..), 0)` term is what makes points
            // *inside* the shape negative, so a zero radius stays fully
            // opaque instead of collapsing to nothing.
            let qx = (x as f64 + 0.5 - cx).abs() - (hw - radius);
            let qy = (y as f64 + 0.5 - cy).abs() - (hh - radius);
            let outside = qx.max(0.0).hypot(qy.max(0.0));
            let distance = outside + qx.max(qy).min(0.0) - radius;
            // Half-pixel feather so the arc does not stair-step.
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            let value = (coverage * 255.0).round() as u8;
            let index = ((y * width + x) * 3) as usize;
            pixels[index] = value;
            pixels[index + 1] = value;
            pixels[index + 2] = value;
        }
    }
    GradientBitmap {
        width,
        height,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::GradientStop;

    #[test]
    fn a_gradient_honors_its_stop_positions() {
        // The whole reason this is a raster and not the ffmpeg filter: a stop
        // at 0.25 must land at a quarter of the way across.
        let gradient = VideoGradient {
            stops: vec![
                GradientStop::new(0.0, 0, 0, 0),
                GradientStop::new(0.25, 255, 255, 255),
                GradientStop::new(1.0, 255, 255, 255),
            ],
            ..VideoGradient::default()
        };
        let bitmap = render_gradient(&gradient, 100, 4);
        // Pixel centers: x = 0 is t≈0, x = 24 is t≈0.24, x = 25 is t≈0.25.
        assert_eq!(bitmap.pixel(0, 0).0, 0);
        assert_eq!(bitmap.pixel(25, 0).0, 255);
        // Halfway to the stop the ramp is only part way there.
        let midway = bitmap.pixel(12, 0).0;
        assert!(midway > 0 && midway < 255, "midway was {midway}");
    }

    #[test]
    fn a_gradient_holds_its_end_colors_past_the_ends() {
        let gradient = VideoGradient {
            stops: vec![
                GradientStop::new(0.25, 10, 20, 30),
                GradientStop::new(0.75, 200, 210, 220),
            ],
            ..VideoGradient::default()
        };
        let bitmap = render_gradient(&gradient, 100, 4);
        assert_eq!(bitmap.pixel(0, 0), (10, 20, 30));
        assert_eq!(bitmap.pixel(99, 0), (200, 210, 220));
    }

    #[test]
    fn a_reversed_gradient_mirrors_the_raster() {
        let forward = VideoGradient::default();
        let reversed = VideoGradient {
            reversed: true,
            ..VideoGradient::default()
        };
        let a = render_gradient(&forward, 64, 8);
        let b = render_gradient(&reversed, 64, 8);
        // Reversal swaps which end color each edge lands on. Sampled at pixel
        // centers the two rasters are mirrored rather than bit-identical, so
        // compare the dominant channels within a rounding step.
        let close = |left: (u8, u8, u8), right: (u8, u8, u8)| {
            let within = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 6;
            within(left.0, right.0) && within(left.1, right.1) && within(left.2, right.2)
        };
        assert!(close(a.pixel(0, 0), b.pixel(63, 0)), "{:?} vs {:?}", a.pixel(0, 0), b.pixel(63, 0));
        assert!(close(a.pixel(63, 0), b.pixel(0, 0)), "{:?} vs {:?}", a.pixel(63, 0), b.pixel(0, 0));
        // And the reversal genuinely took effect rather than being a no-op:
        // the forward ramp starts on the blue stop, the reversed one on white.
        assert!(a.pixel(0, 0).2 > a.pixel(0, 0).0, "forward should start blue");
        assert!(b.pixel(0, 0).0 > 200, "reversed should start white");
    }

    #[test]
    fn a_radial_gradient_reaches_its_last_stop_at_the_farthest_corner() {
        let gradient = VideoGradient {
            kind: GradientKind::Radial,
            ..VideoGradient::default()
        };
        let bitmap = render_gradient(&gradient, 64, 64);
        // Centre is the first stop (blue), the corner is the last (white).
        let center = bitmap.pixel(32, 32);
        assert!(center.2 > 200 && center.0 < 60, "centre was {center:?}");
        let corner = bitmap.pixel(0, 0);
        assert!(
            corner.0 > 240 && corner.1 > 240 && corner.2 > 240,
            "the farthest corner must reach the end stop, got {corner:?}"
        );
        // A point part-way out is between the stops, not one of them.
        let middle = bitmap.pixel(16, 32);
        assert!(middle.0 > 30 && middle.0 < 240, "edge was {middle:?}");
    }

    #[test]
    fn an_angular_gradient_sweeps_clockwise_from_the_angle() {
        let gradient = VideoGradient {
            kind: GradientKind::Angular,
            ..VideoGradient::default()
        };
        let bitmap = render_gradient(&gradient, 64, 64);
        // t = 0 sits just right of centre, t = 0.5 to the left; both are on
        // the horizontal axis that the angle names.
        let start = bitmap.pixel(63, 32);
        assert!(start.2 > 200 && start.0 < 60, "start was {start:?}");
        let half = bitmap.pixel(0, 32);
        assert!(half.0 > 100 && half.0 < 200, "half-turn was {half:?}");
        // Clockwise on screen runs east → south → west → north, so north sits
        // three quarters of the way through the ramp and south one quarter.
        let north = bitmap.pixel(32, 0);
        let south = bitmap.pixel(32, 63);
        assert!(
            north.0 > south.0,
            "north {north:?} must be further along the sweep than south {south:?}"
        );
    }

    #[test]
    fn a_diamond_gradient_bands_are_squares_not_circles() {
        let diamond = VideoGradient {
            kind: GradientKind::Diamond,
            ..VideoGradient::default()
        };
        let radial = VideoGradient {
            kind: GradientKind::Radial,
            ..VideoGradient::default()
        };
        let d = render_gradient(&diamond, 64, 64);
        let r = render_gradient(&radial, 64, 64);
        // Straight out from the centre to the left edge the diamond is at its
        // last stop; the radial is still part way through its ramp.
        let d_edge = d.pixel(0, 32);
        assert!(
            d_edge.0 > 240 && d_edge.1 > 240,
            "the diamond must hit its end stop on the edge midpoint, got {d_edge:?}"
        );
        assert!(
            r.pixel(0, 32).0 < 220,
            "the radial cannot already be at its end stop there"
        );
        // A square band means the whole x = 0 column is the same color; the
        // radial keeps ramping as it approaches the edge midpoint.
        assert_eq!(
            d.pixel(0, 16),
            d.pixel(0, 32),
            "the diamond's bands are axis-aligned squares"
        );
        assert!(
            r.pixel(0, 16).0 > r.pixel(0, 32).0,
            "the radial keeps ramping along the same column"
        );
    }

    #[test]
    fn a_translucent_gradient_fades_toward_black() {
        // Stops composite over the canvas's black backdrop, so a half-opaque
        // blue must render at half luminance rather than as if it were solid.
        let gradient = VideoGradient {
            stops: vec![
                GradientStop::rgba(0.0, 0, 144, 255, 128),
                GradientStop::rgba(1.0, 0, 144, 255, 128),
            ],
            ..VideoGradient::default()
        };
        let bitmap = render_gradient(&gradient, 16, 4);
        let pixel = bitmap.pixel(8, 2);
        assert!(
            (pixel.1 as i32 - 72).abs() <= 2 && (pixel.2 as i32 - 128).abs() <= 2,
            "alpha 128 over black should halve the color, got {pixel:?}"
        );
    }

    #[test]
    fn a_rounded_mask_is_opaque_in_the_middle_and_clear_in_the_corners() {
        let mask = render_rounded_mask(64, 32, 12.0);
        assert_eq!(mask.pixel(32, 16), (255, 255, 255));
        // The extreme corner is outside the arc.
        assert_eq!(mask.pixel(0, 0), (0, 0, 0));
        // Straight edges away from the corners stay fully opaque.
        assert_eq!(mask.pixel(0, 16), (255, 255, 255));
    }

    #[test]
    fn a_zero_radius_mask_is_fully_opaque() {
        let mask = render_rounded_mask(32, 32, 0.0);
        assert!(mask.pixels.iter().all(|byte| *byte == 255));
    }

    #[test]
    fn a_radius_past_the_half_extent_is_clamped() {
        // A radius far larger than the card must not turn it inside out.
        // Clamped to half the short edge, the shape becomes an ellipse that
        // still covers the centre and still feathers at the corners.
        let mask = render_rounded_mask(40, 20, 999.0);
        // Fully covered at the centre...
        assert!(mask.pixel(20, 10).0 > 250, "{}", mask.pixel(20, 10).0);
        // ...and clear in the corners, which the clamp must not swallow.
        assert_eq!(mask.pixel(0, 0), (0, 0, 0));
        // The edge midpoints sit on the ellipse, so they stay essentially opaque.
        assert!(mask.pixel(0, 10).0 > 250, "{}", mask.pixel(0, 10).0);
    }
}
