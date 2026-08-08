//! Per-item actions for the History window.
//!
//! Everything here reuses plumbing that already exists elsewhere in the app —
//! the daemon's default-app helper, the clipboard utilities, the editor
//! subprocess launchers and the cloud upload entry point — so a history card
//! behaves exactly like the same action taken from the tray or the preview.

use std::path::{Path, PathBuf};

use super::scan::{CaptureEntry, MediaKind};

/// Hand the file to the desktop's default application.
pub fn open_in_default_app(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    crate::daemon::open_file(entry.path.clone())?;
    Ok(format!("Opened {}", entry.display_name))
}

/// Open the capture in the matching ApexShot editor.
///
/// Spawned as a subprocess for the same reason the tray does it that way: the
/// editors run their own GTK main loop and must not be started inside a window
/// that already owns one.
pub fn open_in_apexshot_editor(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;

    let (command, editor) = match entry.kind {
        MediaKind::Image => ("edit", "image editor"),
        MediaKind::Video => ("video-editor", "video editor"),
    };

    // The recording editor only re-encodes MP4; GIF/WebM have no editor path.
    if entry.kind == MediaKind::Video && !has_extension(&entry.path, "mp4") {
        return Err("The video editor only supports MP4 recordings".to_string());
    }

    let exe = std::env::current_exe().map_err(|e| format!("Could not find ApexShot: {e}"))?;
    std::process::Command::new(exe)
        .arg(command)
        .arg(&entry.path)
        .spawn()
        .map_err(|e| format!("Could not open the {editor}: {e}"))?;

    Ok(format!("Opening {} in the {editor}", entry.display_name))
}

/// Copy a still to the clipboard as an image, a recording as a file reference.
pub fn copy_to_clipboard(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;

    match entry.kind {
        MediaKind::Image => {
            crate::utils::clipboard::copy_image_to_clipboard(&entry.path)?;
            Ok("Image copied to clipboard".to_string())
        }
        MediaKind::Video => {
            crate::utils::clipboard::copy_uri_to_clipboard(&entry.path)?;
            Ok("File copied to clipboard".to_string())
        }
    }
}

/// Copy arbitrary text (a cloud share link) to the clipboard.
pub fn copy_link_to_clipboard(link: &str) -> Result<String, String> {
    let link = link.trim();
    if link.is_empty() {
        return Err("This upload has no share link".to_string());
    }
    crate::utils::clipboard::copy_text_to_clipboard(link)?;
    Ok("Share link copied to clipboard".to_string())
}

/// Open a URL in the user's browser.
pub fn open_in_browser(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("This upload has no link to open".to_string());
    }
    crate::utils::open::open_url(url)?;
    Ok("Opened in your browser".to_string())
}

/// Show the file in the desktop's file manager, selecting it where the file
/// manager supports that. Falls back to just opening the containing folder.
pub fn reveal_in_file_manager(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;
    let path = entry.path.as_os_str();
    let folder = entry
        .path
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // Flatpak: no host file-manager binaries — open the folder via portal.
    if !crate::app_identity::portal_only() {
        let selecting: [(&str, &[&str]); 4] = [
            ("nautilus", &["--select"]),
            ("dolphin", &["--select"]),
            ("nemo", &[]),
            ("caja", &[]),
        ];
        for (program, args) in selecting {
            let spawned = std::process::Command::new(program)
                .args(args)
                .arg(path)
                .spawn()
                .is_ok();
            if spawned {
                return Ok(format!("Showing {} in your files", entry.display_name));
            }
        }
    }

    // Open the folder with the desktop default handler (portal-aware).
    crate::utils::open::open_path(&folder)?;
    Ok(format!("Opened {}", folder.display()))
}

/// Upload the capture through the shared cloud upload path.
///
/// Blocking (network): call from a worker thread.
pub fn upload_to_cloud(entry: &CaptureEntry) -> Result<String, String> {
    ensure_exists(&entry.path)?;

    let config = crate::config::load_config();
    if !crate::cloud::upload::is_configured(&config) {
        let (title, body) = crate::cloud::upload::not_configured_notification(&config);
        return Err(format!("{title}. {body}"));
    }

    crate::cloud::upload::upload_file_with_notifications(&config, &entry.path)
        .map(|_| format!("Uploaded {}", entry.display_name))
        .map_err(|e| format!("Upload failed: {e}"))
}

/// Permanently remove a capture from disk. The window asks for confirmation
/// before this runs.
pub fn delete_capture(entry: &CaptureEntry) -> Result<String, String> {
    match std::fs::remove_file(&entry.path) {
        Ok(()) => Ok(format!("Deleted {}", entry.display_name)),
        // Already gone is the outcome the user asked for.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(format!("{} was already gone", entry.display_name))
        }
        Err(e) => Err(format!("Could not delete {}: {e}", entry.display_name)),
    }
}

fn ensure_exists(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    Err(format!(
        "{} is no longer on disk",
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    ))
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(extension))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: MediaKind) -> CaptureEntry {
        CaptureEntry {
            path: PathBuf::from(format!("/nonexistent/apexshot-history/{name}")),
            display_name: name.to_string(),
            modified: None,
            size_bytes: 0,
            kind,
        }
    }

    #[test]
    fn actions_refuse_files_that_are_gone() {
        let missing = entry("gone.png", MediaKind::Image);
        for outcome in [
            open_in_default_app(&missing),
            open_in_apexshot_editor(&missing),
            copy_to_clipboard(&missing),
            reveal_in_file_manager(&missing),
            upload_to_cloud(&missing),
        ] {
            let message = outcome.expect_err("missing file must be reported");
            assert!(
                message.contains("no longer on disk"),
                "unexpected message: {message}"
            );
        }
    }

    #[test]
    fn deleting_a_missing_file_is_not_an_error() {
        let outcome = delete_capture(&entry("already-gone.png", MediaKind::Image));
        assert!(outcome.is_ok());
    }

    #[test]
    fn editor_action_rejects_recordings_the_video_editor_cannot_open() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-history-actions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join("clip.gif");
        std::fs::write(&path, b"gif").expect("write clip");

        let gif = CaptureEntry {
            path,
            display_name: "clip.gif".to_string(),
            modified: None,
            size_bytes: 3,
            kind: MediaKind::Video,
        };
        let message = open_in_apexshot_editor(&gif).expect_err("gif has no editor");
        assert!(message.contains("MP4"), "unexpected message: {message}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn link_actions_validate_their_input() {
        assert!(copy_link_to_clipboard("   ").is_err());
        assert!(open_in_browser("").is_err());
    }
}
