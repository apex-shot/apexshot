use super::color::{
    highlighter_stroke_width, CENSOR_BLOCK_SIZE, HIGHLIGHTER_ALPHA_SCALE, NUMBER_FONT_SIZE,
    NUMBER_RADIUS,
};
use super::types::{
    AnnotationAction, DrawColor, FontSettings, FontStyle, MoveHandle, Point, Rect, SelectHandle,
    TextAlignment, TextDecoration, TextEditBounds,
};
use image::{ImageBuffer, RgbaImage};

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

    if context.set_source_surface(&surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
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
        } => draw_circle(context, *rect, *color, *stroke_size),
        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
        } => draw_line(context, *start, *end, *color, *stroke_size),
        AnnotationAction::Arrow {
            start,
            end,
            color,
            stroke_size,
        } => draw_arrow(context, *start, *end, *color, *stroke_size),
        AnnotationAction::Box {
            rect,
            color,
            stroke_size,
        } => draw_box(context, *rect, *color, *stroke_size),
        AnnotationAction::Text {
            position,
            text,
            color,
            font,
        } => draw_text(context, *position, text, *color, font),
        AnnotationAction::Number {
            position,
            number,
            color,
        } => draw_number(context, *position, *number, *color),
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
            draw_pen(context, points, color.with_alpha(0.82), *stroke_size);
        }
        AnnotationAction::Highlighter {
            points,
            color,
            stroke_size,
        } => {
            draw_highlighter(context, points, color.with_alpha(0.72), *stroke_size);
        }
        AnnotationAction::Circle {
            rect,
            color,
            stroke_size,
        } => {
            draw_circle(context, *rect, color.with_alpha(0.82), *stroke_size);
        }
        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
        } => {
            draw_line(context, *start, *end, color.with_alpha(0.82), *stroke_size);
        }
        AnnotationAction::Arrow {
            start,
            end,
            color,
            stroke_size,
        } => {
            draw_arrow(context, *start, *end, color.with_alpha(0.82), *stroke_size);
        }
        AnnotationAction::Box {
            rect,
            color,
            stroke_size,
        } => {
            draw_box(context, *rect, color.with_alpha(0.82), *stroke_size);
        }
        AnnotationAction::Text {
            position,
            text,
            color,
            font,
        } => {
            draw_text(context, *position, text, color.with_alpha(0.9), font);
        }
        AnnotationAction::Number {
            position,
            number,
            color,
        } => {
            draw_number(context, *position, *number, color.with_alpha(0.88));
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
        AnnotationAction::Focus { rect } => {
            draw_focus_rect_outline(context, *rect, true);
        }
    }
}

fn draw_focus_rect_outline(context: &gtk4::cairo::Context, rect: Rect, active: bool) {
    let width = rect.width.max(1) as f64;
    let height = rect.height.max(1) as f64;

    context.rectangle(rect.x as f64, rect.y as f64, width, height);
    context.set_line_width(if active { 2.0 } else { 1.4 });
    context.set_source_rgba(0.94, 0.97, 1.0, if active { 0.95 } else { 0.85 });
    let _ = context.stroke_preserve();

    context.set_dash(&[6.0, 4.0], 0.0);
    context.set_source_rgba(0.16, 0.60, 0.99, if active { 0.92 } else { 0.72 });
    context.set_line_width(1.2);
    let _ = context.stroke();
    context.set_dash(&[], 0.0);
}

pub fn draw_focus_overlay(
    context: &gtk4::cairo::Context,
    image_width: f64,
    image_height: f64,
    rect: Rect,
    active: bool,
) {
    let Some(rect) = rect.clamp_to(image_width as u32, image_height as u32) else {
        return;
    };

    let width = rect.width.max(1) as f64;
    let height = rect.height.max(1) as f64;
    if width <= 1.0 || height <= 1.0 {
        return;
    }

    let _ = context.save();
    context.rectangle(0.0, 0.0, image_width, image_height);
    context.rectangle(rect.x as f64, rect.y as f64, width, height);
    context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
    context.set_source_rgba(0.0, 0.0, 0.0, if active { 0.58 } else { 0.52 });
    let _ = context.fill();
    let _ = context.restore();

    if active {
        draw_focus_rect_outline(context, rect, true);
    }
}

#[allow(dead_code)]
pub fn draw_censor_draft_rect(context: &gtk4::cairo::Context, rect: Rect) {
    context.set_source_rgba(0.06, 0.08, 0.10, 0.34);
    context.rectangle(
        rect.x as f64,
        rect.y as f64,
        rect.width as f64,
        rect.height as f64,
    );
    let _ = context.fill_preserve();

    context.set_source_rgba(0.94, 0.97, 1.0, 0.82);
    context.set_line_width(2.0);
    let _ = context.stroke();

    context.set_source_rgba(0.94, 0.97, 1.0, 0.24);
    context.set_line_width(1.0);
    let step = (CENSOR_BLOCK_SIZE as f64 / 2.0).max(4.0);

    let x_start = rect.x as f64;
    let y_start = rect.y as f64;
    let x_end = x_start + rect.width as f64;
    let y_end = y_start + rect.height as f64;

    let mut x = x_start + step;
    while x < x_end {
        context.move_to(x, y_start);
        context.line_to(x, y_end);
        x += step;
    }

    let mut y = y_start + step;
    while y < y_end {
        context.move_to(x_start, y);
        context.line_to(x_end, y);
        y += step;
    }
    let _ = context.stroke();
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
    context.set_line_width(2.0 / scale);
    context.set_dash(&[], 0.0);

    context.new_path();
    context.move_to(x + radius, y);
    context.line_to(x + width - radius, y);
    context.arc(x + width - radius, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
    context.line_to(x + width, y + height - radius);
    context.arc(x + width - radius, y + height - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
    context.line_to(x + radius, y + height);
    context.arc(x + radius, y + height - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    context.line_to(x, y + radius);
    context.arc(x + radius, y + radius, radius, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2);
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

const TEXT_EDIT_BORDER_COLOR: (f64, f64, f64) = (0.231, 0.510, 0.965); // #3b82f6
const TEXT_EDIT_BORDER_WIDTH: f64 = 2.0;
const TEXT_EDIT_BORDER_RADIUS: f64 = 4.0;

pub fn draw_text_edit_border(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);
    let _ = context.save();

    let rect = &bounds.rect;
    let x = rect.x as f64;
    let y = rect.y as f64;
    let width = rect.width as f64;
    let height = rect.height as f64;

    // Draw rounded rectangle border
    context.set_source_rgba(
        TEXT_EDIT_BORDER_COLOR.0,
        TEXT_EDIT_BORDER_COLOR.1,
        TEXT_EDIT_BORDER_COLOR.2,
        1.0,
    );
    context.set_line_width(TEXT_EDIT_BORDER_WIDTH / scale);

    let radius = TEXT_EDIT_BORDER_RADIUS;
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

const MOVE_HANDLE_RADIUS: f64 = 5.0;
const MOVE_HANDLE_OUTLINE_WIDTH: f64 = 2.0;
const RESIZE_HANDLE_SIZE: f64 = 12.0;

pub fn draw_text_edit_handles(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    active_handle: Option<MoveHandle>,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);
    let _ = context.save();

    // Draw move handles (left and right circles)
    for (handle, center) in &bounds.move_handles {
        let is_active = active_handle.as_ref().is_some_and(|h| *h == *handle);
        let radius = MOVE_HANDLE_RADIUS + if is_active { 1.0 } else { 0.0 };

        // White outline
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
        context.arc(
            center.x,
            center.y,
            radius / scale,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = context.stroke();

        // Blue fill
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        context.arc(
            center.x,
            center.y,
            (radius - MOVE_HANDLE_OUTLINE_WIDTH) / scale,
            0.0,
            std::f64::consts::TAU,
        );
        let _ = context.fill();
    }

    // Draw resize handle (bottom-right box)
    if let Some((_, resize_pos)) = &bounds.resize_handle {
        let size = RESIZE_HANDLE_SIZE;
        let half = size / 2.0;

        // White outline
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
        context.rectangle(
            resize_pos.x - half / scale,
            resize_pos.y - half / scale,
            size / scale,
            size / scale,
        );
        let _ = context.stroke();

        // Blue fill
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        context.rectangle(
            resize_pos.x - half / scale + MOVE_HANDLE_OUTLINE_WIDTH / scale,
            resize_pos.y - half / scale + MOVE_HANDLE_OUTLINE_WIDTH / scale,
            (size - MOVE_HANDLE_OUTLINE_WIDTH * 2.0) / scale,
            (size - MOVE_HANDLE_OUTLINE_WIDTH * 2.0) / scale,
        );
        let _ = context.fill();
    }

    let _ = context.restore();
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
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke + 0.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.move_to(points[0].x, points[0].y);

    for point in &points[1..] {
        context.line_to(point.x, point.y);
    }

    let _ = context.stroke();
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
    context.set_operator(gtk4::cairo::Operator::Multiply);
    context.set_source_rgba(
        color.r,
        color.g,
        color.b,
        (color.a * HIGHLIGHTER_ALPHA_SCALE).clamp(0.05, 0.95),
    );
    context.set_line_width(stroke);
    context.set_line_cap(gtk4::cairo::LineCap::Butt);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.move_to(points[0].x, points[0].y);

    for point in &points[1..] {
        context.line_to(point.x, point.y);
    }

    let _ = context.stroke();
    let _ = context.restore();
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
    let min_radius = radius_x.min(radius_y).max(1.0);

    let _ = context.save();
    context.translate(center_x, center_y);
    context.scale(radius_x.max(1.0), radius_y.max(1.0));
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5) / min_radius);
    context.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
    let _ = context.stroke();
    let _ = context.restore();
}

pub fn draw_line(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5) + 0.4);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(start.x, start.y);
    context.line_to(end.x, end.y);
    let _ = context.stroke();
}

pub fn draw_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
) {
    let stroke = stroke_size.max(0.5);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke + 0.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(start.x, start.y);
    context.line_to(end.x, end.y);
    let _ = context.stroke();

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < 0.1 && dy.abs() < 0.1 {
        return;
    }

    let angle = dy.atan2(dx);
    let line_length = (dx * dx + dy * dy).sqrt().max(1.0);
    let head_length = (stroke * 4.8)
        .clamp(12.0, 120.0)
        .min((line_length * 0.75).max(8.0));
    let spread = 0.55;
    let left_x = end.x - head_length * (angle - spread).cos();
    let left_y = end.y - head_length * (angle - spread).sin();
    let right_x = end.x - head_length * (angle + spread).cos();
    let right_y = end.y - head_length * (angle + spread).sin();

    context.move_to(end.x, end.y);
    context.line_to(left_x, left_y);
    context.line_to(right_x, right_y);
    context.close_path();
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();
}

pub fn draw_box(context: &gtk4::cairo::Context, rect: Rect, color: DrawColor, stroke_size: f64) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5));
    context.rectangle(
        rect.x as f64,
        rect.y as f64,
        rect.width as f64,
        rect.height as f64,
    );
    let _ = context.stroke();
}

pub fn draw_text(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);

    let slant = match font.style {
        FontStyle::Normal | FontStyle::Bold => gtk4::cairo::FontSlant::Normal,
        FontStyle::Italic | FontStyle::BoldItalic => gtk4::cairo::FontSlant::Italic,
    };
    let weight = match font.style {
        FontStyle::Normal | FontStyle::Italic => gtk4::cairo::FontWeight::Normal,
        FontStyle::Bold | FontStyle::BoldItalic => gtk4::cairo::FontWeight::Bold,
    };

    context.select_font_face(&font.family, slant, weight);
    context.set_font_size(font.size.max(1.0));

    // Handle alignment by computing text width
    let x_offset = if font.alignment != TextAlignment::Left {
        if let Ok(extents) = context.text_extents(text) {
            match font.alignment {
                TextAlignment::Center => -extents.width() / 2.0,
                TextAlignment::Right => -extents.width(),
                TextAlignment::Left => 0.0,
            }
        } else {
            0.0
        }
    } else {
        0.0
    };

    context.move_to(position.x + x_offset, position.y);
    let _ = context.show_text(text);

    // Draw decorations (underline/strikethrough)
    if font.decoration != TextDecoration::None {
        if let Ok(extents) = context.text_extents(text) {
            let y = position.y;
            match font.decoration {
                TextDecoration::Underline | TextDecoration::Both => {
                    context.move_to(position.x + x_offset, y + 2.0);
                    context.line_to(position.x + x_offset + extents.width(), y + 2.0);
                    let _ = context.stroke();
                }
                _ => {}
            }
            match font.decoration {
                TextDecoration::Strikethrough | TextDecoration::Both => {
                    let strike_y = y - extents.height() / 2.0 - extents.y_bearing() / 2.0;
                    context.move_to(position.x + x_offset, strike_y);
                    context.line_to(position.x + x_offset + extents.width(), strike_y);
                    let _ = context.stroke();
                }
                _ => {}
            }
        }
    }
}

pub fn draw_number(context: &gtk4::cairo::Context, position: Point, number: u32, color: DrawColor) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.arc(
        position.x,
        position.y,
        NUMBER_RADIUS,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = context.fill_preserve();

    context.set_source_rgba(0.02, 0.03, 0.05, 0.42);
    context.set_line_width(1.5);
    let _ = context.stroke();

    let luminance = (0.299 * color.r) + (0.587 * color.g) + (0.114 * color.b);
    let (text_r, text_g, text_b) = if luminance > 0.65 {
        (0.07, 0.08, 0.10)
    } else {
        (0.98, 0.99, 1.0)
    };

    let label = number.to_string();
    context.set_source_rgba(text_r, text_g, text_b, 0.98);
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(NUMBER_FONT_SIZE);

    if let Ok(extents) = context.text_extents(&label) {
        let text_x = position.x - (extents.width() / 2.0 + extents.x_bearing());
        let text_y = position.y - (extents.height() / 2.0 + extents.y_bearing());
        context.move_to(text_x, text_y);
    } else {
        context.move_to(position.x - 4.0, position.y + 4.0);
    }

    let _ = context.show_text(&label);
}

const BLUR_PERFORMANCE_THRESHOLD: usize = 400 * 400; // 400x400 pixels
const BLUR_DOWNSAMPLE_FACTOR: usize = 4;

pub fn apply_blur_rect(image: &mut RgbaImage, rect: Rect, radius: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if radius <= 0.0 {
        return;
    }

    let image_width = image.width() as usize;
    let image_height = image.height() as usize;
    let rect_width = rect.width as usize;
    let rect_height = rect.height as usize;
    let area = rect_width * rect_height;

    // For large regions, use downsampled blur (never fall back to pixelation).
    // The downsampled path is robust and always produces a real blur.
    if area > BLUR_PERFORMANCE_THRESHOLD {
        apply_blur_rect_downsampled(image, rect, radius);
        return;
    }

    let radius = radius.max(1.0) as usize;

    // Use separable box blur for better memory efficiency
    // This uses O(max(width, height)) memory instead of O(width * height)

    let x0 = rect.x.max(0) as usize;
    let y0 = rect.y.max(0) as usize;
    let x1 = (rect.x + rect.width).min(image_width as i32) as usize;
    let y1 = (rect.y + rect.height).min(image_height as i32) as usize;

    if x1 <= x0 || y1 <= y0 {
        return;
    }

    // Expand the working area by the radius to include blur sampling
    let sample_x0 = x0.saturating_sub(radius);
    let sample_y0 = y0.saturating_sub(radius);
    let sample_x1 = (x1 + radius).min(image_width);
    let sample_y1 = (y1 + radius).min(image_height);

    let work_width = sample_x1 - sample_x0;
    let work_height = sample_y1 - sample_y0;

    if work_width == 0 || work_height == 0 {
        return;
    }

    // Extract working region
    let mut work_buffer: Vec<[u8; 4]> = Vec::with_capacity(work_width * work_height);
    for y in sample_y0..sample_y1 {
        for x in sample_x0..sample_x1 {
            work_buffer.push(image.get_pixel(x as u32, y as u32).0);
        }
    }

    // Horizontal pass
    let mut temp_buffer: Vec<[u32; 4]> = vec![[0; 4]; work_width * work_height];

    for y in 0..work_height {
        let row_start = y * work_width;

        // Compute running sum for this row
        let mut sum = [0u32; 4];

        // Initialize with first radius+1 pixels
        for x in 0..=radius.min(work_width - 1) {
            let pixel = work_buffer[row_start + x];
            sum[0] += pixel[0] as u32;
            sum[1] += pixel[1] as u32;
            sum[2] += pixel[2] as u32;
            sum[3] += pixel[3] as u32;
        }

        // Slide the window
        for x in 0..work_width {
            let left = x.saturating_sub(radius);
            let right = (x + radius + 1).min(work_width);
            let count = (right - left) as u32;

            temp_buffer[row_start + x] = [
                sum[0] / count,
                sum[1] / count,
                sum[2] / count,
                sum[3] / count,
            ];

            // Remove left pixel from sum
            if left > 0 {
                let left_idx = row_start + left - 1;
                sum[0] = sum[0].saturating_sub(work_buffer[left_idx][0] as u32);
                sum[1] = sum[1].saturating_sub(work_buffer[left_idx][1] as u32);
                sum[2] = sum[2].saturating_sub(work_buffer[left_idx][2] as u32);
                sum[3] = sum[3].saturating_sub(work_buffer[left_idx][3] as u32);
            }

            // Add right pixel to sum
            if right < work_width {
                let right_idx = row_start + right;
                sum[0] += work_buffer[right_idx][0] as u32;
                sum[1] += work_buffer[right_idx][1] as u32;
                sum[2] += work_buffer[right_idx][2] as u32;
                sum[3] += work_buffer[right_idx][3] as u32;
            }
        }
    }

    // Vertical pass - write directly back to work_buffer
    for x in 0..work_width {
        let mut sum = [0u32; 4];

        // Initialize with first radius+1 pixels
        for y in 0..=radius.min(work_height - 1) {
            let pixel = temp_buffer[y * work_width + x];
            sum[0] += pixel[0];
            sum[1] += pixel[1];
            sum[2] += pixel[2];
            sum[3] += pixel[3];
        }

        // Slide the window
        for y in 0..work_height {
            let top = y.saturating_sub(radius);
            let bottom = (y + radius + 1).min(work_height);
            let count = (bottom - top) as u32;

            work_buffer[y * work_width + x] = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ];

            // Remove top pixel from sum
            if top > 0 {
                let top_idx = (top - 1) * work_width + x;
                sum[0] = sum[0].saturating_sub(temp_buffer[top_idx][0]);
                sum[1] = sum[1].saturating_sub(temp_buffer[top_idx][1]);
                sum[2] = sum[2].saturating_sub(temp_buffer[top_idx][2]);
                sum[3] = sum[3].saturating_sub(temp_buffer[top_idx][3]);
            }

            // Add bottom pixel to sum
            if bottom < work_height {
                let bottom_idx = bottom * work_width + x;
                sum[0] += temp_buffer[bottom_idx][0];
                sum[1] += temp_buffer[bottom_idx][1];
                sum[2] += temp_buffer[bottom_idx][2];
                sum[3] += temp_buffer[bottom_idx][3];
            }
        }
    }

    // Write back only the rect area (not the expanded sample area)
    for y in y0..y1 {
        for x in x0..x1 {
            let work_x = x - sample_x0;
            let work_y = y - sample_y0;
            let mut pixel = work_buffer[work_y * work_width + work_x];
            // Prevent alpha artifacts from making the checkerboard show through.
            pixel[3] = 255;
            image.put_pixel(x as u32, y as u32, image::Rgba(pixel));
        }
    }
}

fn apply_blur_rect_downsampled(image: &mut RgbaImage, rect: Rect, radius: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if radius <= 0.0 {
        return;
    }

    // Inspired by the Qt overlay approach (downsample + blur + upsample).
    // This is resilient and guarantees we fill the entire target rect (no checkerboard gaps).
    let factor = BLUR_DOWNSAMPLE_FACTOR.max(2) as u32;
    let small_w = (image.width() / factor).max(1);
    let small_h = (image.height() / factor).max(1);

    // 1) Downsample full image
    let mut small = image::imageops::resize(
        image,
        small_w,
        small_h,
        image::imageops::FilterType::Triangle,
    );

    // 2) Blur the corresponding small rect
    let sr = Rect {
        x: (rect.x as f64 / factor as f64).floor() as i32,
        y: (rect.y as f64 / factor as f64).floor() as i32,
        width: ((rect.width as f64) / factor as f64).ceil() as i32,
        height: ((rect.height as f64) / factor as f64).ceil() as i32,
    };

    if let Some(sr) = sr.clamp_to(small.width(), small.height()) {
        let small_radius = (radius / factor as f64).max(1.0) as usize;
        apply_blur_rect_to_buffer(&mut small, sr, small_radius);

        // 3) Extract the blurred small region, then upsample it exactly to the original rect size
        let src_x0 = sr.x.max(0) as u32;
        let src_y0 = sr.y.max(0) as u32;
        let src_w = sr.width.max(1) as u32;
        let src_h = sr.height.max(1) as u32;

        let cropped = image::imageops::crop_imm(&small, src_x0, src_y0, src_w, src_h).to_image();
        let up = image::imageops::resize(
            &cropped,
            rect.width.max(1) as u32,
            rect.height.max(1) as u32,
            image::imageops::FilterType::Triangle,
        );

        // 4) Write back (always fills entire rect)
        let x0 = rect.x.max(0) as u32;
        let y0 = rect.y.max(0) as u32;
        for y in 0..rect.height.max(1) as u32 {
            for x in 0..rect.width.max(1) as u32 {
                let px = x0 + x;
                let py = y0 + y;
                if px < image.width() && py < image.height() {
                    let mut p = *up.get_pixel(x, y);
                    // Prevent any alpha artifacts from making the checkerboard show through.
                    p[3] = 255;
                    image.put_pixel(px, py, p);
                }
            }
        }
    }
}

fn apply_blur_rect_to_buffer(image: &mut image::RgbaImage, rect: Rect, radius: usize) {
    use image::Rgba;

    let (img_width, img_height) = image.dimensions();
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x + rect.width).min(img_width as i32) as u32;
    let y1 = (rect.y + rect.height).min(img_height as i32) as u32;

    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let radius = radius.max(1);
    let sample_x0 = x0.saturating_sub(radius as u32);
    let sample_y0 = y0.saturating_sub(radius as u32);
    let sample_x1 = ((x1 as i32) + radius as i32).min(img_width as i32) as u32;
    let sample_y1 = ((y1 as i32) + radius as i32).min(img_height as i32) as u32;

    let work_width = (sample_x1 - sample_x0) as usize;
    let work_height = (sample_y1 - sample_y0) as usize;

    if work_width == 0 || work_height == 0 {
        return;
    }

    let mut work_buffer: Vec<[u8; 4]> = Vec::with_capacity(work_width * work_height);
    for y in sample_y0..sample_y1 {
        for x in sample_x0..sample_x1 {
            let p = image.get_pixel(x, y);
            work_buffer.push([p[0], p[1], p[2], p[3]]);
        }
    }

    let mut temp_buffer: Vec<[u32; 4]> = vec![[0; 4]; work_width * work_height];

    for y in 0..work_height {
        let row_start = y * work_width;
        let mut sum = [0u32; 4];

        for x in 0..=radius.min(work_width - 1) {
            let pixel = work_buffer[row_start + x];
            sum[0] += pixel[0] as u32;
            sum[1] += pixel[1] as u32;
            sum[2] += pixel[2] as u32;
            sum[3] += pixel[3] as u32;
        }

        for x in 0..work_width {
            let left = x.saturating_sub(radius);
            let right = (x + radius + 1).min(work_width);
            let count = (right - left) as u32;

            temp_buffer[row_start + x] = [
                sum[0] / count,
                sum[1] / count,
                sum[2] / count,
                sum[3] / count,
            ];

            if left > 0 {
                let idx = row_start + left - 1;
                sum[0] = sum[0].saturating_sub(work_buffer[idx][0] as u32);
                sum[1] = sum[1].saturating_sub(work_buffer[idx][1] as u32);
                sum[2] = sum[2].saturating_sub(work_buffer[idx][2] as u32);
                sum[3] = sum[3].saturating_sub(work_buffer[idx][3] as u32);
            }

            if right < work_width {
                let idx = row_start + right;
                sum[0] += work_buffer[idx][0] as u32;
                sum[1] += work_buffer[idx][1] as u32;
                sum[2] += work_buffer[idx][2] as u32;
                sum[3] += work_buffer[idx][3] as u32;
            }
        }
    }

    for _y in 0..work_height {
        let mut sum = [0u32; 4];

        for i in 0..=radius.min(work_height - 1) {
            let idx = i * work_width;
            sum[0] += temp_buffer[idx][0];
            sum[1] += temp_buffer[idx][1];
            sum[2] += temp_buffer[idx][2];
            sum[3] += temp_buffer[idx][3];
        }

        for y_out in 0..work_height {
            let top = y_out.saturating_sub(radius);
            let bottom = (y_out + radius + 1).min(work_height);
            let count = (bottom - top) as u32;

            let target_x0 = sample_x0 as i32;
            let target_y0 = sample_y0 as i32 + y_out as i32;

            if target_x0 >= x0 as i32
                && target_y0 >= y0 as i32
                && target_x0 < x1 as i32
                && target_y0 < y1 as i32
            {
                image.put_pixel(
                    target_x0 as u32,
                    target_y0 as u32,
                    Rgba([
                        (sum[0] / count) as u8,
                        (sum[1] / count) as u8,
                        (sum[2] / count) as u8,
                        (sum[3] / count) as u8,
                    ]),
                );
            }

            if top > 0 {
                let idx = (top - 1) * work_width;
                sum[0] = sum[0].saturating_sub(temp_buffer[idx][0]);
                sum[1] = sum[1].saturating_sub(temp_buffer[idx][1]);
                sum[2] = sum[2].saturating_sub(temp_buffer[idx][2]);
                sum[3] = sum[3].saturating_sub(temp_buffer[idx][3]);
            }

            if bottom < work_height {
                let idx = bottom * work_width;
                sum[0] += temp_buffer[idx][0];
                sum[1] += temp_buffer[idx][1];
                sum[2] += temp_buffer[idx][2];
                sum[3] += temp_buffer[idx][3];
            }
        }
    }
}

pub fn apply_censor_rect(image: &mut RgbaImage, rect: Rect, block_size: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if block_size <= 0.0 {
        return;
    }

    let block = block_size as i32;
    let max_y = rect.y + rect.height;
    let max_x = rect.x + rect.width;

    // For large regions, use a more memory-efficient approach
    // by reading directly from the image instead of cloning
    let mut by = rect.y;
    while by < max_y {
        let block_height = (max_y - by).min(block);

        let mut bx = rect.x;
        while bx < max_x {
            let block_width = (max_x - bx).min(block);

            let mut r_sum: u32 = 0;
            let mut g_sum: u32 = 0;
            let mut b_sum: u32 = 0;
            let mut a_sum: u32 = 0;
            let mut count: u32 = 0;

            // Read directly from image - no clone needed
            for y in by..(by + block_height) {
                for x in bx..(bx + block_width) {
                    if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
                        let p = image.get_pixel(x as u32, y as u32);
                        r_sum += p[0] as u32;
                        g_sum += p[1] as u32;
                        b_sum += p[2] as u32;
                        a_sum += p[3] as u32;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let color = image::Rgba([
                    (r_sum / count) as u8,
                    (g_sum / count) as u8,
                    (b_sum / count) as u8,
                    (a_sum / count) as u8,
                ]);

                for y in by..(by + block_height) {
                    for x in bx..(bx + block_width) {
                        if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32
                        {
                            image.put_pixel(x as u32, y as u32, color);
                        }
                    }
                }
            }

            bx += block;
        }

        by += block;
    }
}

/// Secure pseudo-pixelate that only samples from the fringe (edges) of the selection.
/// This makes the obfuscation irreversible because the interior content is never used.
/// Based on Flameshot's secure pixelate implementation.
#[allow(dead_code)]
pub fn apply_secure_pixelate(image: &mut RgbaImage, rect: Rect, block_size: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if block_size <= 0.0 || rect.width < 3 || rect.height < 3 {
        return;
    }

    let img_width = image.width() as i32;
    let img_height = image.height() as i32;

    // Calculate effect size (downsampled dimensions)
    let effect_width = ((rect.width as f64) * 0.5 / block_size.max(1.0)).max(1.0) as i32;
    let effect_height = ((rect.height as f64) * 0.5 / block_size.max(1.0)).max(1.0) as i32;

    // Extract fringe pixels (edges of the selection)
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.width).min(img_width);
    let y1 = (rect.y + rect.height).min(img_height);

    // Offset for fringe sampling (1 pixel outside the selection if possible)
    let offset_top = if y0 > 0 { -1 } else { 0 };
    let offset_bottom = if y1 < img_height { 1 } else { 0 };
    let offset_left = if x0 > 0 { -1 } else { 0 };
    let offset_right = if x1 < img_width { 1 } else { 0 };

    // Collect fringe colors
    let mut fringe_top: Vec<[u8; 4]> = Vec::new();
    let mut fringe_bottom: Vec<[u8; 4]> = Vec::new();
    let mut fringe_left: Vec<[u8; 4]> = Vec::new();
    let mut fringe_right: Vec<[u8; 4]> = Vec::new();

    // Top fringe
    for x in x0..x1 {
        let y = (y0 + offset_top).max(0).min(img_height - 1);
        fringe_top.push(image.get_pixel(x as u32, y as u32).0);
    }

    // Bottom fringe
    for x in x0..x1 {
        let y = (y1 + offset_bottom - 1).max(0).min(img_height - 1);
        fringe_bottom.push(image.get_pixel(x as u32, y as u32).0);
    }

    // Left fringe
    for y in y0..y1 {
        let x = (x0 + offset_left).max(0).min(img_width - 1);
        fringe_left.push(image.get_pixel(x as u32, y as u32).0);
    }

    // Right fringe
    for y in y0..y1 {
        let x = (x1 + offset_right - 1).max(0).min(img_width - 1);
        fringe_right.push(image.get_pixel(x as u32, y as u32).0);
    }

    // Simple deterministic PRNG for reproducible results
    let mut prng_state: u32 = 42;
    let mut prng_next = || -> u32 {
        prng_state = prng_state.wrapping_mul(1103515245).wrapping_add(12345);
        prng_state
    };

    // Generate pixelated output using fringe sampling
    let mut output: Vec<[u8; 4]> = Vec::with_capacity((effect_width * effect_height) as usize);

    for ey in 0..effect_height {
        for ex in 0..effect_width {
            // Relative position (0.0 to 1.0)
            let horizontal = ex as f64 / effect_width.max(1) as f64;
            let vertical = ey as f64 / effect_height.max(1) as f64;

            // Sample from each fringe with noise
            let noise_val = (prng_next() as f64 / u32::MAX as f64 - 0.5) * 0.1;

            // Sample from all four fringes
            let fringe_refs = [&fringe_top, &fringe_bottom, &fringe_left, &fringe_right];
            let mut samples = [[0.0; 4]; 4];
            for (i, fringe) in fringe_refs.iter().enumerate() {
                let pos = if i < 2 { horizontal } else { vertical };
                let sample_noise = (prng_next() as f64 / u32::MAX as f64 - 0.5) * 0.1;
                let idx = ((pos + sample_noise).clamp(0.0, 0.999) * fringe.len() as f64) as usize;
                let p = fringe.get(idx).copied().unwrap_or([128, 128, 128, 255]);
                samples[i] = [
                    p[0] as f64 / 255.0,
                    p[1] as f64 / 255.0,
                    p[2] as f64 / 255.0,
                    p[3] as f64 / 255.0,
                ];
            }

            // Calculate interpolation weights
            let weight_h = (ex.min(effect_width - 1 - ex) as f64 / effect_width.max(1) as f64)
                - (ey.min(effect_height - 1 - ey) as f64 / effect_height.max(1) as f64)
                + 0.5;
            let weight_v = 1.0 - weight_h;

            // Interpolate between horizontal and vertical samples
            let mut rgb = [0.0; 4];
            for i in 0..4 {
                let horiz = (1.0 - horizontal) * samples[2][i] + horizontal * samples[3][i];
                let vert = (1.0 - vertical) * samples[0][i] + vertical * samples[1][i];
                rgb[i] = weight_h * horiz + weight_v * vert + noise_val;
                rgb[i] = rgb[i].clamp(0.0, 1.0);
            }

            output.push([
                (rgb[0] * 255.0) as u8,
                (rgb[1] * 255.0) as u8,
                (rgb[2] * 255.0) as u8,
                (rgb[3] * 255.0) as u8,
            ]);
        }
    }

    // Scale up and apply to image
    for y in y0..y1 {
        for x in x0..x1 {
            let ex = ((x - x0) as f64 * effect_width as f64 / (x1 - x0) as f64) as i32;
            let ey = ((y - y0) as f64 * effect_height as f64 / (y1 - y0) as f64) as i32;
            let ex = ex.min(effect_width - 1).max(0);
            let ey = ey.min(effect_height - 1).max(0);
            let idx = (ey * effect_width + ex) as usize;
            if let Some(&color) = output.get(idx) {
                image.put_pixel(x as u32, y as u32, image::Rgba(color));
            }
        }
    }
}

/// Secure blur: irreversibly obfuscate a region with a smooth blurred appearance.
///
/// Strategy: downsample the region to a very small thumbnail (aggressively
/// destroying detail), then upsample back and blur multiple times.
/// The result looks like a smooth blur but is cryptographically irreversible
/// because the original pixel content is reduced to a tiny representation.
pub fn apply_secure_blur(image: &mut RgbaImage, rect: Rect, amount: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if rect.width < 2 || rect.height < 2 {
        return;
    }

    // Use a much more aggressive downsample than Blur (Smooth).
    // At amount=1 we downsample to ~1/6, at amount=25 we downsample to ~1/20.
    // This ensures the secure variant always destroys more detail than smooth blur.
    let base_factor = 6.0 + (amount / 25.0) * 14.0; // 6x at min, 20x at max
    let factor = (base_factor as u32)
        .max(4)
        .min((rect.width.min(rect.height) as u32 / 2).max(4));

    let thumb_w = (rect.width as u32 / factor).max(2);
    let thumb_h = (rect.height as u32 / factor).max(2);

    // 1) Crop the region
    let cropped = image::imageops::crop_imm(
        image,
        rect.x.max(0) as u32,
        rect.y.max(0) as u32,
        rect.width as u32,
        rect.height as u32,
    ).to_image();

    // 2) Downsample aggressively (destroys detail — irreversible)
    let thumb = image::imageops::resize(
        &cropped,
        thumb_w,
        thumb_h,
        image::imageops::FilterType::Triangle,
    );

    // 3) Upsample back to original size
    let upscaled = image::imageops::resize(
        &thumb,
        rect.width as u32,
        rect.height as u32,
        image::imageops::FilterType::Triangle,
    );

    // 4) Apply multiple blur passes to produce a smooth, blurred appearance.
    //    More passes = more secure-looking (less structure visible).
    let blur_radius = (amount / 3.0).max(2.0);
    let passes = if amount > 15.0 { 3 } else if amount > 8.0 { 2 } else { 1 };
    let mut blurred = upscaled;
    for _ in 0..passes {
        apply_blur_rect(
            &mut blurred,
            Rect { x: 0, y: 0, width: rect.width, height: rect.height },
            blur_radius,
        );
    }

    // 5) Write back — force alpha=255 to prevent checkerboard bleed
    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    for y in 0..rect.height as u32 {
        for x in 0..rect.width as u32 {
            if x0 + x < image.width() && y0 + y < image.height() {
                let mut p = *blurred.get_pixel(x, y);
                p[3] = 255;
                image.put_pixel(x0 + x, y0 + y, p);
            }
        }
    }
}

/// Apply blackout effect to a rectangular region (solid black fill).
pub fn apply_blackout_rect(image: &mut RgbaImage, rect: &Rect) {
    let x = rect.x.max(0) as u32;
    let y = rect.y.max(0) as u32;
    let width = rect.width as u32;
    let height = rect.height as u32;

    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px < image.width() && py < image.height() {
                image.put_pixel(px, py, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
}

pub fn apply_focus_rect(image: &mut RgbaImage, rect: Rect) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    let image_width = image.width();
    let image_height = image.height();
    if image_width == 0 || image_height == 0 {
        return;
    }

    let x0 = rect.x.max(0) as u32;
    let y0 = rect.y.max(0) as u32;
    let x1 = (rect.x + rect.width).max(0) as u32;
    let y1 = (rect.y + rect.height).max(0) as u32;

    darken_region(image, 0, 0, image_width, y0);
    darken_region(image, 0, y1, image_width, image_height);
    darken_region(image, 0, y0, x0, y1);
    darken_region(image, x1, y0, image_width, y1);
}

fn darken_region(image: &mut RgbaImage, x_start: u32, y_start: u32, x_end: u32, y_end: u32) {
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    const KEEP_NUMERATOR: u16 = 42;
    const KEEP_DENOMINATOR: u16 = 100;

    for y in y_start..y_end {
        for x in x_start..x_end {
            let pixel = image.get_pixel_mut(x, y);
            pixel[0] = ((pixel[0] as u16 * KEEP_NUMERATOR) / KEEP_DENOMINATOR) as u8;
            pixel[1] = ((pixel[1] as u16 * KEEP_NUMERATOR) / KEEP_DENOMINATOR) as u8;
            pixel[2] = ((pixel[2] as u16 * KEEP_NUMERATOR) / KEEP_DENOMINATOR) as u8;
        }
    }
}

pub fn draw_canvas_checkerboard_background(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    tint: Option<DrawColor>,
) {
    fn blend_channel(base: f64, overlay: f64, alpha: f64) -> f64 {
        base * (1.0 - alpha) + overlay * alpha
    }

    let tile_size = 14.0;
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;

    let base_dark = (0.075, 0.075, 0.081);
    let tile_dark = (0.095, 0.095, 0.102);
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

    context.set_source_rgb(base_r, base_g, base_b);
    context.rectangle(0.0, 0.0, width, height);
    let _ = context.fill();

    context.set_source_rgb(tile_r, tile_g, tile_b);
    let cols = (width / tile_size).ceil() as i32 + 1;
    let rows = (height / tile_size).ceil() as i32 + 1;
    for row in 0..rows {
        for col in 0..cols {
            if (row + col) % 2 == 0 {
                context.rectangle(
                    col as f64 * tile_size,
                    row as f64 * tile_size,
                    tile_size,
                    tile_size,
                );
            }
        }
    }
    let _ = context.fill();
}

use rayon::prelude::*;

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
