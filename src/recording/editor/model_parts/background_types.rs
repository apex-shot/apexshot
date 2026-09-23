// Background fill types for the video editor.
//
// `VideoBackground` is the single source of truth for what sits behind the
// video: nothing, a solid color, a user-drawn linear gradient, or an image
// (bundled wallpaper or a file the user picked). The gradient is a real
// multi-stop spec rather than a preset index so preview and export can share
// one description of it — ffmpeg's `gradients` filter distributes stops evenly
// and animates by default, so the export renders a generated still instead.

/// Hard ceiling on gradient stops. The editor clamps to this rather than
/// silently dropping stops the user added.
pub const MAX_GRADIENT_STOPS: usize = 8;
/// A gradient needs at least two stops to have a direction.
pub const MIN_GRADIENT_STOPS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// Where the stop sits along the gradient line, 0 = start, 1 = end.
    /// Stops need not be evenly spaced; export honors these exactly.
    pub position: f64,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl GradientStop {
    pub fn new(position: f64, r: u8, g: u8, b: u8) -> Self {
        Self {
            position,
            r,
            g,
            b,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoGradient {
    pub stops: Vec<GradientStop>,
    /// 0° runs left-to-right, increasing clockwise. The preview and the
    /// exported still both derive their endpoints from this.
    pub angle_degrees: f64,
    pub reversed: bool,
}

impl Default for VideoGradient {
    fn default() -> Self {
        Self {
            stops: vec![
                GradientStop::new(0.0, 0x00, 0x90, 0xFF),
                GradientStop::new(1.0, 0xFF, 0xFF, 0xFF),
            ],
            angle_degrees: 0.0,
            reversed: false,
        }
    }
}

impl VideoGradient {
    /// Sort by position, clamp positions into 0..=1, and enforce the stop
    /// bounds. Call after any edit so downstream code can assume the shape.
    /// Two stops is always the floor: removing one past the floor is a no-op
    /// rather than an error, matching how the rest of the model normalizes.
    pub fn normalized(&self) -> Self {
        let mut stops = self.stops.clone();
        for stop in &mut stops {
            stop.position = if stop.position.is_finite() {
                stop.position.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        stops.sort_by(|a, b| {
            a.position
                .partial_cmp(&b.position)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if stops.len() > MAX_GRADIENT_STOPS {
            stops.truncate(MAX_GRADIENT_STOPS);
        }
        while stops.len() < MIN_GRADIENT_STOPS {
            // Pad by extending the nearer end so a 1-stop gradient still has
            // a direction rather than collapsing to a flat fill.
            let (position, color) = match stops.last() {
                Some(last) => (1.0, (last.r, last.g, last.b)),
                None => (1.0, (0xFF, 0xFF, 0xFF)),
            };
            stops.push(GradientStop::new(position, color.0, color.1, color.2));
        }
        Self {
            stops,
            angle_degrees: if self.angle_degrees.is_finite() {
                self.angle_degrees.rem_euclid(360.0)
            } else {
                0.0
            },
            reversed: self.reversed,
        }
    }

    /// The stops in drawing order. Reversal is a view concern, so preview and
    /// export both read the gradient through this and cannot disagree.
    pub fn draw_stops(&self) -> Vec<GradientStop> {
        let normalized = self.normalized();
        if normalized.reversed {
            normalized.stops.into_iter().rev().collect()
        } else {
            normalized.stops
        }
    }

    /// Start and end points of the gradient line across a `width` x `height`
    /// box. The line is centred on the box and long enough that the extreme
    /// corners still land on the first/last stop, so a rotated gradient still
    /// fills every pixel.
    pub fn endpoints(&self, width: f64, height: f64) -> ((f64, f64), (f64, f64)) {
        let radians = self.angle_degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        let (cx, cy) = (width / 2.0, height / 2.0);
        // Half-length of the line that reaches every corner.
        let half = (cx * cos.abs()).hypot(cy * sin.abs());
        (
            (cx - cos * half, cy - sin * half),
            (cx + cos * half, cy + sin * half),
        )
    }
}
