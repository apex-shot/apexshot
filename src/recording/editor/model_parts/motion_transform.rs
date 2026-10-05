pub const DEFAULT_MOTION_DURATION_SECONDS: f64 = 6.0;
pub const MIN_MOTION_DURATION_SECONDS: f64 = 1.0;
pub const MAX_MOTION_DURATION_SECONDS: f64 = 10.0;
/// A new move targets 200% zoom, and the
/// remembered value is clamped to the 100%..400% range.
pub const DEFAULT_MOTION_ZOOM: f64 = 2.0;
pub const MIN_MOTION_ZOOM: f64 = 1.0;
pub const MAX_MOTION_ZOOM: f64 = 4.0;
pub const DEFAULT_MOTION_END_PERSPECTIVE: f64 = 0.18;
/// Recovered transform-timing editor defaults.
pub const DEFAULT_MOTION_TRANSITION_SECONDS: f64 = 1.2;
pub const DEFAULT_MOTION_EASING_X1: f64 = 0.25;
pub const DEFAULT_MOTION_EASING_Y1: f64 = 1.0;
pub const DEFAULT_MOTION_EASING_X2: f64 = 0.50;
pub const DEFAULT_MOTION_EASING_Y2: f64 = 1.0;
/// Spring bounce bounds. Bounce is the first-peak overshoot of the move, so
/// 0 is a critically damped spring and 0.5 passes the target by half the move.
/// The ceiling keeps the oscillation readable: a 50 % overshoot still settles
/// in roughly three visible swings inside the transition window. The default
/// is a quick launch that eases into the target with only a soft round-off.
pub const MIN_MOTION_SPRING_BOUNCE: f64 = 0.0;
pub const MAX_MOTION_SPRING_BOUNCE: f64 = 0.5;
pub const DEFAULT_MOTION_SPRING_BOUNCE: f64 = 0.05;
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
/// Canvas titles are placed on the whole composition, so their reference
/// rectangle is the full stage and their position band is the full 0..1.
pub const MIN_MOTION_TEXT_CANVAS_POS: f64 = 0.0;
pub const MAX_MOTION_TEXT_CANVAS_POS: f64 = 1.0;
/// New Canvas titles open as a left-aligned paragraph a little under half the
/// composition wide; 0 keeps the legacy natural-width, card-relative title.
pub const DEFAULT_MOTION_TEXT_CANVAS_WIDTH: f64 = 0.45;
pub const DEFAULT_MOTION_TEXT_TYPEWRITER_SECONDS: f64 = 0.6;
pub const DEFAULT_MOTION_TEXT_TRANSITION_SECONDS: f64 = 0.28;
pub const MIN_MOTION_TEXT_TRANSITION_SECONDS: f64 = 0.05;
pub const MAX_MOTION_TEXT_TRANSITION_SECONDS: f64 = 3.0;
pub const DEFAULT_MOTION_TEXT_LINE_SPACING: f64 = 1.0;
pub const MIN_MOTION_TEXT_LINE_SPACING: f64 = 0.5;
pub const MAX_MOTION_TEXT_LINE_SPACING: f64 = 3.0;
pub const DEFAULT_MOTION_TEXT_LETTER_SPACING: f64 = 0.0;
pub const MIN_MOTION_TEXT_LETTER_SPACING: f64 = -0.1;
pub const MAX_MOTION_TEXT_LETTER_SPACING: f64 = 0.5;
pub const MIN_MOTION_TEXT_ROTATION: f64 = -180.0;
pub const MAX_MOTION_TEXT_ROTATION: f64 = 180.0;
pub const MAX_MOTION_TEXT_OUTLINE: f64 = 0.15;
/// Smallest paragraph-box fraction a wrapped title accepts; below this the
/// box is narrower than a couple of glyphs.
pub const MIN_MOTION_TEXT_WIDTH: f64 = 0.1;

/// Identity camera for a still or the start of a motion segment.
/// Field names follow `orientationRotation*` / `perspectiveIntensity`.
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

/// Which curve drives the Motion track's transitions. Ease is the
/// recovered cubic-Bézier timing; Spring is a physical damped oscillator whose
/// overshoot gives a move weight and a settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionTimingKind {
    #[default]
    Ease,
    Spring,
}

impl MotionTimingKind {
    pub const ALL: [Self; 2] = [Self::Ease, Self::Spring];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ease => "Ease",
            Self::Spring => "Spring",
        }
    }
}

/// Recovered `MotionEffectTransformTiming`: a transition duration
/// and cubic-Bézier control points, extended with a physical spring option.
/// ApexShot keeps one per Motion clip so each move owns its timing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionEffectTransformTiming {
    pub transition_duration: f64,
    pub easing_x1: f64,
    pub easing_y1: f64,
    pub easing_x2: f64,
    pub easing_y2: f64,
    pub kind: MotionTimingKind,
    /// First-peak overshoot of a spring transition, as a fraction of the move.
    pub spring_bounce: f64,
}

impl Default for MotionEffectTransformTiming {
    fn default() -> Self {
        Self {
            transition_duration: DEFAULT_MOTION_TRANSITION_SECONDS,
            easing_x1: DEFAULT_MOTION_EASING_X1,
            easing_y1: DEFAULT_MOTION_EASING_Y1,
            easing_x2: DEFAULT_MOTION_EASING_X2,
            easing_y2: DEFAULT_MOTION_EASING_Y2,
            kind: MotionTimingKind::Ease,
            spring_bounce: DEFAULT_MOTION_SPRING_BOUNCE,
        }
    }
}

impl MotionEffectTransformTiming {
    pub fn clamped(self) -> Self {
        let finite = |value: f64, fallback: f64| {
            if value.is_finite() {
                value
            } else {
                fallback
            }
        };
        Self {
            transition_duration: self.transition_duration.clamp(
                MIN_ZOOM_EASE_MS as f64 / 1000.0,
                MAX_ZOOM_EASE_MS as f64 / 1000.0,
            ),
            easing_x1: finite(self.easing_x1, 0.0).clamp(0.0, 1.0),
            easing_y1: finite(self.easing_y1, 0.0).clamp(0.0, 1.0),
            easing_x2: finite(self.easing_x2, 0.0).clamp(0.0, 1.0),
            easing_y2: finite(self.easing_y2, 0.0).clamp(0.0, 1.0),
            kind: self.kind,
            spring_bounce: finite(self.spring_bounce, 0.0)
                .clamp(MIN_MOTION_SPRING_BOUNCE, MAX_MOTION_SPRING_BOUNCE),
        }
    }

    /// Evaluate the transition curve at `progress` through the window. Ease
    /// returns 0..1; Spring may overshoot past 1 before it settles.
    pub fn apply(self, progress: f64) -> f64 {
        let timing = self.clamped();
        match timing.kind {
            MotionTimingKind::Ease => cubic_bezier_ease(timing, progress),
            MotionTimingKind::Spring => spring_ease(timing.spring_bounce, progress),
        }
    }
}

/// Unit-step response of a mass-spring-damper that starts at rest, evaluated
/// at `progress` through the transition window. `bounce` is the first-peak
/// overshoot as a fraction of the move, so 0 is critically damped and 0.3
/// passes the target by 30 % before settling. The natural frequency is chosen
/// so the oscillation envelope decays to [`SPRING_SETTLE_EPSILON`] by the end
/// of the window: the transition duration is the settling time, the same
/// contract as Apple's `spring(duration:bounce:)`.
fn spring_ease(bounce: f64, progress: f64) -> f64 {
    let x = progress.clamp(0.0, 1.0);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let zeta = spring_damping_ratio(bounce);
    if zeta < 1.0 {
        let settle_ln = -SPRING_SETTLE_EPSILON.ln();
        // Underdamped envelope: e^(-zeta * omega0 * t) = epsilon at t = 1.
        let omega_duration = settle_ln / zeta;
        let root = (1.0 - zeta * zeta).sqrt();
        let damped = root * omega_duration * x;
        let decay = (-zeta * omega_duration * x).exp();
        1.0 - decay * (damped.cos() + zeta / root * damped.sin())
    } else {
        // Critically damped: e^(-u) (1 + u) = epsilon at u = 6.64.
        let u = 6.64 * x;
        1.0 - (-u).exp() * (1.0 + u)
    }
}

/// Invert the first-peak overshoot `O = e^(-pi*zeta/sqrt(1-zeta^2))` so the
/// user's bounce slider reads directly as the overshoot they see.
fn spring_damping_ratio(bounce: f64) -> f64 {
    let bounce = bounce.clamp(0.0, MAX_MOTION_SPRING_BOUNCE);
    if bounce <= 0.0 {
        return 1.0;
    }
    let ln = (1.0 / bounce).ln();
    (ln / (std::f64::consts::PI.powi(2) + ln * ln).sqrt()).clamp(0.05, 1.0)
}

/// One percent of the move's range; below this the spring has visibly settled.
const SPRING_SETTLE_EPSILON: f64 = 0.01;

/// One timed camera move. Slice 1 stores these but does not yet author them.
///
/// The field set mirrors the persisted `MotionEffectSegment` schema:
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
    /// Effect strength. This is preserved separately from zoom
    /// even though the current inspector does not expose it yet.
    pub intensity: f64,
    /// Source-artboard anchor for camera zoom, in normalized coordinates.
    pub zoom_anchor_x: f64,
    pub zoom_anchor_y: f64,
    pub is_disabled: bool,
    pub from: MotionTransform,
    pub to: MotionTransform,
    /// This clip's own transition timing. ApexShot keeps the persisted
    /// field set per move so one clip's duration or curve no longer
    /// rewrites every other move on the track.
    pub timing: MotionEffectTransformTiming,
}

impl MotionSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    fn sample(&self, time: f64) -> MotionTransform {
        if self.is_disabled {
            return self.from;
        }
        let target = self.target_transform();
        let span = self.duration();
        if span <= f64::EPSILON {
            return target;
        }
        let timing = self.timing.clamped();
        let ease = timing.transition_duration.clamp(0.0, span);
        // A Motion transform enters once then holds its end pose for the rest
        // of its clip; the eased return to the initial framing happens in the
        // gap after it (`release_after`). A zero transition means the target
        // pose applies for the whole segment.
        if ease <= f64::EPSILON {
            return target;
        }
        if time < self.start + ease {
            let alpha = ((time - self.start) / ease).clamp(0.0, 1.0);
            return lerp_transform_spring(self.from, target, timing.apply(alpha));
        }
        target
    }

    /// Segment intensity is stored separately from its camera values.
    /// Treat it as the blend from the pose entering the segment to the
    /// authored target, so zero is a true no-op and one is the full move.
    fn target_transform(&self) -> MotionTransform {
        lerp_transform(self.from, self.to, self.intensity.clamp(0.0, 1.0))
    }

    /// Pose while the camera eases back to the initial framing in the gap
    /// after this move. The gap's length sets the release speed, capped by
    /// this clip's transition duration; the final move releases over whatever
    /// time is left on the timeline.
    fn release_after(&self, time: f64, available: f64) -> MotionTransform {
        let target = self.target_transform();
        let timing = self.timing.clamped();
        let ease = timing.transition_duration.min(available.max(0.0));
        if ease <= f64::EPSILON {
            return MotionTransform::default();
        }
        let progress = ((time - self.end) / ease).clamp(0.0, 1.0);
        lerp_transform_spring(
            target,
            MotionTransform::default(),
            timing.apply(progress),
        )
    }
}
