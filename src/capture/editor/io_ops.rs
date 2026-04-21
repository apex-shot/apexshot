use super::state::EditorState;
use super::types::EditorError;
use crate::utils::clipboard;
use image::DynamicImage;
use std::path::Path;
use std::process::Command;

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

pub fn copy_uri_to_clipboard(path: &Path) -> Result<(), String> {
    clipboard::copy_uri_to_clipboard(path)
}

#[allow(dead_code)]
pub fn open_target(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open target: {e}"))
}
