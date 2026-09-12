pub const DEFAULT_MOTION_DURATION_SECONDS: f64 = 6.0;
pub const MIN_MOTION_DURATION_SECONDS: f64 = 1.0;
pub const MAX_MOTION_DURATION_SECONDS: f64 = 10.0;
/// Shotbase's `MotionEffectDefaults`: a new move targets 200% zoom, and the
/// remembered value is clamped to the 100%..400% range.
pub const DEFAULT_MOTION_ZOOM: f64 = 2.0;
pub const MIN_MOTION_ZOOM: f64 = 1.0;
pub const MAX_MOTION_ZOOM: f64 = 4.0;
pub const DEFAULT_MOTION_END_PERSPECTIVE: f64 = 0.18;
/// Recovered from Shotbase's Motion transform-timing editor defaults.
pub const DEFAULT_MOTION_TRANSITION_SECONDS: f64 = 1.2;
pub const DEFAULT_MOTION_EASING_X1: f64 = 0.25;
pub const DEFAULT_MOTION_EASING_Y1: f64 = 1.0;
pub const DEFAULT_MOTION_EASING_X2: f64 = 0.50;
pub const DEFAULT_MOTION_EASING_Y2: f64 = 1.0;
pub const MOTION_EXPORT_FPS: u32 = 30;
pub const MIN_MOTION_SEGMENT_SECONDS: f64 = 0.25;
pub const DEFAULT_MOTION_SEGMENT_SECONDS: f64 = 1.0;
pub const MIN_MOTION_YAW: f64 = -24.0;
pub const MAX_MOTION_YAW: f64 = 24.0;
pub const MIN_MOTION_POS: f64 = -1.0;
pub const MAX_MOTION_POS: f64 = 1.0;
pub const DEFAULT_MOTION_TEXT_SECONDS: f64 = 1.0;
pub const DEFAULT_MOTION_TEXT_POS_X: f64 = 0.5;
pub const DEFAULT_MOTION_TEXT_POS_Y: f64 = 0.78;
pub const MIN_MOTION_TEXT_POS: f64 = 0.05;
pub const MAX_MOTION_TEXT_POS: f64 = 0.95;
pub const DEFAULT_MOTION_TEXT_SIZE: f64 = 1.0;
pub const MIN_MOTION_TEXT_SIZE: f64 = 0.5;
pub const MAX_MOTION_TEXT_SIZE: f64 = 2.2;

/// Identity camera for a still or the start of a motion segment.
/// Field names follow Shotbase `orientationRotation*` / `perspectiveIntensity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTransform {
    pub scale: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub perspective: f64,
    pub pos_x: f64,
    pub pos_y: f64,
}

impl Default for MotionTransform {
    fn default() -> Self {
        Self {
            scale: 1.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            perspective: 0.0,
            pos_x: 0.0,
            pos_y: 0.0,
        }
    }
}

/// Shotbase's global `MotionEffectTransformTiming`: a transition duration and
/// cubic-Bézier control points shared by the Motion effects track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEffectTransformTiming {
    pub transition_duration: f64,
    pub easing_x1: f64,
    pub easing_y1: f64,
    pub easing_x2: f64,
    pub easing_y2: f64,
}

impl Default for MotionEffectTransformTiming {
    fn default() -> Self {
        Self {
            transition_duration: DEFAULT_MOTION_TRANSITION_SECONDS,
            easing_x1: DEFAULT_MOTION_EASING_X1,
            easing_y1: DEFAULT_MOTION_EASING_Y1,
            easing_x2: DEFAULT_MOTION_EASING_X2,
            easing_y2: DEFAULT_MOTION_EASING_Y2,
        }
    }
}

impl MotionEffectTransformTiming {
    pub fn clamped(self) -> Self {
        Self {
            transition_duration: self.transition_duration.clamp(
                MIN_ZOOM_EASE_MS as f64 / 1000.0,
                MAX_ZOOM_EASE_MS as f64 / 1000.0,
            ),
            easing_x1: self.easing_x1.clamp(0.0, 1.0),
            easing_y1: self.easing_y1.clamp(0.0, 1.0),
            easing_x2: self.easing_x2.clamp(0.0, 1.0),
            easing_y2: self.easing_y2.clamp(0.0, 1.0),
        }
    }

    fn apply(self, progress: f64) -> f64 {
        cubic_bezier_ease(self.clamped(), progress)
    }
}

/// One timed camera move. Slice 1 stores these but does not yet author them.
///
/// The field set mirrors the recovered Shotbase `MotionEffectSegment` schema:
/// `zoomMode`, `intensity`, `zoom`, `zoomAnchorX/Y`, `positionX/Y`,
/// `rotationX/Y/Z`, and `isDisabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionZoomMode {
    /// A user-authored zoom anchor and transform.
    #[default]
    Manual,
    /// Reserved for pointer-track follow data; static image Motion has no
    /// cursor track to solve, so it currently evaluates as Manual.
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionSegment {
    pub start: f64,
    pub end: f64,
    pub zoom_mode: MotionZoomMode,
    /// Effect strength. This is preserved separately from zoom exactly as in
    /// Shotbase, even though the current inspector does not expose it yet.
    pub intensity: f64,
    /// Source-artboard anchor for camera zoom, in normalized coordinates.
    pub zoom_anchor_x: f64,
    pub zoom_anchor_y: f64,
    pub is_disabled: bool,
    pub from: MotionTransform,
    pub to: MotionTransform,
}

impl MotionSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    fn sample(&self, time: f64, transform_timing: MotionEffectTransformTiming) -> MotionTransform {
        if self.is_disabled {
            return self.from;
        }
        let target = self.target_transform();
        let span = self.duration();
        if span <= f64::EPSILON {
            return target;
        }
        // Shotbase applies one global timing curve to the entire Motion
        // effects track: `transitionDuration` with the recovered Bézier
        // control points. There is no per-segment easing.
        let ease = transform_timing
            .clamped()
            .transition_duration
            .clamp(0.0, span);
        // A Motion transform enters once then holds its end pose; unlike the
        // legacy zoom clip it is not a symmetric in/out animation. A zero
        // transition means the target pose applies for the whole segment.
        if ease <= f64::EPSILON {
            return target;
        }
        if time < self.start + ease {
            let alpha = ((time - self.start) / ease).clamp(0.0, 1.0);
            return lerp_transform(self.from, target, transform_timing.apply(alpha));
        }
        target
    }

    /// Shotbase stores segment intensity separately from its camera values.
    /// Treat it as the blend from the pose entering the segment to the
    /// authored target, so zero is a true no-op and one is the full move.
    fn target_transform(&self) -> MotionTransform {
        lerp_transform(self.from, self.to, self.intensity.clamp(0.0, 1.0))
    }
}
