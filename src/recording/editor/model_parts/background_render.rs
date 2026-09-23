// Still-image rendering for the video editor's background fills.
//
// Preview and export must agree on what a gradient or a rounded card looks
// like, so both render from the functions here rather than each describing
// the background in their own terms. ffmpeg's `gradients` filter is not used:
// it distributes stops evenly (silently dropping a dragged position) and
// animates by default. Export writes these stills to disk and loops them.

use super::{GradientStop, VideoGradient};

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
/// which is what a linear gradient does everywhere else in the app.
pub fn render_gradient(gradient: &VideoGradient, width: u32, height: u32) -> GradientBitmap {
    let width = width.max(1);
    let height = height.max(1);
    let stops = gradient.draw_stops();
    let ((x0, y0), (x1, y1)) = gradient.endpoints(width as f64, height as f64);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length_sq = dx * dx + dy * dy;

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 3];
    for y in 0..height {
        for x in 0..width {
            // Project the pixel onto the gradient line, normalized to 0..=1.
            let t = if length_sq > f64::EPSILON {
                let t = ((x as f64 - x0) * dx + (y as f64 - y0) * dy) / length_sq;
                t.clamp(0.0, 1.0)
            } else {
                0.0
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

/// Color at `t` along the stop list, holding the end colors beyond the ends.
fn sample_stops(stops: &[GradientStop], t: f64) -> (u8, u8, u8) {
    let first = match stops.first() {
        Some(stop) => stop,
        None => return (0, 0, 0),
    };
    let last = stops.last().unwrap_or(first);
    if t <= first.position {
        return (first.r, first.g, first.b);
    }
    if t >= last.position {
        return (last.r, last.g, last.b);
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
                (from as f64 + (to as f64 - from as f64) * local).round().clamp(0.0, 255.0) as u8
            };
            return (mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b));
        }
    }
    (last.r, last.g, last.b)
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
