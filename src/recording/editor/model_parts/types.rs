pub const MIN_TRIM_DURATION_SECONDS: f64 = 0.25;
pub(super) const MIN_DIMENSION: u32 = 64;
pub const DEFAULT_ZOOM_DURATION_SECONDS: f64 = 1.8;
pub const DEFAULT_CURSOR_HIDE_DURATION_SECONDS: f64 = 1.8;
pub const DEFAULT_ZOOM_SCALE: f64 = 1.8;
pub const DEFAULT_ZOOM_EASE_MS: u32 = 600;
pub const MIN_ZOOM_SCALE: f64 = 1.2;
pub const MAX_ZOOM_SCALE: f64 = 5.0;
pub const ZOOM_SCALE_PRESETS: [(&str, f64); 6] = [
    ("1.25×", 1.25),
    ("1.5×", 1.5),
    ("1.8×", 1.8),
    ("2.2×", 2.2),
    ("3.5×", 3.5),
    ("5×", 5.0),
];
pub const CLIP_SPEED_PRESETS: [(&str, f64); 16] = [
    ("0.25×", 0.25),
    ("0.5×", 0.5),
    ("0.75×", 0.75),
    ("1×", 1.0),
    ("1.25×", 1.25),
    ("1.5×", 1.5),
    ("2×", 2.0),
    ("2.5×", 2.5),
    ("3×", 3.0),
    ("4×", 4.0),
    ("5×", 5.0),
    ("8×", 8.0),
    ("10×", 10.0),
    ("15×", 15.0),
    ("20×", 20.0),
    ("30×", 30.0),
];
pub const MIN_CLIP_SPEED: f64 = 0.25;
pub const MAX_CLIP_SPEED: f64 = 30.0;

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub path: PathBuf,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionPreset {
    Original,
    P1080,
    P720,
    P480,
    Custom,
}

impl DimensionPreset {
    pub fn from_label(label: &str) -> Self {
        match label {
            "1920 x 1080" => Self::P1080,
            "1280 x 720" => Self::P720,
            "854 x 480" => Self::P480,
            "Custom" => Self::Custom,
            _ => Self::Original,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    Unchanged,
    Mono,
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoBackground {
    None,
    Plain { r: u8, g: u8, b: u8 },
    Gradient(usize),
}

impl VideoBackground {
    pub fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorTool {
    #[default]
    Cursor,
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorTheme {
    #[default]
    Adwaita,
    Yaru,
    White,
    Black,
    Macos,
    Tahoe,
    TahoeInverted,
    Dot,
    Figma,
}

impl CursorTheme {
    pub const ALL: [Self; 9] = [
        Self::Adwaita,
        Self::Yaru,
        Self::White,
        Self::Black,
        Self::Macos,
        Self::Tahoe,
        Self::TahoeInverted,
        Self::Dot,
        Self::Figma,
    ];

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "yaru" => Self::Yaru,
            "white" | "windows" => Self::White,
            "black" | "inverted" | "dark" => Self::Black,
            "macos" | "mac" => Self::Macos,
            "tahoe" => Self::Tahoe,
            "tahoe_inverted" | "tahoe-inverted" => Self::TahoeInverted,
            "dot" => Self::Dot,
            "figma" | "minimal" => Self::Figma,
            _ => Self::Adwaita,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adwaita => "adwaita",
            Self::Yaru => "yaru",
            Self::White => "white",
            Self::Black => "black",
            Self::Macos => "macos",
            Self::Tahoe => "tahoe",
            Self::TahoeInverted => "tahoe_inverted",
            Self::Dot => "dot",
            Self::Figma => "figma",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Adwaita => "Adwaita",
            Self::Yaru => "Yaru",
            Self::White => "White",
            Self::Black => "Black",
            Self::Macos => "macOS",
            Self::Tahoe => "Tahoe",
            Self::TahoeInverted => "Tahoe Inverted",
            Self::Dot => "Dot",
            Self::Figma => "Minimal",
        }
    }
}

pub const MIN_CURSOR_SIZE: f64 = 0.5;
pub const MAX_CURSOR_SIZE: f64 = 3.0;
pub const DEFAULT_CURSOR_SIZE: f64 = 1.0;
pub const MIN_CURSOR_SPEED: f64 = 0.25;
pub const MAX_CURSOR_SPEED: f64 = 3.0;
pub const DEFAULT_CURSOR_SPEED: f64 = 1.0;
pub const DEFAULT_CURSOR_SHADOW: f64 = 0.4;
pub const DEFAULT_CURSOR_SMOOTH: f64 = 0.35;
pub const DEFAULT_CURSOR_IDLE_MS: f64 = 800.0;
pub const DEFAULT_CLICK_INTENSITY: f64 = 0.7;
pub const DEFAULT_CURSOR_TRAIL: f64 = 0.0;
pub const DEFAULT_CURSOR_TILT: f64 = 0.0;
pub const DEFAULT_CURSOR_SWAY: f64 = 0.0;
pub const DEFAULT_CLICK_COLOR: (u8, u8, u8) = (255, 255, 255);
pub const DEFAULT_CLICK_SCALE: f64 = 1.0;
pub const DEFAULT_CLICK_OPACITY: f64 = 1.0;
pub const DEFAULT_CLICK_DURATION_MS: u32 = 320;
pub const MIN_CLICK_SCALE: f64 = 0.5;
pub const MAX_CLICK_SCALE: f64 = 2.0;
pub const MIN_CLICK_DURATION_MS: u32 = 200;
pub const MAX_CLICK_DURATION_MS: u32 = 1200;
pub const MIN_ZOOM_EASE_MS: u32 = 0;
pub const MAX_ZOOM_EASE_MS: u32 = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickEffect {
    None,
    Spotlight,
    #[default]
    Ripple,
    Echo,
}

impl ClickEffect {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "spotlight" | "pulse" => Self::Spotlight,
            "echo" => Self::Echo,
            _ => Self::Ripple,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Spotlight => "spotlight",
            Self::Ripple => "ripple",
            Self::Echo => "echo",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Spotlight => "Spotlight",
            Self::Ripple => "Ripple",
            Self::Echo => "Echo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomEasing {
    #[default]
    Glide,
    Smooth,
    Snappy,
    Linear,
}

impl ZoomEasing {
    pub const ALL: [Self; 4] = [Self::Glide, Self::Smooth, Self::Snappy, Self::Linear];

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "smooth" => Self::Smooth,
            "snappy" => Self::Snappy,
            "linear" => Self::Linear,
            _ => Self::Glide,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glide => "glide",
            Self::Smooth => "smooth",
            Self::Snappy => "snappy",
            Self::Linear => "linear",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Glide => "Glide",
            Self::Smooth => "Smooth",
            Self::Snappy => "Snappy",
            Self::Linear => "Linear",
        }
    }

    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Glide => 1.0 - (1.0 - t).powi(3),
            Self::Smooth => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Self::Snappy => 1.0 - (1.0 - t).powi(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMotionKnobs {
    pub size: f64,
    pub smooth: f64,
    pub speed: f64,
    pub trail: f64,
    pub tilt: f64,
    pub sway: f64,
}

pub const CURSOR_MOTION_FOCUSED: CursorMotionKnobs = CursorMotionKnobs {
    size: 1.0,
    smooth: 0.15,
    speed: 1.25,
    trail: 0.0,
    tilt: 0.35,
    sway: 0.0,
};

pub const CURSOR_MOTION_SMOOTH: CursorMotionKnobs = CursorMotionKnobs {
    size: 1.2,
    smooth: 0.75,
    speed: 0.75,
    trail: 0.5,
    tilt: 0.0,
    sway: 0.25,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorMotionStyle {
    Focused,
    Smooth,
}

impl CursorMotionStyle {
    pub fn knobs(self) -> CursorMotionKnobs {
        match self {
            Self::Focused => CURSOR_MOTION_FOCUSED,
            Self::Smooth => CURSOR_MOTION_SMOOTH,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Focused => "Focused",
            Self::Smooth => "Smooth",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorSettings {
    pub theme: CursorTheme,
    pub size: f64,
    pub speed: f64,
    pub shadow: f64,
    pub smooth: f64,
    pub hide_idle: bool,
    pub idle_ms: f64,
    pub click_effect: ClickEffect,
    pub click_intensity: f64,
    pub click_color: (u8, u8, u8),
    pub click_scale: f64,
    pub click_opacity: f64,
    pub click_duration_ms: u32,
    pub trail: f64,
    pub tilt: f64,
    pub sway: f64,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            theme: CursorTheme::Adwaita,
            size: DEFAULT_CURSOR_SIZE,
            speed: DEFAULT_CURSOR_SPEED,
            shadow: DEFAULT_CURSOR_SHADOW,
            smooth: DEFAULT_CURSOR_SMOOTH,
            hide_idle: false,
            idle_ms: DEFAULT_CURSOR_IDLE_MS,
            click_effect: ClickEffect::Ripple,
            click_intensity: DEFAULT_CLICK_INTENSITY,
            click_color: DEFAULT_CLICK_COLOR,
            click_scale: DEFAULT_CLICK_SCALE,
            click_opacity: DEFAULT_CLICK_OPACITY,
            click_duration_ms: DEFAULT_CLICK_DURATION_MS,
            trail: DEFAULT_CURSOR_TRAIL,
            tilt: DEFAULT_CURSOR_TILT,
            sway: DEFAULT_CURSOR_SWAY,
        }
    }
}

impl CursorSettings {
    pub fn clamped(self) -> Self {
        Self {
            theme: self.theme,
            size: self.size.clamp(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE),
            speed: self.speed.clamp(MIN_CURSOR_SPEED, MAX_CURSOR_SPEED),
            shadow: self.shadow.clamp(0.0, 1.0),
            smooth: self.smooth.clamp(0.0, 1.0),
            hide_idle: self.hide_idle,
            idle_ms: self.idle_ms.clamp(120.0, 4000.0),
            click_effect: self.click_effect,
            click_intensity: self.click_intensity.clamp(0.0, 1.0),
            click_color: self.click_color,
            click_scale: self.click_scale.clamp(MIN_CLICK_SCALE, MAX_CLICK_SCALE),
            click_opacity: self.click_opacity.clamp(0.0, 1.0),
            click_duration_ms: self
                .click_duration_ms
                .clamp(MIN_CLICK_DURATION_MS, MAX_CLICK_DURATION_MS),
            trail: self.trail.clamp(0.0, 1.0),
            tilt: self.tilt.clamp(0.0, 1.0),
            sway: self.sway.clamp(0.0, 1.0),
        }
    }

    pub fn motion_knobs(self) -> CursorMotionKnobs {
        let settings = self.clamped();
        CursorMotionKnobs {
            size: settings.size,
            smooth: settings.smooth,
            speed: settings.speed,
            trail: settings.trail,
            tilt: settings.tilt,
            sway: settings.sway,
        }
    }

    pub fn apply_motion_preset(&mut self, style: CursorMotionStyle) {
        let knobs = style.knobs();
        self.size = knobs.size;
        self.smooth = knobs.smooth;
        self.speed = knobs.speed;
        self.trail = knobs.trail;
        self.tilt = knobs.tilt;
        self.sway = knobs.sway;
    }

    pub fn matching_motion_preset(self) -> Option<CursorMotionStyle> {
        let knobs = self.motion_knobs();
        if motion_knobs_match(knobs, CURSOR_MOTION_FOCUSED) {
            Some(CursorMotionStyle::Focused)
        } else if motion_knobs_match(knobs, CURSOR_MOTION_SMOOTH) {
            Some(CursorMotionStyle::Smooth)
        } else {
            None
        }
    }

    pub fn click_window_seconds(self) -> f64 {
        self.clamped().click_duration_ms as f64 / 1000.0
    }
}

fn motion_knobs_match(a: CursorMotionKnobs, b: CursorMotionKnobs) -> bool {
    const EPS: f64 = 0.04;
    (a.size - b.size).abs() <= EPS
        && (a.smooth - b.smooth).abs() <= EPS
        && (a.speed - b.speed).abs() <= EPS
        && (a.trail - b.trail).abs() <= EPS
        && (a.tilt - b.tilt).abs() <= EPS
        && (a.sway - b.sway).abs() <= EPS
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoomClip {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    pub ease_ms: u32,
    pub easing: ZoomEasing,
    pub mode: ZoomMode,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub perspective: f64,
}

impl Default for ZoomClip {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: DEFAULT_ZOOM_DURATION_SECONDS,
            scale: DEFAULT_ZOOM_SCALE,
            center: (0.0, 0.0),
            ease_ms: DEFAULT_ZOOM_EASE_MS,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Manual,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            perspective: 0.0,
        }
    }
}

impl ZoomClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn card_pose(&self) -> MotionTransform {
        MotionTransform {
            rotation_x: self.rotation_x,
            rotation_y: self.rotation_y,
            rotation_z: self.rotation_z,
            perspective: self.perspective,
            ..MotionTransform::default()
        }
    }

    pub fn has_card_motion(&self) -> bool {
        self.rotation_x.abs() + self.rotation_y.abs() + self.rotation_z.abs() + self.perspective
            > 0.04
    }
}

pub const DEFAULT_MOTION_DURATION_SECONDS: f64 = 3.0;
pub const MIN_MOTION_DURATION_SECONDS: f64 = 1.0;
pub const MAX_MOTION_DURATION_SECONDS: f64 = 10.0;
pub const DEFAULT_MOTION_END_SCALE: f64 = 1.12;
pub const DEFAULT_MOTION_END_ROTATION_Y: f64 = 8.0;
pub const DEFAULT_MOTION_END_PERSPECTIVE: f64 = 0.18;
/// Recovered from Shotbase's Motion transform-timing editor defaults.
pub const DEFAULT_MOTION_TRANSITION_SECONDS: f64 = 1.2;
pub const DEFAULT_MOTION_EASING_X1: f64 = 0.25;
pub const DEFAULT_MOTION_EASING_Y1: f64 = 1.0;
pub const DEFAULT_MOTION_EASING_X2: f64 = 0.50;
pub const DEFAULT_MOTION_EASING_Y2: f64 = 1.0;
pub const MOTION_EXPORT_FPS: u32 = 30;
pub const MIN_MOTION_SEGMENT_SECONDS: f64 = 0.25;
pub const DEFAULT_MOTION_SEGMENT_SECONDS: f64 = 1.8;
pub const MIN_MOTION_YAW: f64 = -24.0;
pub const MAX_MOTION_YAW: f64 = 24.0;
pub const MIN_MOTION_POS: f64 = -1.0;
pub const MAX_MOTION_POS: f64 = 1.0;
pub const DEFAULT_MOTION_TEXT_SECONDS: f64 = 1.6;
pub const DEFAULT_MOTION_TEXT_POS_X: f64 = 0.5;
pub const DEFAULT_MOTION_TEXT_POS_Y: f64 = 0.78;
pub const MIN_MOTION_TEXT_POS: f64 = 0.05;
pub const MAX_MOTION_TEXT_POS: f64 = 0.95;
pub const DEFAULT_MOTION_TEXT_SIZE: f64 = 1.0;
pub const MIN_MOTION_TEXT_SIZE: f64 = 0.5;
pub const MAX_MOTION_TEXT_SIZE: f64 = 2.2;
pub const MOTION_SCALE_PRESETS: [(&str, f64); 6] = [
    ("1.0×", 1.0),
    ("1.12×", 1.12),
    ("1.25×", 1.25),
    ("1.5×", 1.5),
    ("1.8×", 1.8),
    ("2.2×", 2.2),
];

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
    pub ease_ms: u32,
    pub easing: ZoomEasing,
}

impl MotionSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// ApexShot's default still → Motion move.
    pub fn default_cinematic(duration: f64) -> Self {
        let end = DEFAULT_MOTION_SEGMENT_SECONDS
            .min(duration.max(MIN_MOTION_SEGMENT_SECONDS))
            .max(MIN_MOTION_SEGMENT_SECONDS);
        Self {
            start: 0.0,
            end,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: 0.5,
            zoom_anchor_y: 0.5,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: DEFAULT_MOTION_END_SCALE,
                rotation_y: DEFAULT_MOTION_END_ROTATION_Y,
                ..MotionTransform::default()
            },
            ease_ms: (DEFAULT_MOTION_TRANSITION_SECONDS * 1000.0) as u32,
            easing: ZoomEasing::Glide,
        }
    }

    pub fn sample(&self, time: f64) -> MotionTransform {
        if self.is_disabled {
            return self.from;
        }
        let target = self.target_transform();
        let span = self.duration();
        if span <= f64::EPSILON {
            return target;
        }
        let ease = (self.ease_ms as f64 / 1000.0).clamp(0.0, span / 2.0);
        if ease <= f64::EPSILON {
            let local = ((time - self.start) / span).clamp(0.0, 1.0);
            return lerp_transform(self.from, target, motion_easing_apply(self.easing, local));
        }
        if time < self.start + ease {
            let alpha = ((time - self.start) / ease).clamp(0.0, 1.0);
            return lerp_transform(self.from, target, motion_easing_apply(self.easing, alpha));
        }
        target
    }

    fn sample_with_timing(
        &self,
        time: f64,
        transform_timing: MotionEffectTransformTiming,
    ) -> MotionTransform {
        if self.is_disabled {
            return self.from;
        }
        let target = self.target_transform();
        let span = self.duration();
        if span <= f64::EPSILON {
            return target;
        }
        // Shotbase applies one timing curve to the entire Motion effects
        // track. The old non-Glide presets remain compatibility overrides for
        // users who have explicitly selected them in ApexShot.
        let is_shotbase_timing = self.easing == ZoomEasing::Glide;
        let configured_duration = if is_shotbase_timing {
            transform_timing.clamped().transition_duration
        } else {
            self.ease_ms as f64 / 1000.0
        };
        // A Motion transform enters once then holds its end pose; unlike the
        // legacy zoom clip it is not a symmetric in/out animation.
        let ease = configured_duration.clamp(0.0, span);
        if ease <= f64::EPSILON {
            let local = ((time - self.start) / span).clamp(0.0, 1.0);
            let progress = if is_shotbase_timing {
                transform_timing.apply(local)
            } else {
                motion_easing_apply(self.easing, local)
            };
            return lerp_transform(self.from, target, progress);
        }
        if time < self.start + ease {
            let alpha = ((time - self.start) / ease).clamp(0.0, 1.0);
            let progress = if is_shotbase_timing {
                transform_timing.apply(alpha)
            } else {
                motion_easing_apply(self.easing, alpha)
            };
            return lerp_transform(self.from, target, progress);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextAnimation {
    None,
    Typewriter,
    SlideFromLeft,
    SlideFromRight,
    SlideTop,
    SlideBottom,
}

impl MotionTextAnimation {
    /// Cases recovered from Shotbase `TextEffectPreset` metadata.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Typewriter,
        Self::SlideFromLeft,
        Self::SlideFromRight,
        Self::SlideTop,
        Self::SlideBottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Typewriter => "Typewriter",
            Self::SlideFromLeft => "From left",
            Self::SlideFromRight => "From right",
            Self::SlideTop => "From top",
            Self::SlideBottom => "From bottom",
        }
    }
}

/// Shotbase stores this as the text effect `scope` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextScope {
    Character,
    Word,
    Line,
}

impl MotionTextScope {
    /// Cases recovered from Shotbase's `TextEffectScope` metadata.
    pub const ALL: [Self; 3] = [Self::Character, Self::Word, Self::Line];

    pub fn label(self) -> &'static str {
        match self {
            Self::Character => "Character",
            Self::Word => "Word",
            Self::Line => "Line",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTextStyle {
    pub alpha: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub reveal: f64,
}

/// Where a Motion text annotation is authored. These are the two coordinate
/// spaces named by Shotbase's `MotionTextSegment.annotationCoordinateSpace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionTextCoordinateSpace {
    /// Coordinates are normalized to the moving image card.
    #[default]
    MotionCanvasLocal,
    /// Coordinates are normalized to the untransformed source image.
    CanonicalSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionTextSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub animation: MotionTextAnimation,
    pub scope: MotionTextScope,
    pub typewriter_time: f64,
    pub is_disabled: bool,
    pub annotation_coordinate_space: MotionTextCoordinateSpace,
    pub pos_x: f64,
    pub pos_y: f64,
    pub size: f64,
}

impl MotionTextSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn sample(&self, time: f64) -> Option<MotionTextStyle> {
        if self.is_disabled || time < self.start || time > self.end {
            return None;
        }
        let span = self.duration();
        if span <= f64::EPSILON {
            return Some(MotionTextStyle {
                alpha: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
                reveal: 1.0,
            });
        }
        let local = time - self.start;
        let transition = 0.28_f64.min(span / 3.0).max(0.05);
        let progress = (local / transition).clamp(0.0, 1.0);
        let entrance = 1.0 - (1.0 - progress).powi(3);
        let distance = 36.0 * (1.0 - entrance);
        let (alpha, offset_x, offset_y, reveal) = match self.animation {
            MotionTextAnimation::None => (1.0, 0.0, 0.0, 1.0),
            MotionTextAnimation::Typewriter => (
                1.0,
                0.0,
                0.0,
                (local / self.typewriter_time.max(0.05)).clamp(0.0, 1.0),
            ),
            MotionTextAnimation::SlideFromLeft => (entrance, -distance, 0.0, 1.0),
            MotionTextAnimation::SlideFromRight => (entrance, distance, 0.0, 1.0),
            MotionTextAnimation::SlideTop => (entrance, 0.0, -distance, 1.0),
            MotionTextAnimation::SlideBottom => (entrance, 0.0, distance, 1.0),
        };
        Some(MotionTextStyle { alpha, offset_x, offset_y, reveal })
    }
}

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

fn motion_easing_apply(easing: ZoomEasing, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        ZoomEasing::Linear => t,
        ZoomEasing::Glide => 1.0 - (1.0 - t).powi(3),
        ZoomEasing::Smooth => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        // Opposite of Glide so the four buttons are readable on a short ease window.
        ZoomEasing::Snappy => t.powi(3),
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

fn lerp_transform(from: MotionTransform, to: MotionTransform, t: f64) -> MotionTransform {
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

#[derive(Debug, Clone, PartialEq)]
pub struct MotionState {
    pub duration: f64,
    pub segments: Vec<MotionSegment>,
    pub playhead: f64,
    /// Shotbase's compositor-level `perspectiveIntensity`. It is deliberately
    /// independent of timed effect segments so every camera move shares one
    /// projection depth.
    pub perspective_intensity: f64,
    pub motion_blur: f64,
    pub motion_blur_settings: MotionBlurSettings,
    pub transform_timing: MotionEffectTransformTiming,
    pub selected: Option<usize>,
    pub text_segments: Vec<MotionTextSegment>,
    pub selected_text: Option<usize>,
}

impl Default for MotionState {
    fn default() -> Self {
        Self {
            duration: DEFAULT_MOTION_DURATION_SECONDS,
            segments: Vec::new(),
            playhead: 0.0,
            perspective_intensity: DEFAULT_MOTION_END_PERSPECTIVE,
            motion_blur: 0.0,
            motion_blur_settings: MotionBlurSettings::default(),
            transform_timing: MotionEffectTransformTiming::default(),
            selected: None,
            text_segments: Vec::new(),
            selected_text: None,
        }
    }
}

impl MotionState {
    pub fn clamp_duration(duration: f64) -> f64 {
        duration.clamp(MIN_MOTION_DURATION_SECONDS, MAX_MOTION_DURATION_SECONDS)
    }

    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty() || !self.text_segments.is_empty()
    }

    pub fn set_duration(&mut self, duration: f64) {
        let previous = self.duration;
        self.duration = Self::clamp_duration(duration);
        if self.playhead > self.duration {
            self.playhead = self.duration;
        }
        if self.segments.len() == 1 {
            let segment = &mut self.segments[0];
            if segment.start.abs() < 1e-6 && (segment.end - previous).abs() < 1e-6 {
                segment.end = self.duration;
            }
        }
        self.segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.segments {
            segment.end = segment.end.min(self.duration);
        }
        if let Some(index) = self.selected {
            if index >= self.segments.len() {
                self.selected = None;
            }
        }
        self.text_segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.text_segments {
            segment.end = segment.end.min(self.duration);
        }
        if let Some(index) = self.selected_text {
            if index >= self.text_segments.len() {
                self.selected_text = None;
            }
        }
        self.reconcile_effect_segments();
    }

    pub fn seed_default_cinematic(&mut self) {
        if self.segments.is_empty() {
            self.segments
                .push(MotionSegment::default_cinematic(self.duration));
            self.selected = Some(0);
            self.selected_text = None;
        }
    }

    pub fn add_segment_at(&mut self, start: f64) -> Option<usize> {
        let start = start.clamp(0.0, self.duration);
        if self.segment_index_at(start).is_some() {
            return None;
        }
        let mut end = (start + DEFAULT_MOTION_SEGMENT_SECONDS).min(self.duration);
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            let start = (self.duration - MIN_MOTION_SEGMENT_SECONDS).max(0.0);
            end = self.duration;
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
            return self.insert_segment(start, end);
        }
        if self
            .segments
            .iter()
            .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
        {
            if let Some(next_start) = self
                .segments
                .iter()
                .filter(|segment| segment.start >= start)
                .map(|segment| segment.start)
                .min_by(|a, b| a.total_cmp(b))
            {
                end = next_start;
            }
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
        }
        self.insert_segment(start, end)
    }

    fn insert_segment(&mut self, start: f64, end: f64) -> Option<usize> {
        self.segments.push(MotionSegment {
            start,
            end,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: 0.5,
            zoom_anchor_y: 0.5,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: DEFAULT_MOTION_END_SCALE,
                rotation_y: DEFAULT_MOTION_END_ROTATION_Y,
                ..MotionTransform::default()
            },
            ease_ms: (DEFAULT_MOTION_TRANSITION_SECONDS * 1000.0) as u32,
            easing: ZoomEasing::Glide,
        });
        self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.reconcile_effect_segments();
        let index = self
            .segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)?;
        self.selected = Some(index);
        self.selected_text = None;
        Some(index)
    }

    pub fn segment_index_at(&self, time: f64) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| time >= segment.start && time <= segment.end)
    }

    /// Find the nearest meaningful timeline edge for an effect segment. The
    /// visual timeline deliberately stays uncluttered; this supplies the
    /// Shotbase-style magnetic behavior behind it.
    pub fn snap_effect_time(&self, time: f64, tolerance: f64, excluding: Option<usize>) -> f64 {
        self.snap_time(time, tolerance, excluding, None)
    }

    /// Text uses the same shared timeline magnetism as camera effects, while
    /// ignoring the segment currently being dragged or trimmed.
    pub fn snap_text_time(&self, time: f64, tolerance: f64, excluding: Option<usize>) -> f64 {
        self.snap_time(time, tolerance, None, excluding)
    }

    pub fn remove_selected(&mut self) -> bool {
        if let Some(index) = self.selected_text.take() {
            if index < self.text_segments.len() {
                self.text_segments.remove(index);
                if !self.text_segments.is_empty() {
                    self.selected_text = Some(index.min(self.text_segments.len() - 1));
                }
                return true;
            }
        }
        let Some(index) = self.selected.take() else {
            return false;
        };
        if index >= self.segments.len() {
            return false;
        }
        self.segments.remove(index);
        self.reconcile_effect_segments();
        if !self.segments.is_empty() {
            self.selected = Some(index.min(self.segments.len() - 1));
        }
        true
    }

    pub fn set_segment_range(&mut self, index: usize, start: f64, end: f64) {
        if self.segments.get(index).is_none() {
            return;
        }
        let mut start = start.clamp(0.0, self.duration);
        let mut end = end.clamp(0.0, self.duration);
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            return;
        }
        if self.segments.iter().enumerate().any(|(other, clip)| {
            other != index && motion_ranges_overlap(start, end, clip.start, clip.end)
        }) {
            return;
        }
        if let Some(segment) = self.segments.get_mut(index) {
            segment.start = start;
            segment.end = end;
        }
        self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.reconcile_effect_segments();
        let index = self
            .segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)
            .unwrap_or(index.min(self.segments.len().saturating_sub(1)));
        self.selected = Some(index);
        self.selected_text = None;
    }

    pub fn move_segment(&mut self, index: usize, start: f64) {
        let Some(segment) = self.segments.get(index).cloned() else {
            return;
        };
        let span = segment.duration();
        let start = start.clamp(0.0, (self.duration - span).max(0.0));
        self.set_segment_range(index, start, start + span);
    }

    pub fn selected_segment(&self) -> Option<&MotionSegment> {
        self.selected.and_then(|index| self.segments.get(index))
    }

    pub fn selected_segment_mut(&mut self) -> Option<&mut MotionSegment> {
        self.selected.and_then(|index| self.segments.get_mut(index))
    }

    pub fn set_selected_end_scale(&mut self, scale: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.scale = scale.clamp(1.0, 3.0);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_zoom_mode(&mut self, zoom_mode: MotionZoomMode) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.zoom_mode = zoom_mode;
        }
    }

    pub fn set_selected_intensity(&mut self, intensity: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.intensity = intensity.clamp(0.0, 1.0);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_zoom_anchor(&mut self, x: f64, y: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.zoom_anchor_x = x.clamp(0.0, 1.0);
            segment.zoom_anchor_y = y.clamp(0.0, 1.0);
        }
    }

    pub fn set_selected_disabled(&mut self, is_disabled: bool) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.is_disabled = is_disabled;
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_yaw(&mut self, yaw: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_y = yaw.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_easing(&mut self, easing: ZoomEasing) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.easing = easing;
        }
    }

    pub fn set_selected_ease_ms(&mut self, ease_ms: u32) {
        let ease_ms = ease_ms.clamp(MIN_ZOOM_EASE_MS, MAX_ZOOM_EASE_MS);
        if let Some(segment) = self.selected_segment_mut() {
            segment.ease_ms = ease_ms;
        }
        self.transform_timing.transition_duration = ease_ms as f64 / 1000.0;
    }

    pub fn set_transform_timing(&mut self, timing: MotionEffectTransformTiming) {
        self.transform_timing = timing.clamped();
        let ease_ms = (self.transform_timing.transition_duration * 1000.0).round() as u32;
        if let Some(segment) = self.selected_segment_mut() {
            segment.ease_ms = ease_ms;
        }
    }

    pub fn set_selected_end_pitch(&mut self, pitch: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_x = pitch.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_roll(&mut self, roll: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_z = roll.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_perspective(&mut self, perspective: f64) {
        self.perspective_intensity = perspective.clamp(0.0, 1.0);
        let perspective_intensity = self.perspective_intensity;
        if let Some(segment) = self.selected_segment_mut() {
            // Retain this value for legacy in-memory segment consumers, but
            // rendering reads the global compositor perspective above.
            segment.to.perspective = perspective_intensity;
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_pos_x(&mut self, pos_x: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.pos_x = pos_x.clamp(MIN_MOTION_POS, MAX_MOTION_POS);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_pos_y(&mut self, pos_y: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.pos_y = pos_y.clamp(MIN_MOTION_POS, MAX_MOTION_POS);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_motion_blur(&mut self, motion_blur: f64) {
        self.motion_blur = motion_blur.clamp(0.0, 1.0);
        self.motion_blur_settings.enabled = self.motion_blur > 0.0;
        self.motion_blur_settings.zoom_strength = self.motion_blur;
    }

    /// The scalar used by ApexShot's current still-image renderer. It is a
    /// compatibility projection of Shotbase's separate blur strengths.
    pub fn effective_motion_blur(&self) -> f64 {
        if self.motion_blur_settings.enabled {
            self.motion_blur_settings.zoom_strength.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn text_index_at(&self, time: f64) -> Option<usize> {
        self.text_segments
            .iter()
            .position(|segment| time >= segment.start && time <= segment.end)
    }

    pub fn add_text_at(&mut self, start: f64) -> Option<usize> {
        let start = start.clamp(0.0, self.duration);
        if self.text_index_at(start).is_some() {
            return None;
        }
        let mut end = (start + DEFAULT_MOTION_TEXT_SECONDS).min(self.duration);
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            let start = (self.duration - MIN_MOTION_SEGMENT_SECONDS).max(0.0);
            end = self.duration;
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .text_segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
            return self.insert_text(start, end);
        }
        if self
            .text_segments
            .iter()
            .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
        {
            if let Some(next_start) = self
                .text_segments
                .iter()
                .filter(|segment| segment.start >= start)
                .map(|segment| segment.start)
                .min_by(|a, b| a.total_cmp(b))
            {
                end = next_start;
            }
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .text_segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
        }
        self.insert_text(start, end)
    }

    fn insert_text(&mut self, start: f64, end: f64) -> Option<usize> {
        self.text_segments.push(MotionTextSegment {
            start,
            end,
            text: "Title".into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Character,
            typewriter_time: 0.6,
            is_disabled: false,
            annotation_coordinate_space: MotionTextCoordinateSpace::MotionCanvasLocal,
            pos_x: DEFAULT_MOTION_TEXT_POS_X,
            pos_y: DEFAULT_MOTION_TEXT_POS_Y,
            size: DEFAULT_MOTION_TEXT_SIZE,
        });
        self.text_segments
            .sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .text_segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)?;
        self.selected_text = Some(index);
        self.selected = None;
        Some(index)
    }

    pub fn set_text_range(&mut self, index: usize, start: f64, end: f64) {
        if self.text_segments.get(index).is_none() {
            return;
        }
        let mut start = start.clamp(0.0, self.duration);
        let mut end = end.clamp(0.0, self.duration);
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            return;
        }
        if self.text_segments.iter().enumerate().any(|(other, clip)| {
            other != index && motion_ranges_overlap(start, end, clip.start, clip.end)
        }) {
            return;
        }
        if let Some(segment) = self.text_segments.get_mut(index) {
            segment.start = start;
            segment.end = end;
        }
        self.selected_text = Some(index);
        self.selected = None;
    }

    pub fn move_text(&mut self, index: usize, start: f64) {
        let Some(segment) = self.text_segments.get(index).cloned() else {
            return;
        };
        let span = segment.duration();
        let start = start.clamp(0.0, (self.duration - span).max(0.0));
        self.set_text_range(index, start, start + span);
    }

    pub fn selected_text_segment(&self) -> Option<&MotionTextSegment> {
        self.selected_text
            .and_then(|index| self.text_segments.get(index))
    }

    pub fn set_selected_text_value(&mut self, text: String) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.text = text;
            }
        }
    }

    pub fn set_selected_text_animation(&mut self, animation: MotionTextAnimation) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.animation = animation;
            }
        }
    }

    pub fn set_selected_text_scope(&mut self, scope: MotionTextScope) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.scope = scope;
            }
        }
    }

    pub fn set_selected_text_typewriter_time(&mut self, typewriter_time: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.typewriter_time = typewriter_time.max(0.0);
            }
        }
    }

    pub fn set_selected_text_disabled(&mut self, is_disabled: bool) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.is_disabled = is_disabled;
            }
        }
    }

    pub fn set_selected_text_pos(&mut self, pos_x: f64, pos_y: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.pos_x = pos_x.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS);
                segment.pos_y = pos_y.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS);
            }
        }
    }

    pub fn set_selected_text_size(&mut self, size: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.size = size.clamp(MIN_MOTION_TEXT_SIZE, MAX_MOTION_TEXT_SIZE);
            }
        }
    }

    pub fn sample(&self, time: f64) -> MotionTransform {
        let time = time.clamp(0.0, self.duration.max(0.0));
        let transform = if let Some(segment) = self
            .segments
            .iter()
            .find(|segment| time >= segment.start && time <= segment.end)
        {
            segment.sample_with_timing(time, self.transform_timing)
        } else {
            self.segments
                .iter()
                .rev()
                .find(|segment| time > segment.end && !segment.is_disabled)
                .map(MotionSegment::target_transform)
                .unwrap_or_default()
        };
        MotionTransform {
            perspective: self.perspective_intensity,
            ..transform
        }
    }

    /// Source-artboard zoom focus for the active camera move. The anchor is
    /// deliberately sampled alongside the transform because Shotbase keeps it
    /// on `MotionEffectSegment`, not in the global compositor configuration.
    pub fn zoom_anchor_at(&self, time: f64) -> (f64, f64) {
        let time = time.clamp(0.0, self.duration.max(0.0));
        self.segments
            .iter()
            .find(|segment| time >= segment.start && time <= segment.end && !segment.is_disabled)
            .or_else(|| {
                self.segments
                    .iter()
                    .rev()
                    .find(|segment| time > segment.end && !segment.is_disabled)
            })
            .map(|segment| {
                (
                    segment.zoom_anchor_x.clamp(0.0, 1.0),
                    segment.zoom_anchor_y.clamp(0.0, 1.0),
                )
            })
            .unwrap_or((0.5, 0.5))
    }

    /// Preserve a single camera through every effect segment. Shotbase stores
    /// effect segments as a reconciled track: the next segment begins at the
    /// pose left by the preceding segment, including across an intentional
    /// timeline gap. Without this, adding a second move visibly snaps the card
    /// back to its identity transform.
    fn reconcile_effect_segments(&mut self) {
        let mut previous = MotionTransform::default();
        for segment in &mut self.segments {
            segment.from = previous;
            if !segment.is_disabled {
                previous = segment.target_transform();
            }
        }
    }

    fn snap_time(
        &self,
        time: f64,
        tolerance: f64,
        excluding_effect: Option<usize>,
        excluding_text: Option<usize>,
    ) -> f64 {
        let time = time.clamp(0.0, self.duration);
        let tolerance = tolerance.max(0.0);
        let mut nearest = time;
        let mut distance = tolerance;
        let mut consider = |candidate: f64| {
            let candidate = candidate.clamp(0.0, self.duration);
            let candidate_distance = (candidate - time).abs();
            if candidate_distance <= distance {
                nearest = candidate;
                distance = candidate_distance;
            }
        };
        consider(0.0);
        consider(self.duration);
        consider(self.playhead);
        for (index, segment) in self.segments.iter().enumerate() {
            if Some(index) != excluding_effect {
                consider(segment.start);
                consider(segment.end);
            }
        }
        for (index, segment) in self.text_segments.iter().enumerate() {
            if Some(index) != excluding_text {
                consider(segment.start);
                consider(segment.end);
            }
        }
        nearest
    }
}

fn motion_ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorHideClip {
    pub start: f64,
    pub end: f64,
}

impl CursorHideClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start && t <= self.end
    }
}

pub fn format_zoom_scale(scale: f64) -> String {
    for &(label, preset) in &ZOOM_SCALE_PRESETS {
        if (scale - preset).abs() < 0.04 {
            return label.to_string();
        }
    }
    if (scale - scale.round()).abs() < 0.05 {
        format!("{:.0}×", scale.round())
    } else {
        format!("{scale:.1}×")
    }
}

pub fn nearest_zoom_preset(scale: f64) -> f64 {
    ZOOM_SCALE_PRESETS
        .iter()
        .min_by(|(_, a), (_, b)| (a - scale).abs().total_cmp(&(b - scale).abs()))
        .map(|(_, value)| *value)
        .unwrap_or(DEFAULT_ZOOM_SCALE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropSelection {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMediaKind {
    Video,
    Audio,
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMedia {
    pub path: PathBuf,
    pub display_name: String,
    pub kind: ProjectMediaKind,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct VideoEditState {
    pub metadata: VideoMetadata,
    pub trim_start_seconds: f64,
    pub trim_end_seconds: f64,
    /// Playhead on the composition ruler. Dragging a clip must not move this.
    pub playhead_seconds: f64,
    pub dimension_preset: DimensionPreset,
    pub custom_width: u32,
    pub custom_height: u32,
    pub quality: u8,
    pub audio_mode: AudioMode,
    /// Sorted list of cut points (seconds) within the trim range.
    pub cuts: Vec<f64>,
    /// Whether each segment is kept (true) or removed (false).
    /// Length is always cuts.len() + 1.
    pub segments_kept: Vec<bool>,
    /// Output order of segments (indices into segment_boundaries()).
    /// Length is always cuts.len() + 1.
    pub segment_order: Vec<usize>,
    /// Composition start of each chronological segment. Dragging a cut piece
    /// changes only its start so a gap can open between neighbors.
    pub segment_starts: Vec<f64>,
    pub segment_speeds: Vec<f64>,
    pub segment_muted: Vec<bool>,
    pub zoom_clips: Vec<ZoomClip>,
    pub selected_zoom: Option<usize>,
    pub cursor_hide_clips: Vec<CursorHideClip>,
    pub selected_cursor_hide: Option<usize>,
    pub selected_segment: Option<usize>,
    pub background: VideoBackground,
    pub background_padding: f64,
    pub background_corner_radius: f64,
    pub background_shadow: f64,
    /// Static source crop in original video pixels. `None` keeps the full frame.
    pub crop: Option<CropSelection>,
    pub sidecar: Option<PointerSidecar>,
    pub cursor: CursorSettings,
    pub selected_tool: EditorTool,
    pub project_media: Vec<ProjectMedia>,
    /// Display / export name (file stem, without extension).
    pub title: String,
    pub video_locked: bool,
    pub video_hidden: bool,
    pub audio_locked: bool,
    pub audio_removed: bool,
    pub zoom_locked: bool,
    pub zoom_hidden: bool,
    /// Classic animation keeps a fixed focus point even when the clip is Auto.
    pub zoom_classic: bool,
    /// 0 = fit the whole clip, 100 = 8× time-axis zoom (WebCut scaler).
    pub timeline_scale: f64,
    /// Seconds of empty timeline before the clip. Dragging the clip body
    /// later on the ruler increases this; it is exported as leading black.
    /// Not tied to the source duration — the ruler stays open on the right.
    pub timeline_offset_seconds: f64,
    /// Horizontal pan at fit zoom, in composition seconds. Does not change
    /// pixels-per-second; the clip stays the same size and the track scrolls.
    pub timeline_scroll_seconds: f64,
}

pub(super) fn seed_project_media(metadata: &VideoMetadata) -> Vec<ProjectMedia> {
    let name = metadata
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Recording")
        .to_string();
    let mut items = vec![ProjectMedia {
        path: metadata.path.clone(),
        display_name: name.clone(),
        kind: ProjectMediaKind::Video,
        duration_seconds: Some(metadata.duration_seconds),
    }];
    if metadata.has_audio {
        items.push(ProjectMedia {
            path: metadata.path.clone(),
            display_name: format!("{name} audio"),
            kind: ProjectMediaKind::Audio,
            duration_seconds: Some(metadata.duration_seconds),
        });
    }
    items
}
