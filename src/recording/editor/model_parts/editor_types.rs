use std::path::PathBuf;

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

/// Frame rate assumed for sources whose probe cannot report one.
pub const DEFAULT_FRAME_RATE: f64 = 30.0;

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub path: PathBuf,
    pub duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    pub file_size_bytes: u64,
    pub has_audio: bool,
    /// Average source frames per second as reported by ffprobe; falls back
    /// to [`DEFAULT_FRAME_RATE`] when the source does not say.
    pub frame_rate: f64,
}

impl VideoMetadata {
    /// The rate export animations sample at: the probed frame rate, clamped
    /// to sane values, with [`DEFAULT_FRAME_RATE`] as the fallback.
    pub fn export_frame_rate(&self) -> f64 {
        if self.frame_rate.is_finite() && self.frame_rate > 0.0 {
            self.frame_rate.clamp(1.0, 240.0)
        } else {
            DEFAULT_FRAME_RATE
        }
    }
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

/// Export quality tier. The same three tiers as Settings → recording quality,
/// so an export encodes at least as sharply as the recording tier it came
/// from. `High` is the default, and `needs_reencode` compares against it
/// instead of a magic number, so an untouched export stays a stream copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportQuality {
    Balanced,
    #[default]
    High,
    Ultra,
}

impl ExportQuality {
    /// Settings tier index shared with `rec_video_quality` (`0 / 1 / 2`), so
    /// project files and both quality pickers agree on what each value means.
    pub fn tier(self) -> u8 {
        match self {
            Self::Balanced => 0,
            Self::High => 1,
            Self::Ultra => 2,
        }
    }

    /// x264 CRF, mapped through the same tiers as recording (23 / 20 / 16).
    pub fn crf(self) -> u32 {
        crate::recording::crf_for_quality(self.tier())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VideoBackground {
    None,
    Plain { r: u8, g: u8, b: u8 },
    /// A user-drawn linear gradient. Holds its own stops rather than a preset
    /// index so preview and export describe it identically.
    Gradient(VideoGradient),
    /// Any image behind the video: a bundled wallpaper or a file the user
    /// picked. The path is the only identity — nothing here says which.
    Wallpaper(PathBuf),
}

impl VideoBackground {
    pub fn is_none(&self) -> bool {
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
    Background,
    Timeline,
}
