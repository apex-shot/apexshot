use std::sync::Arc;
use x11rb::{
    connection::Connection,
    protocol::{
        xfixes::ConnectionExt as XFixesExt,
        xproto::{self, ConnectionExt as _, ImageFormat, ImageOrder, Screen, Setup, Visualtype},
    },
    rust_connection::RustConnection,
};

use super::{CaptureData, CursorData, DisplayBackend, DisplayError, DisplayResult, PixelFormat};

#[derive(Debug)]
enum X11Error {
    Connection(x11rb::errors::ConnectionError),
    Reply(x11rb::errors::ReplyError),
}

impl From<x11rb::errors::ConnectionError> for X11Error {
    fn from(err: x11rb::errors::ConnectionError) -> Self {
        X11Error::Connection(err)
    }
}

impl From<x11rb::errors::ReplyError> for X11Error {
    fn from(err: x11rb::errors::ReplyError) -> Self {
        X11Error::Reply(err)
    }
}

impl std::fmt::Display for X11Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            X11Error::Connection(e) => write!(f, "X11 connection error: {}", e),
            X11Error::Reply(e) => write!(f, "X11 reply error: {}", e),
        }
    }
}

impl std::error::Error for X11Error {}

pub struct X11Backend {
    conn: Arc<RustConnection>,
    screen_num: usize,
    root: xproto::Window,
    visual: Visualtype,
    xfixes_version: Option<(u32, u32)>, // (major, minor) version if XFixes is available
}

impl X11Backend {
    fn get_visual(screen: &Screen) -> Option<Visualtype> {
        // Try to find a visual that matches our needs (24/32 bit depth)
        let depth = screen
            .allowed_depths
            .iter()
            .find(|d| d.depth == 24 || d.depth == 32)?;

        depth
            .visuals
            .iter()
            .find(|v| {
                v.class == xproto::VisualClass::TRUE_COLOR
                    && v.bits_per_rgb_value == 8
                    && v.red_mask != 0
                    && v.green_mask != 0
                    && v.blue_mask != 0
            })
            .cloned()
    }

    fn detect_pixel_format(visual: &Visualtype, setup: &Setup) -> PixelFormat {
        // Calculate total bits needed for RGB values
        let rgb_bits = visual.bits_per_rgb_value * 3;

        // Pad to 32 bits if we need more than 24 bits or for alignment
        let bits_per_pixel = if rgb_bits > 24 || visual.bits_per_rgb_value == 8 {
            32
        } else {
            24
        };
        let bytes_per_pixel = (bits_per_pixel + 7) / 8;

        // Handle different byte orders.
        // On LSB_FIRST (little-endian) systems, X11 ZPixmap stores pixels as
        // [BLUE, GREEN, RED, PAD] in memory even though visual masks say RGB.
        // Swap red and blue masks so the PixelFormat reflects the actual byte
        // layout (BGR32), which capture_to_rgb_image() already handles correctly.
        let (red_mask, green_mask, blue_mask) = match setup.image_byte_order {
            ImageOrder::LSB_FIRST => (visual.blue_mask, visual.green_mask, visual.red_mask),
            ImageOrder::MSB_FIRST => (
                visual.red_mask.swap_bytes(),
                visual.green_mask.swap_bytes(),
                visual.blue_mask.swap_bytes(),
            ),
            _ => (visual.red_mask, visual.green_mask, visual.blue_mask), // fallback to LSB for unknown orders
        };

        PixelFormat {
            bits_per_pixel: bits_per_pixel as u8,
            bytes_per_pixel: bytes_per_pixel as u8,
            red_mask,
            green_mask,
            blue_mask,
        }
    }

    fn get_image(&self, x: i32, y: i32, width: u16, height: u16) -> Result<Vec<u8>, X11Error> {
        // Validate coordinates are within i16 range
        if x < i16::MIN as i32 || x > i16::MAX as i32 || y < i16::MIN as i32 || y > i16::MAX as i32
        {
            return Err(X11Error::Connection(
                x11rb::errors::ConnectionError::UnknownError,
            ));
        }

        let cookie = self.conn.get_image(
            ImageFormat::Z_PIXMAP,
            self.root,
            x as i16,
            y as i16,
            width,
            height,
            !0, // plane mask (all planes)
        )?;

        cookie
            .reply()
            .map(|reply| reply.data)
            .map_err(X11Error::from)
    }

    fn get_cursor(&self, x: i32, y: i32, width: i32, height: i32) -> Option<CursorData> {
        // Skip if XFixes not available
        self.xfixes_version?;

        // Get cursor image
        // Convert Result to Option, discarding the error
        let cursor = match self.conn.xfixes_get_cursor_image().ok()?.reply() {
            Ok(reply) => reply,
            Err(_) => return None,
        };

        // Convert cursor position to capture area coordinates
        let cursor_x = cursor.x as i32 - x;
        let cursor_y = cursor.y as i32 - y;

        // Skip if cursor is outside capture area
        if cursor_x >= width
            || cursor_y >= height
            || cursor_x + cursor.width as i32 <= 0
            || cursor_y + cursor.height as i32 <= 0
        {
            return None;
        }

        // Convert ARGB cursor pixels to RGBA
        let mut pixels = Vec::with_capacity(cursor.cursor_image.len() * 4);
        for pixel in cursor.cursor_image {
            let a = ((pixel >> 24) & 0xff) as u8;
            let r = ((pixel >> 16) & 0xff) as u8;
            let g = ((pixel >> 8) & 0xff) as u8;
            let b = (pixel & 0xff) as u8;
            pixels.extend_from_slice(&[r, g, b, a]);
        }

        Some(CursorData {
            pixels,
            width: cursor.width as u32,
            height: cursor.height as u32,
            x: cursor_x,
            y: cursor_y,
            xhot: cursor.xhot as u32,
            yhot: cursor.yhot as u32,
        })
    }
}

impl DisplayBackend for X11Backend {
    fn new() -> DisplayResult<Self> {
        // Connect to X server
        // Connect to X server
        let (conn, screen_num) = RustConnection::connect(None).map_err(|e| {
            DisplayError::InitializationError(format!("Failed to connect to X server: {}", e))
        })?;
        let conn = Arc::new(conn);

        // Get screen and root window
        let setup = conn.setup();
        let screen = &setup.roots[screen_num];
        let root = screen.root;

        // Find appropriate visual
        let visual = Self::get_visual(screen).ok_or_else(|| {
            DisplayError::InitializationError(
                "No suitable visual found (need 24/32 bit TrueColor)".into(),
            )
        })?;

        // Initialize XFixes if available
        let xfixes_version = match conn.xfixes_query_version(5, 0) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => Some((reply.major_version, reply.minor_version)),
                Err(_) => None,
            },
            Err(_) => None,
        };

        Ok(Self {
            conn,
            screen_num,
            root,
            visual,
            xfixes_version,
        })
    }

    fn capture_screen(&self) -> DisplayResult<CaptureData> {
        let screen = &self.conn.setup().roots[self.screen_num];
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;

        let pixels = self
            .get_image(0, 0, width, height)
            .map_err(|e| DisplayError::CaptureError(format!("Failed to capture screen: {}", e)))?;

        let format = Self::detect_pixel_format(&self.visual, self.conn.setup());

        Ok(CaptureData::with_cursor(
            pixels,
            width as u32,
            height as u32,
            format,
            self.get_cursor(0, 0, width as i32, height as i32),
        ))
    }

    fn capture_area(&self, x: i32, y: i32, width: i32, height: i32) -> DisplayResult<CaptureData> {
        // Validate input dimensions and coordinates
        if width <= 0 || height <= 0 || x < 0 || y < 0 {
            return Err(DisplayError::InvalidArea(format!(
                "Invalid dimensions: {}x{}",
                width, height
            )));
        }

        let pixels = self
            .get_image(x, y, width as u16, height as u16)
            .map_err(|e| DisplayError::CaptureError(format!("Failed to capture area: {}", e)))?;

        let format = Self::detect_pixel_format(&self.visual, self.conn.setup());

        Ok(CaptureData::with_cursor(
            pixels,
            width as u32,
            height as u32,
            format,
            self.get_cursor(x, y, width, height),
        ))
    }

    fn capture_window(&self, window_id: u64) -> DisplayResult<CaptureData> {
        // Get window geometry
        let geom = self
            .conn
            .get_geometry(window_id as u32)
            .map_err(|e| {
                DisplayError::CaptureError(format!("Failed to get window geometry: {}", e))
            })?
            .reply()
            .map_err(|e| {
                DisplayError::CaptureError(format!("Failed to get window geometry reply: {}", e))
            })?;

        let data = self.capture_area(
            geom.x as i32,
            geom.y as i32,
            geom.width as i32,
            geom.height as i32,
        )?;

        // For window captures, translate cursor coordinates to window-relative
        Ok(CaptureData {
            cursor: data.cursor.map(|mut c| {
                c.x -= geom.x as i32;
                c.y -= geom.y as i32;
                c
            }),
            ..data
        })
    }

    fn is_supported() -> bool {
        // Try to connect to X server
        RustConnection::connect(None).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn test_is_supported() {
        // This will depend on whether X11 is actually running
        let supported = X11Backend::is_supported();
        println!("X11 backend supported: {}", supported);
    }

    #[test]
    fn test_pixel_format_detection() {
        let visual = Visualtype {
            visual_id: 0,
            class: xproto::VisualClass::TRUE_COLOR,
            bits_per_rgb_value: 8,
            colormap_entries: 256,
            red_mask: 0xFF0000,
            green_mask: 0x00FF00,
            blue_mask: 0x0000FF,
        };

        let setup = Setup {
            status: 0,
            protocol_major_version: 11,
            protocol_minor_version: 0,
            length: 0,
            release_number: 0,
            resource_id_base: 0,
            resource_id_mask: 0,
            motion_buffer_size: 0,
            maximum_request_length: 0,
            image_byte_order: ImageOrder::LSB_FIRST,
            bitmap_format_bit_order: ImageOrder::LSB_FIRST,
            bitmap_format_scanline_unit: 0,
            bitmap_format_scanline_pad: 0,
            min_keycode: 0,
            max_keycode: 0,
            vendor: vec![],
            pixmap_formats: vec![],
            roots: vec![],
        };

        let format = X11Backend::detect_pixel_format(&visual, &setup);
        assert_eq!(format.bits_per_pixel, 32);
        assert_eq!(format.bytes_per_pixel, 4);
        assert_eq!(format.red_mask, 0xFF0000);
        assert_eq!(format.green_mask, 0x00FF00);
        assert_eq!(format.blue_mask, 0x0000FF);
    }

    #[test_case(-1, 0, 100, 100 ; "negative x")]
    #[test_case(0, -1, 100, 100 ; "negative y")]
    #[test_case(0, 0, 0, 100 ; "zero width")]
    #[test_case(0, 0, 100, 0 ; "zero height")]
    #[test_case(0, 0, -1, 100 ; "negative width")]
    #[test_case(0, 0, 100, -1 ; "negative height")]
    fn test_capture_area_invalid_dimensions(x: i32, y: i32, width: i32, height: i32) {
        if let Ok(backend) = X11Backend::new() {
            let result = backend.capture_area(x, y, width, height);
            assert!(result.is_err());
            if let Err(e) = result {
                assert!(matches!(e, DisplayError::InvalidArea(_)));
            }
        }
    }
}
