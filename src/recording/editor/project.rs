//! Edit-list sidecar for the recording editor.
//!
//! Stored under `~/.local/share/apexshot/video-projects/{sha256(source path)}.json`.
//! Pointer samples stay in `{stem}.apexshot.json` next to the MP4.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::model::{
    AudioMode, CropSelection, DimensionPreset, ProjectMedia, ProjectMediaKind, VideoBackground,
    VideoEditState, ZoomClip, ZoomMode,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoomClipFile {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    pub ease_ms: u32,
    pub mode: ZoomModeFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomModeFile {
    Auto,
    Manual,
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
        mode: match clip.mode {
            ZoomMode::Auto => ZoomModeFile::Auto,
            ZoomMode::Manual => ZoomModeFile::Manual,
        },
    }
}

fn zoom_from_file(clip: &ZoomClipFile) -> ZoomClip {
    ZoomClip {
        start: clip.start,
        end: clip.end,
        scale: clip.scale,
        center: clip.center,
        ease_ms: clip.ease_ms,
        mode: match clip.mode {
            ZoomModeFile::Auto => ZoomMode::Auto,
            ZoomModeFile::Manual => ZoomMode::Manual,
        },
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
        assert_eq!(edits_for_compare(&restored.to_project()), edits_for_compare(&state.to_project()));
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
            mode: ZoomMode::Auto,
        });
        assert!(zoomed.session_is_dirty(None));

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
        sidecar.clicks.push(crate::recording::editor::sidecar::ClickSample {
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
}
