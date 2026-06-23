pub mod kde_screenshot;
pub mod portal_permissions;
pub mod screencopy;
pub mod wayland;
pub mod x11;

// Re-export backend implementations
pub use wayland::WaylandBackend;
pub use x11::X11Backend;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DisplayError {
    #[error("Backend not supported: {0}")]
    UnsupportedBackend(String),

    #[error("Failed to initialize display backend: {0}")]
    InitializationError(String),

    #[error("Capture failed: {0}")]
    CaptureError(String),

    #[error("Invalid area: {0}")]
    InvalidArea(String),

    #[error("Portal error: {0}")]
    PortalError(String),

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

pub type DisplayResult<T> = Result<T, DisplayError>;

/// Represents the pixel format of captured image data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormat {
    /// Bits per pixel (e.g. 24 for RGB, 32 for RGBA)
    pub bits_per_pixel: u8,

    /// Bytes per pixel (e.g. 3 for RGB, 4 for RGBA)
    pub bytes_per_pixel: u8,

    /// Bit mask for red channel
    pub red_mask: u32,

    /// Bit mask for green channel
    pub green_mask: u32,

    /// Bit mask for blue channel
    pub blue_mask: u32,
}

impl PixelFormat {
    /// 24-bit RGB format (8 bits per channel)
    pub const RGB24: Self = Self {
        bits_per_pixel: 24,
        bytes_per_pixel: 3,
        red_mask: 0xFF0000,
        green_mask: 0x00FF00,
        blue_mask: 0x0000FF,
    };

    /// 32-bit RGB format (8 bits per channel + 8 bits padding)
    pub const RGB32: Self = Self {
        bits_per_pixel: 32,
        bytes_per_pixel: 4,
        red_mask: 0xFF0000,
        green_mask: 0x00FF00,
        blue_mask: 0x0000FF,
    };

    /// 32-bit RGBA format (8 bits per channel)
    pub const RGBA32: Self = Self {
        bits_per_pixel: 32,
        bytes_per_pixel: 4,
        red_mask: 0xFF000000,
        green_mask: 0x00FF0000,
        blue_mask: 0x0000FF00,
    };

    /// 24-bit BGR format (8 bits per channel)
    pub const BGR24: Self = Self {
        bits_per_pixel: 24,
        bytes_per_pixel: 3,
        red_mask: 0x0000FF,
        green_mask: 0x00FF00,
        blue_mask: 0xFF0000,
    };

    /// 32-bit BGR format (8 bits per channel + 8 bits padding)
    pub const BGR32: Self = Self {
        bits_per_pixel: 32,
        bytes_per_pixel: 4,
        red_mask: 0x0000FF,
        green_mask: 0x00FF00,
        blue_mask: 0xFF0000,
    };

    /// 32-bit BGRA format (8 bits per channel)
    pub const BGRA32: Self = Self {
        bits_per_pixel: 32,
        bytes_per_pixel: 4,
        red_mask: 0x0000FF00,
        green_mask: 0x00FF0000,
        blue_mask: 0xFF000000,
    };
}

/// Cursor information for a capture
#[derive(Debug, Clone)]
pub struct CursorData {
    /// Raw RGBA pixel data for cursor image
    pub pixels: Vec<u8>,

    /// Cursor width in pixels
    pub width: u32,

    /// Cursor height in pixels
    pub height: u32,

    /// Cursor x position relative to capture area
    pub x: i32,

    /// Cursor y position relative to capture area
    pub y: i32,

    /// X offset of cursor hotspot
    pub xhot: u32,

    /// Y offset of cursor hotspot
    pub yhot: u32,
}

/// Raw captured image data and metadata
#[derive(Debug, Clone)]
pub struct CaptureData {
    /// Raw pixel data in the specified format
    pub pixels: Vec<u8>,

    /// Image width in pixels
    pub width: u32,

    /// Image height in pixels
    pub height: u32,

    /// Bytes per row (may include padding)
    pub stride: u32,

    /// Pixel format specification
    pub format: PixelFormat,

    /// Optional cursor overlay data
    pub cursor: Option<CursorData>,

    /// The logical desktop origin (x, y) of the captured output.
    /// Used to correctly offset area-crop coordinates when the capture
    /// comes from a single monitor in a multi-monitor layout.
    pub output_origin_x: i32,
    pub output_origin_y: i32,

    /// The compositor-reported scale factor for this output (≥1).
    /// Physical pixels = logical pixels × scale.
    pub output_scale: i32,
}

impl CaptureData {
    /// Create a new CaptureData instance with validation
    pub fn new(pixels: Vec<u8>, width: u32, height: u32, format: PixelFormat) -> Self {
        Self::with_cursor(pixels, width, height, format, None)
    }

    /// Create a new CaptureData instance with cursor data
    pub fn with_cursor(
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        format: PixelFormat,
        cursor: Option<CursorData>,
    ) -> Self {
        let stride = width * format.bytes_per_pixel as u32;
        let expected_size = height * stride;

        assert_eq!(
            pixels.len() as u32,
            expected_size,
            "pixels length must match dimensions"
        );

        Self {
            pixels,
            width,
            height,
            stride,
            format,
            cursor,
            output_origin_x: 0,
            output_origin_y: 0,
            output_scale: 1,
        }
    }

    /// Get the total size in bytes that this image should occupy
    pub fn size_bytes(&self) -> u32 {
        self.height * self.stride
    }
}

/// Core trait for display server backends
pub trait DisplayBackend {
    /// Initialize a new backend instance
    fn new() -> DisplayResult<Self>
    where
        Self: Sized;

    /// Capture the entire screen
    fn capture_screen(&self) -> DisplayResult<CaptureData>;

    /// Capture a specific area
    ///
    /// # Arguments
    /// * `x` - X coordinate of capture area
    /// * `y` - Y coordinate of capture area
    /// * `width` - Width of capture area
    /// * `height` - Height of capture area
    fn capture_area(&self, x: i32, y: i32, width: i32, height: i32) -> DisplayResult<CaptureData>;

    /// Capture a specific window
    ///
    /// # Arguments
    /// * `window_id` - ID of window to capture
    fn capture_window(&self, window_id: u64) -> DisplayResult<CaptureData>;

    /// Check if this backend is supported on the current system
    fn is_supported() -> bool
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    #[test]
    fn test_pixel_format_rgb24() {
        let format = PixelFormat::RGB24;
        assert_eq!(format.bits_per_pixel, 24);
        assert_eq!(format.bytes_per_pixel, 3);
        assert_eq!(format.red_mask, 0xFF0000);
        assert_eq!(format.green_mask, 0x00FF00);
        assert_eq!(format.blue_mask, 0x0000FF);
    }

    #[test_case(PixelFormat::RGB32, 32, 4, 0xFF0000, 0x00FF00, 0x0000FF ; "rgb32")]
    #[test_case(PixelFormat::RGBA32, 32, 4, 0xFF000000, 0x00FF0000, 0x0000FF00 ; "rgba32")]
    #[test_case(PixelFormat::BGR24, 24, 3, 0x0000FF, 0x00FF00, 0xFF0000 ; "bgr24")]
    #[test_case(PixelFormat::BGR32, 32, 4, 0x0000FF, 0x00FF00, 0xFF0000 ; "bgr32")]
    fn test_pixel_formats(
        format: PixelFormat,
        bits: u8,
        bytes: u8,
        red: u32,
        green: u32,
        blue: u32,
    ) {
        assert_eq!(format.bits_per_pixel, bits);
        assert_eq!(format.bytes_per_pixel, bytes);
        assert_eq!(format.red_mask, red);
        assert_eq!(format.green_mask, green);
        assert_eq!(format.blue_mask, blue);
    }

    #[test]
    fn test_display_errors() {
        assert_eq!(
            DisplayError::UnsupportedBackend("x11".into()).to_string(),
            "Backend not supported: x11"
        );
        assert_eq!(
            DisplayError::InitializationError("failed to connect".into()).to_string(),
            "Failed to initialize display backend: failed to connect"
        );
        assert_eq!(
            DisplayError::CaptureError("timeout".into()).to_string(),
            "Capture failed: timeout"
        );
        assert_eq!(
            DisplayError::InvalidArea("negative width".into()).to_string(),
            "Invalid area: negative width"
        );
        assert_eq!(
            DisplayError::PortalError("permission denied".into()).to_string(),
            "Portal error: permission denied"
        );
        assert_eq!(
            DisplayError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "file not found"
            ))
            .to_string(),
            "file not found"
        );
    }

    #[test]
    fn test_capture_data_creation() {
        let data = CaptureData::new(
            vec![0; 12], // 2x2 RGB24 image
            2,
            2,
            PixelFormat::RGB24,
        );

        assert_eq!(data.pixels.len(), 12);
        assert_eq!(
            data.width * data.height * data.format.bytes_per_pixel as u32,
            12
        );
        assert_eq!(data.stride, data.width * data.format.bytes_per_pixel as u32);
        assert_eq!(data.size_bytes(), 12);
    }

    #[test_case(vec![0; 10], 2, 2, PixelFormat::RGB24 ; "too small buffer")]
    #[test_case(vec![0; 14], 2, 2, PixelFormat::RGB24 ; "too large buffer")]
    #[should_panic(expected = "pixels length must match dimensions")]
    fn test_capture_data_invalid_sizes(
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) {
        let _data = CaptureData::new(pixels, width, height, format);
    }

    #[test_case(vec![0; 16], 2, 2, PixelFormat::RGBA32 ; "rgba32")]
    #[test_case(vec![0; 18], 3, 2, PixelFormat::BGR24 ; "bgr24")]
    fn test_capture_data_different_formats(
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) {
        let data = CaptureData::new(pixels.clone(), width, height, format);
        assert_eq!(data.pixels.len(), pixels.len());
        assert_eq!(data.stride, width * format.bytes_per_pixel as u32);
        assert_eq!(data.size_bytes(), pixels.len() as u32);
    }
}
