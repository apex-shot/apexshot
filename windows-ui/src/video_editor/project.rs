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
//! `zoom_camera`, `cursor_hide_clips`, and the three view fields. Merging the
//! two schemas later is then a promotion rather than a translation. What this
//! editor does not model — background, crop, dimension, quality, audio, extra
//! media — has no key here, so the file never claims a feature the editor would
//! not honour.
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
pub(super) const PROJECT_VERSION: u32 = 2;

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
    /// The layout timeline: the recording's default layout and the changes
    /// that switch it while the composition plays. Absent in files written
    /// before layouts existed, where the two switches above still say what the
    /// default was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layouts: Option<super::webcam::LayoutSettings>,
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
    #[serde(default)]
    pub playhead_seconds: f64,
    #[serde(default)]
    pub timeline_scale: f64,
    #[serde(default)]
    pub timeline_scroll_seconds: f64,
}

impl EditorProjectFile {
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
}

impl Default for ZoomCameraFile {
    fn default() -> Self {
        Self {
            speed: 1.0,
            smoothness: 0.5,
            start_early: false,
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
    saved.sort_by(|left, right| right.0.cmp(&left.0));
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
                layouts: Some(LayoutSettings {
                    default_layout: CameraLayout::SplitScreen,
                    cues: vec![
                        LayoutCue {
                            at: 1.5,
                            layout: CameraLayout::FullscreenCamera,
                        },
                        LayoutCue {
                            at: 4.25,
                            layout: CameraLayout::CameraOverlay,
                        },
                        LayoutCue {
                            at: 9.0,
                            layout: CameraLayout::ScreenOnly,
                        },
                    ],
                    transition_seconds: 0.6,
                    split_ratio: 0.58,
                    split_flip: true,
                    ..LayoutSettings::default()
                }),
            }),
            playhead_seconds: 3.5,
            timeline_scale: 42.0,
            timeline_scroll_seconds: 1.25,
        }
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
        let listed = list_projects();
        assert!(listed.iter().any(|saved| saved.source_path == video));
        assert!(listed.iter().all(|saved| saved.source_is_current()));

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
}
