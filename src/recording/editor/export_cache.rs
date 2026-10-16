use super::model::{VideoBackground, VideoEditState};
use super::project::VideoProjectFile;
use super::sidecar::PointerSidecar;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq)]
struct FileFingerprint {
    path: PathBuf,
    size: u64,
    modified_ns: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl FileFingerprint {
    fn capture(path: &Path) -> Option<Self> {
        let canonical_path = path.canonicalize().ok()?;
        let metadata = fs::metadata(&canonical_path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        let modified_ns = metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos();

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Some(Self {
                path: canonical_path,
                size: metadata.len(),
                modified_ns,
                device: metadata.dev(),
                inode: metadata.ino(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            })
        }

        #[cfg(not(unix))]
        {
            Some(Self {
                path: canonical_path,
                size: metadata.len(),
                modified_ns,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RenderKey {
    project: VideoProjectFile,
    sidecar: Option<PointerSidecar>,
    metadata: MediaMetadata,
    dependencies: Vec<FileFingerprint>,
}

#[derive(Debug, Clone)]
/// Input snapshot captured before an export starts.
pub(crate) struct RenderInputs(RenderKey);

#[derive(Debug, Clone, PartialEq)]
struct MediaMetadata {
    duration_seconds: f64,
    width: u32,
    height: u32,
    file_size_bytes: u64,
    has_audio: bool,
    frame_rate: f64,
}

impl RenderKey {
    fn capture(state: &VideoEditState) -> Option<Self> {
        let mut project = state.to_project();
        project.playhead_seconds = 0.0;
        project.timeline_scale = 0.0;
        project.timeline_scroll_seconds = 0.0;
        project.selected_zoom = None;
        project.selected_cursor_hide = None;

        let mut paths = BTreeSet::from([state.metadata.path.clone()]);
        paths.extend(state.project_media.iter().map(|media| media.path.clone()));
        if let VideoBackground::Wallpaper(path) = &state.background {
            paths.insert(path.clone());
        }
        let dependencies = paths
            .iter()
            .map(|path| FileFingerprint::capture(path))
            .collect::<Option<Vec<_>>>()?;

        Some(Self {
            project,
            sidecar: state.sidecar.clone(),
            metadata: MediaMetadata {
                duration_seconds: state.metadata.duration_seconds,
                width: state.metadata.width,
                height: state.metadata.height,
                file_size_bytes: state.metadata.file_size_bytes,
                has_audio: state.metadata.has_audio,
                frame_rate: state.metadata.frame_rate,
            },
            dependencies,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OutputFingerprint {
    file: FileFingerprint,
    sha256: [u8; 32],
}

impl OutputFingerprint {
    fn capture(path: &Path, cancellation: Option<&AtomicBool>) -> Option<Self> {
        let before = FileFingerprint::capture(path)?;
        let mut reader = BufReader::new(File::open(path).ok()?);
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            if cancellation_requested(cancellation) {
                return None;
            }
            let read = reader.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if cancellation_requested(cancellation) {
            return None;
        }
        let after = FileFingerprint::capture(path)?;
        if cancellation_requested(cancellation) {
            return None;
        }
        (before == after).then(|| Self {
            file: after,
            sha256: hasher.finalize().into(),
        })
    }
}

#[derive(Debug, Clone)]
struct CachedRender {
    key: RenderKey,
    path: PathBuf,
    output: OutputFingerprint,
}

#[derive(Default)]
struct ExportCache {
    entry: Mutex<Option<Arc<CachedRender>>>,
}

impl ExportCache {
    fn lookup(&self, state: &VideoEditState, cancellation: Option<&AtomicBool>) -> Option<PathBuf> {
        if cancellation_requested(cancellation) {
            return None;
        }
        let key = RenderKey::capture(state)?;
        if cancellation_requested(cancellation) {
            return None;
        }
        let cached = self.entry.lock().ok()?.as_ref().cloned()?;
        if cached.key != key || cancellation_requested(cancellation) {
            return None;
        }
        let output = OutputFingerprint::capture(&cached.path, cancellation)?;
        if output != cached.output || cancellation_requested(cancellation) {
            return None;
        }
        if RenderKey::capture(state)? != key || cancellation_requested(cancellation) {
            return None;
        }
        Some(cached.path.clone())
    }

    fn remember(&self, key: RenderKey, path: &Path, cancellation: Option<&AtomicBool>) {
        let Some(output) = OutputFingerprint::capture(path, cancellation) else {
            return;
        };
        if cancellation_requested(cancellation) {
            return;
        }
        if let Ok(mut entry) = self.entry.lock() {
            *entry = Some(Arc::new(CachedRender {
                key,
                path: path.to_path_buf(),
                output,
            }));
        }
    }
}

fn global() -> &'static ExportCache {
    static CACHE: OnceLock<ExportCache> = OnceLock::new();
    CACHE.get_or_init(ExportCache::default)
}

/// Return the last successful export when its inputs and output are unchanged.
pub(crate) fn find(state: &VideoEditState, cancellation: Option<&AtomicBool>) -> Option<PathBuf> {
    crate::studio_entitlement::ensure_export_allowed(
        crate::studio_entitlement::capabilities_for_video(state),
    )
    .ok()?;
    global().lookup(state, cancellation)
}

/// Capture the render-affecting inputs before running an export.
pub(crate) fn capture_inputs(state: &VideoEditState) -> Option<RenderInputs> {
    RenderKey::capture(state).map(RenderInputs)
}

/// Remember a successful output only if all render inputs stayed unchanged.
pub(crate) fn remember_if_inputs_unchanged(
    before: Option<RenderInputs>,
    state: &VideoEditState,
    path: &Path,
    cancellation: Option<&AtomicBool>,
) {
    let Some(RenderInputs(before)) = before else {
        return;
    };
    if !cancellation_requested(cancellation) && RenderKey::capture(state).as_ref() == Some(&before)
    {
        global().remember(before, path, cancellation);
    }
}

/// Remove a completed cache entry when its export is cancelled before return.
pub(crate) fn forget(path: &Path) {
    if let Ok(mut entry) = global().entry.lock() {
        if entry.as_ref().is_some_and(|cached| cached.path == path) {
            *entry = None;
        }
    }
}

fn cancellation_requested(cancellation: Option<&AtomicBool>) -> bool {
    cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::{
        AudioMode, CropSelection, CursorTheme, VideoMetadata, ZoomClip,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "apexshot-export-cache-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn state_for(source: &Path) -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: source.to_path_buf(),
            duration_seconds: 4.0,
            width: 1280,
            height: 720,
            file_size_bytes: 8,
            has_audio: true,
            frame_rate: 60.0,
        })
    }

    #[test]
    fn unchanged_render_cache_ignores_playhead_timeline_view_and_selection() {
        let dir = scratch_dir();
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        fs::write(&source, b"source").unwrap();
        let mut state = state_for(&source);
        let original = RenderKey::capture(&state).unwrap();
        state.playhead_seconds = 2.0;
        state.timeline_scale = 73.0;
        state.timeline_scroll_seconds = 1.5;
        state.selected_zoom = Some(2);
        state.selected_cursor_hide = Some(1);
        assert_eq!(RenderKey::capture(&state).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn render_cache_invalidates_on_edit_sidecar_and_external_file_changes() {
        use crate::recording::editor::sidecar::{CaptureRegion, PointerSidecar};

        let dir = scratch_dir();
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let wallpaper = dir.join("wallpaper.png");
        fs::write(&source, b"source").unwrap();
        fs::write(&wallpaper, b"wallpaper").unwrap();
        let mut state = state_for(&source);
        state.background = VideoBackground::Wallpaper(wallpaper.clone());
        let original = RenderKey::capture(&state).unwrap();

        let mut edited = state.clone();
        edited.audio_mode = AudioMode::Muted;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.quality = crate::recording::editor::model::ExportQuality::Ultra;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.zoom_camera.motion_blur = 0.5;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.zoom_clips.push(ZoomClip::default());
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.cursor.size += 1.0;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.cursor.shadow = 0.8;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.cursor.theme = CursorTheme::Yaru;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.freeze_tail = 0.5;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.background_padding += 1.0;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.trim_end_seconds -= 0.5;
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);
        edited = state.clone();
        edited.crop = Some(CropSelection {
            x: 10,
            y: 20,
            width: 640,
            height: 360,
        });
        assert_ne!(RenderKey::capture(&edited).unwrap(), original);

        let mut with_sidecar = state.clone();
        with_sidecar.sidecar = Some(PointerSidecar::new(
            1,
            CaptureRegion::from_capture(None, None, None, None),
        ));
        assert_ne!(RenderKey::capture(&with_sidecar).unwrap(), original);

        fs::write(&wallpaper, b"WallpapER").unwrap();
        assert_ne!(RenderKey::capture(&state).unwrap(), original);
        fs::write(&source, b"Source").unwrap();
        assert_ne!(RenderKey::capture(&state).unwrap(), original);

        state.project_media.push(super::super::model::ProjectMedia {
            path: dir.join("missing-audio.wav"),
            display_name: "missing audio".into(),
            kind: super::super::model::ProjectMediaKind::Audio,
            duration_seconds: None,
        });
        assert!(RenderKey::capture(&state).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn render_cache_rejects_missing_or_modified_outputs() {
        let dir = scratch_dir();
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let output = dir.join("edited.mp4");
        fs::write(&source, b"source").unwrap();
        fs::write(&output, b"rendered").unwrap();
        let state = state_for(&source);
        let cache = ExportCache::default();
        cache.remember(RenderKey::capture(&state).unwrap(), &output, None);
        assert_eq!(cache.lookup(&state, None), Some(output.clone()));
        fs::write(&output, b"altered!").unwrap();
        assert_eq!(cache.lookup(&state, None), None);
        fs::remove_file(&output).unwrap();
        assert_eq!(cache.lookup(&state, None), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelled_render_lookup_preserves_the_cached_export_file() {
        let dir = scratch_dir();
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let output = dir.join("edited.mp4");
        fs::write(&source, b"source").unwrap();
        fs::write(&output, b"rendered").unwrap();
        let state = state_for(&source);
        let cache = ExportCache::default();
        cache.remember(RenderKey::capture(&state).unwrap(), &output, None);
        let original = fs::read(&output).unwrap();
        let cancellation = AtomicBool::new(true);

        assert_eq!(cache.lookup(&state, Some(&cancellation)), None);
        assert_eq!(fs::read(&output).unwrap(), original);
        cancellation.store(false, Ordering::Release);
        assert_eq!(
            cache.lookup(&state, Some(&cancellation)),
            Some(output.clone())
        );
        assert_eq!(fs::read(&output).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }
}
