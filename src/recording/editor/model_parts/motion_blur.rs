/// Motion blur configuration whose field order and clamp bounds were recovered
/// from Shotbase's `MotionBlurSettings` metadata and implementation.
///
/// The temporal composition policy below is ApexShot's current policy; it is
/// deliberately not described as a byte-for-byte Shotbase reconstruction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionBlurSettings {
    pub enabled: bool,
    pub cursor_strength: f64,
    pub zoom_strength: f64,
    pub capture_movement_strength: f64,
    pub shutter_angle: f64,
    pub zoom_blur_amount_multiplier: f64,
    pub zoom_blur_max_amount: f64,
    pub transform_temporal_exposure_cap: f64,
    pub transform_trail_opacity: f64,
}

impl Default for MotionBlurSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            // These are ApexShot defaults. Shotbase's construction defaults
            // have not yet been recovered from the stripped application.
            cursor_strength: 0.4,
            zoom_strength: 0.0,
            capture_movement_strength: 0.35,
            shutter_angle: 180.0,
            zoom_blur_amount_multiplier: 1.0,
            zoom_blur_max_amount: 1.0,
            transform_temporal_exposure_cap: 1.0 / 24.0,
            transform_trail_opacity: 0.28,
        }
    }
}

/// A past transform sample contributing to the Motion trail. The current
/// transform remains sharp; these are composited oldest-first below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionBlurSample {
    pub offset_seconds: f64,
    pub opacity: f64,
}

/// Motion blur quality mode recovered from Shotbase's `MotionBlurBudgetMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionBlurBudgetMode {
    LivePreviewPlayback,
    FullQuality,
}

impl MotionBlurBudgetMode {
    /// ApexShot's raster fallback budget. In the recovered Shotbase compositor,
    /// `livePreviewPlayback` drives the 3/5 temporal CIColorMatrix path,
    /// whereas `fullQuality` uses Core Image motion/zoom filters rather than
    /// the same trail loop. We retain a small export trail here because this
    /// renderer has no Core Image equivalent.
    fn apexshot_temporal_sample_limit(self) -> usize {
        match self {
            Self::LivePreviewPlayback => 5,
            Self::FullQuality => 3,
        }
    }
}

impl MotionBlurSettings {
    pub fn clamped(self) -> Self {
        let finite = |value: f64, fallback: f64| {
            if value.is_finite() {
                value
            } else {
                fallback
            }
        };
        Self {
            enabled: self.enabled,
            cursor_strength: finite(self.cursor_strength, 0.0).clamp(0.0, 5.0),
            zoom_strength: finite(self.zoom_strength, 0.0).clamp(0.0, 5.0),
            capture_movement_strength: finite(self.capture_movement_strength, 0.0).clamp(0.0, 5.0),
            shutter_angle: finite(self.shutter_angle, 0.0).clamp(0.0, 360.0),
            zoom_blur_amount_multiplier: finite(self.zoom_blur_amount_multiplier, 0.0)
                .clamp(0.0, 3.0),
            zoom_blur_max_amount: finite(self.zoom_blur_max_amount, 0.0).clamp(0.0, 120.0),
            transform_temporal_exposure_cap: finite(self.transform_temporal_exposure_cap, 0.0)
                .clamp(0.0, 8.0),
            transform_trail_opacity: finite(self.transform_trail_opacity, 0.0).clamp(0.0, 0.4),
        }
    }

    /// Strength of the still-image camera move. Cursor/capture strengths are
    /// intentionally excluded: there is no corresponding moving source in a
    /// Motion still, so applying them would incorrectly blur a static card.
    pub fn effective_zoom_amount(self) -> f64 {
        let settings = self.clamped();
        if !settings.enabled {
            return 0.0;
        }
        (settings.zoom_strength * settings.zoom_blur_amount_multiplier)
            .min(settings.zoom_blur_max_amount)
    }

    /// Build ApexShot's bounded, past-looking raster fallback. Shotbase uses
    /// a temporal 3/5-sample path only for `livePreviewPlayback`; its full
    /// quality path applies `CIMotionBlur` and `CIZoomBlur`, neither of which
    /// is available to this Cairo renderer.
    pub fn transform_trail(
        self,
        frame_rate: f64,
        budget: MotionBlurBudgetMode,
    ) -> Vec<MotionBlurSample> {
        let settings = self.clamped();
        let amount = settings.effective_zoom_amount();
        if amount < 0.001 || settings.shutter_angle <= 0.0 || settings.transform_trail_opacity <= 0.0 {
            return Vec::new();
        }
        let frame_duration = 1.0 / frame_rate.max(1.0);
        let exposure = (frame_duration * settings.shutter_angle / 360.0)
            .min(settings.transform_temporal_exposure_cap);
        if exposure <= f64::EPSILON {
            return Vec::new();
        }
        let max_samples = budget.apexshot_temporal_sample_limit();
        let sample_count = if max_samples == 3 || amount <= 0.5 {
            3
        } else {
            max_samples
        };
        (1..=sample_count)
            .rev()
            .map(|index| {
                let progress = index as f64 / sample_count as f64;
                MotionBlurSample {
                    offset_seconds: -exposure * progress,
                    // Recent samples are stronger. The sharp current card is
                    // painted afterwards, matching Shotbase's sharp overlay.
                    opacity: settings.transform_trail_opacity * amount * (1.0 - progress * 0.65),
                }
            })
            .collect()
    }
}

fn cubic_bezier_ease(timing: MotionEffectTransformTiming, progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= f64::EPSILON || (1.0 - progress) <= f64::EPSILON {
        return progress;
    }

    let sample = |u: f64, p1: f64, p2: f64| {
        let inverse = 1.0 - u;
        3.0 * inverse * inverse * u * p1 + 3.0 * inverse * u * u * p2 + u * u * u
    };

    // The X component represents time, so solve it before sampling Y. The
    // editor constrains both X coordinates to [0, 1], making bisection stable.
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..24 {
        let midpoint = (low + high) * 0.5;
        if sample(midpoint, timing.easing_x1, timing.easing_x2) < progress {
            low = midpoint;
        } else {
            high = midpoint;
        }
    }
    sample((low + high) * 0.5, timing.easing_y1, timing.easing_y2)
}

pub(super) fn lerp_transform(from: MotionTransform, to: MotionTransform, t: f64) -> MotionTransform {
    let t = t.clamp(0.0, 1.0);
    MotionTransform {
        scale: from.scale + (to.scale - from.scale) * t,
        rotation_x: from.rotation_x + (to.rotation_x - from.rotation_x) * t,
        rotation_y: from.rotation_y + (to.rotation_y - from.rotation_y) * t,
        rotation_z: from.rotation_z + (to.rotation_z - from.rotation_z) * t,
        perspective: from.perspective + (to.perspective - from.perspective) * t,
        pos_x: from.pos_x + (to.pos_x - from.pos_x) * t,
        pos_y: from.pos_y + (to.pos_y - from.pos_y) * t,
    }
}
