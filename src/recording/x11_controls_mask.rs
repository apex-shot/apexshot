//! Keep the X11 recording controls bar out of the recorded video.
//!
//! The X11 backend reads the root window, so every window on screen lands in
//! the frames. The bar runs in its own process and reports its X11 window id;
//! this module locates that window, and rebuilds the pixels it covers from the
//! windows stacked below it before the frame leaves the capture source.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_video as gst_video;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, MapState, Window};
use x11rb::rust_connection::RustConnection;

/// The controls bar's X11 window id, or 0 when no bar is on screen.
static CONTROLS_WINDOW: AtomicU32 = AtomicU32::new(0);

/// How often the bar's frame rectangle is re-read while recording.
const RECT_REFRESH: Duration = Duration::from_millis(100);
/// Newly created windows settle their geometry; re-read every frame for this
/// long so the first mapped frames cannot show an unmasked bar.
const RECT_SETTLE: Duration = Duration::from_secs(1);
/// Guard against a cycle in a malformed window tree.
const MAX_PARENT_WALK: usize = 16;

pub(crate) fn set_controls_window(xid: u32) {
    CONTROLS_WINDOW.store(xid, Ordering::SeqCst);
}

/// Clear the id only when it still belongs to `xid`, so a bar that closes
/// after its replacement started cannot hide the new window id.
pub(crate) fn clear_controls_window_if(xid: u32) {
    let _ = CONTROLS_WINDOW.compare_exchange(xid, 0, Ordering::SeqCst, Ordering::SeqCst);
}

pub(crate) fn controls_window() -> u32 {
    CONTROLS_WINDOW.load(Ordering::SeqCst)
}

/// A rectangle in root-window coordinates: x, y, width, height.
type Rect = (i32, i32, u32, u32);

/// The intersection of two rectangles, or `None` when they do not overlap.
fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.0.max(b.0);
    let top = a.1.max(b.1);
    let right = (a.0 + a.2 as i32).min(b.0 + b.2 as i32);
    let bottom = (a.1 + a.3 as i32).min(b.1 + b.3 as i32);
    if right <= left || bottom <= top {
        None
    } else {
        Some((left, top, (right - left) as u32, (bottom - top) as u32))
    }
}

/// "Over" composite of a premultiplied ARGB pixel onto an opaque ARGB pixel.
///
/// Depth-32 X11 windows carry premultiplied alpha on the desktops this
/// backend targets, which is also what Cairo and GTK draw into.
fn blend_over(dst: u32, src: u32) -> u32 {
    let alpha = src >> 24;
    if alpha == 0 {
        return dst | 0xFF00_0000;
    }
    if alpha == 255 {
        return src;
    }
    let inverse = 255 - alpha;
    let channel = |s: u32, d: u32| (s + (d * inverse + 127) / 255).min(255);
    let red = channel((src >> 16) & 0xFF, (dst >> 16) & 0xFF);
    let green = channel((src >> 8) & 0xFF, (dst >> 8) & 0xFF);
    let blue = channel(src & 0xFF, dst & 0xFF);
    0xFF00_0000 | (red << 16) | (green << 8) | blue
}

/// Copy `width`x`height` four-byte pixels from `src` into `dst` at
/// (`dst_x`, `dst_y`). Returns false when either buffer is too small.
#[allow(clippy::too_many_arguments)]
fn blit_rect(
    dst: &mut [u8],
    dst_stride: usize,
    dst_x: usize,
    dst_y: usize,
    src: &[u8],
    src_stride: usize,
    width: usize,
    height: usize,
) -> bool {
    if width == 0 || height == 0 {
        return true;
    }
    let Some(row_bytes) = width.checked_mul(4) else {
        return false;
    };
    let Some(dst_start) = dst_y
        .checked_mul(dst_stride)
        .and_then(|offset| offset.checked_add(dst_x.checked_mul(4)?))
    else {
        return false;
    };
    let Some(dst_end) = (height - 1)
        .checked_mul(dst_stride)
        .and_then(|offset| offset.checked_add(row_bytes))
        .and_then(|offset| offset.checked_add(dst_start))
    else {
        return false;
    };
    let Some(src_end) = (height - 1)
        .checked_mul(src_stride)
        .and_then(|offset| offset.checked_add(row_bytes))
    else {
        return false;
    };
    if dst_end > dst.len() || src_end > src.len() {
        return false;
    }

    for row in 0..height {
        let dst_offset = dst_start + row * dst_stride;
        let src_offset = row * src_stride;
        dst[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&src[src_offset..src_offset + row_bytes]);
    }
    true
}

/// Pixel order of the negotiated ximagesrc format.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameLayout {
    Bgrx,
    Bgra,
    Rgbx,
    Rgba,
}

impl FrameLayout {
    fn from_video(format: gst_video::VideoFormat) -> Option<Self> {
        match format {
            gst_video::VideoFormat::Bgrx => Some(Self::Bgrx),
            gst_video::VideoFormat::Bgra => Some(Self::Bgra),
            gst_video::VideoFormat::Rgbx => Some(Self::Rgbx),
            gst_video::VideoFormat::Rgba => Some(Self::Rgba),
            _ => None,
        }
    }

    /// Frame bytes as an ARGB word (0xAARRGGBB).
    fn decode(self, bytes: [u8; 4]) -> u32 {
        match self {
            Self::Bgrx | Self::Bgra => u32::from_le_bytes(bytes),
            Self::Rgbx | Self::Rgba => {
                let word = u32::from_le_bytes(bytes);
                ((word & 0xFF) << 16)
                    | (word & 0xFF00)
                    | ((word >> 16) & 0xFF)
                    | (word & 0xFF00_0000)
            }
        }
    }

    fn encode(self, word: u32) -> [u8; 4] {
        match self {
            Self::Bgrx | Self::Bgra => word.to_le_bytes(),
            Self::Rgbx | Self::Rgba => [
                ((word >> 16) & 0xFF) as u8,
                ((word >> 8) & 0xFF) as u8,
                (word & 0xFF) as u8,
                ((word >> 24) & 0xFF) as u8,
            ],
        }
    }
}

/// Frame format and size from the pad's negotiated caps.
fn frame_format(pad: &gst::Pad) -> Option<(FrameLayout, usize, usize)> {
    let caps = pad.current_caps()?;
    let info = gst_video::VideoInfo::from_caps(&caps).ok()?;
    Some((
        FrameLayout::from_video(info.format())?,
        info.width() as usize,
        info.height() as usize,
    ))
}

/// Composite one window's server image into the patch buffer.
///
/// `patch` holds frame pixels in `layout` order; `image` is a ZPixmap read of
/// the window in the server's pixel order (ARGB on this backend).
fn composite_image(
    patch: &mut [u8],
    patch_rect: Rect,
    part: Rect,
    image: &[u8],
    opaque: bool,
    layout: FrameLayout,
) -> bool {
    if part.2 == 0 || part.3 == 0 {
        return true;
    }
    if part.0 < patch_rect.0 || part.1 < patch_rect.1 {
        return false;
    }
    let patch_x = (part.0 - patch_rect.0) as usize;
    let patch_y = (part.1 - patch_rect.1) as usize;
    if patch_x + part.2 as usize > patch_rect.2 as usize
        || patch_y + part.3 as usize > patch_rect.3 as usize
    {
        return false;
    }

    let patch_stride = patch_rect.2 as usize * 4;
    let image_stride = part.2 as usize * 4;
    let Some(image_end) = (part.3 as usize).checked_mul(image_stride) else {
        return false;
    };
    if image.len() < image_end || patch.len() < (patch_y + part.3 as usize) * patch_stride {
        return false;
    }

    for row in 0..part.3 as usize {
        let image_row = row * image_stride;
        let patch_row = (patch_y + row) * patch_stride + patch_x * 4;
        for column in 0..part.2 as usize {
            let src_at = image_row + column * 4;
            let dst_at = patch_row + column * 4;
            let source = u32::from_le_bytes([
                image[src_at],
                image[src_at + 1],
                image[src_at + 2],
                image[src_at + 3],
            ]);
            let destination = layout.decode([
                patch[dst_at],
                patch[dst_at + 1],
                patch[dst_at + 2],
                patch[dst_at + 3],
            ]);
            let blended = if opaque {
                source | 0xFF00_0000
            } else {
                blend_over(destination, source)
            };
            patch[dst_at..dst_at + 4].copy_from_slice(&layout.encode(blended));
        }
    }
    true
}

struct BarFrame {
    xid: u32,
    window: Window,
    rect: Rect,
}

/// Rebuilds the pixels under the controls bar on a dedicated X connection.
///
/// Only the bar's own rectangle is touched; when the bar is outside the
/// captured region the probe returns before doing any work on that frame.
pub(crate) struct X11ControlsMasker {
    conn: RustConnection,
    root: Window,
    /// Root coordinates of the capture source's first pixel.
    origin: (i32, i32),
    bar: Option<BarFrame>,
    checked_at: Option<Instant>,
    settle_until: Instant,
    warned: bool,
}

impl X11ControlsMasker {
    pub(crate) fn connect(origin: (i32, i32)) -> Option<Self> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;
        Some(Self {
            conn,
            root,
            origin,
            bar: None,
            checked_at: None,
            settle_until: Instant::now() + RECT_SETTLE,
            warned: false,
        })
    }

    /// Replace the bar's pixels in one captured buffer.
    ///
    /// ximagesrc wraps its XImage memory directly, so the buffer has no
    /// video meta: the negotiated caps give the format and size, and the
    /// image's `bytes_per_line` (XImage padding included) is the buffer size
    /// divided by the height.
    pub(crate) fn mask(&mut self, pad: &gst::Pad, info: &mut gst::PadProbeInfo<'_>) {
        let xid = controls_window();
        if xid == 0 {
            self.bar = None;
            return;
        }

        let (layout, width, height) = match frame_format(pad) {
            Some(frame) => frame,
            None => {
                self.warn_once("the capture caps are not a supported ximagesrc layout");
                return;
            }
        };
        if width == 0 || height == 0 {
            return;
        }

        let Some(buffer) = info.buffer_mut() else {
            return;
        };
        // ximagesrc hands out uniquely owned buffers; make_mut only copies if
        // something downstream still holds a reference.
        let buffer = buffer.make_mut();
        let stride = buffer
            .size()
            .checked_div(height)
            .filter(|stride| *stride >= width * 4)
            .unwrap_or(width * 4);

        let source_rect = (self.origin.0, self.origin.1, width as u32, height as u32);
        let Some((bar_window, bar_rect)) = self.bar_frame(xid) else {
            return;
        };
        let Some(patch) = intersect_rect(bar_rect, source_rect) else {
            return;
        };
        let Some(pixels) = self.rebuild_under(bar_window, patch, layout) else {
            return;
        };

        let Ok(mut mapped) = buffer.map_writable() else {
            return;
        };
        let Some(frame) = mapped.get_mut(..) else {
            return;
        };
        let _ = blit_rect(
            frame,
            stride,
            (patch.0 - self.origin.0) as usize,
            (patch.1 - self.origin.1) as usize,
            &pixels,
            patch.2 as usize * 4,
            patch.2 as usize,
            patch.3 as usize,
        );
    }

    fn warn_once(&mut self, message: &str) {
        if !self.warned {
            self.warned = true;
            eprintln!("[recording] X11 controls mask skipped: {message}");
        }
    }

    /// The bar's top-level window and root rectangle, cached for 100 ms.
    fn bar_frame(&mut self, xid: u32) -> Option<(Window, Rect)> {
        let refresh = match &self.bar {
            Some(bar) if bar.xid == xid => match self.checked_at {
                Some(checked) => {
                    checked.elapsed() >= RECT_REFRESH || Instant::now() < self.settle_until
                }
                None => true,
            },
            _ => true,
        };
        if refresh {
            self.bar = self.resolve_bar_frame(xid);
            self.checked_at = Some(Instant::now());
        }
        self.bar.as_ref().map(|bar| (bar.window, bar.rect))
    }

    fn resolve_bar_frame(&self, xid: u32) -> Option<BarFrame> {
        let mut window = xid;
        for _ in 0..MAX_PARENT_WALK {
            let tree = self.conn.query_tree(window).ok()?.reply().ok()?;
            if tree.parent == 0 {
                return None;
            }
            let geometry = self.conn.get_geometry(window).ok()?.reply().ok()?;
            if tree.parent == self.root {
                let (x, y) = match self
                    .conn
                    .translate_coordinates(window, self.root, 0, 0)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                {
                    Some(reply) => (i32::from(reply.dst_x), i32::from(reply.dst_y)),
                    None => (i32::from(geometry.x), i32::from(geometry.y)),
                };
                return Some(BarFrame {
                    xid,
                    window,
                    rect: (x, y, u32::from(geometry.width), u32::from(geometry.height)),
                });
            }
            window = tree.parent;
        }
        None
    }

    /// Composite the windows below `bar` into the patch rectangle.
    fn rebuild_under(&mut self, bar: Window, patch: Rect, layout: FrameLayout) -> Option<Vec<u8>> {
        let children = self.conn.query_tree(self.root).ok()?.reply().ok()?.children;
        let bar_index = children.iter().position(|child| *child == bar)?;

        let stride = patch.2 as usize * 4;
        let mut pixels = vec![0u8; stride * patch.3 as usize];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 0xFF;
        }

        // QueryTree returns children bottom to top, so the windows below the
        // bar are already in compositing order.
        for child in &children[..bar_index] {
            let attributes = match self
                .conn
                .get_window_attributes(*child)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            {
                Some(attributes) => attributes,
                None => continue,
            };
            if attributes.map_state != MapState::VIEWABLE {
                continue;
            }
            let geometry = match self
                .conn
                .get_geometry(*child)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            {
                Some(geometry) => geometry,
                None => continue,
            };
            let child_rect = (
                i32::from(geometry.x),
                i32::from(geometry.y),
                u32::from(geometry.width),
                u32::from(geometry.height),
            );
            let Some(part) = intersect_rect(patch, child_rect) else {
                continue;
            };
            let image = match self
                .conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    *child,
                    (part.0 - child_rect.0) as i16,
                    (part.1 - child_rect.1) as i16,
                    part.2 as u16,
                    part.3 as u16,
                    !0,
                )
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            {
                Some(image) => image,
                None => continue,
            };
            if image.depth != 24 && image.depth != 32 {
                continue;
            }
            let opaque = image.depth != 32;
            let _ = composite_image(&mut pixels, patch, part, &image.data, opaque, layout);
        }

        Some(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::{blend_over, blit_rect, composite_image, intersect_rect, FrameLayout};

    #[test]
    fn overlapping_rectangles_intersect() {
        assert_eq!(
            intersect_rect((0, 0, 100, 100), (50, 20, 100, 40)),
            Some((50, 20, 50, 40))
        );
    }

    #[test]
    fn nested_rectangles_intersect_to_the_inner_one() {
        assert_eq!(
            intersect_rect((10, 10, 200, 200), (40, 50, 20, 30)),
            Some((40, 50, 20, 30))
        );
    }

    #[test]
    fn separated_rectangles_do_not_intersect() {
        assert_eq!(intersect_rect((0, 0, 100, 100), (100, 0, 50, 50)), None);
        assert_eq!(intersect_rect((0, 0, 100, 100), (0, 100, 50, 50)), None);
    }

    #[test]
    fn fully_transparent_pixels_keep_the_background() {
        assert_eq!(blend_over(0xFF_11_22_33, 0x00_FF_FF_FF), 0xFF_11_22_33);
    }

    #[test]
    fn fully_opaque_pixels_replace_the_background() {
        assert_eq!(blend_over(0xFF_00_00_00, 0xFF_44_55_66), 0xFF_44_55_66);
    }

    #[test]
    fn half_transparent_premultiplied_white_over_black_is_mid_grey() {
        assert_eq!(blend_over(0xFF_00_00_00, 0x80_80_80_80), 0xFF_80_80_80);
    }

    #[test]
    fn blitting_offsets_rows_by_the_destination_stride() {
        let mut destination = vec![0u8; 8 * 4 * 4];
        let source = vec![7u8; 2 * 4 * 2];

        assert!(blit_rect(
            &mut destination,
            8 * 4,
            1,
            2,
            &source,
            2 * 4,
            2,
            2
        ));

        assert_eq!(&destination[(2 * 32 + 4)..(2 * 32 + 12)], &[7u8; 8]);
        assert_eq!(&destination[(3 * 32 + 4)..(3 * 32 + 12)], &[7u8; 8]);
        assert_eq!(destination[2 * 32], 0);
        assert_eq!(destination[3 * 32 + 12], 0);
    }

    #[test]
    fn blitting_outside_the_destination_is_refused() {
        let mut destination = vec![0u8; 4 * 4];
        let source = vec![1u8; 4 * 2];
        assert!(!blit_rect(&mut destination, 16, 3, 0, &source, 8, 2, 1));
        assert!(!blit_rect(&mut destination, 16, 0, 3, &source, 8, 1, 2));
    }

    #[test]
    fn opaque_window_copy_replaces_the_patch() {
        let mut patch = vec![0u8; 2 * 2 * 4];
        let image = [
            0x10, 0x20, 0x30, 0x00, 0x40, 0x50, 0x60, 0x00, 0x70, 0x80, 0x90, 0x00, 0xA0, 0xB0,
            0xC0, 0x00,
        ];

        assert!(composite_image(
            &mut patch,
            (0, 0, 2, 2),
            (0, 0, 2, 2),
            &image,
            true,
            FrameLayout::Bgrx
        ));

        assert_eq!(patch[..4], [0x10, 0x20, 0x30, 0xFF]);
        assert_eq!(patch[12..16], [0xA0, 0xB0, 0xC0, 0xFF]);
    }

    #[test]
    fn translucent_premultiplied_window_copy_blends_over_the_patch() {
        let mut patch = vec![0u8; 4];
        let image = [0x80, 0x80, 0x80, 0x80];

        assert!(composite_image(
            &mut patch,
            (0, 0, 1, 1),
            (0, 0, 1, 1),
            &image,
            false,
            FrameLayout::Bgrx
        ));

        assert_eq!(patch, [0x80, 0x80, 0x80, 0xFF]);
    }

    #[test]
    fn rgb_frame_layout_swaps_the_channel_order() {
        let mut patch = vec![0u8; 4];
        let image = [0x10, 0x20, 0x30, 0xFF];

        assert!(composite_image(
            &mut patch,
            (0, 0, 1, 1),
            (0, 0, 1, 1),
            &image,
            true,
            FrameLayout::Rgbx
        ));

        assert_eq!(patch, [0x30, 0x20, 0x10, 0xFF]);
    }
}
