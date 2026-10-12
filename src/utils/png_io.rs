//! PNG encoding at a real deflate level.
//!
//! `image`'s own PNG default is `CompressionType::Fast`, which the `png` crate
//! backs with `fdeflate`. That trades almost all of its compression ratio for
//! encoding speed and falls back to stored (uncompressed) blocks when it cannot
//! beat them, so a detailed full-screen capture can land tens of times larger
//! than the same pixels encoded the way every other PNG writer on the desktop
//! encodes them. Level 6 is libpng's default and what the desktop's own
//! screenshot tool writes, so PNGs stay lossless (the level never changes the
//! pixels) while the files stay small.

use std::io::{BufWriter, Write};
use std::path::Path;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ColorType, DynamicImage, ImageEncoder, ImageResult};

/// Encode raw pixels as PNG into `writer`.
pub fn encode<W: Write>(
    writer: W,
    buf: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
) -> ImageResult<()> {
    // `CompressionType::Default` maps to zlib level 6; see the module docs.
    PngEncoder::new_with_quality(writer, CompressionType::Default, FilterType::Adaptive)
        .write_image(buf, width, height, color)
}

/// Encode a `DynamicImage` as PNG into `writer`.
pub fn encode_image<W: Write>(writer: W, image: &DynamicImage) -> ImageResult<()> {
    encode(
        writer,
        image.as_bytes(),
        image.width(),
        image.height(),
        image.color(),
    )
}

/// Write raw pixels to `path` as PNG.
pub fn save_buffer(
    path: &Path,
    buf: &[u8],
    width: u32,
    height: u32,
    color: ColorType,
) -> ImageResult<()> {
    let mut writer = BufWriter::new(std::fs::File::create(path)?);
    encode(&mut writer, buf, width, height, color)?;
    writer.flush()?;
    Ok(())
}

/// Write a `DynamicImage` to `path` as PNG.
pub fn save_image(path: &Path, image: &DynamicImage) -> ImageResult<()> {
    save_buffer(
        path,
        image.as_bytes(),
        image.width(),
        image.height(),
        image.color(),
    )
}

/// Write an `RgbaImage` to `path` as PNG.
pub fn save_rgba_image(path: &Path, image: &image::RgbaImage) -> ImageResult<()> {
    save_buffer(
        path,
        image.as_raw(),
        image.width(),
        image.height(),
        ColorType::Rgba8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    /// Structured detail, the kind of content a screenshot is full of: plenty
    /// for a real deflate encoder to exploit, none of it runs of identical
    /// bytes for an RLE-style fast encoder to shortcut.
    fn detailed_image() -> RgbaImage {
        RgbaImage::from_fn(640, 400, |x, y| {
            Rgba([
                ((x * 3 + y) & 0xff) as u8,
                ((x + y * 2) & 0xff) as u8,
                ((x / 4 + y / 3) & 0xff) as u8,
                255,
            ])
        })
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("apexshot-png-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join(name)
    }

    #[test]
    fn save_rgba_image_round_trips_pixels() {
        let image = detailed_image();
        let path = temp_path("round-trip.png");
        save_rgba_image(&path, &image).expect("save png");

        let decoded = image::open(&path).expect("decode png").to_rgba8();
        assert_eq!(decoded.dimensions(), image.dimensions());
        assert_eq!(decoded.as_raw(), image.as_raw(), "PNG must stay lossless");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_compresses_instead_of_falling_back_to_stored_blocks() {
        let image = detailed_image();
        let path = temp_path("compressed.png");
        save_rgba_image(&path, &image).expect("save png");
        let ours = std::fs::read(&path).expect("read png");

        let raw = 640 * 400 * 4;
        assert!(
            ours.len() < raw / 4,
            "PNG should compress well below raw ({raw} bytes), got {} bytes",
            ours.len()
        );

        let mut crate_default = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut crate_default),
                ImageFormat::Png,
            )
            .expect("crate default encode");
        assert!(
            ours.len() < crate_default.len(),
            "shared PNG writer ({}) should beat the image crate default ({})",
            ours.len(),
            crate_default.len()
        );

        let _ = std::fs::remove_file(&path);
    }
}
