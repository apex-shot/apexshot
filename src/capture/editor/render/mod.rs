use super::color::{highlighter_stroke_width, HIGHLIGHTER_ALPHA_SCALE};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::types::{
    AnnotationAction, DrawColor, FrameSpec, FrameStyle, Point, Rect, SelectHandle, ViewTransform,
};
use image::{ImageBuffer, RgbaImage};
use rayon::prelude::*;

mod arrows;
mod background_blur;
mod effects;
mod liquid_glass;
mod noise;
mod text;

pub use arrows::{
    double_arrow_outline_points, draw_arrow, draw_arrow_control_handles,
    draw_arrow_selection_outline, thorn_arrow_outline_points,
};
pub use background_blur::{
    apply_background_blur, blur_background_surface, BACKGROUND_BLUR_MAX_RADIUS,
};
pub use effects::{
    apply_blackout_rect, apply_blur_rect, apply_censor_rect, apply_focus_rect, apply_hybrid_blur,
};
#[cfg(test)]
pub use liquid_glass::clear_lens;
#[allow(unused_imports)]
pub use liquid_glass::liquid_preview_backdrop;
pub use liquid_glass::{glass_layer, liquid_preview_layer, GlassLook, GlassRing, LiquidPreview};
pub use noise::{apply_background_noise, paint_background_noise};
#[allow(unused_imports)]
pub use text::{
    cursor_position_for_text_point, draw_active_text_input, draw_text, draw_text_edit_border,
    draw_text_edit_handles, draw_wrapped_text, layout_wrapped_text, measure_text_width,
    text_action_bounds, TextLayout, TextLayoutLine,
};

pub fn draw_rgba_to_context(context: &gtk4::cairo::Context, image: &RgbaImage) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }

    let stride = match gtk4::cairo::Format::ARgb32.stride_for_width(width) {
        Ok(v) => v,
        Err(_) => return,
    };

    let data = rgba_to_cairo_argb_bytes(image);
    let surface = match gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride,
    ) {
        Ok(s) => s,
        Err(_) => return,
    };

    paint_surface_with_filter(context, &surface, 0.0, 0.0, gtk4::cairo::Filter::Nearest);
}

/// Clip vector annotations and drafts to the visible screenshot/background canvas.
pub fn clip_annotation_content_to_canvas(context: &gtk4::cairo::Context, transform: ViewTransform) {
    let (min_x, min_y, max_x, max_y) = transform.canvas_bounds_in_image_coords();
    context.rectangle(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    );
    context.clip();
}
pub fn rgba_image_to_surface(image: &RgbaImage) -> Option<gtk4::cairo::ImageSurface> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let stride = gtk4::cairo::Format::ARgb32.stride_for_width(width).ok()?;
    let data = rgba_to_cairo_argb_bytes(image);

    gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride,
    )
    .ok()
}
pub fn paint_surface_with_filter(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    x: f64,
    y: f64,
    filter: gtk4::cairo::Filter,
) {
    if context.set_source_surface(surface, x, y).is_ok() {
        let source = context.source();
        source.set_filter(filter);
        let _ = context.paint();
    }
}
pub fn editor_image_filter_for_scale(_scale: f64) -> gtk4::cairo::Filter {
    gtk4::cairo::Filter::Good
}

/// Filter for interactive frames (drags, drafts).
///
/// A pointer-driven repaint cannot afford the quality resample — `Good` costs tens
/// of milliseconds per frame on a screenshot-sized surface, which is what made
/// dragging feel laggy. Interactive frames trade a little resample quality for a
/// cheap frame; the resting repaint still uses [`editor_image_filter_for_scale`],
/// so nothing stays soft once the pointer is released.
pub fn editor_interactive_image_filter() -> gtk4::cairo::Filter {
    gtk4::cairo::Filter::Bilinear
}
pub fn draw_annotation_action(context: &gtk4::cairo::Context, action: &AnnotationAction) {
    match action {
        AnnotationAction::Pen {
            points,
            color,
            stroke_size,
        } => draw_pen(context, points, *color, *stroke_size),
        AnnotationAction::Highlighter {
            points,
            color,
            stroke_size,
        } => draw_highlighter(context, points, *color, *stroke_size),
        AnnotationAction::Circle {
            rect,
            color,
            stroke_size,
            shadow,
        } => draw_circle_with_shadow(context, *rect, *color, *stroke_size, *shadow),
        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
            shadow,
        } => draw_line_with_shadow(context, *start, *end, *color, *stroke_size, *shadow),
        AnnotationAction::Arrow {
            start,
            end,
            color,
            stroke_size,
            style,
            control_points,
            shadow,
        } => draw_arrow(
            context,
            *start,
            *end,
            *color,
            *stroke_size,
            *style,
            control_points.clone(),
            *shadow,
        ),
        AnnotationAction::Box {
            rect,
            color,
            stroke_size,
            shadow,
        } => draw_box_with_shadow(context, *rect, *color, *stroke_size, *shadow),
        AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            shadow,
            background_color,
            ..
        } => {
            let available_width = max_width
                .unwrap_or_else(|| {
                    context
                        .clip_extents()
                        .map(|(_, _, width, _)| width - position.x)
                        .unwrap_or(f64::INFINITY)
                })
                .min(
                    context
                        .clip_extents()
                        .map(|(_, _, width, _)| width - position.x)
                        .unwrap_or(f64::INFINITY),
                )
                .max(font.size * 1.8);
            text::draw_text_with_shadow(
                context,
                *position,
                text,
                *color,
                font,
                Some(available_width),
                *shadow,
                *background_color,
            );
        }
        AnnotationAction::Number {
            position,
            number,
            color,
            style,
            size,
            shadow,
        } => draw_number_with_shadow(context, *position, *number, *color, *style, *size, *shadow),
        AnnotationAction::Obfuscate { .. } => {}
        AnnotationAction::Focus { .. } => {}
    }
}
pub fn draw_draft_action(context: &gtk4::cairo::Context, action: &AnnotationAction) {
    match action {
        AnnotationAction::Pen {
            points,
            color,
            stroke_size,
        } => {
            // Full opacity while drafting so release matches the live preview.
            draw_pen(context, points, *color, *stroke_size);
        }
        AnnotationAction::Highlighter {
            points,
            color,
            stroke_size,
        } => {
            draw_highlighter(context, points, *color, *stroke_size);
        }
        AnnotationAction::Circle {
            rect,
            color,
            stroke_size,
            shadow,
        } => {
            draw_circle_with_shadow(context, *rect, *color, *stroke_size, *shadow);
        }
        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
            shadow,
        } => {
            // Full opacity while drafting so release matches the live preview.
            draw_line_with_shadow(context, *start, *end, *color, *stroke_size, *shadow);
        }
        AnnotationAction::Arrow {
            start,
            end,
            color,
            stroke_size,
            style,
            control_points,
            shadow,
        } => {
            draw_arrow(
                context,
                *start,
                *end,
                *color,
                *stroke_size,
                *style,
                control_points.clone(),
                *shadow,
            );
        }
        AnnotationAction::Box {
            rect,
            color,
            stroke_size,
            shadow,
        } => {
            draw_box_with_shadow(context, *rect, *color, *stroke_size, *shadow);
        }
        AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            shadow,
            background_color,
            ..
        } => {
            let available_width = max_width
                .unwrap_or_else(|| {
                    context
                        .clip_extents()
                        .map(|(_, _, width, _)| width - position.x)
                        .unwrap_or(f64::INFINITY)
                })
                .min(
                    context
                        .clip_extents()
                        .map(|(_, _, width, _)| width - position.x)
                        .unwrap_or(f64::INFINITY),
                )
                .max(font.size * 1.8);
            text::draw_text_with_shadow(
                context,
                *position,
                text,
                color.with_alpha(0.9),
                font,
                Some(available_width),
                *shadow,
                *background_color,
            );
        }
        AnnotationAction::Number {
            position,
            number,
            color,
            style,
            size,
            shadow,
        } => {
            draw_number_with_shadow(
                context,
                *position,
                *number,
                color.with_alpha(0.88),
                *style,
                *size,
                *shadow,
            );
        }
        AnnotationAction::Obfuscate { rect, .. } | AnnotationAction::Focus { rect, .. } => {
            draw_effect_draft_rect(context, *rect);
        }
    }
}

fn draw_effect_draft_rect(context: &gtk4::cairo::Context, rect: Rect) {
    context.set_source_rgba(0.18, 0.48, 0.94, 0.18);
    context.rectangle(
        rect.x as f64,
        rect.y as f64,
        rect.width as f64,
        rect.height as f64,
    );
    let _ = context.fill_preserve();
    context.set_source_rgba(0.20, 0.56, 0.98, 0.95);
    context.set_line_width(2.0);
    let _ = context.stroke();
}
#[allow(dead_code)]
/// Paint the crop mask, thirds guides, and resize handles in image space.
pub fn draw_crop_overlay(
    context: &gtk4::cairo::Context,
    image_width: f64,
    image_height: f64,
    rect: Rect,
    active: bool,
    view_scale: f64,
) {
    let x = rect.x as f64;
    let y = rect.y as f64;
    let width = rect.width as f64;
    let height = rect.height as f64;

    if width <= 1.0 || height <= 1.0 {
        return;
    }

    let _ = context.save();
    context.rectangle(0.0, 0.0, image_width, image_height);
    context.clip();
    context.set_source_rgba(0.0, 0.0, 0.0, 0.48);
    for (mx, my, mw, mh) in [
        (0.0, 0.0, image_width, y),
        (0.0, y + height, image_width, image_height - y - height),
        (0.0, y, x, height),
        (x + width, y, image_width - x - width, height),
    ] {
        if mw > 0.0 && mh > 0.0 {
            context.rectangle(mx, my, mw, mh);
        }
    }
    let _ = context.fill();

    context.rectangle(x, y, width, height);
    context.set_line_width(1.0 / view_scale.max(0.1));
    context.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    let _ = context.stroke();

    let mid_x = x + width / 2.0;
    let mid_y = y + height / 2.0;
    context.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    context.set_line_width(1.0 / view_scale.max(0.1));
    for idx in 1..=2 {
        let dx = width * (idx as f64) / 3.0;
        let dy = height * (idx as f64) / 3.0;

        context.move_to(x + dx, y);
        context.line_to(x + dx, y + height);
        context.move_to(x, y + dy);
        context.line_to(x + width, y + dy);
    }
    let _ = context.stroke();

    let handle = (9.0 / view_scale.max(0.1)).clamp(6.0, 14.0);
    for (hx, hy) in [
        (x, y),
        (mid_x, y),
        (x + width, y),
        (x, mid_y),
        (x + width, mid_y),
        (x, y + height),
        (mid_x, y + height),
        (x + width, y + height),
    ] {
        context.rectangle(hx - handle / 2.0, hy - handle / 2.0, handle, handle);
        context.set_source_rgba(0.08, 0.08, 0.09, 0.9);
        let _ = context.fill_preserve();
        context.set_source_rgba(1.0, 1.0, 1.0, if active { 1.0 } else { 0.85 });
        context.set_line_width(1.2 / view_scale.max(0.1));
        let _ = context.stroke();
    }
    let _ = context.restore();
}
pub(super) fn selection_outline_stroke_width(view_scale: f64) -> f64 {
    TEXT_EDIT_BORDER_WIDTH / view_scale.max(0.01)
}
pub fn draw_selection_outline(context: &gtk4::cairo::Context, rect: Rect, view_scale: f64) {
    let scale = view_scale.max(0.01);
    let width = rect.width.max(1) as f64;
    let height = rect.height.max(1) as f64;
    let x = rect.x as f64;
    let y = rect.y as f64;

    let _ = context.save();

    // Solid blue rounded-rect border — same style as the text edit border.
    let radius = (4.0 / scale).min(width / 2.0).min(height / 2.0);
    context.set_source_rgba(
        TEXT_EDIT_BORDER_COLOR.0,
        TEXT_EDIT_BORDER_COLOR.1,
        TEXT_EDIT_BORDER_COLOR.2,
        1.0,
    );
    context.set_line_width(selection_outline_stroke_width(scale));
    context.set_dash(&[], 0.0);

    context.new_path();
    context.move_to(x + radius, y);
    context.line_to(x + width - radius, y);
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.line_to(x + width, y + height - radius);
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.line_to(x + radius, y + height);
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.line_to(x, y + radius);
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
    let _ = context.stroke();

    let _ = context.restore();
}
pub fn draw_selection_handles(
    context: &gtk4::cairo::Context,
    handles: &[(SelectHandle, Point)],
    active_handle: Option<SelectHandle>,
    view_scale: f64,
) {
    if handles.is_empty() {
        return;
    }

    let scale = view_scale.max(0.01);

    let _ = context.save();
    for (handle, center) in handles {
        let is_active = active_handle.is_some_and(|active| active == *handle);
        let radius = (MOVE_HANDLE_RADIUS + if is_active { 1.0 } else { 0.0 }) / scale;

        // White outline ring. `arc()` line-to's the current point, so drop any
        // leftover path (e.g. a number label's `show_text`) first.
        context.new_path();
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
        context.arc(center.x, center.y, radius, 0.0, std::f64::consts::TAU);
        let _ = context.stroke();

        // Blue filled circle
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        context.arc(
            center.x,
            center.y,
            (radius - MOVE_HANDLE_OUTLINE_WIDTH / scale).max(1.0 / scale),
            0.0,
            std::f64::consts::TAU,
        );
        let _ = context.fill();
    }
    let _ = context.restore();
}
pub(super) const TEXT_EDIT_BORDER_COLOR: (f64, f64, f64) = (0.231, 0.510, 0.965); // #3b82f6
pub(super) const TEXT_EDIT_BORDER_WIDTH: f64 = 2.0;
pub(super) const TEXT_EDIT_BORDER_RADIUS: f64 = 4.0;
pub(super) const MOVE_HANDLE_RADIUS: f64 = 7.0;
pub(super) const MOVE_HANDLE_OUTLINE_WIDTH: f64 = 2.0;
pub(super) const RESIZE_HANDLE_SIZE: f64 = 10.0;
/// Build a smooth freehand path from sampled pointer points.
///
/// Uses midpoint quadratic segments (expressed as cubics for Cairo) so the
/// stroke follows the hand without the faceted look of raw polylines, while
/// still hitting the first and last sample exactly.
fn append_smoothed_stroke_path(context: &gtk4::cairo::Context, points: &[Point]) {
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        context.move_to(points[0].x, points[0].y);
        return;
    }
    if points.len() == 2 {
        context.move_to(points[0].x, points[0].y);
        context.line_to(points[1].x, points[1].y);
        return;
    }

    context.move_to(points[0].x, points[0].y);

    // Midpoint smoothing: each sample is a quadratic control point between
    // consecutive segment midpoints. Convert Q(P, C, M) → cubic for Cairo.
    let mut i = 1usize;
    while i < points.len() - 1 {
        let control = points[i];
        let end = Point {
            x: (points[i].x + points[i + 1].x) * 0.5,
            y: (points[i].y + points[i + 1].y) * 0.5,
        };
        // Current point is the cubic start (implicit).
        // C1 = P0 + 2/3 (C - P0), C2 = End + 2/3 (C - End)
        // Cairo curve_to uses absolute coordinates for both controls and end.
        // We approximate the quadratic with a degenerate cubic (both controls
        // at the sample) which is stable and looks natural for ink strokes.
        context.curve_to(control.x, control.y, control.x, control.y, end.x, end.y);
        i += 1;
    }

    let last = points[points.len() - 1];
    let prev = points[points.len() - 2];
    context.curve_to(prev.x, prev.y, prev.x, prev.y, last.x, last.y);
}
pub fn draw_pen(
    context: &gtk4::cairo::Context,
    points: &[Point],
    color: DrawColor,
    stroke_size: f64,
) {
    if points.len() < 2 {
        return;
    }

    let stroke = stroke_size.max(0.5);
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    append_smoothed_stroke_path(context, points);
    let _ = context.stroke();
    let _ = context.restore();
}
pub fn draw_highlighter(
    context: &gtk4::cairo::Context,
    points: &[Point],
    color: DrawColor,
    stroke_size: f64,
) {
    if points.len() < 2 {
        return;
    }

    let stroke = highlighter_stroke_width(stroke_size);
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    context.set_operator(gtk4::cairo::Operator::Multiply);
    context.set_source_rgba(
        color.r,
        color.g,
        color.b,
        (color.a * HIGHLIGHTER_ALPHA_SCALE).clamp(0.05, 0.95),
    );
    context.set_line_width(stroke);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    append_smoothed_stroke_path(context, points);
    let _ = context.stroke();
    let _ = context.restore();
}
fn shadow_color_for(color: DrawColor) -> DrawColor {
    DrawColor::new(0.0, 0.0, 0.0, (color.a * 0.35).clamp(0.12, 0.35))
}
pub(super) fn draw_shadow_layer(
    context: &gtk4::cairo::Context,
    shadow: bool,
    color: DrawColor,
    draw: impl Fn(&gtk4::cairo::Context, DrawColor),
) {
    if shadow {
        let _ = context.save();
        context.translate(3.0, 3.0);
        draw(context, shadow_color_for(color));
        let _ = context.restore();
    }

    draw(context, color);
}
pub fn draw_circle(context: &gtk4::cairo::Context, rect: Rect, color: DrawColor, stroke_size: f64) {
    let width = rect.width as f64;
    let height = rect.height as f64;
    if width <= 1.0 || height <= 1.0 {
        return;
    }

    let center_x = rect.x as f64 + width / 2.0;
    let center_y = rect.y as f64 + height / 2.0;
    let radius_x = width / 2.0;
    let radius_y = height / 2.0;
    let min_radius = radius_x.min(radius_y);

    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    context.set_source_rgba(color.r, color.g, color.b, color.a);

    // When one dimension is much smaller than the stroke size, the
    // scale-based ellipse rendering breaks — the stroke becomes
    // distorted into a line.  Use a rounded-rect path instead, which
    // degrades gracefully to a capsule/stadium shape for thin ellipses.
    if min_radius < stroke_size * 0.75 {
        let r = min_radius.max(0.5);
        let left = rect.x as f64 + r;
        let right = rect.x as f64 + width - r;
        let top = rect.y as f64 + r;
        let bottom = rect.y as f64 + height - r;
        context.set_line_width(stroke_size.max(0.5));
        context.new_sub_path();
        context.arc(
            left,
            top,
            r,
            std::f64::consts::PI,
            1.5 * std::f64::consts::PI,
        );
        context.arc(
            right,
            top,
            r,
            1.5 * std::f64::consts::PI,
            2.0 * std::f64::consts::PI,
        );
        context.arc(right, bottom, r, 0.0, 0.5 * std::f64::consts::PI);
        context.arc(
            left,
            bottom,
            r,
            0.5 * std::f64::consts::PI,
            std::f64::consts::PI,
        );
        context.close_path();
        let _ = context.stroke();
    } else {
        // Normal ellipse rendering for well-proportioned circles/ellipses
        context.translate(center_x, center_y);
        context.scale(radius_x, radius_y);
        context.set_line_width(stroke_size.max(0.5) / min_radius);
        context.new_sub_path();
        context.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
        let _ = context.stroke();
    }

    let _ = context.restore();
}
fn draw_circle_with_shadow(
    context: &gtk4::cairo::Context,
    rect: Rect,
    color: DrawColor,
    stroke_size: f64,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_circle(ctx, rect, draw_color, stroke_size);
    });
}
pub fn draw_line(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
) {
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5));
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.move_to(start.x, start.y);
    context.line_to(end.x, end.y);
    let _ = context.stroke();
    let _ = context.restore();
}
fn draw_line_with_shadow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_line(ctx, start, end, draw_color, stroke_size);
    });
}
pub fn draw_box(context: &gtk4::cairo::Context, rect: Rect, color: DrawColor, stroke_size: f64) {
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5));
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.rectangle(
        rect.x as f64,
        rect.y as f64,
        rect.width as f64,
        rect.height as f64,
    );
    let _ = context.stroke();
    let _ = context.restore();
}
fn draw_box_with_shadow(
    context: &gtk4::cairo::Context,
    rect: Rect,
    color: DrawColor,
    stroke_size: f64,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_box(ctx, rect, draw_color, stroke_size);
    });
}
pub fn draw_number(
    context: &gtk4::cairo::Context,
    position: Point,
    number: u32,
    color: DrawColor,
    style: NumberingStyle,
    size: NumberSize,
) {
    // Clear any existing path to prevent connecting lines between numbers
    context.new_path();

    let radius = size.radius();
    let font_size = size.font_size();

    // Draw filled circle
    context.arc(position.x, position.y, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();

    // Draw border as a new path
    context.new_path();
    context.arc(position.x, position.y, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(0.02, 0.03, 0.05, 0.42);
    context.set_line_width(1.5);
    let _ = context.stroke();

    // Calculate text color based on background luminance
    let luminance = (0.299 * color.r) + (0.587 * color.g) + (0.114 * color.b);
    let (text_r, text_g, text_b) = if luminance > 0.65 {
        (0.07, 0.08, 0.10)
    } else {
        (0.98, 0.99, 1.0)
    };

    // Format number according to style
    let label = style.format(number);

    // Draw text
    context.new_path();
    context.set_source_rgba(text_r, text_g, text_b, 0.98);
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(font_size);

    if let Ok(extents) = context.text_extents(&label) {
        let text_x = position.x - (extents.width() / 2.0 + extents.x_bearing());
        let text_y = position.y - (extents.height() / 2.0 + extents.y_bearing());
        context.move_to(text_x, text_y);
    } else {
        context.move_to(position.x - 4.0, position.y + 4.0);
    }

    let _ = context.show_text(&label);
    context.new_path();
}
fn draw_number_with_shadow(
    context: &gtk4::cairo::Context,
    position: Point,
    number: u32,
    color: DrawColor,
    style: NumberingStyle,
    size: NumberSize,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_number(ctx, position, number, draw_color, style, size);
    });
}
pub fn draw_canvas_checkerboard_background(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    tint: Option<DrawColor>,
    light: bool,
) {
    fn blend_channel(base: f64, overlay: f64, alpha: f64) -> f64 {
        base * (1.0 - alpha) + overlay * alpha
    }

    let tile_size = 14.0;
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;

    let (base_dark, tile_dark) = if light {
        ((0.965, 0.969, 0.984), (0.941, 0.945, 0.961))
    } else {
        ((0.078, 0.078, 0.078), (0.114, 0.114, 0.114))
    };
    let (base_r, base_g, base_b, tile_r, tile_g, tile_b) = if let Some(color) = tint {
        let alpha = color.a.clamp(0.0, 1.0);
        (
            blend_channel(base_dark.0, color.r, alpha),
            blend_channel(base_dark.1, color.g, alpha),
            blend_channel(base_dark.2, color.b, alpha),
            blend_channel(tile_dark.0, color.r, alpha),
            blend_channel(tile_dark.1, color.g, alpha),
            blend_channel(tile_dark.2, color.b, alpha),
        )
    } else {
        (
            base_dark.0,
            base_dark.1,
            base_dark.2,
            tile_dark.0,
            tile_dark.1,
            tile_dark.2,
        )
    };

    // Use a pattern fill for the checkerboard instead of a loop of rectangles.
    // This is much more efficient, especially for large areas.
    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::Rgb24,
        (tile_size * 2.0) as i32,
        (tile_size * 2.0) as i32,
    )
    .expect("failed to create checkerboard surface");
    let pattern_ctx =
        gtk4::cairo::Context::new(&surface).expect("failed to create pattern context");

    // Fill background
    pattern_ctx.set_source_rgb(base_r, base_g, base_b);
    pattern_ctx
        .paint()
        .expect("failed to paint pattern background");

    // Draw two tiles
    pattern_ctx.set_source_rgb(tile_r, tile_g, tile_b);
    pattern_ctx.rectangle(0.0, 0.0, tile_size, tile_size);
    pattern_ctx.rectangle(tile_size, tile_size, tile_size, tile_size);
    pattern_ctx.fill().expect("failed to fill pattern tiles");

    let pattern = gtk4::cairo::SurfacePattern::create(&surface);
    pattern.set_extend(gtk4::cairo::Extend::Repeat);

    context
        .set_source(&pattern)
        .expect("failed to set checkerboard pattern");
    context.rectangle(0.0, 0.0, width, height);
    let _ = context.fill();
}
pub fn rgba_to_cairo_argb_bytes(image: &RgbaImage) -> Vec<u8> {
    image
        .par_chunks_exact(4)
        .flat_map_iter(|pixel| {
            let r = pixel[0] as u32;
            let g = pixel[1] as u32;
            let b = pixel[2] as u32;
            let a = pixel[3] as u32;

            let pr = ((r * a + 127) / 255) as u8;
            let pg = ((g * a + 127) / 255) as u8;
            let pb = ((b * a + 127) / 255) as u8;

            [pb, pg, pr, a as u8]
        })
        .collect()
}
pub fn cairo_argb_to_rgba_image(width: u32, height: u32, stride: usize, data: &[u8]) -> RgbaImage {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        let row = &data[(y * stride)..(y * stride + (width as usize * 4))];
        for chunk in row.chunks_exact(4) {
            let b = chunk[0] as u32;
            let g = chunk[1] as u32;
            let r = chunk[2] as u32;
            let a = chunk[3] as u32;

            if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let rr = ((r * 255 + (a / 2)) / a).min(255) as u8;
            let gg = ((g * 255 + (a / 2)) / a).min(255) as u8;
            let bb = ((b * 255 + (a / 2)) / a).min(255) as u8;
            out.extend_from_slice(&[rr, gg, bb, a as u8]);
        }
    }

    ImageBuffer::from_raw(width, height, out).unwrap_or_else(|| RgbaImage::new(width, height))
}

/// Exponent of the quarter-superellipse every rounded corner in the app uses.
///
/// Corners are superellipses (`|u/r|^n + |v/r|^n = 1`) with n = 4 — "squircles" —
/// rather than circular arcs, so the edge leaves the straight side with zero
/// curvature instead of the tangent break a circle has. [`rounded_rect_path`]
/// draws this shape, and the video editor rasterizes the same one into its
/// exported alpha mask via [`squircle_rounded_rect_contains`].
pub const CORNER_SUPERELLIPSE_EXPONENT: f64 = 4.0;

/// True when `(x, y)` lies inside a `width` x `height` rounded rectangle whose
/// corners are quarter-superellipses ([`CORNER_SUPERELLIPSE_EXPONENT`]).
///
/// Top-left based coordinates. This is the implicit form of the shape
/// [`rounded_rect_path`] draws, so callers that need to rasterize the corner
/// (rather than stroke it) get the identical profile.
pub fn squircle_rounded_rect_contains(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) -> bool {
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    let (hw, hh) = (width / 2.0, height / 2.0);
    let dx = (x - hw).abs();
    let dy = (y - hh).abs();
    if dx > hw || dy > hh {
        return false;
    }
    if radius <= 0.0 {
        return true;
    }
    // Distance into the corner box per axis; zero along a straight side.
    let ax = (dx - (hw - radius)).max(0.0) / radius;
    let ay = (dy - (hh - radius)).max(0.0) / radius;
    let n = CORNER_SUPERELLIPSE_EXPONENT;
    ax.powf(n) + ay.powf(n) <= 1.0
}

/// Closed rounded-rectangle outline at absolute coordinates. Radius is clamped
/// so an inward expansion (a frame band painted inside the card edge) can never
/// hand cairo a negative radius, which would poison the context status.
///
/// Corners are smooth continuous-curvature (squircle) blends, not circular
/// arcs: a circle jumps from curvature 0 on the straight edge to 1/r at the
/// tangent point, which reads as "a curve stuck onto straight lines". Each
/// corner here is a quarter-superellipse (see [`CORNER_SUPERELLIPSE_EXPONENT`])
/// that leaves the edge with zero curvature and peaks mid-corner, so the edge
/// flows into the curve the way modern window frames do.
pub fn rounded_rect_path(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    if width <= 0.01 || height <= 0.01 {
        return;
    }
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    if radius <= 0.0 {
        context.rectangle(x, y, width, height);
        return;
    }
    // Quarter-superellipse per corner: `|cos|^(2/n)` shaping via sqrt, which
    // for the shared exponent (n = 4) is exact and cheaper than powf. Curvature
    // is zero where the corner leaves the straight edge and maximal at 45
    // degrees: no tangent break, no "curve then straight line" step.
    const SEGMENTS_PER_CORNER: usize = 16;
    let right = x + width;
    let bottom = y + height;
    // (center_x, center_y, start_angle) in path order: TR, BR, BL, TL.
    let corners = [
        (right - radius, y + radius, -std::f64::consts::FRAC_PI_2),
        (right - radius, bottom - radius, 0.0),
        (x + radius, bottom - radius, std::f64::consts::FRAC_PI_2),
        (x + radius, y + radius, std::f64::consts::PI),
    ];
    context.new_sub_path();
    let mut first = true;
    for (center_x, center_y, start) in corners {
        for step in 0..=SEGMENTS_PER_CORNER {
            let angle =
                start + (step as f64) / (SEGMENTS_PER_CORNER as f64) * std::f64::consts::FRAC_PI_2;
            let (sine, cosine) = angle.sin_cos();
            let point_x = center_x + radius * cosine.signum() * cosine.abs().sqrt();
            let point_y = center_y + radius * sine.signum() * sine.abs().sqrt();
            if first {
                context.move_to(point_x, point_y);
                first = false;
            } else {
                context.line_to(point_x, point_y);
            }
        }
    }
    context.close_path();
}

/// Styles whose main border joins the image rather than leaving a gap.
pub fn frame_has_attached_border(style: FrameStyle) -> bool {
    matches!(
        style,
        FrameStyle::Default
            | FrameStyle::InsetLight
            | FrameStyle::InsetDark
            | FrameStyle::Border
            | FrameStyle::Retro
    )
}

/// Fill a band using the image's exact corner outline. Outside bands paint
/// before the image and overlap its antialiased edge by one pixel.
pub fn paint_attached_frame_border(
    context: &gtk4::cairo::Context,
    image_rect: &super::composition::FloatRect,
    radius: f64,
    thickness: f64,
    color: DrawColor,
    inset: bool,
) {
    if thickness <= 0.01 || image_rect.width <= 0.0 || image_rect.height <= 0.0 {
        return;
    }
    let outer = if inset { 0.0 } else { thickness };
    let inner = if inset { thickness } else { 1.0 };
    let _ = context.save();
    context.new_path();
    context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
    rounded_rect_path(
        context,
        image_rect.x - outer,
        image_rect.y - outer,
        image_rect.width + outer * 2.0,
        image_rect.height + outer * 2.0,
        if radius <= 0.0 { 0.0 } else { radius + outer },
    );
    rounded_rect_path(
        context,
        image_rect.x + inner,
        image_rect.y + inner,
        image_rect.width - inner * 2.0,
        image_rect.height - inner * 2.0,
        (radius - inner).max(0.0),
    );
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();
    let _ = context.restore();
}

/// Resolved Liquid Glass geometry in device pixels.
pub struct LiquidFrame {
    band: f64,
    rim: f64,
    tint: DrawColor,
    rim_color: DrawColor,
    /// Wide frosted-glass mode (Glass Light/Dark): a diffuse milky/smoked
    /// veil instead of Liquid's clear refractive body.
    frost: bool,
}

impl LiquidFrame {
    /// Resolve a preset into device-pixel geometry. `thickness` is the
    /// caller's own resolved band thickness in `unit`-scaled units (the
    /// editors store the preset's value), falling back to the preset when the
    /// caller has none. `None` for any other preset or a collapsed band.
    pub fn resolve(spec: &FrameSpec, thickness: f64, unit: f64) -> Option<Self> {
        if !spec.liquid {
            return None;
        }
        let band_source = if thickness > 0.01 {
            thickness
        } else {
            spec.border_thickness
        };
        let frame = Self {
            band: (band_source * unit).max(0.0),
            rim: (spec.outer1.map(|outer| outer.thickness).unwrap_or(0.0) * unit).max(0.0),
            tint: spec.border_color,
            rim_color: spec
                .outer1
                .map(|outer| outer.color)
                .unwrap_or(DrawColor::new(1.0, 1.0, 1.0, 0.9)),
            frost: spec.frost,
        };
        if frame.band <= 0.01 && frame.rim <= 0.01 {
            return None;
        }
        Some(frame)
    }

    /// Paint the glass: a clear tinted band with the light pooling along its
    /// top lip, capped by a specular rim that fades around the perimeter,
    /// instead of the flat stroke every other preset uses. `top`/`bottom` are
    /// the card's vertical extent in the context's current user space and
    /// `path` builds a closed card outline expanded outward by its argument
    /// (negative expands inward), so the static renderers' rounded rects and
    /// the motion renderer's projected quads share one recipe.
    ///
    /// Layer order: tinted band, light pooling, lip shadow inside the card
    /// edge, specular rim, shadow line under the bottom of the rim.
    pub fn paint<F>(&self, context: &gtk4::cairo::Context, top: f64, bottom: f64, path: F)
    where
        F: Fn(&gtk4::cairo::Context, f64),
    {
        let LiquidFrame {
            band,
            rim,
            tint,
            rim_color,
            frost,
        } = *self;
        // Gradients need a real span; a degenerate band still paints something
        // sane instead of an empty pattern.
        let (top, bottom) = if bottom - top < 1.0 {
            (top, top + 1.0)
        } else {
            (top, bottom)
        };

        if band > 0.01 {
            let gradient = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            if frost {
                // Frosted body: a diffuse nearly-uniform veil in the tint
                // (whitish milk / darkish smoke) so the backdrop smears
                // behind it, like Shots.so frames. No lensing theatrics.
                let a = tint.a.clamp(0.0, 1.0);
                gradient.add_color_stop_rgba(0.0, tint.r, tint.g, tint.b, a);
                gradient.add_color_stop_rgba(0.5, tint.r, tint.g, tint.b, a * 0.92);
                gradient.add_color_stop_rgba(1.0, tint.r, tint.g, tint.b, a * 0.96);
            } else {
                // Clear glass body: mostly refraction, not paint. Keep the tint
                // whisper-thin with a cool cast so black backdrops stay black and
                // the highlights do the talking, like the reference shader.
                let a = (tint.a * 0.45).clamp(0.0, 1.0);
                gradient.add_color_stop_rgba(
                    0.0,
                    tint.r * 0.92,
                    tint.g * 0.95,
                    (tint.b * 1.05).min(1.0),
                    a * 0.5,
                );
                gradient.add_color_stop_rgba(
                    0.35,
                    tint.r * 0.92,
                    tint.g * 0.95,
                    (tint.b * 1.05).min(1.0),
                    a * 0.12,
                );
                gradient.add_color_stop_rgba(
                    0.78,
                    tint.r * 0.92,
                    tint.g * 0.95,
                    (tint.b * 1.05).min(1.0),
                    a * 0.18,
                );
                gradient.add_color_stop_rgba(
                    1.0,
                    tint.r * 0.92,
                    tint.g * 0.95,
                    (tint.b * 1.05).min(1.0),
                    a * 0.32,
                );
            }
            context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
            path(context, band);
            path(context, 0.0);
            let _ = context.set_source(&gradient);
            let _ = context.fill();
            context.set_fill_rule(gtk4::cairo::FillRule::Winding);
        }

        // Light pooling: one compact top wash sized to the band, so the
        // light sits on the glass instead of flooding past it. On a 3px
        // edge a stacked double wash just turns milky. Frost gets a soft
        // diffuse breath of light instead of a specular pool.
        if band > 1.0 {
            let gradient = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            if frost {
                gradient.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.16);
                gradient.add_color_stop_rgba(0.30, 1.0, 1.0, 1.0, 0.06);
                gradient.add_color_stop_rgba(0.65, 1.0, 1.0, 1.0, 0.0);
                gradient.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
            } else {
                gradient.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.50);
                gradient.add_color_stop_rgba(0.22, 1.0, 1.0, 1.0, 0.22);
                gradient.add_color_stop_rgba(0.58, 1.0, 1.0, 1.0, 0.05);
                gradient.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
            }
            let _ = context.set_source(&gradient);
            context.set_line_width(band * 0.6);
            path(context, band * 0.7);
            let _ = context.stroke();
            // Diagonal sheen on wider refractive bands only: a soft streak
            // across the top so dark frames still show a reflection. Frost
            // stays diffuse — no specular streaks.
            if band > 3.0 && !frost {
                let sheen = gtk4::cairo::LinearGradient::new(
                    0.0,
                    top,
                    band * 3.0,
                    top + (bottom - top) * 0.35,
                );
                sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
                sheen.add_color_stop_rgba(0.35, 1.0, 1.0, 1.0, 0.20);
                sheen.add_color_stop_rgba(0.55, 1.0, 1.0, 1.0, 0.06);
                sheen.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                let _ = context.set_source(&sheen);
                context.set_line_width((band * 0.55).max(1.0));
                path(context, band * 0.5);
                let _ = context.stroke();
            }
        }

        // Faint lip shade just inside the card edge: a whisper of depth so
        // the glass reads as a body. Kept minimal — anything stronger dirties
        // the screenshot content itself.
        if band > 0.5 {
            let lip = (band * 0.35).clamp(0.5, band);
            let shadow = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            shadow.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.05);
            shadow.add_color_stop_rgba(0.22, 0.0, 0.0, 0.0, 0.01);
            shadow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            let _ = context.set_source(&shadow);
            context.set_line_width(lip);
            path(context, -lip / 2.0);
            let _ = context.stroke();
        }

        // Inner lip highlight: the card edge catches the same top light as
        // the outer lip, otherwise the edge looks one-sided on black. Kept
        // visible all around (Fresnel-like) rather than fading to nothing.
        if band > 0.5 {
            let inner = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            inner.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.45);
            inner.add_color_stop_rgba(0.25, 1.0, 1.0, 1.0, 0.25);
            inner.add_color_stop_rgba(0.6, 1.0, 1.0, 1.0, 0.15);
            inner.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.20);
            let _ = context.set_source(&inner);
            context.set_line_width(1.0_f64.max(band * 0.14));
            path(context, 0.5);
            let _ = context.stroke();
        }

        if rim > 0.01 {
            let a = rim_color.a.clamp(0.0, 1.0);
            let (r, g, b) = (rim_color.r, rim_color.g, rim_color.b);
            let gradient = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            gradient.add_color_stop_rgba(0.0, r, g, b, a);
            gradient.add_color_stop_rgba(0.2, r, g, b, a * 0.85);
            gradient.add_color_stop_rgba(0.5, r, g, b, a * 0.60);
            gradient.add_color_stop_rgba(0.82, r, g, b, a * 0.45);
            gradient.add_color_stop_rgba(1.0, r, g, b, a * 0.50);
            let _ = context.set_source(&gradient);
            context.set_line_width(rim);
            path(context, band + rim / 2.0);
            let _ = context.stroke();

            // Shadow line under the bottom of the rim: on a light backdrop a
            // white rim alone washes out, and this is what makes the glass
            // read as a solid body rather than a floating hairline.
            let hair = (rim * 0.55).max(0.5);
            let edge = gtk4::cairo::LinearGradient::new(0.0, top, 0.0, bottom);
            edge.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            edge.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.0);
            edge.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.20);
            let _ = context.set_source(&edge);
            context.set_line_width(hair);
            path(context, band + rim * 0.5);
            let _ = context.stroke();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dump_checkerboard_for_artifact_debug() {
        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 560, 300)
            .expect("surface");
        let context = gtk4::cairo::Context::new(&surface).expect("context");
        draw_canvas_checkerboard_background(&context, 560, 300, None, false);
        surface.flush();
        drop(context);
        // analyze: count pixels differing from the two expected tile colors
        let stride = surface.stride() as usize;
        let data = surface.data().expect("data");
        let mut unexpected = 0usize;
        for y in 0..300usize {
            let row = &data[(y * stride)..];
            for x in 0..560usize {
                let b = row[x * 4] as i32;
                let g = row[x * 4 + 1] as i32;
                let r = row[x * 4 + 2] as i32;
                let is_base = (r - 20).abs() <= 2 && (g - 20).abs() <= 2 && (b - 20).abs() <= 2;
                let is_tile = (r - 29).abs() <= 2 && (g - 29).abs() <= 2 && (b - 29).abs() <= 2;
                if !is_base && !is_tile {
                    unexpected += 1;
                }
            }
        }
        let png = crate::capture::editor::render::cairo_argb_to_rgba_image(560, 300, stride, &data);
        let _ = image::save_buffer(
            "/tmp/checker_test.png",
            &png,
            560,
            300,
            image::ColorType::Rgba8,
        );
        assert_eq!(
            unexpected, 0,
            "checkerboard produced non-tile pixels: {unexpected}"
        );
    }

    use super::*;

    #[test]
    fn partial_annotation_is_clipped_to_composition_canvas_bounds() {
        let mut transform = super::super::types::ViewTransform::for_image(16.0, 16.0);
        transform.has_background = true;
        transform.canvas_width = 24.0;
        transform.canvas_height = 24.0;
        transform.image_rect_x = 4.0;
        transform.image_rect_y = 4.0;

        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 32, 32)
            .expect("surface");
        let context = gtk4::cairo::Context::new(&surface).expect("context");
        context.save().unwrap();
        clip_annotation_content_to_canvas(&context, transform);
        draw_annotation_action(
            &context,
            &AnnotationAction::Line {
                start: Point { x: -20.0, y: 5.0 },
                end: Point { x: 30.0, y: 5.0 },
                color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
                stroke_size: 3.0,
                shadow: false,
            },
        );
        context.restore().unwrap();
        drop(context);
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data");
        let image = cairo_argb_to_rgba_image(32, 32, stride, &data);

        assert!(image.get_pixel(1, 5).0[0] > 200);
        assert!(image.get_pixel(19, 5).0[0] > 200);
        assert_eq!(image.get_pixel(21, 5).0[3], 0);
    }

    #[test]
    fn paint_surface_with_filter_sets_requested_filter() {
        let surface =
            gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 4, 4).expect("surface");
        let context = gtk4::cairo::Context::new(&surface).expect("context");

        paint_surface_with_filter(&context, &surface, 0.0, 0.0, gtk4::cairo::Filter::Nearest);

        assert_eq!(context.source().filter(), gtk4::cairo::Filter::Nearest);
    }

    #[test]
    fn editor_image_filter_uses_good_when_downscaling() {
        assert_eq!(
            editor_image_filter_for_scale(0.75),
            gtk4::cairo::Filter::Good
        );
        assert_eq!(
            editor_image_filter_for_scale(0.25),
            gtk4::cairo::Filter::Good
        );
    }

    #[test]
    fn interactive_frames_use_a_cheap_image_filter() {
        assert_eq!(
            editor_interactive_image_filter(),
            gtk4::cairo::Filter::Bilinear,
            "Dragging must not pay the multi-millisecond `Good` resample per frame"
        );
    }

    #[test]
    fn interactive_blit_is_orders_of_magnitude_cheaper_than_the_quality_one() {
        // Guards the reason the interactive filter exists: if a future change makes
        // `Good` cheap again this can be relaxed, but the pointer-rate path must not
        // silently fall back to the quality resample.
        let source = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 3840, 2160)
            .expect("source surface");
        {
            let context = gtk4::cairo::Context::new(&source).expect("source context");
            let gradient = gtk4::cairo::LinearGradient::new(0.0, 0.0, 3840.0, 2160.0);
            gradient.add_color_stop_rgb(0.0, 0.2, 0.3, 0.5);
            gradient.add_color_stop_rgb(1.0, 0.8, 0.6, 0.4);
            context.set_source(&gradient).unwrap();
            context.paint().unwrap();
        }
        let target = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1400, 800)
            .expect("target surface");
        let scale = 1400.0 / 3840.0;

        let time_blit = |filter: gtk4::cairo::Filter| {
            let context = gtk4::cairo::Context::new(&target).expect("target context");
            let start = std::time::Instant::now();
            for _ in 0..3 {
                context.save().unwrap();
                context.scale(scale, scale);
                context.set_source_surface(&source, 0.0, 0.0).unwrap();
                context.source().set_filter(filter);
                context.paint().unwrap();
                let _ = context.restore();
            }
            start.elapsed() / 3
        };

        let interactive = time_blit(editor_interactive_image_filter());
        let quality = time_blit(editor_image_filter_for_scale(scale));
        assert!(
            interactive * 4 < quality,
            "interactive blit ({interactive:?}) should be far cheaper than the quality blit ({quality:?})"
        );
    }

    #[test]
    fn editor_image_filter_stays_smooth_at_full_scale_and_above() {
        assert_eq!(
            editor_image_filter_for_scale(1.0),
            gtk4::cairo::Filter::Good
        );
        assert_eq!(
            editor_image_filter_for_scale(1.2),
            gtk4::cairo::Filter::Good
        );
        assert_eq!(
            editor_image_filter_for_scale(2.0),
            gtk4::cairo::Filter::Good
        );
    }

    #[test]
    fn selection_handles_do_not_stroke_a_line_to_a_previous_number() {
        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 220, 220)
            .expect("surface");
        let context = gtk4::cairo::Context::new(&surface).expect("context");

        draw_number(
            &context,
            Point { x: 190.0, y: 190.0 },
            1,
            DrawColor::new(0.95, 0.75, 0.12, 1.0),
            NumberingStyle::Numeric,
            NumberSize::Medium,
        );

        let box_rect = Rect {
            x: 16,
            y: 16,
            width: 48,
            height: 32,
        };
        let box_color = DrawColor::new(0.91, 0.33, 0.13, 1.0);
        draw_box(&context, box_rect, box_color, 3.0);

        let handles =
            crate::capture::editor::selection::action_resize_handles(&AnnotationAction::Box {
                rect: box_rect,
                color: box_color,
                stroke_size: 3.0,
                shadow: false,
            });
        draw_selection_handles(&context, &handles, None, 1.0);
        drop(context);

        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data");
        let image = cairo_argb_to_rgba_image(220, 220, stride, &data);

        // Midpoint between the number badge and the box's top-left handle.
        // A leftover Cairo current-point would stroke a connector through here.
        let pixel = image.get_pixel(100, 100);
        assert_eq!(
            pixel.0[3], 0,
            "selecting a box must not draw a connector to a number marker, got {pixel:?}"
        );
    }

    #[test]
    fn smooth_rounded_rect_path_handles_degenerate_inputs() {
        for (w, h, r) in [
            (100.0, 80.0, 0.0),
            (100.0, 80.0, -4.0),
            (100.0, 80.0, 24.0),
            (100.0, 80.0, 10_000.0),
            (0.0, 80.0, 8.0),
            (100.0, 0.0, 8.0),
        ] {
            let surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 120, 100)
                .expect("surface");
            let context = gtk4::cairo::Context::new(&surface).expect("context");
            rounded_rect_path(&context, 10.0, 10.0, w, h, r);
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = context.fill();
            assert!(
                context.status().is_ok(),
                "rounded rect {w}x{h} r={r} poisoned the cairo context"
            );
        }
    }

    #[test]
    fn smooth_rounded_rect_blends_without_a_tangent_step() {
        // 200x200 card, radius 40: a circular arc already cuts the diagonal
        // at ~(12,12); the smooth corner keeps material there and only cuts
        // the extreme corner, so the edge flows into the curve.
        let mut surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 200, 200)
            .expect("surface");
        let context = gtk4::cairo::Context::new(&surface).expect("context");
        rounded_rect_path(&context, 0.0, 0.0, 200.0, 200.0, 40.0);
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let _ = context.fill();
        drop(context);
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data");
        let image = cairo_argb_to_rgba_image(200, 200, stride, &data);
        assert!(
            image.get_pixel(2, 2).0[3] < 128,
            "extreme corner should stay transparent, got {:?}",
            image.get_pixel(2, 2)
        );
        assert_eq!(
            *image.get_pixel(100, 1),
            image::Rgba([255, 255, 255, 255]),
            "straight edge should reach full extent, got {:?}",
            image.get_pixel(100, 1)
        );
        assert!(
            image.get_pixel(10, 10).0[3] > 200,
            "smooth corner should keep diagonal material a circular arc would cut, got {:?}",
            image.get_pixel(10, 10)
        );
        assert_eq!(
            *image.get_pixel(100, 100),
            image::Rgba([255, 255, 255, 255]),
            "card interior should stay filled, got {:?}",
            image.get_pixel(100, 100)
        );
    }
}
