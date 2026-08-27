//! Local capture scanning for the History window.
//!
//! Deliberately GUI-free: every entry point here is safe to call from a
//! background thread so the window can rescan a folder without blocking its
//! main loop.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::config::AppConfig;

/// Still-image extensions ApexShot writes (see `capture::ImageFormat`).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Moving-image extensions the recorder writes.
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "gif"];

/// Which kind of capture a page lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    /// Extensions this kind accepts, lowercase and without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            MediaKind::Image => IMAGE_EXTENSIONS,
            MediaKind::Video => VIDEO_EXTENSIONS,
        }
    }

    /// True when `path` carries one of this kind's extensions. Case-insensitive
    /// because files arrive from editors and file managers too, not only from us.
    pub fn matches(self, path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        let ext = ext.to_ascii_lowercase();
        self.extensions().iter().any(|candidate| *candidate == ext)
    }
}

/// One capture on disk, modelled on what a gallery card needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureEntry {
    pub path: PathBuf,
    /// Filename for display, never empty.
    pub display_name: String,
    /// Last-modified time, or `None` when the filesystem did not report one.
    pub modified: Option<SystemTime>,
    pub size_bytes: u64,
    pub kind: MediaKind,
}

impl CaptureEntry {
    /// Lowercase name used by the search box.
    pub fn search_key(&self) -> String {
        self.display_name.to_ascii_lowercase()
    }
}

/// Folder screenshots are read from: Settings `screenshot_export_location`,
/// else the XDG Pictures directory — the same defaults `SaveConfig` saves into.
pub fn screenshot_folder(config: &AppConfig) -> PathBuf {
    let configured = config.screenshot_export_location.trim();
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    dirs::picture_dir()
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Pictures")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Folder recordings are read from — the recorder's own output directory.
pub fn recording_folder(config: &AppConfig) -> PathBuf {
    crate::recording::recording_output_dir(config)
}

/// List captures of `kind` directly inside `folder`, newest first.
///
/// Subfolders are ignored, and a missing or unreadable folder is an empty
/// result rather than an error: an unconfigured or deleted capture folder is a
/// normal state for this window, not a failure.
pub fn scan_folder(folder: &Path, kind: MediaKind) -> Vec<CaptureEntry> {
    let Ok(read_dir) = std::fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        if !kind.matches(&path) {
            continue;
        }
        // `fs::metadata` follows symlinks, so a linked-in capture still counts;
        // `is_file` then keeps directories that merely look like captures out.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let display_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if display_name.is_empty() {
            continue;
        }
        entries.push(CaptureEntry {
            path,
            display_name,
            modified: metadata.modified().ok(),
            size_bytes: metadata.len(),
            kind,
        });
    }

    sort_newest_first(&mut entries);
    entries
}

/// Newest first, with undated files last and name as a stable tie-break so the
/// grid does not reshuffle between rescans.
fn sort_newest_first(entries: &mut [CaptureEntry]) {
    entries.sort_by(|a, b| match (b.modified, a.modified) {
        (Some(newer), Some(older)) => newer
            .cmp(&older)
            .then_with(|| a.display_name.cmp(&b.display_name)),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.display_name.cmp(&b.display_name),
    });
}

/// Human-friendly modification time: relative for recent captures, an absolute
/// date once "N days ago" stops being useful.
pub fn format_relative_time(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(modified) = modified else {
        return "Date unknown".to_string();
    };

    // A capture stamped in the future (clock skew, copied file) reads as new.
    let elapsed = now
        .duration_since(modified)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    match elapsed {
        0..=59 => "Just now".to_string(),
        60..=3_599 => plural(elapsed / 60, "minute"),
        3_600..=86_399 => plural(elapsed / 3_600, "hour"),
        86_400..=172_799 => "Yesterday".to_string(),
        172_800..=604_799 => plural(elapsed / 86_400, "day"),
        _ => chrono::DateTime::<chrono::Local>::from(modified)
            .format("%b %-d, %Y")
            .to_string(),
    }
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

/// Compact file size for a card's meta line.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f < KB {
        format!("{bytes} B")
    } else if bytes_f < MB {
        format!("{:.0} KB", bytes_f / KB)
    } else if bytes_f < GB {
        format!("{:.1} MB", bytes_f / MB)
    } else {
        format!("{:.2} GB", bytes_f / GB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-history-scan-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Write a file with an explicit modification time so ordering assertions do
    /// not depend on how fast the test machine creates files.
    fn write_capture(dir: &Path, name: &str, modified_secs: u64, size: usize) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![b'x'; size]).expect("write capture");
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open capture");
        file.set_modified(UNIX_EPOCH + Duration::from_secs(modified_secs))
            .expect("set modification time");
        path
    }

    #[test]
    fn scan_keeps_only_the_formats_the_app_writes() {
        let dir = scratch_dir("filtering");
        write_capture(&dir, "shot.png", 1_000, 4);
        write_capture(&dir, "shot.JPEG", 1_001, 4);
        write_capture(&dir, "shot.webp", 1_002, 4);
        write_capture(&dir, "clip.mp4", 1_003, 4);
        write_capture(&dir, "notes.txt", 1_004, 4);
        write_capture(&dir, "archive.png.bak", 1_005, 4);
        write_capture(&dir, "noextension", 1_006, 4);
        write_capture(&dir, "clip.apexshot.json", 1_007, 4);
        write_capture(&dir, "project.json", 1_008, 4);

        let images = scan_folder(&dir, MediaKind::Image);
        let mut names: Vec<&str> = images.iter().map(|e| e.display_name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["shot.JPEG", "shot.png", "shot.webp"]);

        let videos = scan_folder(&dir, MediaKind::Video);
        let names: Vec<&str> = videos.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["clip.mp4"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_lists_recorder_video_formats_including_gif() {
        let dir = scratch_dir("video-formats");
        write_capture(&dir, "a.mp4", 3, 4);
        write_capture(&dir, "b.webm", 2, 4);
        write_capture(&dir, "c.gif", 1, 4);
        write_capture(&dir, "d.mkv", 4, 4);

        let videos = scan_folder(&dir, MediaKind::Video);
        let names: Vec<&str> = videos.iter().map(|e| e.display_name.as_str()).collect();
        // .mkv is not a format the recorder writes, so it stays out.
        assert_eq!(names, vec!["a.mp4", "b.webm", "c.gif"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_orders_newest_first_and_carries_metadata() {
        let dir = scratch_dir("ordering");
        write_capture(&dir, "oldest.png", 1_000, 10);
        write_capture(&dir, "newest.png", 3_000, 30);
        write_capture(&dir, "middle.png", 2_000, 20);

        let entries = scan_folder(&dir, MediaKind::Image);
        let names: Vec<&str> = entries.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["newest.png", "middle.png", "oldest.png"]);

        assert_eq!(entries[0].size_bytes, 30);
        assert_eq!(entries[0].kind, MediaKind::Image);
        assert_eq!(
            entries[0].modified,
            Some(UNIX_EPOCH + Duration::from_secs(3_000))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_breaks_timestamp_ties_by_name() {
        let dir = scratch_dir("ties");
        write_capture(&dir, "b.png", 2_000, 4);
        write_capture(&dir, "a.png", 2_000, 4);
        write_capture(&dir, "c.png", 2_000, 4);

        let entries = scan_folder(&dir, MediaKind::Image);
        let names: Vec<&str> = entries.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["a.png", "b.png", "c.png"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_ignores_subfolders_and_does_not_recurse() {
        let dir = scratch_dir("no-recursion");
        write_capture(&dir, "top.png", 2_000, 4);
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        write_capture(&nested, "buried.png", 3_000, 4);
        // A directory whose name looks like a capture must not become an entry.
        std::fs::create_dir_all(dir.join("decoy.png")).expect("create decoy dir");

        let entries = scan_folder(&dir, MediaKind::Image);
        let names: Vec<&str> = entries.iter().map(|e| e.display_name.as_str()).collect();
        assert_eq!(names, vec!["top.png"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folder_scans_to_an_empty_list() {
        let missing = std::env::temp_dir().join("apexshot-history-scan-does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);

        assert!(scan_folder(&missing, MediaKind::Image).is_empty());
        assert!(scan_folder(&missing, MediaKind::Video).is_empty());
    }

    #[test]
    fn json_sidecars_are_never_captures() {
        assert!(!MediaKind::Image.matches(Path::new("shot.json")));
        assert!(!MediaKind::Video.matches(Path::new("clip.apexshot.json")));
        assert!(!MediaKind::Video.matches(Path::new("abc.json")));
        assert!(MediaKind::Video.matches(Path::new("clip.mp4")));
    }

    #[test]
    fn folders_fall_back_to_xdg_defaults_when_unset() {
        let config = AppConfig::default();
        let screenshots = screenshot_folder(&config);
        assert!(!screenshots.as_os_str().is_empty());

        let configured = AppConfig {
            screenshot_export_location: " /tmp/apexshot-shots ".to_string(),
            video_export_location: "/tmp/apexshot-clips".to_string(),
            ..AppConfig::default()
        };
        assert_eq!(
            screenshot_folder(&configured),
            PathBuf::from("/tmp/apexshot-shots")
        );
        assert_eq!(
            recording_folder(&configured),
            PathBuf::from("/tmp/apexshot-clips")
        );
    }

    #[test]
    fn relative_time_reads_naturally() {
        let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
        let ago = |secs: u64| format_relative_time(Some(now - Duration::from_secs(secs)), now);

        assert_eq!(ago(5), "Just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(600), "10 minutes ago");
        assert_eq!(ago(3_600), "1 hour ago");
        assert_eq!(ago(7_200), "2 hours ago");
        assert_eq!(ago(90_000), "Yesterday");
        assert_eq!(ago(3 * 86_400), "3 days ago");
        // Beyond a week the relative form stops being useful.
        assert!(!ago(40 * 86_400).contains("ago"));
        assert_eq!(format_relative_time(None, now), "Date unknown");
        // A future timestamp must not panic or read as negative.
        assert_eq!(
            format_relative_time(Some(now + Duration::from_secs(500)), now),
            "Just now"
        );
    }

    #[test]
    fn size_formatting_scales_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }
}
