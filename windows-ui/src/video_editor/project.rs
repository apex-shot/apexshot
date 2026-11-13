//! The edit list kept beside a recording.
//!
//! The Windows editor held every edit in memory and threw it away with the
//! window. This module gives it the counterpart of the shared editor's
//! `video-projects` sidecar: one JSON file per recording, keyed by the hash of
//! the canonical video path, so a project can never attach itself to footage it
//! was not written for.
//!
//! The file carries what the Windows editor models, under the names the shared
//! editor's `VideoProjectFile` already uses for the same concepts —
//! `trim_start_seconds`, `cuts`, `segment_starts`, `freeze_tail`, `zoom_clips`,
//! `zoom_camera`, `cursor_hide_clips`, and the three view fields. The stage is
//! three plain keys: the wallpaper the user picked, the inset around the video
//! and its corner radius. A fill the panel paints itself has no key, because
//! the Windows model holds it as a closure and a schema for gradients is a
//! separate design. Merging the two schemas later is then a promotion rather
//! than a translation. What this editor does not model — crop, dimension,
//! quality, extra media — has no key here, so the file never claims a feature
//! the editor would not honour.
//!
//! The directory is its own rather than the shared `video-projects` one: the
//! shared editor parses every file in that folder with its own loader, and a
//! project written here would either be skipped silently or, if it ever gained
//! the shared fields, lose them on the next save from either side.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Format version of the file. Bumped when a field changes meaning; readers
/// keep working through the `#[serde(default)]` on every optional key.
///
/// Version 2 gives the camera layer's `enabled` switch its own meaning — the
/// layer draws at all — where version 1 files used it to say the default
/// layout was the screen alone.
///
/// Version 3 adds the stage: the wallpaper behind the video, its padding and
/// its corner radius. All three read back as a flat stage in a version 2 file.
pub(super) const PROJECT_VERSION: u32 = 3;

/// The stage behind the video as the file stores it: the wallpaper the user
/// picked, by its bundled catalog name. A recording opened with no fill reads
/// back with no selection, which is also how a fill the panel paints itself
/// comes back, since that painter has no serialised form.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct StageFile {
    #[serde(default)]
    pub selection: Option<String>,
    /// The frame the project was saved with; absent means the recording's own shape.
    #[serde(default)]
    pub aspect: Option<String>,
}

/// The camera layer as the project file stores it: fractions of the frame, a
/// corner radius as a fraction of the shorter side, and the two switches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct WebcamLayerFile {
    /// Whether the layer draws at all. A file from before the layouts block
    /// used this to say the camera was not in the picture, which the
    /// screen-only default already covers; those files read as an enabled
    /// layer.
    pub enabled: bool,
    /// Whether the layer fills the frame instead of sitting in a bubble. Read
    /// only by a file that has no layouts block.
    pub fullscreen: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub mirror: bool,
    /// Manual camera sync in seconds. Absent in files written before it
    /// existed, which played the camera with no adjustment.
    #[serde(default)]
    pub delay_seconds: f64,
    /// The camera's size while zoomed. Absent in older files, which shrank to the studied 0.8.
    #[serde(default = "default_zoomed_scale")]
    pub zoomed_scale: f64,
    /// Hide the camera during silences this long; absent is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_during_silence: Option<f64>,
    /// The layout timeline: the recording's default layout and the changes
    /// that switch it while the composition plays. Absent in files written
    /// before layouts existed, where the two switches above still say what the
    /// default was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layouts: Option<super::webcam::LayoutSettings>,
}

fn default_zoomed_scale() -> f64 {
    super::webcam::ZOOMED_CAMERA_SCALE
}

impl WebcamLayerFile {
    /// The layout timeline this file describes.
    ///
    /// A file written before layouts existed carries only `enabled` and
    /// `fullscreen`, so those two say what the default layout was: a hidden
    /// camera is Screen only, a fullscreen camera is Fullscreen camera, and
    /// anything else is the overlay bubble. An explicit `layouts` block always
    /// wins, even when the older switches disagree with it, because the two
    /// switches are the coarse view of that same default and cannot describe
    /// the changes that follow it.
    pub(super) fn resolved_layouts(&self) -> super::webcam::LayoutSettings {
        if let Some(layouts) = &self.layouts {
            return layouts.clone().clamped();
        }
        super::webcam::LayoutSettings {
            default_layout: if !self.enabled {
                super::webcam::CameraLayout::ScreenOnly
            } else if self.fullscreen {
                super::webcam::CameraLayout::FullscreenCamera
            } else {
                super::webcam::CameraLayout::CameraOverlay
            },
            ..super::webcam::LayoutSettings::default()
        }
    }

    /// A layer and its layout timeline as the file stores them. `fullscreen`
    /// still describes the default layout, so a build that only knows that
    /// switch opens on the same starting picture.
    pub(super) fn from_layer(
        layer: super::webcam::LayerSettings,
        layouts: &super::webcam::LayoutSettings,
    ) -> Self {
        Self {
            enabled: layer.enabled,
            fullscreen: layouts.default_layout == super::webcam::CameraLayout::FullscreenCamera,
            x: layer.x,
            y: layer.y,
            w: layer.w,
            h: layer.h,
            radius: layer.radius,
            mirror: layer.mirror,
            delay_seconds: layer.delay_seconds,
            zoomed_scale: layer.zoomed_scale,
            hide_during_silence: layer.hide_during_silence,
            layouts: Some(layouts.clone()),
        }
    }
}

/// One recording's saved edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct EditorProjectFile {
    pub version: u32,
    pub source_path: PathBuf,
    pub source_size: u64,
    pub source_mtime_secs: i64,
    pub trim_start_seconds: f64,
    pub trim_end_seconds: f64,
    #[serde(default)]
    pub cuts: Vec<f64>,
    #[serde(default)]
    pub segment_starts: Vec<f64>,
    /// Playback speed of each cut piece, one entry per segment. A file written
    /// before speeds existed reads as every piece at 1×.
    #[serde(default)]
    pub segment_speeds: Vec<f64>,
    /// Source starts of typing suggestions the user applied or removed. A
    /// suggestion whose start is listed here is not offered again.
    #[serde(default)]
    pub typing_suggestions_resolved: Vec<f64>,
    #[serde(default)]
    pub freeze_tail: f64,
    #[serde(default)]
    pub zoom_clips: Vec<ZoomClipFile>,
    #[serde(default)]
    pub zoom_camera: ZoomCameraFile,
    #[serde(default)]
    pub cursor_hide_clips: Vec<HideClipFile>,
    /// The camera layer once it has been moved away from what the recording
    /// captured. Absent means the recording's own placement is still in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webcam: Option<WebcamLayerFile>,
    /// The wallpaper behind the video. Absent in a file written before the
    /// stage was modelled, which opens with no fill.
    #[serde(default)]
    pub background: StageFile,
    /// The inset around the video, against a 400px long edge. Absent reads as
    /// the panel's own starting padding rather than a square stage.
    #[serde(default = "default_padding")]
    pub background_padding: f64,
    /// The video card's corner radius, in the same units as the padding.
    /// Absent reads as square corners.
    #[serde(default)]
    pub background_radius: f64,
    #[serde(default)]
    pub playhead_seconds: f64,
    #[serde(default)]
    pub timeline_scale: f64,
    #[serde(default)]
    pub timeline_scroll_seconds: f64,
    /// The click sound by its catalog name. Absent means no sound.
    #[serde(default)]
    pub cursor_click_sound: Option<String>,
    /// The click sound's volume, from 0 to 1.
    #[serde(default = "default_click_sound_volume")]
    pub cursor_click_sound_volume: f64,
    /// Whether keyboard shortcuts are captioned over the video.
    #[serde(default)]
    pub captions_shortcuts: bool,
    /// Whether single letters and digits are captioned on their own.
    #[serde(default)]
    pub captions_show_single_letters: bool,
    /// The cursor look the project applies. Absent reads as the panel's own
    /// starting look, which is what a file written before the editor stored a
    /// cursor carries.
    #[serde(default)]
    pub cursor: super::presets::CursorFile,
    /// The project's sound: volumes, the microphone mode and the background music.
    #[serde(default)]
    pub sound: super::sound::SoundSettings,
    /// Each cut piece's own sound, one entry per segment.
    #[serde(default)]
    pub clip_sounds: Vec<super::sound::ClipSound>,
}

const DEFAULT_CLICK_SOUND_VOLUME: f64 = 0.25;

fn default_click_sound_volume() -> f64 {
    DEFAULT_CLICK_SOUND_VOLUME
}

fn default_padding() -> f64 {
    super::background::DEFAULT_PADDING
}

/// The effects a project carries beyond the cursor and the camera: the click
/// sound, the keyboard captions and the sound.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct EffectsSettings {
    pub click_sound: Option<String>,
    pub click_sound_volume: f64,
    pub captions: super::keystrokes::Settings,
    /// The project's sound settings.
    pub sound: super::sound::SoundSettings,
    /// Each cut piece's own sound, one entry per segment, like `segment_speeds`.
    pub clip_sounds: Vec<super::sound::ClipSound>,
}

impl Default for EffectsSettings {
    fn default() -> Self {
        Self {
            click_sound: None,
            click_sound_volume: DEFAULT_CLICK_SOUND_VOLUME,
            captions: super::keystrokes::Settings::default(),
            sound: super::sound::SoundSettings::default(),
            clip_sounds: Vec::new(),
        }
    }
}

impl EffectsSettings {
    /// Only the sound this editor ships is kept, and a volume that is not a
    /// number or is out of range falls back to the default level.
    pub(super) fn clamped(self) -> Self {
        Self {
            click_sound: self.click_sound.filter(|sound| sound == "soft"),
            click_sound_volume: if self.click_sound_volume.is_finite() {
                self.click_sound_volume.clamp(0.0, 1.0)
            } else {
                DEFAULT_CLICK_SOUND_VOLUME
            },
            captions: self.captions,
            sound: self.sound.clamped(),
            clip_sounds: self
                .clip_sounds
                .into_iter()
                .map(super::sound::ClipSound::clamped)
                .collect(),
        }
    }
}

impl EditorProjectFile {
    /// The effects this file carries, in the form the editor applies them.
    pub(super) fn effects(&self) -> EffectsSettings {
        EffectsSettings {
            click_sound: self.cursor_click_sound.clone(),
            click_sound_volume: self.cursor_click_sound_volume,
            captions: super::keystrokes::Settings {
                shortcuts: self.captions_shortcuts,
                show_single_letters: self.captions_show_single_letters,
            },
            sound: self.sound.clone(),
            clip_sounds: self.clip_sounds.clone(),
        }
        .clamped()
    }

    /// Whether the file on disk is still the footage this project was written
    /// against. A recording replaced in place keeps its path, so size and
    /// modification time are what tell the two apart.
    pub(super) fn source_is_current(&self) -> bool {
        fingerprint(&self.source_path).is_some_and(|(size, mtime)| {
            size == self.source_size && mtime == self.source_mtime_secs
        })
    }

    /// A display name for a History card: the recording's file name.
    pub(super) fn display_name(&self) -> String {
        self.source_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.source_path.display().to_string())
    }

    /// Whether two snapshots describe the same edits. The view — playhead,
    /// timeline zoom and scroll — does not count: scrolling or playing is not
    /// an edit, so it must not trigger a write on its own.
    fn same_content(&self, other: &Self) -> bool {
        let without_view = |file: &Self| Self {
            playhead_seconds: 0.0,
            timeline_scale: 0.0,
            timeline_scroll_seconds: 0.0,
            ..file.clone()
        };
        without_view(self) == without_view(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct ZoomClipFile {
    pub start: f64,
    pub end: f64,
    pub scale: f64,
    pub center: (f64, f64),
    #[serde(default)]
    pub mode: ZoomModeFile,
    #[serde(default)]
    pub instant: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<ZoomAnchorFile>,
}

/// The footage a generated clip follows through composition edits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct ZoomAnchorFile {
    pub source_start: f64,
    pub source_end: f64,
}

/// A clip missing its mode loads as Manual: holding a chosen focus point is the
/// safe reading of a clip whose behaviour is unknown, since Auto would move a
/// framing nobody asked it to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ZoomModeFile {
    Auto,
    #[default]
    Manual,
}

/// The camera-wide knobs, under the shared editor's names.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct ZoomCameraFile {
    pub speed: f64,
    pub smoothness: f64,
    pub start_early: bool,
    #[serde(default = "default_motion_blur")]
    pub motion_blur: f64,
}

fn default_motion_blur() -> f64 {
    1.0
}

impl Default for ZoomCameraFile {
    fn default() -> Self {
        Self {
            speed: 1.0,
            smoothness: 0.5,
            start_early: false,
            motion_blur: default_motion_blur(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct HideClipFile {
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub hidden: bool,
}

/// Where this build keeps saved edits. Beside the pointer sidecars the recorder
/// writes, so the two halves of an edit session share one root.
fn project_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("apexshot")
        .join("video-projects-windows")
}

/// The project file for one recording, named by the hash of its canonical path
/// so a copied or moved project cannot land on different footage.
pub(super) fn project_path_for_video(video: &Path) -> PathBuf {
    let canonical = video.canonicalize().unwrap_or_else(|_| video.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    project_directory().join(format!("{:x}.json", hasher.finalize()))
}

/// Size and modification time of the recording, in the shape the file stores.
pub(super) fn fingerprint(video: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(video).ok()?;
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((metadata.len(), mtime))
}

fn write_at(project_path: &Path, file: &EditorProjectFile) -> std::io::Result<()> {
    if let Some(directory) = project_path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let json = serde_json::to_string_pretty(file)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let temp_path = project_path.with_extension("json.tmp");
    std::fs::write(&temp_path, json.as_bytes())?;
    std::fs::rename(&temp_path, project_path)?;
    Ok(())
}

fn read_at(project_path: &Path, video: &Path) -> Option<EditorProjectFile> {
    let json = std::fs::read_to_string(project_path).ok()?;
    let file: EditorProjectFile = match serde_json::from_str(&json) {
        Ok(file) => file,
        Err(err) => {
            eprintln!(
                "[windows-video-editor] failed to parse project {}: {err}",
                project_path.display()
            );
            return None;
        }
    };
    let (size, mtime) = fingerprint(video)?;
    if size != file.source_size || mtime != file.source_mtime_secs {
        return None;
    }
    Some(file)
}

pub(super) fn save_project(video: &Path, file: &EditorProjectFile) -> std::io::Result<()> {
    write_at(&project_path_for_video(video), file)
}

pub(super) fn load_project(video: &Path) -> Option<EditorProjectFile> {
    read_at(&project_path_for_video(video), video)
}

pub(super) fn delete_project(video: &Path) {
    let project_path = project_path_for_video(video);
    if project_path.exists() {
        if let Err(err) = std::fs::remove_file(&project_path) {
            eprintln!(
                "[windows-video-editor] failed to delete project {}: {err}",
                project_path.display()
            );
        }
    }
}

/// Every saved edit whose recording is still on disk, newest edit first. This
/// is what the History window's Video Edits page lists.
pub(super) fn list_projects() -> Vec<EditorProjectFile> {
    let Ok(entries) = std::fs::read_dir(project_directory()) else {
        return Vec::new();
    };
    let mut saved: Vec<(SystemTime, EditorProjectFile)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let saved_at = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        let Ok(json) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<EditorProjectFile>(&json) else {
            continue;
        };
        if !file.source_is_current() {
            continue;
        }
        saved.push((saved_at, file));
    }
    saved.sort_by_key(|left| std::cmp::Reverse(left.0));
    saved.into_iter().map(|(_, file)| file).collect()
}

/// The last state written, so a repaint-driven tick can tell a real edit from a
/// redraw and a playhead tick from a cut.
#[derive(Clone, Default)]
pub(super) struct ProjectSync {
    last: Rc<RefCell<Option<EditorProjectFile>>>,
    dirty: Rc<Cell<bool>>,
}

impl ProjectSync {
    /// Note that the editor repainted. Cheap; the decision waits for the tick.
    pub(super) fn mark_changed(&self) {
        self.dirty.set(true);
    }

    pub(super) fn take_changed(&self) -> bool {
        self.dirty.replace(false)
    }

    /// Take the state just loaded as the baseline: nothing is worth writing
    /// until the user changes something.
    pub(super) fn adopt(&self, loaded: Option<EditorProjectFile>) {
        *self.last.borrow_mut() = loaded;
        self.dirty.set(false);
    }

    /// Write the snapshot when it differs from the file on disk, and delete the
    /// file when the session is back to a freshly opened recording.
    ///
    /// `force` also records a change confined to the view — the playhead, the
    /// timeline zoom and the scroll. Those belong in the file but are saved
    /// when the window closes or the document changes, not mid-edit.
    pub(super) fn sync(&self, snapshot: Option<EditorProjectFile>, force: bool) -> bool {
        let unchanged = match (self.last.borrow().as_ref(), snapshot.as_ref()) {
            (None, None) => true,
            (Some(saved), Some(current)) => {
                saved == current || (!force && saved.same_content(current))
            }
            _ => false,
        };
        if unchanged {
            return false;
        }
        match &snapshot {
            Some(file) => {
                if let Err(err) = save_project(&file.source_path, file) {
                    eprintln!(
                        "[windows-video-editor] failed to save project for {}: {err}",
                        file.source_path.display()
                    );
                    return false;
                }
            }
            None => {
                if let Some(saved) = self.last.borrow().as_ref() {
                    delete_project(&saved.source_path);
                }
            }
        }
        *self.last.borrow_mut() = snapshot;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::webcam::{CameraLayout, LayoutCue, LayoutSettings};
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "apexshot-windows-project-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn recording(directory: &Path) -> PathBuf {
        let video = directory.join("recording.mp4");
        fs::write(&video, b"not really a video").unwrap();
        video
    }

    fn project(video: &Path) -> EditorProjectFile {
        let (source_size, source_mtime_secs) = fingerprint(video).unwrap();
        EditorProjectFile {
            version: PROJECT_VERSION,
            source_path: video.to_path_buf(),
            source_size,
            source_mtime_secs,
            trim_start_seconds: 1.5,
            trim_end_seconds: 12.0,
            cuts: vec![4.0, 8.0],
            segment_starts: vec![0.0, 2.5, 6.5],
            segment_speeds: vec![1.0, 2.0, 1.0],
            typing_suggestions_resolved: vec![4.0],
            freeze_tail: 0.75,
            zoom_clips: vec![ZoomClipFile {
                start: 0.5,
                end: 2.3,
                scale: 2.0,
                center: (0.4, 0.6),
                mode: ZoomModeFile::Auto,
                instant: true,
                hidden: false,
                anchor: Some(ZoomAnchorFile {
                    source_start: 2.0,
                    source_end: 3.8,
                }),
            }],
            zoom_camera: ZoomCameraFile {
                speed: 1.5,
                smoothness: 0.25,
                start_early: true,
                motion_blur: 0.7,
            },
            cursor_hide_clips: vec![HideClipFile {
                start: 3.0,
                end: 4.0,
                hidden: false,
            }],
            webcam: Some(WebcamLayerFile {
                enabled: true,
                fullscreen: false,
                x: 0.62,
                y: 0.31,
                w: 0.18,
                h: 0.24,
                radius: 0.5,
                mirror: true,
                delay_seconds: 0.0,
                zoomed_scale: super::super::webcam::ZOOMED_CAMERA_SCALE,
                hide_during_silence: None,
                layouts: Some(LayoutSettings {
                    default_layout: CameraLayout::SplitScreen,
                    cues: vec![
                        LayoutCue {
                            at: 1.5,
                            layout: CameraLayout::FullscreenCamera,
                            position: None,
                            cutout_x: None,
                        },
                        LayoutCue {
                            at: 4.25,
                            layout: CameraLayout::CameraOverlay,
                            position: None,
                            cutout_x: None,
                        },
                        LayoutCue {
                            at: 9.0,
                            layout: CameraLayout::ScreenOnly,
                            position: None,
                            cutout_x: None,
                        },
                    ],
                    transition_seconds: 0.6,
                    split_ratio: 0.58,
                    split_flip: true,
                    ..LayoutSettings::default()
                }),
            }),
            background: StageFile::default(),
            background_padding: default_padding(),
            background_radius: 0.0,
            playhead_seconds: 3.5,
            timeline_scale: 42.0,
            timeline_scroll_seconds: 1.25,
            cursor_click_sound: None,
            cursor_click_sound_volume: DEFAULT_CLICK_SOUND_VOLUME,
            captions_shortcuts: false,
            captions_show_single_letters: false,
            cursor: super::super::presets::CursorFile::default(),
            sound: Default::default(),
            clip_sounds: Vec::new(),
        }
    }

    #[test]
    fn an_older_zoom_camera_defaults_motion_blur_on() {
        let camera: super::ZoomCameraFile =
            serde_json::from_str(r#"{"speed":1.0,"smoothness":0.5,"start_early":false}"#).unwrap();
        assert_eq!(camera.motion_blur, 1.0);
    }

    #[test]
    fn a_project_round_trips_through_its_file() {
        let directory = scratch("round-trip");
        let video = recording(&directory);
        let file = project(&video);
        let path = directory.join("round-trip.json");

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");

        let layouts = loaded
            .webcam
            .as_ref()
            .and_then(|webcam| webcam.layouts.as_ref())
            .expect("the file carries its layout timeline");
        assert_eq!(layouts.cues.len(), 3);
        assert_eq!(layouts.cues[1].at, 4.25);
        assert_eq!(layouts.cues[2].layout, CameraLayout::ScreenOnly);
        assert_eq!(layouts.default_layout, CameraLayout::SplitScreen);
        assert_eq!(loaded, file);
    }

    #[test]
    fn the_stage_round_trips_through_the_project_file() {
        let directory = scratch("stage");
        let video = recording(&directory);
        let path = directory.join("stage.json");
        let mut file = project(&video);
        file.background = StageFile {
            selection: Some("wallpaper-004.jpg".to_owned()),
            aspect: None,
        };
        file.background_padding = 48.0;
        file.background_radius = 6.0;

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");

        assert_eq!(
            loaded.background.selection.as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert!((loaded.background_padding - 48.0).abs() < 1e-9);
        assert!((loaded.background_radius - 6.0).abs() < 1e-9);
        assert_eq!(loaded, file);
    }

    #[test]
    fn the_stage_frame_round_trips_through_the_project_file() {
        let directory = scratch("stage-aspect");
        let video = recording(&directory);
        let path = directory.join("stage-aspect.json");
        let mut file = project(&video);
        file.background = StageFile {
            selection: None,
            aspect: Some("9:16".to_owned()),
        };

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");

        assert_eq!(loaded.background.aspect.as_deref(), Some("9:16"));
        assert_eq!(loaded, file);
    }

    #[test]
    fn a_stage_without_a_frame_keeps_the_recording_shape() {
        let stage: StageFile =
            serde_json::from_str(r#"{"selection":null}"#).expect("a stage without a frame reads");
        assert_eq!(stage.aspect, None);
    }

    #[test]
    fn a_version_two_file_opens_on_a_flat_stage() {
        let directory = scratch("stage-legacy");
        let video = recording(&directory);
        let path = directory.join("stage-legacy.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&project(&video)).unwrap()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.insert("version".to_owned(), serde_json::json!(2));
        for key in ["background", "background_padding", "background_radius"] {
            assert!(
                object.remove(key).is_some(),
                "the fixture writes the stage key {key}"
            );
        }
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(loaded.background.selection, None);
        assert_eq!(loaded.background_padding, 24.0);
        assert_eq!(loaded.background_radius, 0.0);
    }

    #[test]
    fn the_stage_is_content_where_the_view_is_not() {
        let directory = scratch("stage-content");
        let video = recording(&directory);
        let saved = EditorProjectFile {
            background: StageFile {
                selection: Some("wallpaper-004.jpg".to_owned()),
                aspect: None,
            },
            background_padding: 48.0,
            background_radius: 6.0,
            ..project(&video)
        };

        assert!(saved.same_content(&EditorProjectFile {
            playhead_seconds: 9.0,
            timeline_scale: 80.0,
            timeline_scroll_seconds: 4.0,
            ..saved.clone()
        }));

        assert!(!saved.same_content(&EditorProjectFile {
            background: StageFile::default(),
            ..saved.clone()
        }));

        assert!(!saved.same_content(&EditorProjectFile {
            background_padding: 24.0,
            ..saved.clone()
        }));

        assert!(!saved.same_content(&EditorProjectFile {
            background_radius: 0.0,
            ..saved.clone()
        }));
    }

    #[test]
    fn a_cutout_placement_round_trips_and_stays_inside_the_frame() {
        let directory = scratch("cutout-position");
        let video = recording(&directory);
        let path = directory.join("cutout-position.json");
        let mut file = project(&video);
        file.webcam
            .as_mut()
            .and_then(|webcam| webcam.layouts.as_mut())
            .expect("the saved camera carries its layout timeline")
            .cutout_position_x = 0.2;

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(loaded, file);
        assert!(
            (loaded
                .webcam
                .as_ref()
                .expect("the camera block survives")
                .resolved_layouts()
                .cutout_position_x
                - 0.2)
                .abs()
                < 1e-9
        );

        let mut wild = project(&video);
        wild.webcam
            .as_mut()
            .and_then(|webcam| webcam.layouts.as_mut())
            .expect("the saved camera carries its layout timeline")
            .cutout_position_x = 4.0;
        write_at(&path, &wild).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(
            loaded
                .webcam
                .as_ref()
                .expect("the camera block survives")
                .resolved_layouts()
                .cutout_position_x,
            1.0
        );
    }

    #[test]
    fn an_older_project_without_a_cutout_position_opens_centred() {
        let directory = scratch("cutout-legacy");
        let video = recording(&directory);
        let path = directory.join("cutout-legacy.json");
        let file = project(&video);
        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&file).unwrap()).unwrap();
        let removed = json
            .get_mut("webcam")
            .and_then(|webcam| webcam.get_mut("layouts"))
            .and_then(|layouts| layouts.as_object_mut())
            .expect("the layout block is an object")
            .remove("cutout_position_x");
        assert!(removed.is_some(), "the fixture writes the cutout position");
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(loaded, file);
        let layouts = loaded
            .webcam
            .as_ref()
            .expect("the camera block survives")
            .resolved_layouts();
        assert_eq!(layouts.cutout_position_x, 0.5);
        assert_eq!(layouts.default_layout, CameraLayout::SplitScreen);
    }

    /// A camera block as a build before layouts wrote it: the two switches and
    /// the placement, with no layouts key at all.
    fn legacy_webcam(enabled: bool, fullscreen: bool) -> WebcamLayerFile {
        serde_json::from_value(serde_json::json!({
            "enabled": enabled,
            "fullscreen": fullscreen,
            "x": 0.62,
            "y": 0.31,
            "w": 0.18,
            "h": 0.24,
            "radius": 0.5,
            "mirror": true,
        }))
        .expect("a legacy camera block reads")
    }

    #[test]
    fn an_older_file_derives_its_default_layout_from_the_two_switches() {
        assert_eq!(
            legacy_webcam(false, false)
                .resolved_layouts()
                .default_layout,
            CameraLayout::ScreenOnly
        );
        assert_eq!(
            legacy_webcam(true, true).resolved_layouts().default_layout,
            CameraLayout::FullscreenCamera
        );
        assert_eq!(
            legacy_webcam(true, false).resolved_layouts().default_layout,
            CameraLayout::CameraOverlay
        );
        assert!(legacy_webcam(true, false)
            .resolved_layouts()
            .cues
            .is_empty());
    }

    #[test]
    fn an_explicit_layout_wins_over_the_older_switches() {
        let mut file = legacy_webcam(false, true);
        file.layouts = Some(LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            cues: vec![LayoutCue {
                at: 2.0,
                layout: CameraLayout::CameraCutout,
                position: None,
                cutout_x: None,
            }],
            transition_seconds: 0.3,
            split_ratio: 0.4,
            split_flip: true,
            ..LayoutSettings::default()
        });

        let layouts = file.resolved_layouts();
        assert_eq!(layouts.default_layout, CameraLayout::SplitScreen);
        assert_eq!(layouts.cues.len(), 1);
        assert_eq!(layouts.cues[0].at, 2.0);
        assert_eq!(layouts.cues[0].layout, CameraLayout::CameraCutout);
    }

    #[test]
    fn the_file_keeps_the_layer_switch_beside_the_default_layout() {
        let layer = super::super::webcam::LayerSettings::default();
        let mut layouts = LayoutSettings {
            default_layout: CameraLayout::ScreenOnly,
            ..LayoutSettings::default()
        };
        let file = WebcamLayerFile::from_layer(layer, &layouts);
        assert!(file.enabled, "the layer draws; the default is the screen");
        assert!(!file.fullscreen);

        let off = super::super::webcam::LayerSettings {
            enabled: false,
            ..layer
        };
        let file = WebcamLayerFile::from_layer(off, &layouts);
        assert!(!file.enabled, "a switched-off layer writes its own switch");

        layouts.default_layout = CameraLayout::FullscreenCamera;
        let file = WebcamLayerFile::from_layer(layer, &layouts);
        assert!(file.enabled);
        assert!(file.fullscreen);
    }

    #[test]
    fn the_camera_delay_reads_as_zero_before_it_existed_and_survives_a_round_trip() {
        let legacy: WebcamLayerFile = serde_json::from_str(
            r#"{"enabled":true,"fullscreen":false,"x":0.62,"y":0.31,"w":0.18,"h":0.24,"radius":0.5,"mirror":true}"#,
        )
        .expect("a camera block without a delay reads");
        assert_eq!(legacy.delay_seconds, 0.0);

        let layer = super::super::webcam::LayerSettings {
            delay_seconds: 0.05,
            ..super::super::webcam::LayerSettings::default()
        };
        let file = WebcamLayerFile::from_layer(layer, &LayoutSettings::default());
        let text = serde_json::to_string(&file).unwrap();
        let loaded: WebcamLayerFile = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.delay_seconds, 0.05);
    }

    #[test]
    fn the_zoomed_scale_reads_as_the_studied_default_before_it_existed() {
        let legacy: WebcamLayerFile = serde_json::from_str(
            r#"{"enabled":true,"fullscreen":false,"x":0.62,"y":0.31,"w":0.18,"h":0.24,"radius":0.5,"mirror":true}"#,
        )
        .expect("a camera block without a zoomed scale reads");
        assert_eq!(
            legacy.zoomed_scale,
            super::super::webcam::ZOOMED_CAMERA_SCALE
        );
        assert_eq!(legacy.zoomed_scale, 0.8);
    }

    #[test]
    fn a_camera_block_without_a_silence_length_reads_it_as_off() {
        let legacy: WebcamLayerFile = serde_json::from_str(
            r#"{"enabled":true,"fullscreen":false,"x":0.62,"y":0.31,"w":0.18,"h":0.24,"radius":0.5,"mirror":true}"#,
        )
        .expect("a camera block without a silence length reads");
        assert_eq!(legacy.hide_during_silence, None);
    }

    #[test]
    fn a_layout_block_without_an_automatic_split_reads_it_as_on() {
        let layouts: LayoutSettings =
            serde_json::from_str(r#"{"default_layout":"split_screen","split_ratio":0.5}"#).unwrap();
        assert!(layouts.split_auto);
    }

    #[test]
    fn an_older_file_states_its_hidden_camera_as_a_screen_only_default() {
        let layouts = legacy_webcam(false, false).resolved_layouts();
        assert_eq!(layouts.default_layout, CameraLayout::ScreenOnly);
        assert!(
            !legacy_webcam(false, false).enabled,
            "the older switch is what those files set"
        );
    }

    #[test]
    fn a_replaced_recording_invalidates_its_project() {
        let directory = scratch("stale");
        let video = recording(&directory);
        let path = directory.join("stale.json");
        write_at(&path, &project(&video)).unwrap();

        fs::write(&video, b"a different recording entirely").unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        fs::write(&video, b"a different recording entirely, rewritten").unwrap();

        assert!(read_at(&path, &video).is_none());
    }

    #[test]
    fn missing_files_read_as_no_project() {
        let directory = scratch("missing");
        let video = recording(&directory);
        assert!(read_at(&directory.join("absent.json"), &video).is_none());
    }

    #[test]
    fn the_view_alone_is_not_an_edit() {
        let directory = scratch("view");
        let video = recording(&directory);
        let saved = project(&video);

        let scrolled = EditorProjectFile {
            playhead_seconds: 9.0,
            timeline_scale: 80.0,
            timeline_scroll_seconds: 4.0,
            ..saved.clone()
        };
        assert!(saved.same_content(&scrolled));

        let trimmed = EditorProjectFile {
            trim_start_seconds: 2.0,
            ..saved.clone()
        };
        assert!(!saved.same_content(&trimmed));

        let rezoomed = EditorProjectFile {
            zoom_clips: Vec::new(),
            ..saved.clone()
        };
        assert!(!saved.same_content(&rezoomed));

        let moved_camera = EditorProjectFile {
            webcam: None,
            ..saved.clone()
        };
        assert!(!saved.same_content(&moved_camera));

        let relaid = EditorProjectFile {
            webcam: saved.webcam.as_ref().map(|webcam| WebcamLayerFile {
                layouts: Some(LayoutSettings {
                    default_layout: CameraLayout::ScreenOnly,
                    ..LayoutSettings::default()
                }),
                ..webcam.clone()
            }),
            ..saved.clone()
        };
        assert!(!saved.same_content(&relaid));
    }

    #[test]
    fn a_project_without_a_camera_still_opens() {
        let directory = scratch("no-webcam");
        let video = recording(&directory);
        let path = directory.join("no-webcam.json");
        let file = project(&video);
        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&file).unwrap()).unwrap();
        // A project written before the camera layer existed has no key at all.
        json.as_object_mut().unwrap().remove("webcam");
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = read_at(&path, &video).expect("project loads");
        assert!(loaded.webcam.is_none());
        assert_eq!(loaded.trim_start_seconds, file.trim_start_seconds);
    }

    #[test]
    fn each_recording_gets_its_own_project_path() {
        let directory = scratch("paths");
        let first = recording(&directory);
        let second = directory.join("second.mp4");
        fs::write(&second, b"another video").unwrap();

        assert_eq!(
            project_path_for_video(&first),
            project_path_for_video(&first)
        );
        assert_ne!(
            project_path_for_video(&first),
            project_path_for_video(&second)
        );
        assert_eq!(
            project_path_for_video(&first)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("json")
        );
    }

    #[test]
    fn a_saved_edit_list_is_what_history_lists() {
        let directory = scratch("listing");
        let video = recording(&directory);
        let file = project(&video);

        save_project(&video, &file).unwrap();
        // The store is the machine's real one, shared with the user's own
        // projects and with tests running in parallel whose recordings can
        // vanish mid-listing, so only this test's own entry is checked.
        let listed = list_projects();
        let ours = listed
            .iter()
            .find(|saved| saved.source_path == video)
            .expect("the saved project is listed");
        assert!(ours.source_is_current());

        delete_project(&video);
        assert!(!list_projects()
            .iter()
            .any(|saved| saved.source_path == video));

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_sync_writes_once_per_change_and_deletes_when_the_session_is_fresh() {
        let directory = scratch("sync");
        let video = recording(&directory);
        let sync = ProjectSync::default();
        let file = project(&video);
        let on_disk = project_path_for_video(&video);

        sync.adopt(None);
        assert!(sync.sync(Some(file.clone()), false));
        assert!(on_disk.is_file());

        assert!(!sync.sync(Some(file.clone()), false));

        let view_only = EditorProjectFile {
            playhead_seconds: 7.0,
            ..file.clone()
        };
        assert!(!sync.sync(Some(view_only.clone()), false));
        assert!(sync.sync(Some(view_only), true));

        assert!(sync.sync(None, true));
        assert!(!on_disk.exists());

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_seeded_project_is_saved_at_once_and_not_rewritten() {
        let directory = scratch("seed");
        let video = recording(&directory);
        let mut file = project(&video);
        super::super::import::seed_with_preset(&mut file, None);
        let sync = ProjectSync::default();
        sync.adopt(None);

        assert!(sync.sync(Some(file.clone()), true));
        let saved = load_project(&video).expect("the seeded project reloads");
        assert_eq!(
            saved.background.selection.as_deref(),
            Some("wallpaper-004.jpg")
        );
        assert!((saved.background_padding - super::super::import::IMPORT_PADDING).abs() < 1e-9);

        assert!(!sync.sync(Some(file), true));

        delete_project(&video);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_effects_round_trip_through_their_flat_fields() {
        let directory = scratch("effects-round-trip");
        let video = recording(&directory);
        let path = directory.join("effects-round-trip.json");
        let mut file = project(&video);
        file.cursor_click_sound = Some("soft".to_owned());
        file.cursor_click_sound_volume = 0.6;
        file.captions_shortcuts = true;
        file.captions_show_single_letters = true;

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");

        assert_eq!(loaded.cursor_click_sound.as_deref(), Some("soft"));
        assert!((loaded.cursor_click_sound_volume - 0.6).abs() < 1e-9);
        assert_eq!(
            loaded.effects(),
            EffectsSettings {
                click_sound: Some("soft".to_owned()),
                click_sound_volume: 0.6,
                captions: super::super::keystrokes::Settings {
                    shortcuts: true,
                    show_single_letters: true,
                },
                sound: Default::default(),
                clip_sounds: Vec::new(),
            }
        );
        assert_eq!(loaded, file);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_sound_round_trips_through_the_project_file() {
        let directory = scratch("sound-round-trip");
        let video = recording(&directory);
        let path = directory.join("sound-round-trip.json");
        let mut file = project(&video);
        file.sound.volume = 0.5;
        file.sound.microphone_preset = super::super::sound::MicPreset::Loud;
        file.clip_sounds = vec![super::super::sound::ClipSound {
            muted: true,
            ..super::super::sound::ClipSound::default()
        }];

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");
        let effects = loaded.effects();

        assert!((effects.sound.volume - 0.5).abs() < 1e-9);
        assert_eq!(
            effects.sound.microphone_preset,
            super::super::sound::MicPreset::Loud
        );
        assert_eq!(effects.clip_sounds.len(), 1);
        assert!(effects.clip_sounds[0].muted);
        assert_eq!(loaded, file);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_project_without_sound_keys_opens_on_the_defaults() {
        let directory = scratch("sound-legacy");
        let video = recording(&directory);
        let path = directory.join("sound-legacy.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&project(&video)).unwrap()).unwrap();
        let object = json.as_object_mut().unwrap();
        for key in ["sound", "clip_sounds"] {
            assert!(
                object.remove(key).is_some(),
                "the fixture writes the sound key {key}"
            );
        }
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(
            loaded.effects().sound,
            super::super::sound::SoundSettings::default()
        );
        assert!(loaded.effects().clip_sounds.is_empty());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_version_two_file_opens_with_no_effects() {
        let directory = scratch("effects-legacy");
        let video = recording(&directory);
        let path = directory.join("effects-legacy.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&project(&video)).unwrap()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.insert("version".to_owned(), serde_json::json!(2));
        for key in [
            "cursor_click_sound",
            "cursor_click_sound_volume",
            "captions_shortcuts",
            "captions_show_single_letters",
        ] {
            assert!(
                object.remove(key).is_some(),
                "the fixture writes the effect key {key}"
            );
        }
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(loaded.effects(), EffectsSettings::default());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_cursor_look_round_trips_and_reads_the_starting_look_before_it_existed() {
        let directory = scratch("cursor-look");
        let video = recording(&directory);
        let path = directory.join("cursor-look.json");
        let cursor = super::super::presets::CursorFile {
            theme: "white".to_owned(),
            size: 1.6,
            click_effect: "circle".to_owned(),
            ..super::super::presets::CursorFile::default()
        };
        let file = EditorProjectFile {
            cursor: cursor.clone(),
            ..project(&video)
        };

        write_at(&path, &file).unwrap();
        let loaded = read_at(&path, &video).expect("project loads");
        assert_eq!(loaded.cursor, cursor);

        let mut json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&project(&video)).unwrap()).unwrap();
        assert!(
            json.as_object_mut().unwrap().remove("cursor").is_some(),
            "the fixture writes the cursor key"
        );
        fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        let legacy = read_at(&path, &video).expect("project loads");
        assert_eq!(legacy.cursor, super::super::presets::CursorFile::default());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn effects_keep_only_the_shipped_sound_and_a_usable_volume() {
        let clamped = |sound: &str, volume: f64| {
            EffectsSettings {
                click_sound: Some(sound.to_owned()),
                click_sound_volume: volume,
                captions: Default::default(),
                sound: Default::default(),
                clip_sounds: Vec::new(),
            }
            .clamped()
        };
        assert_eq!(clamped("soft", 0.5).click_sound.as_deref(), Some("soft"));
        assert_eq!(clamped("hard", 0.5).click_sound, None);
        assert_eq!(clamped("soft", 3.0).click_sound_volume, 1.0);
        assert_eq!(clamped("soft", -1.0).click_sound_volume, 0.0);
        assert_eq!(clamped("soft", f64::NAN).click_sound_volume, 0.25);
        assert_eq!(EffectsSettings::default().click_sound, None);
        assert_eq!(EffectsSettings::default().click_sound_volume, 0.25);
    }
}
