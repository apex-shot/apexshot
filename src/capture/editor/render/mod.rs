use super::color::{highlighter_stroke_width, HIGHLIGHTER_ALPHA_SCALE};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::types::{AnnotationAction, DrawColor, Point, Rect, SelectHandle};
use image::{ImageBuffer, RgbaImage};
use rayon::prelude::*;

mod arrows;
mod effects;
mod text;

pub use arrows::{
    double_arrow_outline_points, draw_arrow, draw_arrow_control_handles,
    draw_arrow_selection_outline, thorn_arrow_outline_points,
};
pub use effects::{
    apply_blackout_rect, apply_blur_rect, apply_censor_rect, apply_focus_rect, apply_hybrid_blur,
};
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
        AnnotationAction::Obfuscate { rect, .. } => {
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
        AnnotationAction::Focus { rect, .. } => {
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
    }
}
pub fn draw_crop_overlay(
    context: &gtk4::cairo::Context,
    _image_width: f64,
    _image_height: f64,
    rect: Rect,
    active: bool,
) {
    let x = rect.x as f64;
    let y = rect.y as f64;
    let width = rect.width as f64;
    let height = rect.height as f64;

    if width <= 1.0 || height <= 1.0 {
        return;
    }

    let _ = context.save();
    context.rectangle(x, y, width, height);
    context.set_line_width(if active { 1.0 } else { 0.8 });
    context.set_source_rgba(1.0, 1.0, 1.0, 0.52);
    let _ = context.stroke();

    let edge_dash_len = (width.min(height) * 0.13).clamp(14.0, 30.0);
    let half_edge_dash_len = edge_dash_len / 2.0;
    let mid_x = x + width / 2.0;
    let mid_y = y + height / 2.0;

    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_width(if active { 2.2 } else { 1.8 });
    context.set_source_rgba(1.0, 1.0, 1.0, if active { 0.92 } else { 0.8 });

    context.move_to(mid_x - half_edge_dash_len, y);
    context.line_to(mid_x + half_edge_dash_len, y);
    context.move_to(mid_x - half_edge_dash_len, y + height);
    context.line_to(mid_x + half_edge_dash_len, y + height);
    context.move_to(x, mid_y - half_edge_dash_len);
    context.line_to(x, mid_y + half_edge_dash_len);
    context.move_to(x + width, mid_y - half_edge_dash_len);
    context.line_to(x + width, mid_y + half_edge_dash_len);
    let _ = context.stroke();

    context.set_line_cap(gtk4::cairo::LineCap::Butt);
    context.set_source_rgba(1.0, 1.0, 1.0, 0.36);
    context.set_line_width(1.0);
    for idx in 1..=2 {
        let dx = width * (idx as f64) / 3.0;
        let dy = height * (idx as f64) / 3.0;

        context.move_to(x + dx, y);
        context.line_to(x + dx, y + height);
        context.move_to(x, y + dy);
        context.line_to(x + width, y + dy);
    }
    let _ = context.stroke();

    let corner_len = (width.min(height) * 0.12).clamp(12.0, 26.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 0.98);
    context.set_line_width(if active { 3.2 } else { 2.5 });

    context.move_to(x, y + corner_len);
    context.line_to(x, y);
    context.line_to(x + corner_len, y);

    context.move_to(x + width - corner_len, y);
    context.line_to(x + width, y);
    context.line_to(x + width, y + corner_len);

    context.move_to(x, y + height - corner_len);
    context.line_to(x, y + height);
    context.line_to(x + corner_len, y + height);

    context.move_to(x + width - corner_len, y + height);
    context.line_to(x + width, y + height);
    context.line_to(x + width, y + height - corner_len);

    let _ = context.stroke();
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

        // White outline ring
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
