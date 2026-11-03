//! Canvas cursors for the image editor.
//!
//! The pen and the highlighter carry a preview of their own brush instead of
//! the crosshair every other tool uses. The highlighter's preview follows the
//! detected text height in Detect text mode, so it grows over a recognised
//! line and falls back to the stroke size elsewhere; while detection runs it
//! shows a dashed outline instead of a solid brush.

use std::cell::RefCell;

use gtk4::gdk;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::prelude::*;

use crate::capture::editor::pen_weight::HighlighterMode;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::text_detect::{clamp_cursor_size, DetectionStatus};
use crate::capture::editor::types::{Point, Tool, ViewTransform};

/// Default highlighter cursor size (when no text detected).
pub(super) const DEFAULT_HIGHLIGHTER_CURSOR_SIZE: f64 = 16.0;

/// Cursor width ratio relative to height.
const CURSOR_WIDTH_RATIO: f64 = 1.0;

/// Corner radius for the highlighter cursor.
const CURSOR_CORNER_RADIUS: f64 = 4.0;

/// Padding around the drawn brush so its outline is never clipped.
const CURSOR_PAD: f64 = 6.0;

/// A cursor the canvas hands to its drawing area: either a theme cursor by
/// name or a brush preview built here.
pub(super) enum CanvasCursor {
    Name(&'static str),
    Custom(gdk::Cursor),
}

/// The brush size the highlighter previews, in view pixels.
pub(super) fn highlighter_cursor_height_for_view(image_height: f64, view_scale: f64) -> f64 {
    clamp_cursor_size(image_height * view_scale.max(0.0001))
}

/// The pen's or highlighter's cursor at `view_point`; `None` for the tools
/// that keep their name cursor.
pub(super) fn brush_cursor(
    state: &EditorState,
    transform: ViewTransform,
    view_point: Point,
) -> Option<CanvasCursor> {
    if !matches!(state.selected_tool, Tool::Pen | Tool::Highlighter) {
        return None;
    }
    if !transform.contains_view(view_point) {
        return Some(CanvasCursor::Name("pointer"));
    }
    let cursor = match state.selected_tool {
        Tool::Pen => pen_cursor(state.stroke_size),
        _ => highlighter_cursor(
            state,
            transform.view_to_image_clamped(view_point),
            transform.scale,
        ),
    };
    Some(cursor.map_or(CanvasCursor::Name("crosshair"), CanvasCursor::Custom))
}

/// What the highlighter preview should show: a brush of a given height, or
/// the dashed outline while detection is still running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum HighlighterPreview {
    Pending,
    Height(f64),
}

/// Resolve the highlighter's preview at `image_point`. A locked size wins;
/// text-aware mode adopts the detected line height under the pointer, then
/// falls back to the default brush; freehand follows the stroke size.
pub(super) fn highlighter_preview(
    state: &EditorState,
    image_point: Point,
    view_scale: f64,
) -> HighlighterPreview {
    if let Some(locked) = state.locked_highlighter_stroke_size {
        return HighlighterPreview::Height(highlighter_cursor_height_for_view(locked, view_scale));
    }
    if state.highlighter_mode == HighlighterMode::TextAware {
        if let Ok(detector) = state.text_detector.lock() {
            match detector.status() {
                DetectionStatus::Pending => return HighlighterPreview::Pending,
                DetectionStatus::Ready => {
                    if let Some(height) = detector.best_text_height_at_point(image_point) {
                        return HighlighterPreview::Height(highlighter_cursor_height_for_view(
                            height, view_scale,
                        ));
                    }
                }
                DetectionStatus::Failed(_) => {}
            }
        }
        return HighlighterPreview::Height(highlighter_cursor_height_for_view(
            DEFAULT_HIGHLIGHTER_CURSOR_SIZE,
            view_scale,
        ));
    }
    HighlighterPreview::Height(highlighter_cursor_height_for_view(
        state.stroke_size,
        view_scale,
    ))
}

fn highlighter_cursor(
    state: &EditorState,
    image_point: Point,
    view_scale: f64,
) -> Option<gdk::Cursor> {
    match highlighter_preview(state, image_point, view_scale) {
        HighlighterPreview::Pending => detecting_cursor(),
        HighlighterPreview::Height(height) => highlighter_brush_cursor(height),
    }
}

const HIGHLIGHTER_BRUSH: u8 = 0;
const DETECTING_BRUSH: u8 = 1;
const PEN_BRUSH: u8 = 2;

thread_local! {
    /// Pointer motion and the change listener both ask for a brush on every
    /// event, so the last one built is kept and handed back while its size
    /// holds.
    static LAST_BRUSH: RefCell<Option<((u8, i32), gdk::Cursor)>> = const { RefCell::new(None) };
}

fn cached_brush(
    key: (u8, i32),
    build: impl FnOnce() -> Option<gdk::Cursor>,
) -> Option<gdk::Cursor> {
    LAST_BRUSH.with(|last| {
        if let Some((last_key, cursor)) = last.borrow().as_ref() {
            if *last_key == key {
                return Some(cursor.clone());
            }
        }
        let cursor = build()?;
        *last.borrow_mut() = Some((key, cursor.clone()));
        Some(cursor)
    })
}

fn highlighter_brush_cursor(height: f64) -> Option<gdk::Cursor> {
    cached_brush((HIGHLIGHTER_BRUSH, height.round() as i32), || {
        let texture = surface_to_texture(create_highlighter_cursor_surface(height)?)?;
        let hotspot_x = (texture.width() as f64 * 0.35).ceil() as i32;
        let hotspot_y = texture.height() / 2;
        Some(gdk::Cursor::from_texture(
            &texture, hotspot_x, hotspot_y, None,
        ))
    })
}

fn detecting_cursor() -> Option<gdk::Cursor> {
    cached_brush((DETECTING_BRUSH, 0), || {
        let texture = surface_to_texture(create_highlighter_detecting_cursor()?)?;
        let hotspot_x =
            ((DEFAULT_HIGHLIGHTER_CURSOR_SIZE * CURSOR_WIDTH_RATIO) / 2.0 + CURSOR_PAD) as i32;
        let hotspot_y = (DEFAULT_HIGHLIGHTER_CURSOR_SIZE / 2.0 + CURSOR_PAD) as i32;
        Some(gdk::Cursor::from_texture(
            &texture, hotspot_x, hotspot_y, None,
        ))
    })
}

fn pen_cursor(size: f64) -> Option<gdk::Cursor> {
    let size = clamp_cursor_size(size);
    cached_brush((PEN_BRUSH, size.round() as i32), || {
        let texture = surface_to_texture(create_pen_cursor_surface(size)?)?;
        let hotspot = (CURSOR_PAD + size / 2.0) as i32;
        Some(gdk::Cursor::from_texture(&texture, hotspot, hotspot, None))
    })
}

fn rounded_rect(context: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    context.new_sub_path();
    context.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    context.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    context.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
}

/// Create the rounded rectangle that previews a highlighter stroke.
fn create_highlighter_cursor_surface(height: f64) -> Option<gtk4::cairo::ImageSurface> {
    let height = clamp_cursor_size(height);
    let width = height * CURSOR_WIDTH_RATIO;
    let surface_width = (width + CURSOR_PAD * 2.0).ceil() as i32;
    let surface_height = (height + CURSOR_PAD * 2.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;
    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);
    rounded_rect(&context, CURSOR_PAD, CURSOR_PAD, width, height, radius);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(3.5);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();

    rounded_rect(&context, CURSOR_PAD, CURSOR_PAD, width, height, radius);
    context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    context.set_line_width(1.5);
    let _ = context.stroke();

    Some(surface)
}

/// The dashed outline shown while text detection is still running.
fn create_highlighter_detecting_cursor() -> Option<gtk4::cairo::ImageSurface> {
    let height = DEFAULT_HIGHLIGHTER_CURSOR_SIZE;
    let width = height * CURSOR_WIDTH_RATIO;
    let surface_width = (width + CURSOR_PAD * 2.0).ceil() as i32;
    let surface_height = (height + CURSOR_PAD * 2.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;
    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);
    rounded_rect(&context, CURSOR_PAD, CURSOR_PAD, width, height, radius);
    context.set_source_rgba(0.5, 0.5, 0.5, 0.8);
    context.set_line_width(2.0);
    context.set_dash(&[4.0, 4.0], 0.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    let _ = context.stroke();

    Some(surface)
}

/// Create the circle that previews a pen stroke.
fn create_pen_cursor_surface(size: f64) -> Option<gtk4::cairo::ImageSurface> {
    let size = clamp_cursor_size(size);
    let surface_size = (size + CURSOR_PAD * 2.0).ceil() as i32;

    let surface =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, surface_size, surface_size)
            .ok()?;
    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let center = CURSOR_PAD + size / 2.0;
    let radius = size / 2.0;

    context.arc(center, center, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(3.5);
    context.stroke().ok()?;

    context.arc(center, center, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    context.set_line_width(1.5);
    context.stroke().ok()?;

    Some(surface)
}

fn surface_to_texture(mut surface: gtk4::cairo::ImageSurface) -> Option<gdk::Texture> {
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride();
    let data = surface.data().ok()?.to_vec();

    let pixbuf = Pixbuf::from_bytes(
        &gtk4::glib::Bytes::from(&data),
        Colorspace::Rgb,
        true,
        8,
        width,
        height,
        stride,
    );

    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::text_detect::{TextDetector, TextRegion};
    use crate::capture::editor::types::Rect;
    use image::RgbaImage;
    use std::sync::{Arc, Mutex};

    fn state_with_regions(regions: Vec<TextRegion>) -> EditorState {
        let mut state = EditorState::new(RgbaImage::new(400, 200));
        state.text_detector = Arc::new(Mutex::new(TextDetector::with_regions(regions)));
        state
    }

    fn line(x: i32, y: i32, width: i32, height: i32, text_height: f64) -> TextRegion {
        TextRegion {
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            text_height,
            baseline_y: f64::from(y + height),
            words: Vec::new(),
        }
    }

    #[test]
    fn highlighter_cursor_height_tracks_view_scale() {
        assert_eq!(highlighter_cursor_height_for_view(32.0, 1.0), 32.0);
        assert_eq!(highlighter_cursor_height_for_view(32.0, 0.5), 16.0);
        assert_eq!(highlighter_cursor_height_for_view(32.0, 2.0), 64.0);
    }

    #[test]
    fn highlighter_cursor_height_remains_clamped() {
        assert_eq!(
            highlighter_cursor_height_for_view(4.0, 0.5),
            crate::capture::editor::text_detect::MIN_CURSOR_SIZE
        );
        assert_eq!(
            highlighter_cursor_height_for_view(80.0, 2.0),
            crate::capture::editor::text_detect::MAX_CURSOR_SIZE
        );
    }

    #[test]
    fn text_aware_preview_follows_the_detected_line_under_the_pointer() {
        let mut state = state_with_regions(vec![line(10, 10, 100, 30, 24.0)]);
        state.highlighter_mode = HighlighterMode::TextAware;
        assert_eq!(
            highlighter_preview(&state, Point { x: 40.0, y: 20.0 }, 1.0),
            HighlighterPreview::Height(32.0)
        );
        assert_eq!(
            highlighter_preview(&state, Point { x: 40.0, y: 20.0 }, 2.0),
            HighlighterPreview::Height(64.0)
        );
        // Away from detected text the brush stays at the default size.
        assert_eq!(
            highlighter_preview(&state, Point { x: 320.0, y: 170.0 }, 1.0),
            HighlighterPreview::Height(DEFAULT_HIGHLIGHTER_CURSOR_SIZE)
        );
    }

    #[test]
    fn freehand_preview_uses_the_stroke_size_and_pending_detection_is_dashed() {
        let mut state = state_with_regions(vec![line(10, 10, 100, 30, 24.0)]);
        state.highlighter_mode = HighlighterMode::Freehand;
        state.stroke_size = 28.0;
        assert_eq!(
            highlighter_preview(&state, Point { x: 40.0, y: 20.0 }, 1.0),
            HighlighterPreview::Height(28.0)
        );

        let mut pending = EditorState::new(RgbaImage::new(400, 200));
        pending.highlighter_mode = HighlighterMode::TextAware;
        assert_eq!(
            highlighter_preview(&pending, Point { x: 40.0, y: 20.0 }, 1.0),
            HighlighterPreview::Pending
        );
    }

    #[test]
    fn only_the_brush_tools_take_a_custom_cursor() {
        let mut state = EditorState::new(RgbaImage::new(400, 200));
        let transform = ViewTransform::for_image(400.0, 200.0);
        let point = Point { x: 10.0, y: 10.0 };
        for tool in [Tool::Select, Tool::Crop, Tool::Text, Tool::Arrow] {
            state.selected_tool = tool;
            assert!(brush_cursor(&state, transform, point).is_none());
        }
        for tool in [Tool::Pen, Tool::Highlighter] {
            state.selected_tool = tool;
            assert!(brush_cursor(&state, transform, point).is_some());
        }
    }
}
