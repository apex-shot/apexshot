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
}

impl ZoomClip {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
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
