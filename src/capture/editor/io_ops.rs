use super::state::EditorState;
use super::types::EditorError;
use image::DynamicImage;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn save_edited_image(path: &Path, state: &EditorState) -> Result<(), EditorError> {
    let final_image = state.to_final_image()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    let result = match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = DynamicImage::ImageRgba8(final_image).to_rgb8();
            rgb.save_with_format(path, image::ImageFormat::Jpeg)
        }
        "png" => final_image.save_with_format(path, image::ImageFormat::Png),
        _ => final_image.save(path),
    };

    result.map_err(|e| EditorError::ImageSave(e.to_string()))
}

pub fn file_uri(path: &Path) -> Result<String, String> {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .map_err(|_| "Failed to convert path to file URI".to_string())
}

pub fn copy_uri_to_clipboard(path: &Path) -> Result<(), String> {
    let uri = file_uri(path)?;
    let payload = format!("{uri}\r\n");

    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let mut child = Command::new("wl-copy")
            .arg("--type")
            .arg("text/uri-list")
            .stdin(Stdio::piped())
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

    let mut child = Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .arg("-t")
        .arg("text/uri-list")
        .arg("-i")
        .stdin(Stdio::piped())
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

pub fn open_target(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open target: {e}"))
}
