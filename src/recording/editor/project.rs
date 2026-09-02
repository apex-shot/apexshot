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
    DimensionPreset, ProjectMedia, ProjectMediaKind, VideoBackground, VideoEditState, ZoomClip,
    ZoomEasing, ZoomMode, DEFAULT_CLICK_COLOR, DEFAULT_CLICK_DURATION_MS, DEFAULT_CLICK_INTENSITY,
    DEFAULT_CLICK_OPACITY, DEFAULT_CLICK_SCALE, DEFAULT_CURSOR_IDLE_MS, DEFAULT_CURSOR_SHADOW,
    DEFAULT_CURSOR_SIZE, DEFAULT_CURSOR_SMOOTH, DEFAULT_CURSOR_SPEED, DEFAULT_CURSOR_SWAY,
    DEFAULT_CURSOR_TILT, DEFAULT_CURSOR_TRAIL,
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
    pub background_corner_radius: f64,
    pub background_shadow: f64,
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
    #[serde(alias = "minimal")]
    Figma,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackgroundFile {
    None,
    Plain { r: u8, g: u8, b: u8 },
    Gradient { index: usize },
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

fn background_to_file(bg: VideoBackground) -> BackgroundFile {
    match bg {
        VideoBackground::None => BackgroundFile::None,
        VideoBackground::Plain { r, g, b } => BackgroundFile::Plain { r, g, b },
        VideoBackground::Gradient(index) => BackgroundFile::Gradient { index },
    }
}

fn background_from_file(bg: BackgroundFile) -> VideoBackground {
    match bg {
        BackgroundFile::None => VideoBackground::None,
        BackgroundFile::Plain { r, g, b } => VideoBackground::Plain { r, g, b },
        BackgroundFile::Gradient { index } => VideoBackground::Gradient(index),
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
        CursorTheme::Figma => CursorThemeFile::Figma,
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
        CursorThemeFile::Figma => CursorTheme::Figma,
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
            background: background_to_file(self.background),
            background_padding: self.background_padding,
            background_corner_radius: self.background_corner_radius,
            background_shadow: self.background_shadow,
            dimension_preset: dimension_to_file(self.dimension_preset),
            custom_width: self.custom_width,
            custom_height: self.custom_height,
            quality: self.quality,
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
        self.background_shadow = file.background_shadow;
        self.dimension_preset = dimension_from_file(file.dimension_preset);
        self.custom_width = file.custom_width;
        self.custom_height = file.custom_height;
        self.quality = file.quality;
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
}
