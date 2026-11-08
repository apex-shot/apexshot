//! Save/copy for the flattened editor document.
//!
//! Same contract as the Linux editor: a Save As writes through a unique
//! temporary sibling then renames, so a failed encode never clobbers the
//! destination and the source file is never overwritten automatically.

use super::state::EditorState;
use super::types::EditorError;
use image::DynamicImage;
use image::RgbaImage;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn save_edited_image(path: &Path, state: &EditorState) -> Result<(), EditorError> {
    let final_image = state.to_final_image()?;
    save_image_snapshot(path, final_image)
}

/// Encode and save an immutable flattened editor image using the destination format.
pub fn save_edited_image_snapshot(path: &Path, final_image: RgbaImage) -> Result<(), EditorError> {
    save_image_snapshot(path, final_image)
}

/// Encode `final_image` to bytes for `path`'s extension, write a temp
/// sibling, then atomically replace the destination. Encode failures never
/// touch the destination; the temp file is cleaned on any error.
pub fn save_image_snapshot(path: &Path, final_image: RgbaImage) -> Result<(), EditorError> {
    let encoded = encode_image(path, &final_image)?;
    let temporary_path = temporary_export_path(path)?;
    if let Err(error) = std::fs::write(&temporary_path, &encoded) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(EditorError::ImageSave(error.to_string()));
    }
    if let Err(error) = replace_file(&temporary_path, path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(EditorError::ImageSave(error.to_string()));
    }
    Ok(())
}

/// Encode for the destination extension. JPEG flattens onto white; unknown
/// extensions are rejected rather than silently PNG'd.
fn encode_image(path: &Path, image: &RgbaImage) -> Result<Vec<u8>, EditorError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mut bytes = std::io::Cursor::new(Vec::new());
    match ext.as_str() {
        "png" => image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| EditorError::ImageSave(e.to_string())),
        "jpg" | "jpeg" => {
            // JPEG has no alpha: composite onto white like the Linux export.
            let mut flat = RgbaImage::from_pixel(
                image.width(),
                image.height(),
                image::Rgba([255, 255, 255, 255]),
            );
            image::imageops::overlay(&mut flat, image, 0, 0);
            DynamicImage::ImageRgba8(flat)
                .to_rgb8()
                .write_to(&mut bytes, image::ImageFormat::Jpeg)
                .map_err(|e| EditorError::ImageSave(e.to_string()))
        }
        "webp" => image
            .write_to(&mut bytes, image::ImageFormat::WebP)
            .map_err(|e| EditorError::ImageSave(e.to_string())),
        other => Err(EditorError::ImageSave(format!(
            "unsupported image extension: {other}"
        ))),
    }
    .map(|()| bytes.into_inner())
}

/// Atomically move `from` over `to`. Windows `MoveFileExW` with
/// REPLACE_EXISTING is the safe replace: the old destination survives when
/// the move itself fails.
#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from_wide: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to_wide: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

fn temporary_export_path(path: &Path) -> Result<PathBuf, EditorError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("image");
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .unwrap_or("png");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    for attempt in 0..8 {
        let temporary_path = parent.join(format!(
            ".{stem}.apexshot-{}-{stamp}-{attempt}.{extension}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => {
                drop(file);
                return Ok(temporary_path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(EditorError::ImageSave(error.to_string())),
        }
    }

    Err(EditorError::ImageSave(
        "could not allocate a temporary image export file".to_string(),
    ))
}

/// PNG-encode the flattened document for the clipboard.
pub fn clipboard_png_bytes(state: &EditorState) -> Result<Vec<u8>, EditorError> {
    png_bytes(&state.to_final_image()?)
}

/// PNG-encode an already-flattened frame for the clipboard.
pub fn png_bytes(image: &RgbaImage) -> Result<Vec<u8>, EditorError> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| EditorError::ImageSave(error.to_string()))?;
    Ok(bytes.into_inner())
}

/// BMP-encode a frame for the Windows clipboard. GTK's Win32 backend maps
/// `image/bmp` onto the native `CF_DIB` family, which most Windows apps paste
/// from; a texture alone only advertises PNG/JPEG/TIFF.
pub fn bmp_bytes(image: &RgbaImage) -> Result<Vec<u8>, EditorError> {
    const FILE_HEADER: usize = 14;
    const INFO_HEADER: usize = 40;
    let (width, height) = image.dimensions();
    let row = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| EditorError::ImageSave("image is too wide to copy".to_string()))?;
    let pixels = row
        .checked_mul(height as usize)
        .ok_or_else(|| EditorError::ImageSave("image is too large to copy".to_string()))?;
    let file_size = FILE_HEADER + INFO_HEADER + pixels;
    let file_size = u32::try_from(file_size)
        .map_err(|_| EditorError::ImageSave("image is too large to copy".to_string()))?;
    let pixels = u32::try_from(pixels)
        .map_err(|_| EditorError::ImageSave("image is too large to copy".to_string()))?;

    let mut bytes = Vec::with_capacity(file_size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&((FILE_HEADER + INFO_HEADER) as u32).to_le_bytes());
    bytes.extend_from_slice(&(INFO_HEADER as u32).to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&pixels.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..height).rev() {
        for x in 0..width {
            let pixel = image.get_pixel(x, y);
            bytes.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(bytes)
}

/// Save a clipboard snapshot PNG under `dir`, never overwriting an earlier copy.
pub fn save_clipboard_image(dir: &Path, state: &EditorState) -> Result<PathBuf, EditorError> {
    let final_image = state.to_final_image()?;
    std::fs::create_dir_all(dir).map_err(|e| EditorError::ImageSave(e.to_string()))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| EditorError::ImageSave(e.to_string()))?
        .as_nanos();
    let path = dir.join(format!(
        "apexshot-clipboard-{stamp}-{}.png",
        std::process::id()
    ));
    let result = {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        final_image
            .write_to(&mut writer, image::ImageFormat::Png)
            .and_then(|()| writer.flush().map_err(image::ImageError::from))
    };
    if let Err(error) = result {
        let _ = std::fs::remove_file(&path);
        return Err(EditorError::ImageSave(error.to_string()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn bmp_bytes_writes_a_bottom_up_32_bit_dib() {
        let mut image = RgbaImage::new(2, 2);
        image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        image.put_pixel(0, 1, Rgba([0, 0, 255, 255]));
        image.put_pixel(1, 1, Rgba([255, 255, 255, 255]));
        let bmp = bmp_bytes(&image).unwrap();

        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()), 70);
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        assert_eq!(u32::from_le_bytes(bmp[14..18].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(bmp[18..22].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(bmp[22..26].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(bmp[26..28].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(bmp[28..30].try_into().unwrap()), 32);
        // The file starts with the image's bottom row, stored BGRA.
        assert_eq!(&bmp[54..58], &[255, 0, 0, 255]);
        assert_eq!(&bmp[58..62], &[255, 255, 255, 255]);
        assert_eq!(&bmp[62..66], &[0, 0, 255, 255]);
        assert_eq!(&bmp[66..70], &[0, 255, 0, 255]);
    }
}
