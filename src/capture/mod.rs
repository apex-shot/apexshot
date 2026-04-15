//! Image saving, format conversion, and cursor compositing
//!
//! This module handles converting raw `CaptureData` into standard image formats
//! and saving them to disk with proper naming conventions.

pub mod editor;
mod preview_overlay;
pub use editor::types::{
    AnnotationAction, ArrowStyle, DrawColor, FontSettings, ObfuscateMethod, Point, Rect,
};
pub use editor::{open_image_editor, EditorError};
pub fn copy_capture_uri_to_clipboard(path: &Path) -> Result<(), String> {
    editor::copy_file_uri_to_clipboard(path)
}
pub use preview_overlay::{show_capture_preview_overlay, CapturePreviewError};

use crate::backend::{CaptureData, CursorData, PixelFormat};
use image::buffer::ConvertBuffer;
use image::{ImageBuffer, RgbImage, Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

/// Errors that can occur during image saving
#[derive(Debug, Error)]
pub enum SaveError {
    #[error("Invalid pixel format for conversion: {0:?}")]
    InvalidPixelFormat(PixelFormat),

    #[error("Failed to generate filename: {0}")]
    FilenameError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Image encoding error: {0}")]
    ImageError(#[from] image::ImageError),
}

pub type SaveResult<T> = Result<T, SaveError>;

/// Output image format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg { quality: u8 },
    WebP,
}

impl ImageFormat {
    /// Get the file extension for this format
    pub fn extension(&self) -> &str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg { .. } => "jpg",
            ImageFormat::WebP => "webp",
        }
    }

    /// Validate JPEG quality (1-100)
    pub fn validate_jpeg_quality(quality: u8) -> SaveResult<()> {
        if !(1..=100).contains(&quality) {
            return Err(SaveError::FilenameError(
                "JPEG quality must be between 1 and 100".into(),
            ));
        }
        Ok(())
    }
}

/// Configuration for saving captures
#[derive(Debug, Clone)]
pub struct SaveConfig {
    /// Output directory (defaults to XDG Pictures directory)
    pub output_dir: Option<PathBuf>,
    /// Image format to use
    pub format: ImageFormat,
    /// Whether to include cursor overlay
    pub include_cursor: bool,
    /// Optional prefix for filenames
    pub filename_prefix: Option<String>,
    /// Optional timestamp format (strftime-style)
    /// Default: "%Y-%m-%d_%H-%M-%S"
    pub timestamp_format: Option<String>,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            output_dir: None,
            format: ImageFormat::Png,
            include_cursor: true,
            filename_prefix: None,
            timestamp_format: None,
        }
    }
}

impl SaveConfig {
    /// Create a new save config with the specified output directory
    pub fn with_output_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.output_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Create a new save config with the specified format
    pub fn with_format(mut self, format: ImageFormat) -> Self {
        self.format = format;
        self
    }

    /// Create a new save config with cursor inclusion
    pub fn with_cursor(mut self, include: bool) -> Self {
        self.include_cursor = include;
        self
    }

    /// Create a new save config with a filename prefix
    pub fn with_prefix<S: Into<String>>(mut self, prefix: S) -> Self {
        self.filename_prefix = Some(prefix.into());
        self
    }

    /// Get the output directory, defaulting to XDG Pictures
    pub fn get_output_dir(&self) -> SaveResult<PathBuf> {
        if let Some(dir) = &self.output_dir {
            Ok(dir.clone())
        } else {
            dirs::picture_dir().ok_or_else(|| {
                SaveError::FilenameError("Could not determine Pictures directory".into())
            })
        }
    }
}

/// Convert `CaptureData` to an `RgbImage`
///
/// This handles various pixel formats (RGB/BGR, 24/32-bit) and converts
/// them to the standard RGB8 format expected by the image crate.
fn capture_to_rgb_image(capture: &CaptureData) -> Result<RgbImage, SaveError> {
    let format = capture.format;
    let width = capture.width;
    let height = capture.height;

    let pixels = if format == PixelFormat::RGB24 {
        // Direct conversion - just need to handle stride
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            let row_end = row_start + (width * 3) as usize;
            pixels.extend_from_slice(&capture.pixels[row_start..row_end]);
        }
        pixels
    } else if format == PixelFormat::RGB32 {
        // Skip padding byte
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            for x in 0..width {
                let pixel_start = row_start + (x * 4) as usize;
                pixels.push(capture.pixels[pixel_start]); // R
                pixels.push(capture.pixels[pixel_start + 1]); // G
                pixels.push(capture.pixels[pixel_start + 2]); // B
                                                              // Skip padding byte at pixel_start + 3
            }
        }
        pixels
    } else if format == PixelFormat::RGBA32 {
        // Drop alpha channel
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            for x in 0..width {
                let pixel_start = row_start + (x * 4) as usize;
                pixels.push(capture.pixels[pixel_start]); // R
                pixels.push(capture.pixels[pixel_start + 1]); // G
                pixels.push(capture.pixels[pixel_start + 2]); // B
                                                              // Skip alpha at pixel_start + 3
            }
        }
        pixels
    } else if format == PixelFormat::BGR24 {
        // Swap R and B
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            let row_end = row_start + (width * 3) as usize;
            for chunk in capture.pixels[row_start..row_end].chunks_exact(3) {
                pixels.push(chunk[2]); // B -> R
                pixels.push(chunk[1]); // G
                pixels.push(chunk[0]); // R -> B
            }
        }
        pixels
    } else if format == PixelFormat::BGR32 {
        // Swap R and B, skip padding
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            for x in 0..width {
                let pixel_start = row_start + (x * 4) as usize;
                pixels.push(capture.pixels[pixel_start + 2]); // B -> R
                pixels.push(capture.pixels[pixel_start + 1]); // G
                pixels.push(capture.pixels[pixel_start]); // R -> B
                                                          // Skip padding at pixel_start + 3
            }
        }
        pixels
    } else if format == PixelFormat::BGRA32 {
        // Swap R and B, drop alpha
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            for x in 0..width {
                let pixel_start = row_start + (x * 4) as usize;
                pixels.push(capture.pixels[pixel_start + 2]); // B -> R
                pixels.push(capture.pixels[pixel_start + 1]); // G
                pixels.push(capture.pixels[pixel_start]); // R -> B
                                                          // Skip alpha at pixel_start + 3
            }
        }
        pixels
    } else {
        return Err(SaveError::InvalidPixelFormat(format));
    };

    ImageBuffer::from_raw(width, height, pixels)
        .ok_or_else(|| SaveError::InvalidPixelFormat(format))
}

/// Convert `CaptureData` to an `RgbaImage`
///
/// Similar to `capture_to_rgb_image` but preserves alpha channel.
pub fn capture_to_rgba_image(capture: &CaptureData) -> Result<RgbaImage, SaveError> {
    let format = capture.format;
    let width = capture.width;
    let height = capture.height;

    let pixels = if format == PixelFormat::RGB24 || format == PixelFormat::BGR24 {
        // Add opaque alpha
        let rgb = capture_to_rgb_image(capture)?;
        // Convert RGB to RGBA by adding alpha channel
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for pixel in rgb.as_raw().chunks_exact(3) {
            pixels.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
        }
        pixels
    } else if format == PixelFormat::RGB32 || format == PixelFormat::BGR32 {
        // Add opaque alpha, skip padding
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            for x in 0..width {
                let pixel_start = row_start + (x * 4) as usize;
                let r = capture.pixels[pixel_start];
                let g = capture.pixels[pixel_start + 1];
                let b = capture.pixels[pixel_start + 2];

                if format == PixelFormat::BGR32 {
                    pixels.extend_from_slice(&[b, g, r, 255]);
                } else {
                    pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
        }
        pixels
    } else if format == PixelFormat::RGBA32 || format == PixelFormat::BGRA32 {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let row_start = (y * capture.stride) as usize;
            let row_end = row_start + (width * 4) as usize;
            let row = &capture.pixels[row_start..row_end];

            if format == PixelFormat::BGRA32 {
                // Swap R and B, keep alpha
                for chunk in row.chunks_exact(4) {
                    pixels.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
                }
            } else {
                pixels.extend_from_slice(row);
            }
        }
        pixels
    } else {
        return Err(SaveError::InvalidPixelFormat(format));
    };

    ImageBuffer::from_raw(width, height, pixels)
        .ok_or_else(|| SaveError::InvalidPixelFormat(format))
}

/// Composite cursor data onto an image
fn composite_cursor(image: &mut RgbaImage, cursor: &CursorData) {
    let CursorData {
        pixels,
        width,
        height,
        x,
        y,
        xhot: _,
        yhot: _,
    } = cursor;

    // Clamp cursor position to image bounds
    let start_x = (*x).max(0) as u32;
    let start_y = (*y).max(0) as u32;

    // Iterate over cursor pixels
    for cy in 0..*height {
        for cx in 0..*width {
            let img_x = start_x + cx;
            let img_y = start_y + cy;

            // Check bounds
            if img_x >= image.width() || img_y >= image.height() {
                continue;
            }

            let cursor_idx = ((cy * *width + cx) * 4) as usize;
            let r = pixels[cursor_idx];
            let g = pixels[cursor_idx + 1];
            let b = pixels[cursor_idx + 2];
            let a = pixels[cursor_idx + 3];

            // Skip fully transparent pixels
            if a == 0 {
                continue;
            }

            // Simple alpha blending
            let pixel = image.get_pixel_mut(img_x, img_y);
            let inv_alpha = 255 - a;
            *pixel = Rgba([
                ((r as u32 * a as u32 + pixel[0] as u32 * inv_alpha as u32) / 255) as u8,
                ((g as u32 * a as u32 + pixel[1] as u32 * inv_alpha as u32) / 255) as u8,
                ((b as u32 * a as u32 + pixel[2] as u32 * inv_alpha as u32) / 255) as u8,
                255,
            ]);
        }
    }
}

/// Generate a timestamped filename
fn generate_filename(config: &SaveConfig) -> String {
    let timestamp = if config.timestamp_format.is_some() {
        "custom".to_string()
    } else {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let datetime = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
            .unwrap_or_else(chrono::Utc::now);
        datetime.format("%Y-%m-%d_%H-%M-%S").to_string()
    };

    let prefix = config.filename_prefix.as_deref().unwrap_or("ApexShot");
    format!("{}{}.{}", prefix, timestamp, config.format.extension())
}

pub fn save_existing_png(source_path: &Path, config: &SaveConfig) -> SaveResult<PathBuf> {
    let filename = generate_filename(config);
    let output_dir = config.get_output_dir()?;
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join(filename);

    if source_path == output_path {
        return Ok(output_path);
    }

    match config.format {
        ImageFormat::Png => match std::fs::rename(source_path, &output_path) {
            Ok(()) => Ok(output_path),
            Err(_) => {
                std::fs::copy(source_path, &output_path)?;
                std::fs::remove_file(source_path)?;
                Ok(output_path)
            }
        },
        ImageFormat::Jpeg { quality } => {
            ImageFormat::validate_jpeg_quality(quality)?;
            let image = image::open(source_path)?;
            let rgb_image: RgbImage = image.into_rgb8();
            rgb_image.save_with_format(&output_path, image::ImageFormat::Jpeg)?;
            let _ = std::fs::remove_file(source_path);
            Ok(output_path)
        }
        ImageFormat::WebP => {
            let image = image::open(source_path)?;
            image.save_with_format(&output_path, image::ImageFormat::WebP)?;
            let _ = std::fs::remove_file(source_path);
            Ok(output_path)
        }
    }
}

pub fn save_capture(capture: &CaptureData, config: &SaveConfig) -> SaveResult<PathBuf> {
    // Convert to RGBA for potential cursor compositing
    let mut image = capture_to_rgba_image(capture)?;

    // Composite cursor if enabled and present
    if config.include_cursor {
        if let Some(cursor) = &capture.cursor {
            composite_cursor(&mut image, cursor);
        }
    }

    // Generate filename and path
    let filename = generate_filename(config);
    let output_dir = config.get_output_dir()?;

    // Ensure output directory exists
    std::fs::create_dir_all(&output_dir)?;

    let output_path = output_dir.join(&filename);

    // Save based on format
    match config.format {
        ImageFormat::Png => {
            image.save(&output_path)?;
        }
        ImageFormat::Jpeg { quality } => {
            ImageFormat::validate_jpeg_quality(quality)?;
            // Convert to RGB for JPEG (no alpha)
            let rgb_image: RgbImage = image.convert();
            rgb_image.save_with_format(&output_path, image::ImageFormat::Jpeg)?;
        }
        ImageFormat::WebP => {
            image.save_with_format(&output_path, image::ImageFormat::WebP)?;
        }
    }

    Ok(output_path)
}

/// Quick save with default configuration
///
/// Saves to XDG Pictures directory with PNG format and timestamped filename.
pub fn quick_save(capture: &CaptureData) -> SaveResult<PathBuf> {
    save_capture(capture, &SaveConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::PixelFormat;
    use image::Rgb;

    #[test]
    fn test_image_format_extension() {
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Jpeg { quality: 90 }.extension(), "jpg");
        assert_eq!(ImageFormat::WebP.extension(), "webp");
    }

    #[test]
    fn test_jpeg_quality_validation() {
        assert!(ImageFormat::validate_jpeg_quality(1).is_ok());
        assert!(ImageFormat::validate_jpeg_quality(100).is_ok());
        assert!(ImageFormat::validate_jpeg_quality(0).is_err());
        assert!(ImageFormat::validate_jpeg_quality(101).is_err());
    }

    #[test]
    fn test_save_config_default() {
        let config = SaveConfig::default();
        assert!(config.output_dir.is_none());
        assert_eq!(config.format, ImageFormat::Png);
        assert!(config.include_cursor);
        assert!(config.filename_prefix.is_none());
    }

    #[test]
    fn test_save_config_builder() {
        let config = SaveConfig::default()
            .with_format(ImageFormat::Jpeg { quality: 85 })
            .with_cursor(false)
            .with_prefix("test");

        assert_eq!(config.format, ImageFormat::Jpeg { quality: 85 });
        assert!(!config.include_cursor);
        assert_eq!(config.filename_prefix, Some("test".to_string()));
    }

    #[test]
    fn test_rgb24_conversion() {
        // Create a 2x2 RGB24 image
        let pixels = vec![
            255, 0, 0, // Red
            0, 255, 0, // Green
            0, 0, 255, // Blue
            255, 255, 0, // Yellow
        ];
        let capture = CaptureData::new(pixels, 2, 2, PixelFormat::RGB24);
        let img = capture_to_rgb_image(&capture).unwrap();

        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(img.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(img.get_pixel(1, 0), &Rgb([0, 255, 0]));
        assert_eq!(img.get_pixel(0, 1), &Rgb([0, 0, 255]));
        assert_eq!(img.get_pixel(1, 1), &Rgb([255, 255, 0]));
    }

    #[test]
    fn test_bgr24_conversion() {
        // Create a 2x2 BGR24 image (stored as BGR)
        let pixels = vec![
            0, 0, 255, // Red (stored as BGR)
            0, 255, 0, // Green
            255, 0, 0, // Blue (stored as BGR)
            0, 255, 255, // Yellow (stored as BGR)
        ];
        let capture = CaptureData::new(pixels, 2, 2, PixelFormat::BGR24);
        let img = capture_to_rgb_image(&capture).unwrap();

        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(img.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(img.get_pixel(1, 0), &Rgb([0, 255, 0]));
        assert_eq!(img.get_pixel(0, 1), &Rgb([0, 0, 255]));
        assert_eq!(img.get_pixel(1, 1), &Rgb([255, 255, 0]));
    }

    #[test]
    fn test_rgba32_conversion() {
        // Create a 2x2 RGBA32 image
        let pixels = vec![
            255, 0, 0, 255, // Opaque red
            0, 255, 0, 128, // Half-transparent green
            0, 0, 255, 255, // Opaque blue
            255, 255, 0, 0, // Fully transparent yellow
        ];
        let capture = CaptureData::new(pixels, 2, 2, PixelFormat::RGBA32);
        let img = capture_to_rgba_image(&capture).unwrap();

        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(img.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(img.get_pixel(1, 0), &Rgba([0, 255, 0, 128]));
        assert_eq!(img.get_pixel(0, 1), &Rgba([0, 0, 255, 255]));
        assert_eq!(img.get_pixel(1, 1), &Rgba([255, 255, 0, 0]));
    }

    #[test]
    fn test_rgb32_conversion() {
        // Create a 2x2 RGB32 image (with padding)
        let pixels = vec![
            255, 0, 0, 0, // Red + padding
            0, 255, 0, 0, // Green + padding
            0, 0, 255, 0, // Blue + padding
            255, 255, 0, 0, // Yellow + padding
        ];
        let capture = CaptureData::new(pixels, 2, 2, PixelFormat::RGB32);
        let img = capture_to_rgb_image(&capture).unwrap();

        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(img.get_pixel(0, 0), &Rgb([255, 0, 0]));
        assert_eq!(img.get_pixel(1, 0), &Rgb([0, 255, 0]));
        assert_eq!(img.get_pixel(0, 1), &Rgb([0, 0, 255]));
        assert_eq!(img.get_pixel(1, 1), &Rgb([255, 255, 0]));
    }

    #[test]
    fn test_cursor_compositing() {
        // Create a 10x10 white image
        let mut image: RgbaImage = ImageBuffer::new(10, 10);
        for pixel in image.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        // Create a 2x2 red cursor at position (5, 5)
        let cursor = CursorData {
            pixels: vec![
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ], // 2x2 red, opaque
            width: 2,
            height: 2,
            x: 5,
            y: 5,
            xhot: 0,
            yhot: 0,
        };

        composite_cursor(&mut image, &cursor);

        // Check that cursor was composited correctly
        assert_eq!(image.get_pixel(5, 5), &Rgba([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(6, 5), &Rgba([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(5, 6), &Rgba([255, 0, 0, 255]));
        assert_eq!(image.get_pixel(6, 6), &Rgba([255, 0, 0, 255]));

        // Check that other pixels remain white
        assert_eq!(image.get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(image.get_pixel(9, 9), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn test_cursor_alpha_blending() {
        // Create a 10x10 white image
        let mut image: RgbaImage = ImageBuffer::new(10, 10);
        for pixel in image.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        // Create a 2x2 black cursor with 50% alpha at position (5, 5)
        let cursor = CursorData {
            pixels: vec![0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0, 128, 0, 0, 0, 128], // 2x2 black, 50% alpha
            width: 2,
            height: 2,
            x: 5,
            y: 5,
            xhot: 0,
            yhot: 0,
        };

        composite_cursor(&mut image, &cursor);

        // Check that cursor was blended (should be ~127 gray)
        let pixel = image.get_pixel(5, 5);
        assert!(pixel[0] < 255 && pixel[0] > 100); // Not white, not black
        assert_eq!(pixel[3], 255); // Result is opaque
    }

    #[test]
    fn test_cursor_out_of_bounds() {
        // Create a 10x10 white image
        let mut image: RgbaImage = ImageBuffer::new(10, 10);
        for pixel in image.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 255]);
        }

        // Create a cursor that extends beyond image bounds
        let mut pixels = vec![255u8; 36 * 4]; // 6x6 red cursor
                                              // Fill with red (alpha is already 255)
        for i in (1..pixels.len()).step_by(4) {
            pixels[i] = 0;
            pixels[i + 1] = 0;
        }
        let cursor = CursorData {
            pixels,
            width: 6,
            height: 6,
            x: 5,
            y: 5,
            xhot: 0,
            yhot: 0,
        };

        composite_cursor(&mut image, &cursor);

        // Should not panic, should only draw within bounds
        // Pixel at (9, 9) should be red (cursor extends to 10,10)
        assert_eq!(image.get_pixel(9, 9), &Rgba([255, 0, 0, 255]));
    }
}
