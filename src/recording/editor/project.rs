//! Edit-list sidecar for the recording editor.
//!
//! Stored under `~/.local/share/apexshot/video-projects/{sha256(source path)}.json`.
//! Pointer samples stay under `~/.local/share/apexshot/pointer-sidecars/`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::model::{
    AudioMode, ClickEffect, CropSelection, CursorHideClip, CursorSettings, CursorTheme,
    DimensionPreset, ExportQuality, GradientKind, GradientStop, ProjectMedia, ProjectMediaKind,
    VideoBackground, VideoEditState, VideoGradient, ZoomClip, ZoomEasing, ZoomMode,
    DEFAULT_CLICK_COLOR, DEFAULT_CLICK_DURATION_MS, DEFAULT_CLICK_INTENSITY, DEFAULT_CLICK_OPACITY,
    DEFAULT_CLICK_SCALE, DEFAULT_CURSOR_IDLE_MS, DEFAULT_CURSOR_SHADOW, DEFAULT_CURSOR_SIZE,
    DEFAULT_CURSOR_SMOOTH, DEFAULT_CURSOR_SPEED, DEFAULT_CURSOR_SWAY, DEFAULT_CURSOR_TILT,
    DEFAULT_CURSOR_TRAIL,
};

pub const VIDEO_PROJECT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoProjectFile {
    pub version: u32,
    pub source_path: PathBuf,
    pub source_size: u64,
    pub source_mtime_secs: i64,
    pub title: String,
    pub trim_start_seconds: f64,
    pub trim_end_seconds: f64,
    pub cuts: Vec<f64>,
    /// Held-last-frame tail. 0 = no freeze, so old projects load clean.
    #[serde(default)]
    pub freeze_tail: f64,
    #[serde(default)]
    pub frozen_segment: Option<usize>,
    pub segments_kept: Vec<bool>,
    pub segment_order: Vec<usize>,
    pub segment_starts: Vec<f64>,
    pub segment_speeds: Vec<f64>,
    pub segment_muted: Vec<bool>,
    pub timeline_offset_seconds: f64,
    pub zoom_clips: Vec<ZoomClipFile>,
    #[serde(default)]
    pub cursor_hide_clips: Vec<CursorHideClipFile>,
    pub zoom_classic: bool,
    pub zoom_hidden: bool,
    pub zoom_locked: bool,
    pub crop: Option<CropFile>,
    pub background: BackgroundFile,
    pub background_padding: f64,
    // The radius was persisted but never rendered, so older sidecars carry
    // non-zero values that were never visible. It defaults to 0 rather than
    // inheriting the old default, which would round every existing project.
    //
    // `background_stroke` and `background_shadow` were in the same spot and
    // are gone: nothing ever read them, and an inert control is worse than no
    // control. Serde ignores unknown keys, so sidecars that still carry them
    // load unchanged.
    #[serde(default)]
    pub background_corner_radius: f64,
    pub dimension_preset: DimensionFile,
    pub custom_width: u32,
    pub custom_height: u32,
    pub quality: u8,
    pub audio_mode: AudioFile,
    pub audio_removed: bool,
    pub audio_locked: bool,
    pub video_locked: bool,
    pub video_hidden: bool,
    pub extra_media: Vec<MediaFile>,
    pub playhead_seconds: f64,
    pub timeline_scale: f64,
    pub timeline_scroll_seconds: f64,
    #[serde(default)]
    pub selected_zoom: Option<usize>,
    #[serde(default)]
    pub selected_cursor_hide: Option<usize>,
    #[serde(default)]
    pub cursor_theme: CursorThemeFile,
    #[serde(default = "default_cursor_size")]
    pub cursor_size: f64,
    #[serde(default = "default_cursor_speed")]
    pub cursor_speed: f64,
    #[serde(default = "default_cursor_shadow")]
    pub cursor_shadow: f64,
    #[serde(default = "default_cursor_smooth")]
    pub cursor_smooth: f64,
    #[serde(default)]
    pub cursor_hide_idle: bool,
    #[serde(default = "default_cursor_idle_ms")]
    pub cursor_idle_ms: f64,
    #[serde(default)]
    pub cursor_click_effect: ClickEffectFile,
    #[serde(default = "default_click_intensity")]
    pub cursor_click_intensity: f64,
    #[serde(default = "default_click_color")]
    pub cursor_click_color: (u8, u8, u8),
    #[serde(default = "default_click_scale")]
    pub cursor_click_scale: f64,
    #[serde(default = "default_click_opacity")]
    pub cursor_click_opacity: f64,
    #[serde(default = "default_click_duration_ms")]
    pub cursor_click_duration_ms: u32,
    #[serde(default = "default_cursor_trail")]
    pub cursor_trail: f64,
    #[serde(default = "default_cursor_tilt")]
    pub cursor_tilt: f64,
    #[serde(default = "default_cursor_sway")]
    pub cursor_sway: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorHideClipFile {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoomClipFile {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    pub ease_ms: u32,
    #[serde(default)]
    pub easing: ZoomEasingFile,
    pub mode: ZoomModeFile,
    #[serde(default)]
    pub rotation_x: f64,
    #[serde(default)]
    pub rotation_y: f64,
    #[serde(default)]
    pub rotation_z: f64,
    #[serde(default)]
    pub perspective: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomEasingFile {
    #[default]
    Glide,
    Smooth,
    Snappy,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomModeFile {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorThemeFile {
    #[default]
    #[serde(
        alias = "classic",
        alias = "crosshair",
        alias = "hand",
        alias = "circle"
    )]
    Adwaita,
    Yaru,
    #[serde(alias = "windows")]
    White,
    #[serde(alias = "inverted", alias = "dark")]
    Black,
    Macos,
    Tahoe,
    #[serde(alias = "tahoe-inverted")]
    TahoeInverted,
    Dot,
    #[serde(alias = "figma")]
    Minimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickEffectFile {
    None,
    #[serde(alias = "pulse")]
    Spotlight,
    #[default]
    Ripple,
    Echo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CropFile {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackgroundFile {
    None,
    Plain {
        r: u8,
        g: u8,
        b: u8,
    },
    /// Legacy preset reference. Gradients are drawn by hand now, so this is
    /// only read — it deserializes to a default two-stop gradient.
    Gradient {
        #[serde(default)]
        index: usize,
    },
    /// A hand-drawn gradient. `stops` carries the exact positions so a dragged
    /// stop round-trips instead of snapping to an even distribution.
    GradientSpec {
        #[serde(default)]
        stops: Vec<GradientStopFile>,
        #[serde(default)]
        angle_degrees: f64,
        #[serde(default)]
        reversed: bool,
        /// Added after the first hand-drawn gradients shipped, so older files
        /// default to linear.
        #[serde(default)]
        kind: GradientKindFile,
    },
    Wallpaper {
        path: PathBuf,
    },
}

/// One gradient stop as stored on disk. `position` is 0..=1 along the ramp.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStopFile {
    #[serde(default)]
    pub position: f64,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Straight alpha. Files written before per-stop opacity existed have no
    /// alpha and must stay fully opaque.
    #[serde(default = "opaque_alpha")]
    pub a: u8,
}

fn opaque_alpha() -> u8 {
    u8::MAX
}

/// How a hand-drawn gradient travels, mirroring the model's `GradientKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientKindFile {
    #[default]
    Linear,
    Radial,
    Angular,
    Diamond,
}

impl From<GradientKind> for GradientKindFile {
    fn from(kind: GradientKind) -> Self {
        match kind {
            GradientKind::Linear => Self::Linear,
            GradientKind::Radial => Self::Radial,
            GradientKind::Angular => Self::Angular,
            GradientKind::Diamond => Self::Diamond,
        }
    }
}

impl From<GradientKindFile> for GradientKind {
    fn from(kind: GradientKindFile) -> Self {
        match kind {
            GradientKindFile::Linear => Self::Linear,
            GradientKindFile::Radial => Self::Radial,
            GradientKindFile::Angular => Self::Angular,
            GradientKindFile::Diamond => Self::Diamond,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionFile {
    Original,
    P1080,
    P720,
    P480,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFile {
    Unchanged,
    Mono,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub path: PathBuf,
    pub display_name: String,
    pub kind: MediaKindFile,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKindFile {
    Video,
    Audio,
    Image,
}

fn video_projects_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("video-projects")
}

fn canonical_video_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn project_path_for_video(path: &Path) -> PathBuf {
    let canonical = canonical_video_path(path);
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    video_projects_directory().join(format!("{hash}.json"))
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

pub fn load_project(path: &Path) -> Option<VideoProjectFile> {
    let project_path = project_path_for_video(path);
    if !project_path.exists() {
        return None;
    }
    let json = match std::fs::read_to_string(&project_path) {
        Ok(json) => json,
        Err(err) => {
            eprintln!(
                "[recording-editor] failed to read project {}: {err}",
                project_path.display()
            );
            return None;
        }
    };
    let file: VideoProjectFile = match serde_json::from_str(&json) {
        Ok(file) => file,
        Err(err) => {
            eprintln!(
                "[recording-editor] failed to parse project {}: {err}",
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

pub fn save_project(path: &Path, file: &VideoProjectFile) -> std::io::Result<()> {
    let project_path = project_path_for_video(path);
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
    let project_path = project_path_for_video(path);
    if project_path.exists() {
        if let Err(err) = std::fs::remove_file(&project_path) {
            eprintln!(
                "[recording-editor] failed to delete project {}: {err}",
                project_path.display()
            );
        }
    }
}

impl VideoProjectFile {
    /// A display name for a History card: the persisted title, falling back
    /// to the source file's stem.
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

    /// The source recording is still on disk and unchanged, so the edit still
    /// applies to it.
    pub fn source_is_current(&self) -> bool {
        match source_fingerprint(&self.source_path) {
            Some((size, mtime)) => size == self.source_size && mtime == self.source_mtime_secs,
            None => false,
        }
    }
}

/// Every persisted video project, newest source first, for the History window.
///
/// Unreadable or corrupt files are skipped rather than failing the listing,
/// and entries whose source recording is gone or changed are dropped: those
/// edits can no longer be applied to anything.
pub fn list_projects() -> Vec<VideoProjectFile> {
    let Ok(read_dir) = std::fs::read_dir(video_projects_directory()) else {
        return Vec::new();
    };
    let mut projects: Vec<VideoProjectFile> = read_dir
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|json| serde_json::from_str::<VideoProjectFile>(&json).ok())
        .filter(VideoProjectFile::source_is_current)
        .collect();
    projects.sort_by(|a, b| b.source_mtime_secs.cmp(&a.source_mtime_secs));
    projects
}

fn zoom_to_file(clip: &ZoomClip) -> ZoomClipFile {
    ZoomClipFile {
        start: clip.start,
        end: clip.end,
        scale: clip.scale,
        center: clip.center,
        ease_ms: clip.ease_ms,
        easing: zoom_easing_to_file(clip.easing),
        mode: match clip.mode {
            ZoomMode::Auto => ZoomModeFile::Auto,
            ZoomMode::Manual => ZoomModeFile::Manual,
        },
        rotation_x: clip.rotation_x,
        rotation_y: clip.rotation_y,
        rotation_z: clip.rotation_z,
        perspective: clip.perspective,
    }
}

fn hide_to_file(clip: &CursorHideClip) -> CursorHideClipFile {
    CursorHideClipFile {
        start: clip.start,
        end: clip.end,
    }
}

fn hide_from_file(clip: &CursorHideClipFile) -> CursorHideClip {
    CursorHideClip {
        start: clip.start,
        end: clip.end,
    }
}

fn zoom_from_file(clip: &ZoomClipFile) -> ZoomClip {
    ZoomClip {
        start: clip.start,
        end: clip.end,
        scale: clip.scale,
        center: clip.center,
        ease_ms: clip.ease_ms,
        easing: zoom_easing_from_file(clip.easing),
        mode: match clip.mode {
            ZoomModeFile::Auto => ZoomMode::Auto,
            ZoomModeFile::Manual => ZoomMode::Manual,
        },
        rotation_x: clip.rotation_x,
        rotation_y: clip.rotation_y,
        rotation_z: clip.rotation_z,
        perspective: clip.perspective,
    }
}

fn zoom_easing_to_file(easing: ZoomEasing) -> ZoomEasingFile {
    match easing {
        ZoomEasing::Glide => ZoomEasingFile::Glide,
        ZoomEasing::Smooth => ZoomEasingFile::Smooth,
        ZoomEasing::Snappy => ZoomEasingFile::Snappy,
        ZoomEasing::Linear => ZoomEasingFile::Linear,
    }
}

fn zoom_easing_from_file(easing: ZoomEasingFile) -> ZoomEasing {
    match easing {
        ZoomEasingFile::Glide => ZoomEasing::Glide,
        ZoomEasingFile::Smooth => ZoomEasing::Smooth,
        ZoomEasingFile::Snappy => ZoomEasing::Snappy,
        ZoomEasingFile::Linear => ZoomEasing::Linear,
    }
}

fn crop_to_file(crop: CropSelection) -> CropFile {
    CropFile {
        x: crop.x,
        y: crop.y,
        width: crop.width,
        height: crop.height,
    }
}

fn crop_from_file(crop: CropFile) -> CropSelection {
    CropSelection {
        x: crop.x,
        y: crop.y,
        width: crop.width,
        height: crop.height,
    }
}

fn background_to_file(bg: &VideoBackground) -> BackgroundFile {
    match bg {
        VideoBackground::None => BackgroundFile::None,
        VideoBackground::Plain { r, g, b } => BackgroundFile::Plain {
            r: *r,
            g: *g,
            b: *b,
        },
        VideoBackground::Gradient(gradient) => {
            let gradient = gradient.normalized();
            BackgroundFile::GradientSpec {
                stops: gradient
                    .stops
                    .iter()
                    .map(|stop| GradientStopFile {
                        position: stop.position,
                        r: stop.r,
                        g: stop.g,
                        b: stop.b,
                        a: stop.a,
                    })
                    .collect(),
                angle_degrees: gradient.angle_degrees,
                reversed: gradient.reversed,
                kind: gradient.kind.into(),
            }
        }
        VideoBackground::Wallpaper(path) => BackgroundFile::Wallpaper { path: path.clone() },
    }
}

/// True when `path` looks like a bundled wallpaper reference rather than a
/// user-picked file. Bundled entries are stored as a bare file name with no
/// directory, so anything carrying a parent is a real user path and must be
/// left alone when it goes missing.
fn bundled_wallpaper_name(path: &Path) -> Option<String> {
    if path
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return None;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_owned())
}

fn background_from_file(bg: BackgroundFile) -> VideoBackground {
    match bg {
        BackgroundFile::None => VideoBackground::None,
        BackgroundFile::Plain { r, g, b } => VideoBackground::Plain { r, g, b },
        // Legacy preset references predating the hand-drawn gradient editor.
        // There is no preset table any more, so they open as the default
        // two-stop gradient rather than being dropped.
        BackgroundFile::Gradient { .. } => VideoBackground::Gradient(VideoGradient::default()),
        BackgroundFile::GradientSpec {
            stops,
            angle_degrees,
            reversed,
            kind,
        } => VideoBackground::Gradient(
            VideoGradient {
                kind: kind.into(),
                stops: stops
                    .iter()
                    .map(|stop| GradientStop::rgba(stop.position, stop.r, stop.g, stop.b, stop.a))
                    .collect(),
                angle_degrees,
                reversed,
            }
            .normalized(),
        ),
        BackgroundFile::Wallpaper { path } => {
            // Bundled wallpapers are stored by bare file name and resolve
            // against the asset directory. A user-picked image stores its real
            // path, and must NOT be retried as a bundled name: if the user
            // later moved or deleted it, that fallback would silently swap in
            // an unrelated stock wallpaper. Those keep their path and fall
            // through to the renderer's own missing-file handling instead.
            let resolved = if path.is_file() {
                path
            } else {
                match bundled_wallpaper_name(&path) {
                    Some(name) => {
                        let candidate =
                            crate::capture::editor::window::background_panel::background_gradient_asset_path(
                                &name,
                            );
                        if candidate.is_file() {
                            candidate
                        } else {
                            path
                        }
                    }
                    None => path,
                }
            };
            VideoBackground::Wallpaper(resolved)
        }
    }
}

fn dimension_to_file(preset: DimensionPreset) -> DimensionFile {
    match preset {
        DimensionPreset::Original => DimensionFile::Original,
        DimensionPreset::P1080 => DimensionFile::P1080,
        DimensionPreset::P720 => DimensionFile::P720,
        DimensionPreset::P480 => DimensionFile::P480,
        DimensionPreset::Custom => DimensionFile::Custom,
    }
}

fn dimension_from_file(preset: DimensionFile) -> DimensionPreset {
    match preset {
        DimensionFile::Original => DimensionPreset::Original,
        DimensionFile::P1080 => DimensionPreset::P1080,
        DimensionFile::P720 => DimensionPreset::P720,
        DimensionFile::P480 => DimensionPreset::P480,
        DimensionFile::Custom => DimensionPreset::Custom,
    }
}

fn cursor_theme_to_file(theme: CursorTheme) -> CursorThemeFile {
    match theme {
        CursorTheme::Adwaita => CursorThemeFile::Adwaita,
        CursorTheme::Yaru => CursorThemeFile::Yaru,
        CursorTheme::White => CursorThemeFile::White,
        CursorTheme::Black => CursorThemeFile::Black,
        CursorTheme::Macos => CursorThemeFile::Macos,
        CursorTheme::Tahoe => CursorThemeFile::Tahoe,
        CursorTheme::TahoeInverted => CursorThemeFile::TahoeInverted,
        CursorTheme::Dot => CursorThemeFile::Dot,
        CursorTheme::Minimal => CursorThemeFile::Minimal,
    }
}

fn cursor_theme_from_file(theme: CursorThemeFile) -> CursorTheme {
    match theme {
        CursorThemeFile::Adwaita => CursorTheme::Adwaita,
        CursorThemeFile::Yaru => CursorTheme::Yaru,
        CursorThemeFile::White => CursorTheme::White,
        CursorThemeFile::Black => CursorTheme::Black,
        CursorThemeFile::Macos => CursorTheme::Macos,
        CursorThemeFile::Tahoe => CursorTheme::Tahoe,
        CursorThemeFile::TahoeInverted => CursorTheme::TahoeInverted,
        CursorThemeFile::Dot => CursorTheme::Dot,
        CursorThemeFile::Minimal => CursorTheme::Minimal,
    }
}

fn default_cursor_size() -> f64 {
    DEFAULT_CURSOR_SIZE
}

fn default_cursor_speed() -> f64 {
    DEFAULT_CURSOR_SPEED
}

fn default_cursor_shadow() -> f64 {
    DEFAULT_CURSOR_SHADOW
}

fn default_cursor_smooth() -> f64 {
    DEFAULT_CURSOR_SMOOTH
}

fn default_cursor_idle_ms() -> f64 {
    DEFAULT_CURSOR_IDLE_MS
}

fn default_click_intensity() -> f64 {
    DEFAULT_CLICK_INTENSITY
}

fn default_click_color() -> (u8, u8, u8) {
    DEFAULT_CLICK_COLOR
}

fn default_click_scale() -> f64 {
    DEFAULT_CLICK_SCALE
}

fn default_click_opacity() -> f64 {
    DEFAULT_CLICK_OPACITY
}

fn default_click_duration_ms() -> u32 {
    DEFAULT_CLICK_DURATION_MS
}

fn default_cursor_trail() -> f64 {
    DEFAULT_CURSOR_TRAIL
}

fn default_cursor_tilt() -> f64 {
    DEFAULT_CURSOR_TILT
}

fn default_cursor_sway() -> f64 {
    DEFAULT_CURSOR_SWAY
}

fn click_effect_to_file(effect: ClickEffect) -> ClickEffectFile {
    match effect {
        ClickEffect::None => ClickEffectFile::None,
        ClickEffect::Spotlight => ClickEffectFile::Spotlight,
        ClickEffect::Ripple => ClickEffectFile::Ripple,
        ClickEffect::Echo => ClickEffectFile::Echo,
    }
}

fn click_effect_from_file(effect: ClickEffectFile) -> ClickEffect {
    match effect {
        ClickEffectFile::None => ClickEffect::None,
        ClickEffectFile::Spotlight => ClickEffect::Spotlight,
        ClickEffectFile::Ripple => ClickEffect::Ripple,
        ClickEffectFile::Echo => ClickEffect::Echo,
    }
}

fn audio_to_file(mode: AudioMode) -> AudioFile {
    match mode {
        AudioMode::Unchanged => AudioFile::Unchanged,
        AudioMode::Mono => AudioFile::Mono,
        AudioMode::Muted => AudioFile::Muted,
    }
}

fn audio_from_file(mode: AudioFile) -> AudioMode {
    match mode {
        AudioFile::Unchanged => AudioMode::Unchanged,
        AudioFile::Mono => AudioMode::Mono,
        AudioFile::Muted => AudioMode::Muted,
    }
}

/// Tier index from a project file. Pre-tier files stored the 0-100 slider
/// value (always 70, the only value the old code ever wrote); those mean the
/// default tier, same as any other unknown value.
fn quality_from_file(quality: u8) -> ExportQuality {
    match quality {
        0 => ExportQuality::Balanced,
        2 => ExportQuality::Ultra,
        _ => ExportQuality::High,
    }
}

fn media_to_file(item: &ProjectMedia) -> MediaFile {
    MediaFile {
        path: item.path.clone(),
        display_name: item.display_name.clone(),
        kind: match item.kind {
            ProjectMediaKind::Video => MediaKindFile::Video,
            ProjectMediaKind::Audio => MediaKindFile::Audio,
            ProjectMediaKind::Image => MediaKindFile::Image,
        },
        duration_seconds: item.duration_seconds,
    }
}

fn media_from_file(item: &MediaFile) -> ProjectMedia {
    ProjectMedia {
        path: item.path.clone(),
        display_name: item.display_name.clone(),
        kind: match item.kind {
            MediaKindFile::Video => ProjectMediaKind::Video,
            MediaKindFile::Audio => ProjectMediaKind::Audio,
            MediaKindFile::Image => ProjectMediaKind::Image,
        },
        duration_seconds: item.duration_seconds,
    }
}

fn extra_media(state: &VideoEditState) -> Vec<MediaFile> {
    state
        .project_media
        .iter()
        .filter(|item| item.path != state.metadata.path)
        .map(media_to_file)
        .collect()
}

fn edits_for_compare(file: &VideoProjectFile) -> VideoProjectFile {
    let mut file = file.clone();
    file.playhead_seconds = 0.0;
    file.timeline_scroll_seconds = 0.0;
    file
}

impl VideoEditState {
    pub fn to_project(&self) -> VideoProjectFile {
        let (source_size, source_mtime_secs) =
            source_fingerprint(&self.metadata.path).unwrap_or((self.metadata.file_size_bytes, 0));
        VideoProjectFile {
            version: VIDEO_PROJECT_VERSION,
            source_path: self.metadata.path.clone(),
            source_size,
            source_mtime_secs,
            title: self.title.clone(),
            trim_start_seconds: self.trim_start_seconds,
            trim_end_seconds: self.trim_end_seconds,
            cuts: self.cuts.clone(),
            freeze_tail: self.freeze_tail,
            frozen_segment: self.frozen_segment,
            segments_kept: self.segments_kept.clone(),
            segment_order: self.segment_order.clone(),
            segment_starts: self.segment_starts.clone(),
            segment_speeds: self.segment_speeds.clone(),
            segment_muted: self.segment_muted.clone(),
            timeline_offset_seconds: self.timeline_offset_seconds,
            zoom_clips: self.zoom_clips.iter().map(zoom_to_file).collect(),
            cursor_hide_clips: self.cursor_hide_clips.iter().map(hide_to_file).collect(),
            zoom_classic: self.zoom_classic,
            zoom_hidden: self.zoom_hidden,
            zoom_locked: self.zoom_locked,
            crop: self.crop.map(crop_to_file),
            background: background_to_file(&self.background),
            background_padding: self.background_padding,
            background_corner_radius: self.background_corner_radius,
            dimension_preset: dimension_to_file(self.dimension_preset),
            custom_width: self.custom_width,
            custom_height: self.custom_height,
            quality: self.quality.tier(),
            audio_mode: audio_to_file(self.audio_mode),
            audio_removed: self.audio_removed,
            audio_locked: self.audio_locked,
            video_locked: self.video_locked,
            video_hidden: self.video_hidden,
            extra_media: extra_media(self),
            playhead_seconds: self.playhead_seconds,
            timeline_scale: self.timeline_scale,
            timeline_scroll_seconds: self.timeline_scroll_seconds,
            selected_zoom: self.selected_zoom,
            selected_cursor_hide: self.selected_cursor_hide,
            cursor_theme: cursor_theme_to_file(self.cursor.theme),
            cursor_size: self.cursor.size,
            cursor_speed: self.cursor.speed,
            cursor_shadow: self.cursor.shadow,
            cursor_smooth: self.cursor.smooth,
            cursor_hide_idle: self.cursor.hide_idle,
            cursor_idle_ms: self.cursor.idle_ms,
            cursor_click_effect: click_effect_to_file(self.cursor.click_effect),
            cursor_click_intensity: self.cursor.click_intensity,
            cursor_click_color: self.cursor.click_color,
            cursor_click_scale: self.cursor.click_scale,
            cursor_click_opacity: self.cursor.click_opacity,
            cursor_click_duration_ms: self.cursor.click_duration_ms,
            cursor_trail: self.cursor.trail,
            cursor_tilt: self.cursor.tilt,
            cursor_sway: self.cursor.sway,
        }
    }

    pub fn apply_project(&mut self, file: VideoProjectFile) {
        self.title = file.title;
        self.trim_start_seconds = file.trim_start_seconds;
        self.trim_end_seconds = file.trim_end_seconds;
        self.cuts = file.cuts;
        // A tail is only meaningful with a final segment to hold; a project
        // saved before a re-trim can name a segment that no longer exists.
        self.frozen_segment = file.frozen_segment;
        self.freeze_tail = if self.frozen_segment.is_some() {
            file.freeze_tail.max(0.0)
        } else {
            0.0
        };
        self.segments_kept = file.segments_kept;
        self.segment_order = file.segment_order;
        self.segment_starts = file.segment_starts;
        self.segment_speeds = file.segment_speeds;
        self.segment_muted = file.segment_muted;
        self.timeline_offset_seconds = file.timeline_offset_seconds;
        self.zoom_clips = file.zoom_clips.iter().map(zoom_from_file).collect();
        self.cursor_hide_clips = file.cursor_hide_clips.iter().map(hide_from_file).collect();
        self.zoom_classic = file.zoom_classic;
        self.zoom_hidden = file.zoom_hidden;
        self.zoom_locked = file.zoom_locked;
        self.crop = file.crop.map(crop_from_file);
        self.background = background_from_file(file.background);
        self.background_padding = file.background_padding;
        self.background_corner_radius = file.background_corner_radius;
        self.dimension_preset = dimension_from_file(file.dimension_preset);
        self.custom_width = file.custom_width;
        self.custom_height = file.custom_height;
        self.quality = quality_from_file(file.quality);
        self.audio_mode = audio_from_file(file.audio_mode);
        self.audio_removed = file.audio_removed;
        self.audio_locked = file.audio_locked;
        self.video_locked = file.video_locked;
        self.video_hidden = file.video_hidden;
        self.playhead_seconds = file.playhead_seconds;
        self.timeline_scale = file.timeline_scale;
        self.timeline_scroll_seconds = file.timeline_scroll_seconds;
        self.selected_zoom = file
            .selected_zoom
            .filter(|index| *index < self.zoom_clips.len());
        self.selected_cursor_hide = file
            .selected_cursor_hide
            .filter(|index| *index < self.cursor_hide_clips.len());
        self.cursor = CursorSettings {
            theme: cursor_theme_from_file(file.cursor_theme),
            size: file.cursor_size,
            speed: file.cursor_speed,
            shadow: file.cursor_shadow,
            smooth: file.cursor_smooth,
            hide_idle: file.cursor_hide_idle,
            idle_ms: file.cursor_idle_ms,
            click_effect: click_effect_from_file(file.cursor_click_effect),
            click_intensity: file.cursor_click_intensity,
            click_color: file.cursor_click_color,
            click_scale: file.cursor_click_scale,
            click_opacity: file.cursor_click_opacity,
            click_duration_ms: file.cursor_click_duration_ms,
            trail: file.cursor_trail,
            tilt: file.cursor_tilt,
            sway: file.cursor_sway,
        }
        .clamped();
        self.project_media
            .retain(|item| item.path == self.metadata.path);
        for item in file.extra_media {
            self.add_project_media(media_from_file(&item));
        }
    }

    pub fn session_is_default(&self) -> bool {
        let current = edits_for_compare(&self.to_project());
        let fresh = edits_for_compare(&VideoEditState::new(self.metadata.clone()).to_project());
        current == fresh
    }

    pub fn session_is_dirty(&self, last: Option<&VideoProjectFile>) -> bool {
        if last.is_none() && self.session_is_default() {
            return false;
        }
        match last {
            None => true,
            Some(saved) => saved != &self.to_project(),
        }
    }
}

pub fn restore_into(state: &mut VideoEditState) {
    if let Some(project) = load_project(&state.metadata.path) {
        state.apply_project(project);
    }
}

pub fn persist_video_session(state: &VideoEditState) {
    if !state.has_source_video() {
        return;
    }
    let last = load_project(&state.metadata.path);
    if !state.session_is_dirty(last.as_ref()) {
        return;
    }
    if state.session_is_default() {
        delete_project(&state.metadata.path);
        return;
    }
    if let Err(err) = save_project(&state.metadata.path, &state.to_project()) {
        eprintln!(
            "[recording-editor] failed to save project for {}: {err}",
            state.metadata.path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::model::MIN_GRADIENT_STOPS;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-video-project-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_video(dir: &Path, name: &str, bytes: usize) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, vec![b'v'; bytes]).unwrap();
        path
    }

    fn metadata_for(path: &Path, size: u64) -> VideoMetadata {
        VideoMetadata {
            path: path.to_path_buf(),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: size,
            has_audio: true,
            frame_rate: 30.0,
        }
    }

    fn cleanup_project(path: &Path) {
        delete_project(path);
    }

    #[test]
    fn different_paths_do_not_share_a_project_file() {
        assert_ne!(
            project_path_for_video(Path::new("/tmp/a.mp4")),
            project_path_for_video(Path::new("/tmp/b.mp4"))
        );
    }

    #[test]
    fn legacy_figma_cursor_theme_key_still_loads() {
        // Projects saved before the rename stored "figma" for the Minimal
        // theme. New saves write "minimal", which older builds already accept
        // through the alias they carried for that key.
        let legacy: CursorThemeFile = serde_json::from_str("\"figma\"").unwrap();
        assert_eq!(legacy, CursorThemeFile::Minimal);
        assert_eq!(cursor_theme_from_file(legacy), CursorTheme::Minimal);
        assert_eq!(
            serde_json::to_string(&CursorThemeFile::Minimal).unwrap(),
            "\"minimal\""
        );
        assert_eq!(CursorTheme::parse("figma"), CursorTheme::Minimal);
        assert_eq!(CursorTheme::Minimal.as_str(), "minimal");
        assert_eq!(CursorTheme::Minimal.label(), "Minimal");
    }

    #[test]
    fn roundtrip_trim_zoom_crop_background_and_extra_media() {
        let dir = scratch("roundtrip");
        let video = write_video(&dir, "clip.mp4", 32);
        let extra = dir.join("b-roll.mp4");
        fs::write(&extra, b"extra").unwrap();
        let mut state = VideoEditState::new(metadata_for(&video, 32));
        state.trim_start_seconds = 1.0;
        state.trim_end_seconds = 8.0;
        state.zoom_clips.push(ZoomClip {
            start: 2.0,
            end: 3.8,
            scale: 1.8,
            center: (0.4, 0.6),
            ease_ms: 200,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Manual,
            ..Default::default()
        });
        state.crop = Some(CropSelection {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        });
        state.background = VideoBackground::Plain {
            r: 12,
            g: 24,
            b: 36,
        };
        state.background_padding = 40.0;
        state.cursor.theme = CursorTheme::White;
        state.cursor.size = 1.5;
        state.cursor.speed = 2.0;
        state.cursor.shadow = 0.7;
        state.cursor_hide_clips.push(CursorHideClip {
            start: 4.0,
            end: 5.5,
        });
        state.add_project_media(ProjectMedia {
            path: extra.clone(),
            display_name: "B-roll".into(),
            kind: ProjectMediaKind::Video,
            duration_seconds: Some(4.0),
        });
        let project = state.to_project();
        save_project(&video, &project).unwrap();

        let loaded = load_project(&video).expect("project should load");
        let mut restored = VideoEditState::new(metadata_for(&video, 32));
        restored.apply_project(loaded);
        assert_eq!(
            edits_for_compare(&restored.to_project()),
            edits_for_compare(&state.to_project())
        );
        assert!(restored.sidecar.is_none());

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_session_is_not_dirty_trim_or_zoom_is() {
        let dir = scratch("dirty");
        let video = write_video(&dir, "clip.mp4", 16);
        let state = VideoEditState::new(metadata_for(&video, 16));
        assert!(state.session_is_default());
        assert!(!state.session_is_dirty(None));

        let mut trimmed = state.clone();
        trimmed.trim_start_seconds = 1.5;
        assert!(trimmed.session_is_dirty(None));

        let mut zoomed = VideoEditState::new(metadata_for(&video, 16));
        zoomed.zoom_clips.push(ZoomClip {
            start: 0.0,
            end: 1.8,
            scale: 2.0,
            center: (0.5, 0.5),
            ease_ms: 200,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Auto,
            ..Default::default()
        });
        assert!(zoomed.session_is_dirty(None));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_replaces_removed_manual_zoom_with_auto_zoom() {
        let dir = scratch("replace-manual-with-auto");
        let video = write_video(&dir, "clip.mp4", 16);
        let mut state = VideoEditState::new(metadata_for(&video, 16));
        state.zoom_clips.push(ZoomClip {
            start: 1.0,
            end: 2.8,
            scale: 1.8,
            center: (960.0, 540.0),
            ease_ms: 400,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Manual,
            ..Default::default()
        });
        persist_video_session(&state);

        state.zoom_clips.clear();
        state.zoom_clips.push(ZoomClip {
            start: 4.0,
            end: 6.0,
            scale: 1.5,
            center: (1200.0, 600.0),
            ease_ms: 400,
            easing: ZoomEasing::Glide,
            mode: ZoomMode::Auto,
            ..Default::default()
        });
        persist_video_session(&state);

        let mut restored = VideoEditState::new(metadata_for(&video, 16));
        restore_into(&mut restored);
        assert_eq!(restored.zoom_clips.len(), 1);
        assert_eq!(restored.zoom_clips[0].mode, ZoomMode::Auto);
        assert!((restored.zoom_clips[0].start - 4.0).abs() < 1e-9);

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_mismatch_returns_none() {
        let dir = scratch("fingerprint");
        let video = write_video(&dir, "clip.mp4", 16);
        let state = VideoEditState::new(metadata_for(&video, 16));
        save_project(&video, &state.to_project()).unwrap();
        assert!(load_project(&video).is_some());

        fs::write(&video, vec![b'v'; 64]).unwrap();
        let touched = SystemTime::now() + Duration::from_secs(2);
        let _ = fs::File::options()
            .write(true)
            .open(&video)
            .unwrap()
            .set_modified(touched);
        assert!(load_project(&video).is_none());

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_json_does_not_contain_pointer_samples() {
        let dir = scratch("without-samples");
        let video = write_video(&dir, "clip.mp4", 8);
        let mut state = VideoEditState::new(metadata_for(&video, 8));
        attach_dummy_pointer(&mut state);
        let json = serde_json::to_string(&state.to_project()).unwrap();
        assert!(!json.contains("pointer"));
        assert!(!json.contains("clicks"));
        assert!(!json.contains("t0_monotonic"));
        let _ = fs::remove_dir_all(&dir);
    }

    fn attach_dummy_pointer(state: &mut VideoEditState) {
        let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
        );
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t: 0.0,
                x: 1.0,
                y: 2.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
        sidecar
            .clicks
            .push(crate::recording::editor::sidecar::ClickSample {
                t: 0.4,
                x: 1.0,
                y: 2.0,
                button: 1,
            });
        state.sidecar = Some(sidecar);
    }

    #[test]
    fn reset_video_edits_plus_delete_project_loads_default() {
        let dir = scratch("reset");
        let video = write_video(&dir, "clip.mp4", 16);
        let mut state = VideoEditState::new(metadata_for(&video, 16));
        state.trim_start_seconds = 2.0;
        save_project(&video, &state.to_project()).unwrap();
        assert!(load_project(&video).is_some());

        state.reset_video_edits();
        delete_project(&video);
        assert!(load_project(&video).is_none());
        let restored = VideoEditState::new(metadata_for(&video, 16));
        assert!(restored.session_is_default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_project_json_defaults_click_settings_and_zoom_easing() {
        let dir = scratch("old-defaults");
        let video = write_video(&dir, "clip.mp4", 16);
        let mut state = VideoEditState::new(metadata_for(&video, 16));
        state.zoom_clips.push(ZoomClip {
            start: 1.0,
            end: 2.8,
            scale: 1.8,
            center: (0.5, 0.5),
            ease_ms: 400,
            easing: ZoomEasing::Snappy,
            mode: ZoomMode::Manual,
            ..Default::default()
        });
        state.cursor.click_color = (12, 34, 56);
        state.cursor.click_scale = 1.6;
        state.cursor.click_opacity = 0.4;
        state.cursor.click_duration_ms = 900;
        let mut json = serde_json::to_value(state.to_project()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("cursor_click_color");
        object.remove("cursor_click_scale");
        object.remove("cursor_click_opacity");
        object.remove("cursor_click_duration_ms");
        for clip in json
            .get_mut("zoom_clips")
            .and_then(|value| value.as_array_mut())
            .unwrap()
        {
            clip.as_object_mut().unwrap().remove("easing");
        }
        let file: VideoProjectFile = serde_json::from_value(json).unwrap();
        assert_eq!(file.cursor_click_color, DEFAULT_CLICK_COLOR);
        assert!((file.cursor_click_scale - DEFAULT_CLICK_SCALE).abs() < 1e-12);
        assert!((file.cursor_click_opacity - DEFAULT_CLICK_OPACITY).abs() < 1e-12);
        assert_eq!(file.cursor_click_duration_ms, DEFAULT_CLICK_DURATION_MS);
        assert_eq!(file.zoom_clips[0].easing, ZoomEasingFile::Glide);

        let mut restored = VideoEditState::new(metadata_for(&video, 16));
        restored.apply_project(file);
        assert_eq!(restored.cursor.click_color, DEFAULT_CLICK_COLOR);
        assert_eq!(restored.zoom_clips[0].easing, ZoomEasing::Glide);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_preserves_click_effect_styling_and_zoom_easing() {
        let dir = scratch("click-style");
        let video = write_video(&dir, "clip.mp4", 24);
        let mut state = VideoEditState::new(metadata_for(&video, 24));
        state.cursor.click_color = (32, 160, 240);
        state.cursor.click_scale = 1.4;
        state.cursor.click_opacity = 0.55;
        state.cursor.click_duration_ms = 800;
        state.zoom_clips.push(ZoomClip {
            start: 0.5,
            end: 2.3,
            scale: 2.0,
            center: (0.4, 0.6),
            ease_ms: 480,
            easing: ZoomEasing::Snappy,
            mode: ZoomMode::Manual,
            ..Default::default()
        });
        save_project(&video, &state.to_project()).unwrap();
        let loaded = load_project(&video).expect("project should load");
        let mut restored = VideoEditState::new(metadata_for(&video, 24));
        restored.apply_project(loaded);
        assert_eq!(restored.cursor.click_color, (32, 160, 240));
        assert!((restored.cursor.click_scale - 1.4).abs() < 1e-12);
        assert!((restored.cursor.click_opacity - 0.55).abs() < 1e-12);
        assert_eq!(restored.cursor.click_duration_ms, 800);
        assert_eq!(restored.zoom_clips[0].easing, ZoomEasing::Snappy);
        assert_eq!(restored.zoom_clips[0].ease_ms, 480);
        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_preserves_wallpaper_background_and_padding() {
        let dir = scratch("wallpaper-bg");
        let video = write_video(&dir, "clip.mp4", 24);
        let wallpaper = dir.join("wallpaper-001.jpg");
        fs::write(&wallpaper, b"fake-jpg").unwrap();
        let mut state = VideoEditState::new(metadata_for(&video, 24));
        state.background = VideoBackground::Wallpaper(wallpaper.clone());
        state.background_padding = 40.0;
        save_project(&video, &state.to_project()).unwrap();
        let loaded = load_project(&video).expect("project should load");
        let mut restored = VideoEditState::new(metadata_for(&video, 24));
        restored.apply_project(loaded);
        assert_eq!(restored.background, VideoBackground::Wallpaper(wallpaper));
        assert!((restored.background_padding - 40.0).abs() < 1e-12);
        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_name_prefers_the_title_over_the_file_stem() {
        let dir = scratch("display-name");
        let video = write_video(&dir, "clip.mp4", 8);
        let state = VideoEditState::new(metadata_for(&video, 8));
        let mut project = state.to_project();

        project.title = "My zoom".into();
        assert_eq!(project.display_name(), "My zoom");

        project.title = "   ".into();
        assert_eq!(project.display_name(), "clip");

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_source_is_not_current() {
        let dir = scratch("source-current");
        let video = write_video(&dir, "clip.mp4", 8);
        let state = VideoEditState::new(metadata_for(&video, 8));
        let project = state.to_project();
        assert!(project.source_is_current());

        fs::write(&video, vec![b'v'; 64]).unwrap();
        let touched = SystemTime::now() + Duration::from_secs(2);
        let _ = fs::File::options()
            .write(true)
            .open(&video)
            .unwrap()
            .set_modified(touched);
        assert!(!project.source_is_current());

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_project_appears_in_the_listing() {
        let dir = scratch("listing");
        let video = write_video(&dir, "clip.mp4", 8);
        let mut state = VideoEditState::new(metadata_for(&video, 8));
        state.trim_start_seconds = 1.0;
        persist_video_session(&state);

        let listed = list_projects();
        assert!(
            listed
                .iter()
                .any(|project| project.source_path == video.canonicalize().unwrap()),
            "the saved project should be listed"
        );

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_whose_source_changed_is_left_out_of_the_listing() {
        let dir = scratch("listing-stale");
        let video = write_video(&dir, "clip.mp4", 8);
        let mut state = VideoEditState::new(metadata_for(&video, 8));
        state.trim_start_seconds = 1.0;
        persist_video_session(&state);

        fs::write(&video, vec![b'v'; 64]).unwrap();
        let touched = SystemTime::now() + Duration::from_secs(2);
        let _ = fs::File::options()
            .write(true)
            .open(&video)
            .unwrap()
            .set_modified(touched);

        let listed = list_projects();
        assert!(
            !listed
                .iter()
                .any(|project| project.source_path == video.canonicalize().unwrap()),
            "an edit whose source changed can no longer be applied, so it must not be listed"
        );

        cleanup_project(&video);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hand_drawn_gradient_survives_the_json_round_trip() {
        // Uneven stop positions are the whole point of the new type — an even
        // distribution would not prove they are actually persisted.
        let gradient = VideoGradient {
            kind: GradientKind::Diamond,
            stops: vec![
                GradientStop::new(0.0, 0x00, 0x90, 0xFF),
                GradientStop::rgba(0.37, 0x12, 0x34, 0x56, 0x80),
                GradientStop::new(1.0, 0xFF, 0xFF, 0xFF),
            ],
            angle_degrees: 42.0,
            reversed: true,
        };
        let file = background_to_file(&VideoBackground::Gradient(gradient.clone()));
        let json = serde_json::to_string(&file).expect("gradient serializes");
        let parsed: BackgroundFile = serde_json::from_str(&json).expect("gradient deserializes");
        assert_eq!(
            background_from_file(parsed),
            VideoBackground::Gradient(gradient)
        );
    }

    #[test]
    fn a_legacy_gradient_index_still_loads() {
        // Projects written before the gradient editor stored a preset index.
        // There is no preset table any more, so it must open as a usable
        // default rather than failing to deserialize.
        let legacy: BackgroundFile = serde_json::from_str(r#"{"type":"gradient","index":3}"#)
            .expect("legacy gradient loads");
        match background_from_file(legacy) {
            VideoBackground::Gradient(gradient) => {
                assert_eq!(gradient.stops.len(), MIN_GRADIENT_STOPS);
            }
            other => panic!("expected a gradient, got {other:?}"),
        }
    }

    #[test]
    fn a_gradient_without_a_kind_loads_as_linear() {
        // `kind` arrived after the first hand-drawn gradients, so a sidecar
        // that predates it must keep opening as the linear gradient it was.
        let legacy: BackgroundFile = serde_json::from_str(
            r#"{"type":"gradient_spec","stops":[{"position":0.0,"r":0,"g":144,"b":255},{"position":1.0,"r":255,"g":255,"b":255}],"angle_degrees":0.0,"reversed":false}"#,
        )
        .expect("a spec without kind loads");
        match background_from_file(legacy) {
            VideoBackground::Gradient(gradient) => {
                assert_eq!(gradient.kind, GradientKind::Linear)
            }
            other => panic!("expected a gradient, got {other:?}"),
        }
    }

    #[test]
    fn a_project_without_the_new_background_fields_defaults_them_to_zero() {
        // Older sidecars carry an inert non-zero radius. It must load as 0 so
        // no existing project silently gains rounded corners the first time
        // it is opened.
        let project = VideoEditState::new(metadata_for(Path::new("/tmp/clip.mp4"), 8)).to_project();
        let mut value = serde_json::to_value(&project).expect("project serializes");
        let object = value.as_object_mut().expect("project is an object");
        object.remove("background_corner_radius");

        let restored: VideoProjectFile =
            serde_json::from_value(value).expect("project without the new fields loads");
        assert_eq!(restored.background_corner_radius, 0.0);
    }

    #[test]
    fn sidecars_carrying_the_removed_stroke_and_shadow_still_load() {
        // Both fields were dropped once it was clear nothing rendered them.
        // Every project written while they existed must keep opening, so the
        // loader has to tolerate the keys rather than reject the file.
        let project = VideoEditState::new(metadata_for(Path::new("/tmp/clip.mp4"), 8)).to_project();
        let mut value = serde_json::to_value(&project).expect("project serializes");
        let object = value.as_object_mut().expect("project is an object");
        object.insert("background_stroke".into(), serde_json::json!(12.0));
        object.insert("background_shadow".into(), serde_json::json!(15.0));

        serde_json::from_value::<VideoProjectFile>(value)
            .expect("a sidecar written with stroke and shadow still loads");
    }

    #[test]
    fn a_missing_user_image_is_not_replaced_by_a_bundled_wallpaper() {
        // Bundled wallpapers are stored as a bare file name; user-picked images
        // keep a real path. Retrying a moved user image against the asset
        // directory would silently swap in unrelated stock art.
        let moved = PathBuf::from("/home/someone/Pictures/my-photo.jpg");
        let resolved = background_from_file(BackgroundFile::Wallpaper {
            path: moved.clone(),
        });
        assert_eq!(resolved, VideoBackground::Wallpaper(moved));
    }
}
