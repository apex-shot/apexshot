// Utility functions and shared helpers
// TODO: Add config handling, logging setup, etc

pub mod capture_sound;
pub mod clipboard;
pub mod desktop_env;
pub mod notify;
pub mod open;
pub mod png_io;

/// Check whether two paths identify the same file, including hard links.
pub(crate) fn paths_alias(left: &std::path::Path, right: &std::path::Path) -> bool {
    if left == right {
        return true;
    }

    if let (Some(left), Some(right)) = (resolved_path(left), resolved_path(right)) {
        if left == right {
            return true;
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if let (Ok(left), Ok(right)) = (std::fs::metadata(left), std::fs::metadata(right)) {
            return left.dev() == right.dev() && left.ino() == right.ino();
        }
    }

    false
}

fn resolved_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Ok(path) = path.canonicalize() {
        return Some(path);
    }

    let file_name = path.file_name()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    Some(parent.canonicalize().ok()?.join(file_name))
}

/// Run `f` on a thread that is not driving a Tokio runtime.
///
/// `zbus::blocking` builds its own multi-thread runtime and panics with
/// "Cannot start a runtime from within a runtime" if called on a Tokio
/// worker. Release builds use `panic = "abort"`, so that takes down the daemon.
pub fn run_off_tokio<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(f)
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    } else {
        f()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_off_tokio_from_runtime_does_not_panic() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let value = rt.block_on(async { super::run_off_tokio(|| 7) });
        assert_eq!(value, 7);
    }

    #[test]
    fn paths_alias_detects_existing_hard_links_and_new_destination_paths() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-path-alias-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let hard_link = dir.join("hard-link.mp4");
        let new_destination = dir.join("edited.mp4");
        std::fs::write(&source, b"source media").unwrap();
        std::fs::hard_link(&source, &hard_link).unwrap();

        assert!(super::paths_alias(&source, &hard_link));
        assert!(!super::paths_alias(&source, &new_destination));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
