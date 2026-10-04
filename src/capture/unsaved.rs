//! App-owned storage for captures the user has not saved yet.
//!
//! With "Save" turned off in After capture settings a capture still has to
//! outlive the Quick Access card, the annotate editor, drag-and-drop, the
//! clipboard and an auto-upload. It lives in a directory this app owns so that
//! stale captures are cleaned up instead of piling up in the export folder or
//! `/tmp`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long an unsaved capture is kept before it becomes eligible for cleanup.
pub const UNSAVED_CAPTURE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// File-name prefixes of the temporary captures this app writes itself. Only
/// these are consumed by [`UnsavedCaptureStore::keep`]; a path that merely
/// looks like a capture belongs to whoever created it.
const APP_TEMP_CAPTURE_PREFIXES: [&str; 2] = ["apexshot_capture_", "apexshot_clipboard_"];

/// Owns the directory that holds captures auto-save did not write.
pub struct UnsavedCaptureStore {
    dir: PathBuf,
}

impl UnsavedCaptureStore {
    /// The store the running app uses.
    pub fn app_owned() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("apexshot")
            .join("unsaved");
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// True when the capture already lives in this store's directory.
    pub fn owns(&self, path: &Path) -> bool {
        path.parent() == Some(self.dir.as_path())
    }

    /// Take ownership of a freshly captured file so the capture survives every
    /// after-capture consumer even though it was never saved.
    pub fn keep(&self, source: &Path) -> std::io::Result<PathBuf> {
        if self.owns(source) {
            return Ok(source.to_path_buf());
        }

        std::fs::create_dir_all(&self.dir)?;
        let destination = self.dir.join(unsaved_file_name(source));
        let disposable = is_app_temp_capture(source);

        if disposable && std::fs::rename(source, &destination).is_ok() {
            return Ok(destination);
        }

        std::fs::copy(source, &destination)?;
        if disposable {
            let _ = std::fs::remove_file(source);
        }
        Ok(destination)
    }

    /// Remove unsaved captures older than `max_age`. `keep` is the capture the
    /// app is still pointing at; it is never removed.
    pub fn clean_stale(&self, keep: Option<&Path>, max_age: Duration) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };

        let now = SystemTime::now();
        let mut removed = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() || keep.is_some_and(|keep| keep == path) {
                continue;
            }
            if !is_older_than(&entry, now, max_age) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

fn is_older_than(entry: &std::fs::DirEntry, now: SystemTime, max_age: Duration) -> bool {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age > max_age)
}

fn unsaved_file_name(source: &Path) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png");
    format!(
        "apexshot-unsaved-{stamp}-{}.{extension}",
        std::process::id()
    )
}

fn is_app_temp_capture(path: &Path) -> bool {
    if path.parent() != Some(std::env::temp_dir().as_path()) {
        return false;
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            APP_TEMP_CAPTURE_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-unsaved-test-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn store_in(dir: &Path) -> UnsavedCaptureStore {
        UnsavedCaptureStore {
            dir: dir.to_path_buf(),
        }
    }

    fn stamp() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    }

    fn aged_file(dir: &Path, name: &str, age: Duration) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"capture").expect("write capture");
        let modified = SystemTime::now() - age;
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open capture")
            .set_modified(modified)
            .expect("age capture");
        path
    }

    #[test]
    fn keep_moves_a_temp_capture_into_app_storage() {
        let store_dir = scratch_dir("keep-store");
        let source = std::env::temp_dir().join(format!(
            "apexshot_capture_{}-{}.png",
            std::process::id(),
            stamp()
        ));
        std::fs::write(&source, b"pixels").expect("write source");

        let store = store_in(&store_dir);
        let kept = store.keep(&source).expect("keep capture");

        assert_eq!(kept.parent(), Some(store_dir.as_path()));
        assert_eq!(std::fs::read(&kept).expect("read kept"), b"pixels");
        assert!(!source.exists(), "the consumed temp capture is gone");

        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[test]
    fn keep_leaves_a_foreign_source_file_alone() {
        let source_dir = scratch_dir("foreign-source");
        let store_dir = scratch_dir("foreign-store");
        let source = source_dir.join("my-own-screenshot.png");
        std::fs::write(&source, b"pixels").expect("write source");

        let store = store_in(&store_dir);
        let kept = store.keep(&source).expect("keep capture");

        assert_eq!(std::fs::read(&kept).expect("read kept"), b"pixels");
        assert!(
            source.exists(),
            "a path the app did not create is never deleted"
        );

        let _ = std::fs::remove_dir_all(&source_dir);
        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[test]
    fn keep_is_idempotent_for_a_capture_already_in_app_storage() {
        let store_dir = scratch_dir("already-stored");
        let stored = store_dir.join("apexshot-unsaved-1-2.png");
        std::fs::write(&stored, b"pixels").expect("write capture");

        let store = store_in(&store_dir);
        let kept = store.keep(&stored).expect("keep capture");

        assert_eq!(kept, stored);
        assert!(stored.exists());

        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[test]
    fn cleanup_removes_only_stale_captures_and_never_the_active_one() {
        let store_dir = scratch_dir("cleanup");
        let stale = aged_file(&store_dir, "stale.png", Duration::from_secs(3 * 60 * 60));
        let stale_but_active =
            aged_file(&store_dir, "active.png", Duration::from_secs(3 * 60 * 60));
        let fresh = aged_file(&store_dir, "fresh.png", Duration::from_secs(60));

        let store = store_in(&store_dir);
        let removed = store.clean_stale(Some(&stale_but_active), Duration::from_secs(60 * 60));

        assert_eq!(removed, 1);
        assert!(!stale.exists());
        assert!(stale_but_active.exists(), "the active capture is preserved");
        assert!(fresh.exists(), "a recent capture is preserved");

        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[test]
    fn cleanup_without_a_keep_path_clears_every_stale_capture() {
        let store_dir = scratch_dir("cleanup-all");
        let stale = aged_file(&store_dir, "stale.png", Duration::from_secs(3 * 60 * 60));
        let fresh = aged_file(&store_dir, "fresh.png", Duration::from_secs(60));

        let store = store_in(&store_dir);
        assert_eq!(store.clean_stale(None, Duration::from_secs(60 * 60)), 1);

        assert!(!stale.exists());
        assert!(fresh.exists());

        let _ = std::fs::remove_dir_all(&store_dir);
    }

    #[test]
    fn only_app_created_temp_captures_are_disposable() {
        assert!(is_app_temp_capture(
            &std::env::temp_dir().join("apexshot_capture_1699999999.png")
        ));
        assert!(is_app_temp_capture(
            &std::env::temp_dir().join("apexshot_clipboard_1699999999.png")
        ));
        assert!(!is_app_temp_capture(
            &std::env::temp_dir().join("apexshot-unsaved-1-2.png")
        ));
        assert!(!is_app_temp_capture(Path::new(
            "/home/someone/Pictures/apexshot_capture_1699999999.png"
        )));
        assert!(!is_app_temp_capture(Path::new(
            "/home/someone/Documents/apexshot_clipboard_1699999999.png"
        )));
    }

    #[test]
    fn keep_never_consumes_a_look_alike_outside_the_temp_directory() {
        let source_dir = scratch_dir("look-alike");
        let store_dir = scratch_dir("look-alike-store");
        let source = source_dir.join("apexshot_capture_123.png");
        std::fs::write(&source, b"pixels").expect("write source");

        let store = store_in(&store_dir);
        let kept = store.keep(&source).expect("keep capture");

        assert_eq!(std::fs::read(&kept).expect("read kept"), b"pixels");
        assert!(
            source.exists(),
            "a capture-prefixed file outside the temp directory belongs to the user"
        );

        let _ = std::fs::remove_dir_all(&source_dir);
        let _ = std::fs::remove_dir_all(&store_dir);
    }
}
