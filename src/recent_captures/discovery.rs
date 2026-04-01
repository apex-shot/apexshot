use std::path::PathBuf;
use std::time::SystemTime;

use crate::{capture::SaveConfig, config::load_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureModeFilter {
    All,
    Screenshots,
    Recordings,
}

#[derive(Debug, Clone)]
pub struct RecentCaptureEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub modified_at: SystemTime,
}

#[derive(Debug, Clone, Default)]
pub struct RecentCaptureCollection {
    pub featured: Option<RecentCaptureEntry>,
    pub remaining: Vec<RecentCaptureEntry>,
}

const DEFAULT_RECENT_CAPTURE_LIMIT: usize = 13;

pub fn recent_capture_source_dir() -> Option<PathBuf> {
    let config = load_config().sanitized();
    if !config.screenshot_export_location.is_empty() {
        return Some(PathBuf::from(config.screenshot_export_location));
    }
    SaveConfig::default().get_output_dir().ok()
}

pub fn discover_recent_captures(filter: CaptureModeFilter) -> RecentCaptureCollection {
    let Some(dir) = recent_capture_source_dir() else {
        return RecentCaptureCollection::default();
    };
    discover_recent_captures_in_dir(&dir, DEFAULT_RECENT_CAPTURE_LIMIT, filter)
}

pub(crate) fn discover_recent_captures_in_dir(
    dir: &std::path::Path,
    limit: usize,
    filter: CaptureModeFilter,
) -> RecentCaptureCollection {
    if limit == 0 {
        return RecentCaptureCollection::default();
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return RecentCaptureCollection::default(),
    };

    let mut captures: Vec<RecentCaptureEntry> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() {
                return None;
            }

            let path = entry.path();
            if !is_supported_capture_file(&path) {
                return None;
            }

            if filter != CaptureModeFilter::All {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                let is_anim = matches!(ext.as_str(), "mp4" | "webm" | "mkv" | "mov" | "gif");
                if filter == CaptureModeFilter::Screenshots && is_anim {
                    return None;
                }
                if filter == CaptureModeFilter::Recordings && !is_anim {
                    return None;
                }
            }

            let modified_at = entry.metadata().ok()?.modified().ok()?;
            let file_name = path.file_name()?.to_string_lossy().to_string();

            Some(RecentCaptureEntry {
                path,
                file_name,
                modified_at,
            })
        })
        .collect();

    captures.sort_by(|left, right| right.modified_at.cmp(&left.modified_at));
    captures.truncate(limit);

    let mut iter = captures.into_iter();
    let featured = iter.next();
    let remaining = iter.collect();

    RecentCaptureCollection {
        featured,
        remaining,
    }
}

fn is_supported_capture_file(path: &std::path::Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "mp4" | "webm" | "mkv" | "mov"
    )
}

#[cfg(test)]
mod tests {
    use super::{discover_recent_captures_in_dir, CaptureModeFilter};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("apexshot-recent-captures-{name}-{unique}"))
    }

    fn create_file(path: &Path) {
        fs::write(path, b"test").expect("write test file");
        thread::sleep(Duration::from_millis(5));
    }

    #[test]
    fn discover_recent_captures_returns_empty_for_missing_dir() {
        let missing = temp_test_dir("missing");
        let result = discover_recent_captures_in_dir(&missing, 13, CaptureModeFilter::All);

        assert!(result.featured.is_none());
        assert!(result.remaining.is_empty());
    }

    #[test]
    fn discover_recent_captures_sorts_newest_first() {
        let dir = temp_test_dir("sorts");
        fs::create_dir_all(&dir).expect("create dir");

        let oldest = dir.join("oldest.png");
        create_file(&oldest);
        let newest = dir.join("newest.png");
        create_file(&newest);

        let result = discover_recent_captures_in_dir(&dir, 13, CaptureModeFilter::All);

        assert_eq!(
            result.featured.as_ref().map(|entry| entry.file_name.as_str()),
            Some("newest.png")
        );
        assert_eq!(result.remaining.len(), 1);
        assert_eq!(result.remaining[0].file_name, "oldest.png");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_recent_captures_uses_newest_as_featured() {
        let dir = temp_test_dir("featured");
        fs::create_dir_all(&dir).expect("create dir");

        create_file(&dir.join("one.png"));
        create_file(&dir.join("two.png"));
        create_file(&dir.join("three.png"));

        let result = discover_recent_captures_in_dir(&dir, 13, CaptureModeFilter::All);

        assert_eq!(
            result.featured.as_ref().map(|entry| entry.file_name.as_str()),
            Some("three.png")
        );
        assert_eq!(result.remaining.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_recent_captures_caps_to_thirteen_items() {
        let dir = temp_test_dir("cap");
        fs::create_dir_all(&dir).expect("create dir");

        for index in 0..20 {
            create_file(&dir.join(format!("capture-{index:02}.png")));
        }

        let result = discover_recent_captures_in_dir(&dir, 13, CaptureModeFilter::All);

        assert!(result.featured.is_some());
        assert_eq!(result.remaining.len(), 12);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_recent_captures_ignores_non_images() {
        let dir = temp_test_dir("non-images");
        fs::create_dir_all(&dir).expect("create dir");

        create_file(&dir.join("capture.png"));
        create_file(&dir.join("notes.txt"));
        create_file(&dir.join("audio.mp3"));

        let result = discover_recent_captures_in_dir(&dir, 13, CaptureModeFilter::All);

        assert_eq!(
            result.featured.as_ref().map(|entry| entry.file_name.as_str()),
            Some("capture.png")
        );
        assert!(result.remaining.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_recent_captures_ignores_directories() {
        let dir = temp_test_dir("dirs");
        fs::create_dir_all(&dir).expect("create dir");

        fs::create_dir_all(dir.join("nested.png")).expect("create nested dir");
        create_file(&dir.join("capture.png"));

        let result = discover_recent_captures_in_dir(&dir, 13, CaptureModeFilter::All);

        assert_eq!(
            result.featured.as_ref().map(|entry| entry.file_name.as_str()),
            Some("capture.png")
        );
        assert!(result.remaining.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
