//! The Motion videos the Windows image editor exported.
//!
//! A Motion export is an MP4 in the same folder recordings land in, so a
//! folder scan cannot tell them apart. The editor records each finished
//! export in a small ledger; History lists those under Motion Edits and
//! keeps them out of Recordings. Files that carry the editor's default
//! export name (`<image> Motion.mp4`) count too, so videos exported before
//! the ledger existed move over as well.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::scan::{self, CaptureEntry, MediaKind};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    exports: Vec<PathBuf>,
}

fn ledger_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("apexshot")
        .join("motion-exports.json")
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Windows paths compare without case.
fn key(path: &Path) -> String {
    canonical(path).to_string_lossy().to_lowercase()
}

fn read_ledger_at(ledger: &Path) -> Ledger {
    std::fs::read_to_string(ledger)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Remember `video` as a Motion export.
pub fn record_export(video: &Path) {
    record_export_at(&ledger_path(), video);
}

fn record_export_at(ledger: &Path, video: &Path) {
    let mut entries = read_ledger_at(ledger);
    let video = canonical(video);
    let video_key = key(&video);
    // Deleted videos drop out as the ledger is rewritten, so it never grows
    // past what is still on disk.
    entries
        .exports
        .retain(|path| path.exists() && key(path) != video_key);
    entries.exports.push(video);
    if let Some(directory) = ledger.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    let Ok(json) = serde_json::to_string_pretty(&entries) else {
        return;
    };
    let temporary = ledger.with_extension("json.tmp");
    if std::fs::write(&temporary, json).is_ok() {
        let _ = std::fs::rename(&temporary, ledger);
    }
}

fn recorded_keys_at(ledger: &Path) -> HashSet<String> {
    read_ledger_at(ledger)
        .exports
        .iter()
        .map(|path| key(path))
        .collect()
}

/// The editor's default export name, `<image> Motion.mp4`.
fn has_motion_export_name(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase().ends_with(" motion.mp4"))
        .unwrap_or(false)
}

fn is_motion_export(path: &Path, recorded: &HashSet<String>) -> bool {
    has_motion_export_name(path) || recorded.contains(&key(path))
}

/// `entries` without the Motion exports, for the Recordings page.
pub fn without_motion_exports(entries: Vec<CaptureEntry>) -> Vec<CaptureEntry> {
    let recorded = recorded_keys_at(&ledger_path());
    entries
        .into_iter()
        .filter(|entry| !is_motion_export(&entry.path, &recorded))
        .collect()
}

fn entry_for(path: &Path) -> Option<CaptureEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let display_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Some(CaptureEntry {
        path: path.to_path_buf(),
        display_name,
        modified: metadata.modified().ok(),
        size_bytes: metadata.len(),
        kind: MediaKind::Video,
    })
}

/// Every Motion export still on disk, newest first: the recorded ones plus
/// default-named videos in the recordings and Motion export folders.
pub fn list_exports() -> Vec<CaptureEntry> {
    let config = crate::config::load_config();
    let recordings = scan::recording_folder(&config);
    let motion_folder =
        config.motion_export_dir(&crate::recording::recording_output_dir(&config));
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for path in read_ledger_at(&ledger_path()).exports {
        if let Some(entry) = entry_for(&path) {
            if seen.insert(key(&entry.path)) {
                entries.push(entry);
            }
        }
    }
    for folder in [recordings, motion_folder] {
        for entry in scan::scan_folder(&folder, MediaKind::Video) {
            if has_motion_export_name(&entry.path) && seen.insert(key(&entry.path)) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let directory = std::env::temp_dir().join(format!(
            "apexshot-motion-exports-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_recorded_export_is_kept_out_of_recordings() {
        let directory = scratch("ledger");
        let ledger = directory.join("motion-exports.json");
        let video = directory.join("Promo.mp4");
        let recording = directory.join("ApexShot Recording.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        std::fs::write(&recording, b"mp4").unwrap();

        record_export_at(&ledger, &video);
        record_export_at(&ledger, &video);
        let recorded = recorded_keys_at(&ledger);
        assert_eq!(recorded.len(), 1, "a second export of the same file is one entry");
        assert!(is_motion_export(&video, &recorded));
        assert!(!is_motion_export(&recording, &recorded));

        std::fs::remove_file(&video).unwrap();
        let other = directory.join("Other.mp4");
        std::fs::write(&other, b"mp4").unwrap();
        record_export_at(&ledger, &other);
        assert_eq!(recorded_keys_at(&ledger).len(), 1, "deleted videos drop out");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_default_export_name_counts_without_a_ledger_entry() {
        let recorded = HashSet::new();
        assert!(is_motion_export(Path::new("C:/Videos/ApexShot Motion.mp4"), &recorded));
        assert!(is_motion_export(Path::new("C:/Videos/shot 1 motion.MP4"), &recorded));
        assert!(!is_motion_export(Path::new("C:/Videos/ApexShot Recording.mp4"), &recorded));
        assert!(!is_motion_export(Path::new("C:/Videos/Motion.mp4"), &recorded));
    }
}
