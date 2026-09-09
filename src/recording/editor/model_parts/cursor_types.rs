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
