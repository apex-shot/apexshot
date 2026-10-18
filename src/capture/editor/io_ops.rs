use super::state::EditorState;
use super::types::EditorError;
use crate::utils::clipboard;
use image::DynamicImage;
use image::RgbaImage;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn save_edited_image(path: &Path, state: &EditorState) -> Result<(), EditorError> {
    crate::studio_entitlement::ensure_export_allowed(
        crate::studio_entitlement::capabilities_for_image_export(
            state.static_image_transform,
            state.source_upscale.factor() > 1,
        ),
    )
    .map_err(|error| EditorError::ImageSave(error.to_string()))?;
    let final_image = state.to_final_image()?;
    save_edited_image_snapshot(path, final_image)
}

/// Encode and save an immutable flattened editor image using the destination format.
pub(crate) fn save_edited_image_snapshot(
    path: &Path,
    final_image: RgbaImage,
) -> Result<(), EditorError> {
    let temporary_path = temporary_export_path(path)?;
    let ext = temporary_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    let result = match ext.as_str() {
        "jpg" | "jpeg" => {
            let rgb = DynamicImage::ImageRgba8(final_image).to_rgb8();
            rgb.save_with_format(&temporary_path, image::ImageFormat::Jpeg)
        }
        "png" => crate::utils::png_io::save_rgba_image(&temporary_path, &final_image),
        _ => final_image.save(&temporary_path),
    };

    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(EditorError::ImageSave(error.to_string()));
    }

    if let Err(error) = std::fs::rename(&temporary_path, path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(EditorError::ImageSave(error.to_string()));
    }

    Ok(())
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

pub fn copy_uri_to_clipboard(path: &Path) -> Result<(), String> {
    clipboard::copy_uri_to_clipboard(path)
}

pub fn save_clipboard_image(dir: &Path, state: &EditorState) -> Result<PathBuf, EditorError> {
    crate::studio_entitlement::ensure_export_allowed(
        crate::studio_entitlement::capabilities_for_image_export(
            state.static_image_transform,
            state.source_upscale.factor() > 1,
        ),
    )
    .map_err(|error| EditorError::ImageSave(error.to_string()))?;
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
        let encoded = crate::utils::png_io::encode(
            &mut writer,
            final_image.as_raw(),
            final_image.width(),
            final_image.height(),
            image::ColorType::Rgba8,
        );
        encoded.and_then(|()| writer.flush().map_err(image::ImageError::from))
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
    use crate::capture::editor::types::{
        AnnotationAction, BackgroundStyle, DrawColor, Point, Rect,
    };
    use image::{Rgba, RgbaImage};

    fn scratch_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "apexshot-editor-clipboard-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn clipboard_snapshots_follow_edits_crop_and_undo_without_overwriting_earlier_copies() {
        let dir = scratch_dir();
        let original = RgbaImage::from_pixel(100, 80, Rgba([255, 255, 255, 255]));
        let mut state = EditorState::new(original.clone());
        state.push_action(AnnotationAction::Line {
            start: Point { x: 10.0, y: 10.0 },
            end: Point { x: 60.0, y: 50.0 },
            color: DrawColor::new(0.0, 0.0, 0.0, 1.0),
            stroke_size: 4.0,
            shadow: false,
        });
        let first = save_clipboard_image(&dir, &state).unwrap();
        let first_bytes = std::fs::read(&first).unwrap();
        let first_image = image::open(&first).unwrap().to_rgba8();
        assert_ne!(first_image, original);
        assert_eq!(first_image, state.to_final_image().unwrap());

        state.crop_rect = Some(Rect {
            x: 5,
            y: 5,
            width: 70,
            height: 60,
        });
        assert!(state.apply_pending_crop());
        let cropped = save_clipboard_image(&dir, &state).unwrap();
        assert_ne!(first, cropped);
        assert_eq!(
            image::open(&cropped).unwrap().to_rgba8().dimensions(),
            (70, 60)
        );
        assert_eq!(
            image::open(&cropped).unwrap().to_rgba8(),
            state.to_final_image().unwrap()
        );
        assert_eq!(std::fs::read(&first).unwrap(), first_bytes);

        state.undo();
        let restored = save_clipboard_image(&dir, &state).unwrap();
        assert_ne!(restored, first);
        assert_eq!(image::open(restored).unwrap().to_rgba8(), first_image);
        assert_eq!(state.base_image.as_ref(), &original);
        assert_eq!(state.actions.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clipboard_snapshot_includes_background_composition_and_preserves_source_file() {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("original.jpg");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(80, 60, Rgba([255, 0, 0, 255])))
            .to_rgb8()
            .save(&source)
            .unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        let mut state = EditorState::new(image::open(&source).unwrap().to_rgba8());
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 1.0, 1.0));
        state.background_padding = 20.0;
        let snapshot = save_clipboard_image(&dir.join("clipboard"), &state).unwrap();
        let bytes = std::fs::read(&snapshot).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let rendered = image::open(snapshot).unwrap().to_rgba8();
        assert_eq!(rendered, state.to_final_image().unwrap());
        assert!(rendered.width() > state.base_image.width());
        assert!(rendered.height() > state.base_image.height());
        assert_eq!(std::fs::read(source).unwrap(), source_bytes);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clipboard_snapshot_reports_storage_and_render_failures() {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("not-a-directory");
        std::fs::write(&blocked, b"keep").unwrap();
        let state = EditorState::new(RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255])));
        assert!(save_clipboard_image(&blocked, &state).is_err());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"keep");
        let invalid = EditorState::new(RgbaImage::new(0, 0));
        let destination = dir.join("invalid");
        assert!(save_clipboard_image(&destination, &invalid).is_err());
        assert!(!destination.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn flattened_snapshot_saves_the_same_bytes_as_editor_save_for_png_and_jpeg() {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let state = EditorState::new(RgbaImage::from_pixel(12, 8, Rgba([40, 90, 180, 255])));
        let snapshot = state.to_final_image().unwrap();

        for extension in ["png", "jpg"] {
            let direct = dir.join(format!("direct.{extension}"));
            let from_snapshot = dir.join(format!("snapshot.{extension}"));
            save_edited_image(&direct, &state).unwrap();
            save_edited_image_snapshot(&from_snapshot, snapshot.clone()).unwrap();
            assert_eq!(
                std::fs::read(direct).unwrap(),
                std::fs::read(from_snapshot).unwrap()
            );
        }

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_export_encoding_preserves_an_existing_destination() {
        let dir = scratch_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let destination = dir.join("existing.unsupported");
        std::fs::write(&destination, b"keep this file").unwrap();

        let result = save_edited_image_snapshot(
            &destination,
            RgbaImage::from_pixel(12, 8, Rgba([40, 90, 180, 255])),
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"keep this file");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
