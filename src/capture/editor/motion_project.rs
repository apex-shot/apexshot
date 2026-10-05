//! Motion edit sidecar for the capture-image Motion mode.
//!
//! Stored under `~/.local/share/apexshot/motion-projects/{sha256(source path)}.json`,
//! the same keyed-by-source layout the recording editor uses for
//! `video-projects`. Motion state otherwise lives only in memory, so without
//! this a zoom/text/appearance setup is lost the moment the editor closes.
//!
//! The `*File` DTOs below mirror the in-memory motion types in
//! `recording::editor::model` rather than deriving serde on those types
//! directly: the model carries renderer-only fields and is free to change,
//! while a persisted schema must stay stable. Every field added after v1 gets
//! `#[serde(default)]` so old files keep loading, and [`MotionProjectFile`]
//! reserves `title` from the start — user-chosen naming can be wired up later
//! without a schema migration.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::capture::editor::types::FrameStyle;
use crate::recording::editor::model::{
    GradientStop, MotionAppearance, MotionBackgroundFillType, MotionBlurSettings,
    MotionEffectTransformTiming, MotionFrame, MotionFramePreset, MotionSceneShadow,
    MotionSceneShadowPlacement, MotionSceneShadowPreset, MotionSegment, MotionState,
    MotionTextAlignment, MotionTextAnimation, MotionTextCoordinateSpace, MotionTextFormat,
    MotionTextScope, MotionTextSegment, MotionTimingKind, MotionTransform, MotionWatermark,
    MotionZoomMode, VideoGradient, DEFAULT_MOTION_TEXT_TRANSITION_SECONDS,
};

pub const MOTION_PROJECT_VERSION: u32 = 1;

fn motion_projects_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("motion-projects")
}

fn canonical_source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Sidecar path for the Motion edits authored against `path`.
pub fn project_path_for_image(path: &Path) -> PathBuf {
    let canonical = canonical_source_path(path);
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    motion_projects_directory().join(format!("{hash}.json"))
}

fn source_fingerprint(path: &Path) -> Option<(u64, i64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((meta.len(), mtime))
}

// --- DTOs -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionBackgroundFillTypeFile {
    #[default]
    None,
    Color,
    Gradient,
    Wallpaper,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionFramePresetFile {
    #[default]
    Standard,
    Instagram,
    X,
    YouTube,
    SixteenNine,
    ThreeTwo,
    FourThree,
    FiveFour,
    OneOne,
    FourFive,
    ThreeFour,
    TwoThree,
    NineSixteen,
    TenTwentyOne,
    YouTubeBanner,
    YouTubeThumbnail,
    YouTubeVideo,
    TwitterTweet,
    TwitterCover,
    InstagramPost,
    InstagramPortrait,
    InstagramStory,
    PinterestLong,
    PinterestOptimal,
    PinterestSquare,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionSceneShadowPresetFile {
    #[default]
    None,
    Diagonal,
    Window,
    Top,
    Bottom,
    Vignette,
    Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionSceneShadowPlacementFile {
    #[default]
    Underlay,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionTimingKindFile {
    #[default]
    Ease,
    Spring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionZoomModeFile {
    #[default]
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionTextAnimationFile {
    #[default]
    None,
    Typewriter,
    SlideFromLeft,
    SlideFromRight,
    SlideTop,
    SlideBottom,
    Fade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionTextScopeFile {
    #[default]
    Character,
    Word,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionTextCoordinateSpaceFile {
    #[default]
    MotionCanvasLocal,
    CanonicalSource,
    Canvas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionTextAlignmentFile {
    Left,
    #[default]
    Center,
    Right,
}

/// A title's typography. Every field defaults to the legacy card title, so a
/// sidecar written before formatting existed loads with the styling it had.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTextFormatFile {
    #[serde(default = "default_text_font_family")]
    pub font_family: String,
    #[serde(default = "default_true")]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default = "default_text_color")]
    pub color: [f64; 4],
    #[serde(default)]
    pub alignment: MotionTextAlignmentFile,
    #[serde(default)]
    pub wrap_width: f64,
    #[serde(default = "default_one")]
    pub line_spacing: f64,
    #[serde(default)]
    pub letter_spacing: f64,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub outline_width: f64,
    #[serde(default = "default_true")]
    pub shadow: bool,
}

impl Default for MotionTextFormatFile {
    fn default() -> Self {
        Self {
            font_family: default_text_font_family(),
            bold: true,
            italic: false,
            color: default_text_color(),
            alignment: MotionTextAlignmentFile::Center,
            wrap_width: 0.0,
            line_spacing: 1.0,
            letter_spacing: 0.0,
            rotation: 0.0,
            outline_width: 0.0,
            shadow: true,
        }
    }
}

fn default_text_font_family() -> String {
    crate::typography::UI_FONT_FAMILY.to_string()
}

fn default_true() -> bool {
    true
}

fn default_one() -> f64 {
    1.0
}

fn default_text_color() -> [f64; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_text_transition_seconds() -> f64 {
    DEFAULT_MOTION_TEXT_TRANSITION_SECONDS
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MotionTransformFile {
    pub scale: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub perspective: f64,
    pub pos_x: f64,
    pub pos_y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct MotionTimingFile {
    pub transition_duration: f64,
    pub easing_x1: f64,
    pub easing_y1: f64,
    pub easing_x2: f64,
    pub easing_y2: f64,
    pub kind: MotionTimingKindFile,
    pub spring_bounce: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionSegmentFile {
    pub start: f64,
    pub end: f64,
    pub zoom_mode: MotionZoomModeFile,
    pub intensity: f64,
    pub zoom_anchor_x: f64,
    pub zoom_anchor_y: f64,
    pub is_disabled: bool,
    pub from: MotionTransformFile,
    pub to: MotionTransformFile,
    pub timing: MotionTimingFile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTextSegmentFile {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub animation: MotionTextAnimationFile,
    pub scope: MotionTextScopeFile,
    pub typewriter_time: f64,
    pub is_disabled: bool,
    pub annotation_coordinate_space: MotionTextCoordinateSpaceFile,
    pub pos_x: f64,
    pub pos_y: f64,
    pub size: f64,
    #[serde(default)]
    pub format: MotionTextFormatFile,
    #[serde(default = "default_text_transition_seconds")]
    pub transition_duration: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionAppearanceFile {
    pub background_padding: f64,
    pub background_fill_type: MotionBackgroundFillTypeFile,
    pub background_color: [f64; 4],
    /// The background gradient, stored in the video editor's own shape so a
    /// Motion fill and a Custom Wallpaper fill round-trip identically.
    /// Files written before this shipped carry the two flat colors below
    /// instead, so both are read and the flat pair is folded into stops.
    #[serde(default)]
    pub gradient: VideoGradient,
    #[serde(default, skip_serializing)]
    pub gradient_color_1: [f64; 4],
    #[serde(default, skip_serializing)]
    pub gradient_color_2: [f64; 4],
    pub selected_gradient_preset_index: Option<usize>,
    pub wallpaper_image_name: Option<String>,
    pub custom_background_image: Option<String>,
    pub background_blur: f64,
    pub background_noise: f64,
    pub border_radius: f64,
    pub border_thickness: f64,
    pub border_fill_color: [f64; 4],
    pub frame_style: FrameStyle,
    pub shadow_blur: f64,
    pub shadow_opacity: f64,
    pub shadow_position: (f64, f64),
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MotionWatermarkFile {
    pub image_file_name: Option<String>,
    pub size: f64,
    pub inset: f64,
    pub position: (f64, f64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionFrameFile {
    pub preset: MotionFramePresetFile,
    pub custom_width: u32,
    pub custom_height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionSceneShadowFile {
    pub preset: MotionSceneShadowPresetFile,
    pub opacity: f64,
    pub placement: MotionSceneShadowPlacementFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MotionBlurSettingsFile {
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

/// A Motion edit as persisted on disk.
///
/// `title` is deliberately present from v1 and defaults to the source file's
/// stem. Nothing sets a custom value yet; it exists so user-chosen naming can
/// land later without migrating existing files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionProjectFile {
    pub version: u32,
    pub source_path: PathBuf,
    pub source_size: u64,
    pub source_mtime_secs: i64,
    #[serde(default)]
    pub title: String,
    pub duration: f64,
    pub perspective_intensity: f64,
    pub motion_blur: f64,
    pub motion_blur_settings: MotionBlurSettingsFile,
    pub segments: Vec<MotionSegmentFile>,
    pub text_segments: Vec<MotionTextSegmentFile>,
    pub appearance: MotionAppearanceFile,
    pub watermark: MotionWatermarkFile,
    pub frame: MotionFrameFile,
    pub scene_shadow: MotionSceneShadowFile,
}

// --- conversions ----------------------------------------------------------

impl From<&MotionBackgroundFillType> for MotionBackgroundFillTypeFile {
    fn from(value: &MotionBackgroundFillType) -> Self {
        match value {
            MotionBackgroundFillType::None => Self::None,
            MotionBackgroundFillType::Color => Self::Color,
            MotionBackgroundFillType::Gradient => Self::Gradient,
            MotionBackgroundFillType::Wallpaper => Self::Wallpaper,
            MotionBackgroundFillType::Image => Self::Image,
        }
    }
}

impl From<MotionBackgroundFillTypeFile> for MotionBackgroundFillType {
    fn from(value: MotionBackgroundFillTypeFile) -> Self {
        match value {
            MotionBackgroundFillTypeFile::None => Self::None,
            MotionBackgroundFillTypeFile::Color => Self::Color,
            MotionBackgroundFillTypeFile::Gradient => Self::Gradient,
            MotionBackgroundFillTypeFile::Wallpaper => Self::Wallpaper,
            MotionBackgroundFillTypeFile::Image => Self::Image,
        }
    }
}

impl From<MotionTransform> for MotionTransformFile {
    fn from(value: MotionTransform) -> Self {
        Self {
            scale: value.scale,
            rotation_x: value.rotation_x,
            rotation_y: value.rotation_y,
            rotation_z: value.rotation_z,
            perspective: value.perspective,
            pos_x: value.pos_x,
            pos_y: value.pos_y,
        }
    }
}

impl From<MotionTransformFile> for MotionTransform {
    fn from(value: MotionTransformFile) -> Self {
        Self {
            scale: value.scale,
            rotation_x: value.rotation_x,
            rotation_y: value.rotation_y,
            rotation_z: value.rotation_z,
            perspective: value.perspective,
            pos_x: value.pos_x,
            pos_y: value.pos_y,
        }
    }
}

impl From<MotionTimingKind> for MotionTimingKindFile {
    fn from(value: MotionTimingKind) -> Self {
        match value {
            MotionTimingKind::Ease => Self::Ease,
            MotionTimingKind::Spring => Self::Spring,
        }
    }
}

impl From<MotionTimingKindFile> for MotionTimingKind {
    fn from(value: MotionTimingKindFile) -> Self {
        match value {
            MotionTimingKindFile::Ease => Self::Ease,
            MotionTimingKindFile::Spring => Self::Spring,
        }
    }
}

impl From<MotionEffectTransformTiming> for MotionTimingFile {
    fn from(value: MotionEffectTransformTiming) -> Self {
        Self {
            transition_duration: value.transition_duration,
            easing_x1: value.easing_x1,
            easing_y1: value.easing_y1,
            easing_x2: value.easing_x2,
            easing_y2: value.easing_y2,
            kind: value.kind.into(),
            spring_bounce: value.spring_bounce,
        }
    }
}

impl From<MotionTimingFile> for MotionEffectTransformTiming {
    fn from(value: MotionTimingFile) -> Self {
        Self {
            transition_duration: value.transition_duration,
            easing_x1: value.easing_x1,
            easing_y1: value.easing_y1,
            easing_x2: value.easing_x2,
            easing_y2: value.easing_y2,
            kind: value.kind.into(),
            spring_bounce: value.spring_bounce,
        }
    }
}

impl From<MotionZoomMode> for MotionZoomModeFile {
    fn from(value: MotionZoomMode) -> Self {
        match value {
            MotionZoomMode::Manual => Self::Manual,
            MotionZoomMode::Auto => Self::Auto,
        }
    }
}

impl From<MotionZoomModeFile> for MotionZoomMode {
    fn from(value: MotionZoomModeFile) -> Self {
        match value {
            MotionZoomModeFile::Manual => Self::Manual,
            MotionZoomModeFile::Auto => Self::Auto,
        }
    }
}

impl From<&MotionSegment> for MotionSegmentFile {
    fn from(value: &MotionSegment) -> Self {
        Self {
            start: value.start,
            end: value.end,
            zoom_mode: value.zoom_mode.into(),
            intensity: value.intensity,
            zoom_anchor_x: value.zoom_anchor_x,
            zoom_anchor_y: value.zoom_anchor_y,
            is_disabled: value.is_disabled,
            from: value.from.into(),
            to: value.to.into(),
            timing: value.timing.into(),
        }
    }
}

impl From<&MotionSegmentFile> for MotionSegment {
    fn from(value: &MotionSegmentFile) -> Self {
        Self {
            start: value.start,
            end: value.end,
            zoom_mode: value.zoom_mode.into(),
            intensity: value.intensity,
            zoom_anchor_x: value.zoom_anchor_x,
            zoom_anchor_y: value.zoom_anchor_y,
            is_disabled: value.is_disabled,
            from: value.from.into(),
            to: value.to.into(),
            timing: value.timing.into(),
        }
    }
}

impl From<MotionTextAnimation> for MotionTextAnimationFile {
    fn from(value: MotionTextAnimation) -> Self {
        match value {
            MotionTextAnimation::None => Self::None,
            MotionTextAnimation::Typewriter => Self::Typewriter,
            MotionTextAnimation::SlideFromLeft => Self::SlideFromLeft,
            MotionTextAnimation::SlideFromRight => Self::SlideFromRight,
            MotionTextAnimation::SlideTop => Self::SlideTop,
            MotionTextAnimation::SlideBottom => Self::SlideBottom,
            MotionTextAnimation::Fade => Self::Fade,
        }
    }
}

impl From<MotionTextAnimationFile> for MotionTextAnimation {
    fn from(value: MotionTextAnimationFile) -> Self {
        match value {
            MotionTextAnimationFile::None => Self::None,
            MotionTextAnimationFile::Typewriter => Self::Typewriter,
            MotionTextAnimationFile::SlideFromLeft => Self::SlideFromLeft,
            MotionTextAnimationFile::SlideFromRight => Self::SlideFromRight,
            MotionTextAnimationFile::SlideTop => Self::SlideTop,
            MotionTextAnimationFile::SlideBottom => Self::SlideBottom,
            MotionTextAnimationFile::Fade => Self::Fade,
        }
    }
}

impl From<MotionTextAlignment> for MotionTextAlignmentFile {
    fn from(value: MotionTextAlignment) -> Self {
        match value {
            MotionTextAlignment::Left => Self::Left,
            MotionTextAlignment::Center => Self::Center,
            MotionTextAlignment::Right => Self::Right,
        }
    }
}

impl From<MotionTextAlignmentFile> for MotionTextAlignment {
    fn from(value: MotionTextAlignmentFile) -> Self {
        match value {
            MotionTextAlignmentFile::Left => Self::Left,
            MotionTextAlignmentFile::Center => Self::Center,
            MotionTextAlignmentFile::Right => Self::Right,
        }
    }
}

impl From<&MotionTextFormat> for MotionTextFormatFile {
    fn from(value: &MotionTextFormat) -> Self {
        Self {
            font_family: value.font_family.clone(),
            bold: value.bold,
            italic: value.italic,
            color: value.color,
            alignment: value.alignment.into(),
            wrap_width: value.wrap_width,
            line_spacing: value.line_spacing,
            letter_spacing: value.letter_spacing,
            rotation: value.rotation,
            outline_width: value.outline_width,
            shadow: value.shadow,
        }
    }
}

impl From<&MotionTextFormatFile> for MotionTextFormat {
    fn from(value: &MotionTextFormatFile) -> Self {
        Self {
            font_family: value.font_family.clone(),
            bold: value.bold,
            italic: value.italic,
            color: value.color,
            alignment: value.alignment.into(),
            wrap_width: value.wrap_width,
            line_spacing: value.line_spacing,
            letter_spacing: value.letter_spacing,
            rotation: value.rotation,
            outline_width: value.outline_width,
            shadow: value.shadow,
        }
    }
}

impl From<MotionTextScope> for MotionTextScopeFile {
    fn from(value: MotionTextScope) -> Self {
        match value {
            MotionTextScope::Character => Self::Character,
            MotionTextScope::Word => Self::Word,
            MotionTextScope::Line => Self::Line,
        }
    }
}

impl From<MotionTextScopeFile> for MotionTextScope {
    fn from(value: MotionTextScopeFile) -> Self {
        match value {
            MotionTextScopeFile::Character => Self::Character,
            MotionTextScopeFile::Word => Self::Word,
            MotionTextScopeFile::Line => Self::Line,
        }
    }
}

impl From<MotionTextCoordinateSpace> for MotionTextCoordinateSpaceFile {
    fn from(value: MotionTextCoordinateSpace) -> Self {
        match value {
            MotionTextCoordinateSpace::MotionCanvasLocal => Self::MotionCanvasLocal,
            MotionTextCoordinateSpace::CanonicalSource => Self::CanonicalSource,
            MotionTextCoordinateSpace::Canvas => Self::Canvas,
        }
    }
}

impl From<MotionTextCoordinateSpaceFile> for MotionTextCoordinateSpace {
    fn from(value: MotionTextCoordinateSpaceFile) -> Self {
        match value {
            MotionTextCoordinateSpaceFile::MotionCanvasLocal => Self::MotionCanvasLocal,
            MotionTextCoordinateSpaceFile::CanonicalSource => Self::CanonicalSource,
            MotionTextCoordinateSpaceFile::Canvas => Self::Canvas,
        }
    }
}

impl From<&MotionTextSegment> for MotionTextSegmentFile {
    fn from(value: &MotionTextSegment) -> Self {
        Self {
            start: value.start,
            end: value.end,
            text: value.text.clone(),
            animation: value.animation.into(),
            scope: value.scope.into(),
            typewriter_time: value.typewriter_time,
            is_disabled: value.is_disabled,
            annotation_coordinate_space: value.annotation_coordinate_space.into(),
            pos_x: value.pos_x,
            pos_y: value.pos_y,
            size: value.size,
            format: (&value.format).into(),
            transition_duration: value.transition_duration,
        }
    }
}

impl From<&MotionTextSegmentFile> for MotionTextSegment {
    fn from(value: &MotionTextSegmentFile) -> Self {
        Self {
            start: value.start,
            end: value.end,
            text: value.text.clone(),
            animation: value.animation.into(),
            scope: value.scope.into(),
            typewriter_time: value.typewriter_time,
            is_disabled: value.is_disabled,
            annotation_coordinate_space: value.annotation_coordinate_space.into(),
            pos_x: value.pos_x,
            pos_y: value.pos_y,
            size: value.size,
            format: (&value.format).into(),
            transition_duration: value.transition_duration,
        }
    }
}

impl From<&MotionAppearance> for MotionAppearanceFile {
    fn from(value: &MotionAppearance) -> Self {
        Self {
            background_padding: value.background_padding,
            background_fill_type: (&value.background_fill_type).into(),
            background_color: value.background_color,
            gradient: value.gradient.clone(),
            // Skipped on write; the zeros match serde's default so a re-read
            // still reads as "no legacy pair present".
            gradient_color_1: [0.0, 0.0, 0.0, 0.0],
            gradient_color_2: [0.0, 0.0, 0.0, 0.0],
            selected_gradient_preset_index: value.selected_gradient_preset_index,
            wallpaper_image_name: value.wallpaper_image_name.clone(),
            custom_background_image: value.custom_background_image.clone(),
            background_blur: value.background_blur,
            background_noise: value.background_noise,
            border_radius: value.border_radius,
            border_thickness: value.border_thickness,
            border_fill_color: value.border_fill_color,
            frame_style: value.frame_style,
            shadow_blur: value.shadow_blur,
            shadow_opacity: value.shadow_opacity,
            shadow_position: value.shadow_position,
        }
    }
}

impl From<&MotionAppearanceFile> for MotionAppearance {
    fn from(value: &MotionAppearanceFile) -> Self {
        let gradient = gradient_from_file(value);
        Self {
            background_padding: value.background_padding,
            background_fill_type: value.background_fill_type.into(),
            background_color: value.background_color,
            gradient,
            selected_gradient_preset_index: value.selected_gradient_preset_index,
            wallpaper_image_name: value.wallpaper_image_name.clone(),
            custom_background_image: value.custom_background_image.clone(),
            background_blur: value.background_blur,
            background_noise: value.background_noise,
            border_radius: value.border_radius,
            border_thickness: value.border_thickness,
            border_fill_color: value.border_fill_color,
            frame_style: value.frame_style,
            shadow_blur: value.shadow_blur,
            shadow_opacity: value.shadow_opacity,
            shadow_position: value.shadow_position,
        }
    }
}

/// The file's gradient, or the legacy two-color pair folded into a two-stop
/// ramp. `gradient` defaults to the model default when the field is absent, so
/// a pre-migration file is recognized by its flat colors still being present.
fn gradient_from_file(value: &MotionAppearanceFile) -> VideoGradient {
    let untouched = value.gradient == VideoGradient::default();
    // The legacy fields are all-zero when absent, so a non-zero pair is a real
    // pre-migration gradient rather than the serde default.
    let legacy_pair = value.gradient_color_1 != [0.0, 0.0, 0.0, 0.0]
        || value.gradient_color_2 != [0.0, 0.0, 0.0, 0.0];
    if untouched && legacy_pair {
        let stop = |position: f64, color: [f64; 4]| {
            let component = |channel: f64| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            GradientStop::rgba(
                position,
                component(color[0]),
                component(color[1]),
                component(color[2]),
                component(color[3]),
            )
        };
        return VideoGradient {
            stops: vec![
                stop(0.0, value.gradient_color_1),
                stop(1.0, value.gradient_color_2),
            ],
            ..VideoGradient::default()
        };
    }
    value.gradient.clone()
}

impl From<&MotionWatermark> for MotionWatermarkFile {
    fn from(value: &MotionWatermark) -> Self {
        Self {
            image_file_name: value.image_file_name.clone(),
            size: value.size,
            inset: value.inset,
            position: value.position,
        }
    }
}

impl From<&MotionWatermarkFile> for MotionWatermark {
    fn from(value: &MotionWatermarkFile) -> Self {
        Self {
            image_file_name: value.image_file_name.clone(),
            size: value.size,
            inset: value.inset,
            position: value.position,
        }
    }
}

impl From<MotionFramePreset> for MotionFramePresetFile {
    fn from(value: MotionFramePreset) -> Self {
        match value {
            MotionFramePreset::Standard => Self::Standard,
            MotionFramePreset::Instagram => Self::Instagram,
            MotionFramePreset::X => Self::X,
            MotionFramePreset::YouTube => Self::YouTube,
            MotionFramePreset::SixteenNine => Self::SixteenNine,
            MotionFramePreset::ThreeTwo => Self::ThreeTwo,
            MotionFramePreset::FourThree => Self::FourThree,
            MotionFramePreset::FiveFour => Self::FiveFour,
            MotionFramePreset::OneOne => Self::OneOne,
            MotionFramePreset::FourFive => Self::FourFive,
            MotionFramePreset::ThreeFour => Self::ThreeFour,
            MotionFramePreset::TwoThree => Self::TwoThree,
            MotionFramePreset::NineSixteen => Self::NineSixteen,
            MotionFramePreset::TenTwentyOne => Self::TenTwentyOne,
            MotionFramePreset::YouTubeBanner => Self::YouTubeBanner,
            MotionFramePreset::YouTubeThumbnail => Self::YouTubeThumbnail,
            MotionFramePreset::YouTubeVideo => Self::YouTubeVideo,
            MotionFramePreset::TwitterTweet => Self::TwitterTweet,
            MotionFramePreset::TwitterCover => Self::TwitterCover,
            MotionFramePreset::InstagramPost => Self::InstagramPost,
            MotionFramePreset::InstagramPortrait => Self::InstagramPortrait,
            MotionFramePreset::InstagramStory => Self::InstagramStory,
            MotionFramePreset::PinterestLong => Self::PinterestLong,
            MotionFramePreset::PinterestOptimal => Self::PinterestOptimal,
            MotionFramePreset::PinterestSquare => Self::PinterestSquare,
            MotionFramePreset::Custom => Self::Custom,
        }
    }
}

impl From<MotionFramePresetFile> for MotionFramePreset {
    fn from(value: MotionFramePresetFile) -> Self {
        match value {
            MotionFramePresetFile::Standard => Self::Standard,
            MotionFramePresetFile::Instagram => Self::Instagram,
            MotionFramePresetFile::X => Self::X,
            MotionFramePresetFile::YouTube => Self::YouTube,
            MotionFramePresetFile::SixteenNine => Self::SixteenNine,
            MotionFramePresetFile::ThreeTwo => Self::ThreeTwo,
            MotionFramePresetFile::FourThree => Self::FourThree,
            MotionFramePresetFile::FiveFour => Self::FiveFour,
            MotionFramePresetFile::OneOne => Self::OneOne,
            MotionFramePresetFile::FourFive => Self::FourFive,
            MotionFramePresetFile::ThreeFour => Self::ThreeFour,
            MotionFramePresetFile::TwoThree => Self::TwoThree,
            MotionFramePresetFile::NineSixteen => Self::NineSixteen,
            MotionFramePresetFile::TenTwentyOne => Self::TenTwentyOne,
            MotionFramePresetFile::YouTubeBanner => Self::YouTubeBanner,
            MotionFramePresetFile::YouTubeThumbnail => Self::YouTubeThumbnail,
            MotionFramePresetFile::YouTubeVideo => Self::YouTubeVideo,
            MotionFramePresetFile::TwitterTweet => Self::TwitterTweet,
            MotionFramePresetFile::TwitterCover => Self::TwitterCover,
            MotionFramePresetFile::InstagramPost => Self::InstagramPost,
            MotionFramePresetFile::InstagramPortrait => Self::InstagramPortrait,
            MotionFramePresetFile::InstagramStory => Self::InstagramStory,
            MotionFramePresetFile::PinterestLong => Self::PinterestLong,
            MotionFramePresetFile::PinterestOptimal => Self::PinterestOptimal,
            MotionFramePresetFile::PinterestSquare => Self::PinterestSquare,
            MotionFramePresetFile::Custom => Self::Custom,
        }
    }
}

impl From<&MotionFrame> for MotionFrameFile {
    fn from(value: &MotionFrame) -> Self {
        Self {
            preset: value.preset.into(),
            custom_width: value.custom_width,
            custom_height: value.custom_height,
        }
    }
}

impl From<&MotionFrameFile> for MotionFrame {
    fn from(value: &MotionFrameFile) -> Self {
        Self {
            preset: value.preset.into(),
            custom_width: value.custom_width,
            custom_height: value.custom_height,
        }
    }
}

impl From<MotionSceneShadowPreset> for MotionSceneShadowPresetFile {
    fn from(value: MotionSceneShadowPreset) -> Self {
        match value {
            MotionSceneShadowPreset::None => Self::None,
            MotionSceneShadowPreset::Diagonal => Self::Diagonal,
            MotionSceneShadowPreset::Window => Self::Window,
            MotionSceneShadowPreset::Top => Self::Top,
            MotionSceneShadowPreset::Bottom => Self::Bottom,
            MotionSceneShadowPreset::Vignette => Self::Vignette,
            MotionSceneShadowPreset::Side => Self::Side,
        }
    }
}

impl From<MotionSceneShadowPresetFile> for MotionSceneShadowPreset {
    fn from(value: MotionSceneShadowPresetFile) -> Self {
        match value {
            MotionSceneShadowPresetFile::None => Self::None,
            MotionSceneShadowPresetFile::Diagonal => Self::Diagonal,
            MotionSceneShadowPresetFile::Window => Self::Window,
            MotionSceneShadowPresetFile::Top => Self::Top,
            MotionSceneShadowPresetFile::Bottom => Self::Bottom,
            MotionSceneShadowPresetFile::Vignette => Self::Vignette,
            MotionSceneShadowPresetFile::Side => Self::Side,
        }
    }
}

impl From<MotionSceneShadowPlacement> for MotionSceneShadowPlacementFile {
    fn from(value: MotionSceneShadowPlacement) -> Self {
        match value {
            MotionSceneShadowPlacement::Underlay => Self::Underlay,
            MotionSceneShadowPlacement::Overlay => Self::Overlay,
        }
    }
}

impl From<MotionSceneShadowPlacementFile> for MotionSceneShadowPlacement {
    fn from(value: MotionSceneShadowPlacementFile) -> Self {
        match value {
            MotionSceneShadowPlacementFile::Underlay => Self::Underlay,
            MotionSceneShadowPlacementFile::Overlay => Self::Overlay,
        }
    }
}

impl From<&MotionSceneShadow> for MotionSceneShadowFile {
    fn from(value: &MotionSceneShadow) -> Self {
        Self {
            preset: value.preset.into(),
            opacity: value.opacity,
            placement: value.placement.into(),
        }
    }
}

impl From<&MotionSceneShadowFile> for MotionSceneShadow {
    fn from(value: &MotionSceneShadowFile) -> Self {
        Self {
            preset: value.preset.into(),
            opacity: value.opacity,
            placement: value.placement.into(),
        }
    }
}

impl From<&MotionBlurSettings> for MotionBlurSettingsFile {
    fn from(value: &MotionBlurSettings) -> Self {
        Self {
            enabled: value.enabled,
            cursor_strength: value.cursor_strength,
            zoom_strength: value.zoom_strength,
            capture_movement_strength: value.capture_movement_strength,
            shutter_angle: value.shutter_angle,
            zoom_blur_amount_multiplier: value.zoom_blur_amount_multiplier,
            zoom_blur_max_amount: value.zoom_blur_max_amount,
            transform_temporal_exposure_cap: value.transform_temporal_exposure_cap,
            transform_trail_opacity: value.transform_trail_opacity,
        }
    }
}

impl From<&MotionBlurSettingsFile> for MotionBlurSettings {
    fn from(value: &MotionBlurSettingsFile) -> Self {
        Self {
            enabled: value.enabled,
            cursor_strength: value.cursor_strength,
            zoom_strength: value.zoom_strength,
            capture_movement_strength: value.capture_movement_strength,
            shutter_angle: value.shutter_angle,
            zoom_blur_amount_multiplier: value.zoom_blur_amount_multiplier,
            zoom_blur_max_amount: value.zoom_blur_max_amount,
            transform_temporal_exposure_cap: value.transform_temporal_exposure_cap,
            transform_trail_opacity: value.transform_trail_opacity,
        }
    }
}

/// Build a persistable project from live Motion state.
pub fn to_project(motion: &MotionState, source_path: &Path) -> MotionProjectFile {
    let (source_size, source_mtime_secs) = source_fingerprint(source_path).unwrap_or((0, 0));
    let title = source_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    MotionProjectFile {
        version: MOTION_PROJECT_VERSION,
        source_path: canonical_source_path(source_path),
        source_size,
        source_mtime_secs,
        title,
        duration: motion.duration,
        perspective_intensity: motion.perspective_intensity,
        motion_blur: motion.motion_blur,
        motion_blur_settings: (&motion.motion_blur_settings).into(),
        segments: motion.segments.iter().map(Into::into).collect(),
        text_segments: motion.text_segments.iter().map(Into::into).collect(),
        appearance: (&motion.appearance).into(),
        watermark: (&motion.watermark).into(),
        frame: (&motion.frame).into(),
        scene_shadow: (&motion.scene_shadow).into(),
    }
}

impl MotionProjectFile {
    /// Restore this project's edits onto live Motion state.
    pub fn apply_to(&self, motion: &mut MotionState) {
        motion.duration = self.duration;
        motion.perspective_intensity = self.perspective_intensity;
        motion.motion_blur = self.motion_blur;
        motion.motion_blur_settings = (&self.motion_blur_settings).into();
        motion.segments = self.segments.iter().map(Into::into).collect();
        motion.text_segments = self.text_segments.iter().map(Into::into).collect();
        motion.appearance = (&self.appearance).into();
        motion.watermark = (&self.watermark).into();
        motion.frame = (&self.frame).into();
        motion.scene_shadow = (&self.scene_shadow).into();
        // Selection is transient UI state, never persisted.
        motion.selected = None;
        motion.selected_text = None;
        motion.playhead = 0.0;
    }

    /// A display name for a History card: the reserved title, falling back to
    /// the source file's stem for a file written before a title was set.
    pub fn display_name(&self) -> String {
        let title = self.title.trim();
        if !title.is_empty() {
            return title.to_string();
        }
        self.source_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// The source image is still on disk and unchanged, so the edit still
    /// applies to it. Mirrors the video editor's fingerprint check.
    pub fn source_is_current(&self) -> bool {
        match source_fingerprint(&self.source_path) {
            Some((size, mtime)) => size == self.source_size && mtime == self.source_mtime_secs,
            None => false,
        }
    }
}

// --- disk -----------------------------------------------------------------

pub fn load_project(path: &Path) -> Option<MotionProjectFile> {
    let project_path = project_path_for_image(path);
    if !project_path.exists() {
        return None;
    }
    let json = match std::fs::read_to_string(&project_path) {
        Ok(json) => json,
        Err(err) => {
            eprintln!(
                "[motion] failed to read project {}: {err}",
                project_path.display()
            );
            return None;
        }
    };
    let file: MotionProjectFile = match serde_json::from_str(&json) {
        Ok(file) => file,
        Err(err) => {
            eprintln!(
                "[motion] failed to parse project {}: {err}",
                project_path.display()
            );
            return None;
        }
    };
    match source_fingerprint(path) {
        Some((size, mtime)) if size == file.source_size && mtime == file.source_mtime_secs => {
            Some(file)
        }
        _ => None,
    }
}

pub fn save_project(path: &Path, file: &MotionProjectFile) -> std::io::Result<()> {
    let project_path = project_path_for_image(path);
    if let Some(dir) = project_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(file)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let temp_path = project_path.with_extension("json.tmp");
    let mut handle = std::fs::File::create(&temp_path)?;
    handle.write_all(json.as_bytes())?;
    handle.sync_all()?;
    std::fs::rename(&temp_path, &project_path)?;
    Ok(())
}

pub fn delete_project(path: &Path) {
    let project_path = project_path_for_image(path);
    if project_path.exists() {
        if let Err(err) = std::fs::remove_file(&project_path) {
            eprintln!(
                "[motion] failed to delete project {}: {err}",
                project_path.display()
            );
        }
    }
}

/// Restore persisted Motion edits onto `motion` when the source image still
/// matches. Called once when the editor window opens.
pub fn restore_into(motion: &mut MotionState, source_path: &Path) {
    if let Some(project) = load_project(source_path) {
        project.apply_to(motion);
    }
}

/// Whether the user authored anything worth keeping. A Motion session that is
/// still just the per-image defaults writes no sidecar and clears any stale
/// one, so opening the editor and closing it never leaves a file behind.
pub fn has_user_edits(motion: &MotionState) -> bool {
    if !motion.segments.is_empty() || !motion.text_segments.is_empty() {
        return true;
    }
    if motion.watermark.image_file_name.is_some() {
        return true;
    }
    if motion.appearance.background_fill_type != MotionBackgroundFillType::None {
        return true;
    }
    if motion.appearance.wallpaper_image_name.is_some()
        || motion.appearance.custom_background_image.is_some()
    {
        return true;
    }
    if motion.appearance.selected_gradient_preset_index.is_some() {
        return true;
    }
    if motion.frame.preset != MotionFramePreset::Standard {
        return true;
    }
    if motion.scene_shadow.preset != MotionSceneShadowPreset::None {
        return true;
    }
    false
}

/// Persist `motion` for `source_path`, or clear the sidecar when nothing was
/// authored. Mirrors `recording::editor::project::persist_video_session`.
pub fn persist_motion_session(motion: &MotionState, source_path: &Path) {
    if !has_user_edits(motion) {
        delete_project(source_path);
        return;
    }
    let project = to_project(motion, source_path);
    if let Some(existing) = load_project(source_path) {
        if existing == project {
            return;
        }
    }
    if let Err(err) = save_project(source_path, &project) {
        eprintln!(
            "[motion] failed to save project for {}: {err}",
            source_path.display()
        );
    }
}

/// Every persisted Motion project, newest first, for the History window.
///
/// Unreadable or corrupt files are skipped rather than failing the listing,
/// and entries whose source image is gone or changed are dropped: those edits
/// can no longer be applied to anything.
pub fn list_projects() -> Vec<MotionProjectFile> {
    let Ok(read_dir) = std::fs::read_dir(motion_projects_directory()) else {
        return Vec::new();
    };
    let mut projects: Vec<MotionProjectFile> = read_dir
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|json| serde_json::from_str::<MotionProjectFile>(&json).ok())
        .filter(MotionProjectFile::source_is_current)
        .collect();
    projects.sort_by(|a, b| b.source_mtime_secs.cmp(&a.source_mtime_secs));
    projects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::{
        MotionTextFormat, MotionTextSegment, MotionTransform, DEFAULT_MOTION_TEXT_CANVAS_WIDTH,
        MAX_MOTION_TEXT_LETTER_SPACING, MAX_MOTION_TEXT_LINE_SPACING, MAX_MOTION_TEXT_OUTLINE,
        MAX_MOTION_TEXT_POS, MAX_MOTION_TEXT_ROTATION, MIN_MOTION_TEXT_LETTER_SPACING,
        MIN_MOTION_TEXT_LINE_SPACING, MIN_MOTION_TEXT_POS,
    };

    /// A unique source path per call: these tests write real sidecars, and a
    /// shared path would let parallel tests stomp each other's files.
    fn source_image() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "apexshot-motion-project-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::write(&path, b"not a real image, only the fingerprint matters").unwrap();
        path
    }

    #[test]
    fn default_state_has_no_user_edits() {
        assert!(!has_user_edits(&MotionState::default()));
    }

    #[test]
    fn a_segment_counts_as_a_user_edit() {
        let mut motion = MotionState::default();
        motion.segments.push(MotionSegment {
            start: 0.0,
            end: 2.0,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: 0.5,
            zoom_anchor_y: 0.5,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: 2.0,
                ..MotionTransform::default()
            },
            timing: MotionEffectTransformTiming::default(),
        });
        assert!(has_user_edits(&motion));
    }

    #[test]
    fn a_text_segment_counts_as_a_user_edit() {
        let mut motion = MotionState::default();
        motion.text_segments.push(MotionTextSegment {
            start: 0.0,
            end: 1.0,
            text: "hello".into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Word,
            typewriter_time: 0.0,
            is_disabled: false,
            annotation_coordinate_space: MotionTextCoordinateSpace::MotionCanvasLocal,
            pos_x: 0.5,
            pos_y: 0.5,
            size: 1.0,
            format: MotionTextFormat::default(),
            transition_duration: DEFAULT_MOTION_TEXT_TRANSITION_SECONDS,
        });
        assert!(has_user_edits(&motion));
    }

    #[test]
    fn a_background_fill_counts_as_a_user_edit() {
        let mut motion = MotionState::default();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
        assert!(has_user_edits(&motion));
    }

    #[test]
    fn a_round_trip_preserves_every_edit_field() {
        let source = source_image();
        let mut motion = MotionState {
            duration: 7.5,
            perspective_intensity: 0.42,
            ..MotionState::default()
        };
        motion.appearance.background_padding = 48.0;
        motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
        motion.appearance.background_color = [0.1, 0.2, 0.3, 1.0];
        motion.frame.preset = MotionFramePreset::NineSixteen;
        motion.frame.custom_width = 1080;
        motion.frame.custom_height = 1920;
        motion.scene_shadow.preset = MotionSceneShadowPreset::Diagonal;
        motion.scene_shadow.opacity = 0.5;
        motion.watermark.image_file_name = Some("mark.png".into());
        motion.segments.push(MotionSegment {
            start: 0.0,
            end: 3.0,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 0.8,
            zoom_anchor_x: 0.25,
            zoom_anchor_y: 0.75,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: 2.5,
                rotation_y: 12.0,
                ..MotionTransform::default()
            },
            timing: MotionEffectTransformTiming {
                kind: MotionTimingKind::Spring,
                spring_bounce: 0.3,
                ..MotionEffectTransformTiming::default()
            },
        });

        let project = to_project(&motion, &source);
        let mut restored = MotionState::default();
        project.apply_to(&mut restored);

        assert_eq!(restored.duration, 7.5);
        assert_eq!(restored.perspective_intensity, 0.42);
        assert_eq!(restored.appearance.background_padding, 48.0);
        assert_eq!(
            restored.appearance.background_fill_type,
            MotionBackgroundFillType::Gradient
        );
        assert_eq!(restored.frame.preset, MotionFramePreset::NineSixteen);
        assert_eq!(restored.frame.custom_height, 1920);
        assert_eq!(
            restored.scene_shadow.preset,
            MotionSceneShadowPreset::Diagonal
        );
        assert_eq!(
            restored.watermark.image_file_name.as_deref(),
            Some("mark.png")
        );
        assert_eq!(restored.segments.len(), 1);
        assert_eq!(restored.segments[0].to.scale, 2.5);
        assert_eq!(restored.segments[0].timing.kind, MotionTimingKind::Spring);
        assert_eq!(restored.segments[0].timing.spring_bounce, 0.3);
    }

    #[test]
    fn a_project_json_carries_the_reserved_title() {
        let source = source_image();
        let project = to_project(&MotionState::default(), &source);
        assert!(!project.title.is_empty());
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains("\"title\""));
    }

    #[test]
    fn display_name_prefers_the_title_over_the_file_stem() {
        let source = source_image();
        let mut project = to_project(&MotionState::default(), &source);
        project.title = "My zoom".into();
        assert_eq!(project.display_name(), "My zoom");

        project.title = "   ".into();
        assert_eq!(
            project.display_name(),
            source.file_stem().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn a_changed_source_is_not_current() {
        let source = source_image();
        let project = to_project(&MotionState::default(), &source);
        assert!(project.source_is_current());

        std::fs::write(&source, b"different contents entirely").unwrap();
        assert!(!project.source_is_current());
    }

    #[test]
    fn persist_then_load_round_trips_through_disk() {
        let source = source_image();
        delete_project(&source);

        let mut motion = MotionState::default();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        persist_motion_session(&motion, &source);

        let loaded = load_project(&source).expect("project should load");
        assert_eq!(
            loaded.appearance.background_fill_type,
            MotionBackgroundFillTypeFile::Color
        );

        delete_project(&source);
    }

    /// Sidecars written before Motion's gradient moved onto the shared
    /// `VideoGradient` carry `gradient_color_1/2` and no `gradient` field. Those
    /// files must keep loading, with the flat pair folded into a two-stop ramp
    /// rather than silently reverting to the default blue-to-white gradient.
    #[test]
    fn a_legacy_two_color_gradient_file_migrates_to_stops() {
        let json = serde_json::json!({
            "background_padding": 12.0,
            "background_fill_type": "gradient",
            "background_color": [0.0, 0.0, 0.0, 1.0],
            "gradient_color_1": [0.08, 0.12, 0.22, 1.0],
            "gradient_color_2": [0.42, 0.18, 0.54, 1.0],
            "selected_gradient_preset_index": null,
            "wallpaper_image_name": null,
            "custom_background_image": null,
            "background_blur": 0.0,
            "background_noise": 0.0,
            "border_radius": 0.0,
            "border_thickness": 0.0,
            "border_fill_color": [1.0, 1.0, 1.0, 1.0],
            "frame_style": "Default",
            "shadow_blur": 0.0,
            "shadow_opacity": 0.0,
            "shadow_position": [0.0, 0.0]
        });
        let file: MotionAppearanceFile = serde_json::from_value(json).expect("legacy file parses");
        let appearance: MotionAppearance = (&file).into();
        assert_eq!(appearance.gradient.stops.len(), 2);
        assert_eq!(appearance.gradient.stops[0].r, 20);
        assert_eq!(appearance.gradient.stops[0].g, 31);
        assert_eq!(appearance.gradient.stops[0].b, 56);
        assert_eq!(appearance.gradient.stops[1].r, 107);
        assert_eq!(appearance.gradient.stops[1].g, 46);
        assert_eq!(appearance.gradient.stops[1].b, 138);
    }

    /// A file written after this change stores `gradient`; the folded legacy
    /// pair is skipped, so the shared spec survives the round trip intact.
    #[test]
    fn the_shared_gradient_round_trips_and_omits_the_legacy_pair() {
        let mut motion = MotionState::default();
        motion.appearance.gradient = VideoGradient {
            stops: vec![
                GradientStop::rgba(0.0, 10, 20, 30, 128),
                GradientStop::new(0.4, 200, 10, 10),
                GradientStop::new(1.0, 0, 0, 255),
            ],
            angle_degrees: 135.0,
            reversed: true,
            ..VideoGradient::default()
        };
        let file = to_project(&motion, Path::new("/tmp/whatever.png"));
        let json = serde_json::to_string(&file).expect("project serializes");
        assert!(
            !json.contains("gradient_color_1"),
            "the legacy pair must not be written back"
        );
        let restored: MotionProjectFile = serde_json::from_str(&json).expect("project parses");
        let appearance: MotionAppearance = (&restored.appearance).into();
        assert_eq!(appearance.gradient.stops.len(), 3);
        assert_eq!(appearance.gradient.stops[0].a, 128);
        assert_eq!(appearance.gradient.angle_degrees, 135.0);
        assert!(appearance.gradient.reversed);
    }

    /// A title sidecar written before formatting and attachments existed must
    /// keep its card staging and its legacy styling.
    #[test]
    fn a_legacy_title_file_loads_with_legacy_styling() {
        let json = serde_json::json!({
            "start": 0.0,
            "end": 1.0,
            "text": "Old title",
            "animation": "slide_from_left",
            "scope": "word",
            "typewriter_time": 0.6,
            "is_disabled": false,
            "annotation_coordinate_space": "motion_canvas_local",
            "pos_x": 0.5,
            "pos_y": 0.78,
            "size": 1.0
        });
        let file: MotionTextSegmentFile =
            serde_json::from_value(json).expect("legacy title parses");
        let segment: MotionTextSegment = (&file).into();
        assert_eq!(
            segment.annotation_coordinate_space,
            MotionTextCoordinateSpace::MotionCanvasLocal
        );
        assert!(!segment.annotation_coordinate_space.is_canvas());
        assert_eq!(segment.format, MotionTextFormat::default());
        assert_eq!(segment.format.alignment, MotionTextAlignment::Center);
        assert!((segment.format.wrap_width - 0.0).abs() < f64::EPSILON);
        assert!(segment.format.bold && segment.format.shadow);
        assert_eq!(segment.format.color, [1.0, 1.0, 1.0, 1.0]);
        assert!(
            (segment.transition_duration - DEFAULT_MOTION_TEXT_TRANSITION_SECONDS).abs()
                < f64::EPSILON
        );
    }

    /// A Canvas headline stores and restores every formatting field, including
    /// the Fade entrance.
    #[test]
    fn a_canvas_title_round_trips_every_formatting_field() {
        let source = source_image();
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        motion.set_selected_text_value("Headline\nover two lines".into());
        motion.set_selected_text_animation(MotionTextAnimation::Fade);
        motion.set_selected_text_scope(MotionTextScope::Line);
        motion.set_selected_text_transition_duration(1.5);
        motion.set_selected_text_typewriter_time(0.9);
        motion.set_selected_text_pos(0.12, 0.34);
        motion.set_selected_text_size(1.4);
        motion.set_selected_text_font_family("DejaVu Sans".into());
        motion.set_selected_text_bold(false);
        motion.set_selected_text_italic(true);
        motion.set_selected_text_color([0.2, 0.4, 0.6, 0.8]);
        motion.set_selected_text_alignment(MotionTextAlignment::Right);
        motion.set_selected_text_wrap_width(0.6);
        motion.set_selected_text_line_spacing(1.4);
        motion.set_selected_text_letter_spacing(0.08);
        motion.set_selected_text_rotation(35.0);
        motion.set_selected_text_outline_width(0.05);
        motion.set_selected_text_shadow(true);
        let expected = motion.text_segments[index].clone();

        let project = to_project(&motion, &source);
        let json = serde_json::to_string(&project).expect("project serializes");
        assert!(json.contains("\"canvas\"") && json.contains("\"fade\""));
        let restored: MotionProjectFile = serde_json::from_str(&json).expect("project parses");
        let mut loaded = MotionState::default();
        restored.apply_to(&mut loaded);

        assert_eq!(loaded.text_segments, vec![expected]);
        assert_eq!(
            loaded.text_segments[0].format.alignment,
            MotionTextAlignment::Right
        );
        assert_eq!(
            loaded.text_segments[0].annotation_coordinate_space,
            MotionTextCoordinateSpace::Canvas
        );
    }

    /// A new title is a composition headline: Canvas staging, a left-aligned
    /// paragraph box, no card shadow — and the attachment switch is explicit
    /// about which band each reference stores.
    #[test]
    fn new_titles_are_canvas_headlines_with_their_own_position_band() {
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        let segment = &motion.text_segments[index];
        assert!(segment.annotation_coordinate_space.is_canvas());
        assert_eq!(segment.format.alignment, MotionTextAlignment::Left);
        assert_eq!(segment.format.wrap_width, DEFAULT_MOTION_TEXT_CANVAS_WIDTH);
        assert!(!segment.format.shadow);
        assert!(
            (segment.transition_duration - DEFAULT_MOTION_TEXT_TRANSITION_SECONDS).abs()
                < f64::EPSILON
        );

        motion.set_selected_text_pos(0.0, 1.0);
        assert!((motion.text_segments[index].pos_x - 0.0).abs() < 1e-9);
        assert!((motion.text_segments[index].pos_y - 1.0).abs() < 1e-9);

        motion.set_selected_text_attachment(MotionTextCoordinateSpace::CanonicalSource, 0.0, 1.0);
        let segment = &motion.text_segments[index];
        assert_eq!(
            segment.annotation_coordinate_space,
            MotionTextCoordinateSpace::MotionCanvasLocal
        );
        assert!(!segment.annotation_coordinate_space.is_canvas());
        assert!((segment.pos_x - MIN_MOTION_TEXT_POS).abs() < 1e-9);
        assert!((segment.pos_y - MAX_MOTION_TEXT_POS).abs() < 1e-9);
    }

    #[test]
    fn title_style_setters_clamp_to_their_control_bounds() {
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        motion.set_selected_text_line_spacing(9.0);
        motion.set_selected_text_letter_spacing(-4.0);
        motion.set_selected_text_rotation(900.0);
        motion.set_selected_text_outline_width(3.0);
        motion.set_selected_text_wrap_width(4.0);
        motion.set_selected_text_opacity(3.0);
        let format = &motion.text_segments[index].format;
        assert!((format.line_spacing - MAX_MOTION_TEXT_LINE_SPACING).abs() < f64::EPSILON);
        assert!(
            MIN_MOTION_TEXT_LINE_SPACING <= format.line_spacing
                && format.letter_spacing <= MAX_MOTION_TEXT_LETTER_SPACING
        );
        assert!((format.letter_spacing - MIN_MOTION_TEXT_LETTER_SPACING).abs() < f64::EPSILON);
        assert!((format.rotation - MAX_MOTION_TEXT_ROTATION).abs() < f64::EPSILON);
        assert!((format.outline_width - MAX_MOTION_TEXT_OUTLINE).abs() < f64::EPSILON);
        assert!((format.wrap_width - 1.0).abs() < f64::EPSILON);
        assert!((format.color[3] - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn persisting_an_untouched_session_writes_no_file() {
        let source = source_image();
        delete_project(&source);

        persist_motion_session(&MotionState::default(), &source);
        assert!(
            load_project(&source).is_none(),
            "an untouched session must not leave a sidecar behind"
        );
    }

    #[test]
    fn clearing_the_edits_removes_a_previously_written_sidecar() {
        let source = source_image();
        delete_project(&source);

        let mut motion = MotionState::default();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        persist_motion_session(&motion, &source);
        assert!(load_project(&source).is_some());

        persist_motion_session(&MotionState::default(), &source);
        assert!(load_project(&source).is_none());
    }

    #[test]
    fn different_images_do_not_share_a_sidecar() {
        let first = source_image();
        let second = source_image();
        delete_project(&first);
        delete_project(&second);

        let mut motion = MotionState::default();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        persist_motion_session(&motion, &first);

        assert!(load_project(&first).is_some());
        assert!(load_project(&second).is_none());

        delete_project(&first);
        let _ = std::fs::remove_file(&second);
    }
}
