//! Shared clipboard utilities for Wayland and X11.
//!
//! Provides consistent clipboard operations across all capture modes
//! (area, fullscreen, crosshair, OCR, preview overlay, editor).

use std::io::Write;
use std::path::Path;

/// Copy a file URI to the clipboard as `text/uri-list`.
///
/// On Wayland uses `wl-copy`, on X11 uses `xclip`.
pub fn copy_uri_to_clipboard(path: &Path) -> Result<(), String> {
    let uri = url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| "Failed to convert path to file URI".to_string())?;
    let payload = format!("{uri}\r\n");

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut child = std::process::Command::new("wl-copy")
            .arg("--type")
            .arg("text/uri-list")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Clipboard tool not found (install wl-clipboard)".to_string()
                } else {
                    format!("Clipboard command failed: {e}")
                }
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(payload.as_bytes())
                .map_err(|e| format!("Clipboard command failed: {e}"))?;
        }

        if child
            .wait()
            .map_err(|e| format!("Clipboard command failed: {e}"))?
            .success()
        {
            return Ok(());
        }

        return Err("Clipboard command failed".to_string());
    }

    let mut child = std::process::Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .arg("-t")
        .arg("text/uri-list")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Clipboard tool not found (install xclip)".to_string()
            } else {
                format!("Clipboard command failed: {e}")
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload.as_bytes())
            .map_err(|e| format!("Clipboard command failed: {e}"))?;
    }

    if child
        .wait()
        .map_err(|e| format!("Clipboard command failed: {e}"))?
        .success()
    {
        Ok(())
    } else {
        Err("Clipboard command failed".to_string())
    }
}

/// Copy text to the system clipboard.
///
/// On Wayland uses `wl-copy` (piped via stdin for reliability),
/// falls back to `arboard` crate on X11 or if `wl-copy` is unavailable.
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        // Use wl-copy for Wayland — pipe via stdin to handle special chars and long text
        match std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                match child.wait() {
                    Ok(status) if status.success() => return Ok(()),
                    Ok(status) => {
                        return Err(format!("wl-copy exited with status: {}", status));
                    }
                    Err(e) => {
                        return Err(format!("wl-copy wait failed: {e}"));
                    }
                }
            }
            Err(e) => {
                // Fall through to arboard if wl-copy not found
                eprintln!("Warning: wl-copy failed, trying arboard: {e}");
            }
        }
    }

    // Try arboard (works on X11 and as fallback on Wayland)
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("Failed to access clipboard: {e}"))?;

    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard text: {e}"))?;

    Ok(())
}

/// Copy an image file to the clipboard as a PNG image.
///
/// On Wayland uses `wl-copy --type image/png`, on X11 uses `xclip`.
pub fn copy_image_to_clipboard(path: &Path) -> Result<(), String> {
    let image_data = std::fs::read(path)
        .map_err(|e| format!("Failed to read image file: {e}"))?;

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut child = std::process::Command::new("wl-copy")
            .arg("--type")
            .arg("image/png")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Clipboard tool not found (install wl-clipboard)".to_string()
                } else {
                    format!("Clipboard command failed: {e}")
                }
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&image_data)
                .map_err(|e| format!("Clipboard command failed: {e}"))?;
        }

        if child
            .wait()
            .map_err(|e| format!("Clipboard command failed: {e}"))?
            .success()
        {
            return Ok(());
        }

        return Err("Clipboard command failed".to_string());
    }

    let mut child = std::process::Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .arg("-t")
        .arg("image/png")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Clipboard tool not found (install xclip)".to_string()
            } else {
                format!("Clipboard command failed: {e}")
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&image_data)
            .map_err(|e| format!("Clipboard command failed: {e}"))?;
    }

    if child
        .wait()
        .map_err(|e| format!("Clipboard command failed: {e}"))?
        .success()
    {
        Ok(())
    } else {
        Err("Clipboard command failed".to_string())
    }
}
