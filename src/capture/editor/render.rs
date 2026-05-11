use super::color::{highlighter_stroke_width, CENSOR_BLOCK_SIZE, HIGHLIGHTER_ALPHA_SCALE};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::types::{
    AnnotationAction, ArrowStyle, DrawColor, FontSettings, FontStyle, MoveHandle, Point, Rect,
    SelectHandle, TextAlignment, TextDecoration, TextEditBounds,
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

    #[test]
    fn focus_effect_uses_configurable_intensity() {
        let rect = Rect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        };
        let mut low = RgbaImage::from_pixel(8, 8, image::Rgba([200, 180, 160, 255]));
        let mut high = low.clone();

        apply_focus_rect(&mut low, rect, 20.0);
        apply_focus_rect(&mut high, rect, 80.0);

        let low_outside = low.get_pixel(0, 0);
        let high_outside = high.get_pixel(0, 0);
        let inside = high.get_pixel(3, 3);

        assert!(high_outside[0] < low_outside[0]);
        assert!(high_outside[1] < low_outside[1]);
        assert!(high_outside[2] < low_outside[2]);
        assert_eq!(*inside, image::Rgba([200, 180, 160, 255]));
    }
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
            draw_text_with_shadow(
                context,
                *position,
                text,
                *color,
                font,
                Some(available_width),
                *shadow,
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
            shadow,
        } => {
            draw_circle_with_shadow(
                context,
                *rect,
                color.with_alpha(0.82),
                *stroke_size,
                *shadow,
            );
        }
        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
            shadow,
        } => {
            draw_line_with_shadow(
                context,
                *start,
                *end,
                color.with_alpha(0.82),
                *stroke_size,
                *shadow,
            );
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
                color.with_alpha(0.82),
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
            draw_box_with_shadow(
                context,
                *rect,
                color.with_alpha(0.82),
                *stroke_size,
                *shadow,
            );
        }
        AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            shadow,
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
            draw_text_with_shadow(
                context,
                *position,
                text,
                color.with_alpha(0.9),
                font,
                Some(available_width),
                *shadow,
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

fn selection_outline_stroke_width(view_scale: f64) -> f64 {
    TEXT_EDIT_BORDER_WIDTH / view_scale.max(0.01)
}

pub fn draw_arrow_selection_outline(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
    view_scale: f64,
) {
    let _ = context.save();

    let path_built = match style {
        ArrowStyle::Double => {
            build_double_arrow_path(context, start, end, stroke_size, &control_points)
        }
        ArrowStyle::Fancy => {
            build_thorn_arrow_path(context, start, end, stroke_size, false, &control_points)
        }
        _ => {
            // Standard and Curved both use is_smooth = true
            build_thorn_arrow_path(context, start, end, stroke_size, true, &control_points)
        }
    };

    if path_built {
        let scale = view_scale.max(0.01);
        // Slightly expand the stroke so it sits just outside the fill
        context.set_line_width(
            (stroke_size.max(0.5) * 0.2 + 1.0) + selection_outline_stroke_width(scale) * 2.0,
        );
        context.set_line_join(gtk4::cairo::LineJoin::Round);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        context.set_dash(&[], 0.0);
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            0.9,
        );
        let _ = context.stroke();
    }

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

const MOVE_HANDLE_RADIUS: f64 = 7.0;
const MOVE_HANDLE_OUTLINE_WIDTH: f64 = 2.0;
const RESIZE_HANDLE_SIZE: f64 = 10.0;

pub fn text_action_bounds(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    font: &FontSettings,
    max_width: Option<f64>,
) -> TextEditBounds {
    let padding_x = 10.0;
    let padding_y = 8.0;
    let content_width = max_width
        .map(|width| (width - padding_x * 2.0).max(font.size * 0.8))
        .unwrap_or_else(|| measure_text_width(context, text, font).max(font.size * 1.8));
    let layout = layout_wrapped_text(context, text, font, content_width);
    let line_height = (font.size * 1.2).max(font.size + 4.0);
    let width = (layout.max_width + padding_x * 2.0).max(font.size * 1.8);
    let height =
        (layout.lines.len().max(1) as f64 * line_height + font.size * 0.2 + padding_y * 2.0)
            .max(44.0);
    let top_left = Point {
        x: position.x,
        y: position.y - font.size - padding_y,
    };
    TextEditBounds::new(top_left, width, height)
}

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

fn shadow_color_for(color: DrawColor) -> DrawColor {
    DrawColor::new(0.0, 0.0, 0.0, (color.a * 0.35).clamp(0.12, 0.35))
}

fn draw_shadow_layer(
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
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke_size.max(0.5) + 0.4);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.move_to(start.x, start.y);
    context.line_to(end.x, end.y);
    let _ = context.stroke();
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

fn draw_arrow_head(
    context: &gtk4::cairo::Context,
    tip: Point,
    angle: f64,
    head_length: f64,
    spread: f64,
    color: DrawColor,
) {
    let left_x = tip.x - head_length * (angle - spread).cos();
    let left_y = tip.y - head_length * (angle - spread).sin();
    let right_x = tip.x - head_length * (angle + spread).cos();
    let right_y = tip.y - head_length * (angle + spread).sin();

    context.move_to(tip.x, tip.y);
    context.line_to(left_x, left_y);
    context.line_to(right_x, right_y);
    context.close_path();
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();
}

fn bezier_point(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let u = 1.0 - t;
    Point {
        x: u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
        y: u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
    }
}

fn bezier_tangent(p0: Point, p1: Point, p2: Point, t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let dx = 2.0 * u * (p1.x - p0.x) + 2.0 * t * (p2.x - p1.x);
    let dy = 2.0 * u * (p1.y - p0.y) + 2.0 * t * (p2.y - p1.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.0001 {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

fn build_thorn_arrow_path(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    is_smooth: bool,
    control_points: &Option<Vec<Point>>,
) -> bool {
    let outline = thorn_arrow_outline_points(start, end, stroke_size, is_smooth, control_points);
    if outline.is_empty() {
        return false;
    }
    context.new_path();
    context.move_to(outline[0].x, outline[0].y);
    for pt in &outline[1..] {
        context.line_to(pt.x, pt.y);
    }
    context.close_path();
    true
}

pub fn thorn_arrow_outline_points(
    start: Point,
    end: Point,
    stroke_size: f64,
    is_smooth: bool,
    control_points: &Option<Vec<Point>>,
) -> Vec<Point> {
    let is_curved = control_points
        .as_ref()
        .map(|v| v.len() >= 3)
        .unwrap_or(false);
    let p0 = start;
    let p2 = end;
    let p1 = control_points
        .as_ref()
        .and_then(|c| c.get(1).copied())
        .unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });

    let mut line_length = 0.0;
    if is_curved {
        let mut prev = p0;
        for i in 1..=20 {
            let t = i as f64 / 20.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dx = pt.x - prev.x;
            let dy = pt.y - prev.y;
            line_length += (dx * dx + dy * dy).sqrt();
            prev = pt;
        }
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        line_length = (dx * dx + dy * dy).sqrt();
    }

    if line_length < 0.1 {
        return Vec::new();
    }

    let stroke = stroke_size.max(0.5);
    let w = stroke * 3.0 + 3.0;
    let h = (stroke * 6.0 + 10.0)
        .clamp(12.0, 120.0)
        .min((line_length * 0.75).max(8.0));
    let w_h = h * 1.15;
    let s = h * 0.35;

    let t_neck = if is_curved {
        let mut best_t = 0.0;
        for i in (0..=100).rev() {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p2.x - pt.x) * (p2.x - pt.x) + (p2.y - pt.y) * (p2.y - pt.y)).sqrt();
            if dist >= h {
                best_t = t;
                break;
            }
        }
        best_t
    } else {
        1.0 - (h / line_length).min(1.0)
    };

    let (vx, vy) = if is_curved {
        bezier_tangent(p0, p1, p2, 1.0)
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        (dx / line_length, dy / line_length)
    };
    let nx = -vy;
    let ny = vx;

    let neck_pt = if is_curved {
        bezier_point(p0, p1, p2, t_neck)
    } else {
        Point {
            x: end.x - vx * h,
            y: end.y - vy * h,
        }
    };

    let wing_center_x = neck_pt.x - vx * s;
    let wing_center_y = neck_pt.y - vy * s;
    let right_wing_x = wing_center_x + nx * (w_h / 2.0);
    let right_wing_y = wing_center_y + ny * (w_h / 2.0);
    let left_wing_x = wing_center_x - nx * (w_h / 2.0);
    let left_wing_y = wing_center_y - ny * (w_h / 2.0);

    let (tail_vx, tail_vy) = if is_curved {
        bezier_tangent(p0, p1, p2, 0.0)
    } else {
        (vx, vy)
    };
    let tail_angle = tail_vy.atan2(tail_vx);

    let mut outline = Vec::new();

    if is_curved {
        let steps = 20;
        let mut left_body = Vec::new();
        let mut right_body = Vec::new();

        for i in 0..=steps {
            let t = (i as f64 / steps as f64) * t_neck;
            let pt = bezier_point(p0, p1, p2, t);
            let (tvx, tvy) = bezier_tangent(p0, p1, p2, t);
            let tnx = -tvy;
            let tny = tvx;
            let current_w = w * (t / t_neck);
            left_body.push(Point {
                x: pt.x - tnx * current_w / 2.0,
                y: pt.y - tny * current_w / 2.0,
            });
            right_body.push(Point {
                x: pt.x + tnx * current_w / 2.0,
                y: pt.y + tny * current_w / 2.0,
            });
        }

        for pt in right_body.iter().rev() {
            outline.push(*pt);
        }

        if is_smooth {
            let tail_r = (stroke * 0.5).max(1.0);
            let tail_cx = p0.x + tail_vx * tail_r;
            let tail_cy = p0.y + tail_vy * tail_r;
            let arc_start = tail_angle + std::f64::consts::FRAC_PI_2;
            let arc_end = tail_angle + 3.0 * std::f64::consts::FRAC_PI_2;
            let arc_steps = 12;
            for i in 0..=arc_steps {
                let a = arc_start + (i as f64 / arc_steps as f64) * (arc_end - arc_start);
                outline.push(Point {
                    x: tail_cx + tail_r * a.cos(),
                    y: tail_cy + tail_r * a.sin(),
                });
            }
        } else {
            outline.push(p0);
        }

        for pt in left_body.iter().skip(1) {
            outline.push(*pt);
        }
    } else {
        let right_neck_x = neck_pt.x + nx * (w / 2.0);
        let right_neck_y = neck_pt.y + ny * (w / 2.0);
        let left_neck_x = neck_pt.x - nx * (w / 2.0);
        let left_neck_y = neck_pt.y - ny * (w / 2.0);

        outline.push(Point {
            x: right_neck_x,
            y: right_neck_y,
        });

        if is_smooth {
            let tail_r = (stroke * 0.5).max(1.0);
            let tail_cx = p0.x + tail_vx * tail_r;
            let tail_cy = p0.y + tail_vy * tail_r;
            let arc_start = tail_angle + std::f64::consts::FRAC_PI_2;
            let arc_end = tail_angle + 3.0 * std::f64::consts::FRAC_PI_2;
            let arc_steps = 12;
            for i in 0..=arc_steps {
                let a = arc_start + (i as f64 / arc_steps as f64) * (arc_end - arc_start);
                outline.push(Point {
                    x: tail_cx + tail_r * a.cos(),
                    y: tail_cy + tail_r * a.sin(),
                });
            }
        } else {
            outline.push(p0);
        }

        outline.push(Point {
            x: left_neck_x,
            y: left_neck_y,
        });
    }

    outline.push(Point {
        x: left_wing_x,
        y: left_wing_y,
    });

    if is_smooth {
        let head_r = (stroke * 0.4).max(1.0);
        let head_cx = end.x - vx * head_r;
        let head_cy = end.y - vy * head_r;
        let left_dx = head_cx - left_wing_x;
        let left_dy = head_cy - left_wing_y;
        let left_angle = left_dy.atan2(left_dx) - std::f64::consts::FRAC_PI_2;
        let right_dx = head_cx - right_wing_x;
        let right_dy = head_cy - right_wing_y;
        let right_angle = right_dy.atan2(right_dx) + std::f64::consts::FRAC_PI_2;
        let arc_steps = 12;
        for i in 0..=arc_steps {
            let a = left_angle + (i as f64 / arc_steps as f64) * (right_angle - left_angle);
            outline.push(Point {
                x: head_cx + head_r * a.cos(),
                y: head_cy + head_r * a.sin(),
            });
        }
    } else {
        outline.push(end);
    }

    outline.push(Point {
        x: right_wing_x,
        y: right_wing_y,
    });

    outline
}

fn draw_thorn_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    is_smooth: bool,
    control_points: Option<Vec<Point>>,
) {
    if !build_thorn_arrow_path(context, start, end, stroke_size, is_smooth, &control_points) {
        return;
    }
    let stroke = stroke_size.max(0.5);

    // Drop shadow
    let shadow_offset = (stroke * 0.4).clamp(1.5, 4.0);
    let _ = context.save();
    context.translate(shadow_offset, shadow_offset + 1.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.35 * color.a);
    let _ = context.fill_preserve();
    let _ = context.restore();

    // Fill
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill_preserve();

    // Outline
    context.set_source_rgba(0.1, 0.1, 0.1, color.a);
    context.set_line_width(stroke * 0.2 + 1.0);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();
}

pub fn double_arrow_outline_points(
    start: Point,
    end: Point,
    stroke_size: f64,
    control_points: &Option<Vec<Point>>,
) -> Vec<Point> {
    let is_curved = control_points.is_some();
    let p0 = start;
    let p2 = end;
    let p1 = control_points
        .as_ref()
        .and_then(|c| c.get(1).copied())
        .unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });

    let mut line_length = 0.0;
    if is_curved {
        let mut prev = p0;
        for i in 1..=20 {
            let t = i as f64 / 20.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dx = pt.x - prev.x;
            let dy = pt.y - prev.y;
            line_length += (dx * dx + dy * dy).sqrt();
            prev = pt;
        }
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        line_length = (dx * dx + dy * dy).sqrt();
    }

    if line_length < 0.1 {
        return Vec::new();
    }

    let stroke = stroke_size.max(0.5);
    let w = stroke * 3.0 + 3.0;
    let h = (stroke * 6.0 + 10.0)
        .clamp(12.0, 120.0)
        .min((line_length * 0.4).max(8.0));
    let w_h = h * 1.15;
    let s = h * 0.35;

    let (t_neck_start, t_neck_end) = if is_curved {
        let mut t_end = 0.0;
        for i in (0..=100).rev() {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p2.x - pt.x) * (p2.x - pt.x) + (p2.y - pt.y) * (p2.y - pt.y)).sqrt();
            if dist >= h {
                t_end = t;
                break;
            }
        }
        let mut t_start = 0.0;
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p0.x - pt.x) * (p0.x - pt.x) + (p0.y - pt.y) * (p0.y - pt.y)).sqrt();
            if dist >= h {
                t_start = t;
                break;
            }
        }
        (t_start, t_end)
    } else {
        ((h / line_length).min(0.5), (1.0 - h / line_length).max(0.5))
    };

    let (vx_e, vy_e) = if is_curved {
        bezier_tangent(p0, p1, p2, 1.0)
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        (dx / line_length, dy / line_length)
    };
    let nx_e = -vy_e;
    let ny_e = vx_e;

    let neck_pt_e = if is_curved {
        bezier_point(p0, p1, p2, t_neck_end)
    } else {
        Point {
            x: end.x - vx_e * h,
            y: end.y - vy_e * h,
        }
    };

    let wing_ce_x = neck_pt_e.x - vx_e * s;
    let wing_ce_y = neck_pt_e.y - vy_e * s;

    let right_wing_e = Point {
        x: wing_ce_x + nx_e * (w_h / 2.0),
        y: wing_ce_y + ny_e * (w_h / 2.0),
    };
    let left_wing_e = Point {
        x: wing_ce_x - nx_e * (w_h / 2.0),
        y: wing_ce_y - ny_e * (w_h / 2.0),
    };

    let (vx_s, vy_s) = if is_curved {
        bezier_tangent(p0, p1, p2, 0.0)
    } else {
        (vx_e, vy_e)
    };
    let nx_s = -vy_s;
    let ny_s = vx_s;

    let neck_pt_s = if is_curved {
        bezier_point(p0, p1, p2, t_neck_start)
    } else {
        Point {
            x: start.x + vx_s * h,
            y: start.y + vy_s * h,
        }
    };

    let wing_cs_x = neck_pt_s.x + vx_s * s;
    let wing_cs_y = neck_pt_s.y + vy_s * s;

    let left_wing_s = Point {
        x: wing_cs_x - nx_s * (w_h / 2.0),
        y: wing_cs_y - ny_s * (w_h / 2.0),
    };
    let right_wing_s = Point {
        x: wing_cs_x + nx_s * (w_h / 2.0),
        y: wing_cs_y + ny_s * (w_h / 2.0),
    };

    let mut outline = Vec::new();
    let steps = 20;

    for i in 0..=steps {
        let t = t_neck_start + (i as f64 / steps as f64) * (t_neck_end - t_neck_start);
        let (pt, tvx, tvy) = if is_curved {
            (
                bezier_point(p0, p1, p2, t),
                bezier_tangent(p0, p1, p2, t).0,
                bezier_tangent(p0, p1, p2, t).1,
            )
        } else {
            (
                Point {
                    x: p0.x + vx_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                    y: p0.y + vy_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                },
                vx_s,
                vy_s,
            )
        };
        let tnx = -tvy;
        let tny = tvx;
        outline.push(Point {
            x: pt.x + tnx * (w / 2.0),
            y: pt.y + tny * (w / 2.0),
        });
    }

    outline.push(right_wing_e);

    let head_r = (stroke * 0.4).max(1.0);
    let head_cx_e = p2.x - vx_e * head_r;
    let head_cy_e = p2.y - vy_e * head_r;
    let left_dx_e = head_cx_e - left_wing_e.x;
    let left_dy_e = head_cy_e - left_wing_e.y;
    let left_angle_e = left_dy_e.atan2(left_dx_e) - std::f64::consts::FRAC_PI_2;
    let right_dx_e = head_cx_e - right_wing_e.x;
    let right_dy_e = head_cy_e - right_wing_e.y;
    let right_angle_e = right_dy_e.atan2(right_dx_e) + std::f64::consts::FRAC_PI_2;
    let arc_steps = 12;
    for i in 0..=arc_steps {
        let a = left_angle_e + (i as f64 / arc_steps as f64) * (right_angle_e - left_angle_e);
        outline.push(Point {
            x: head_cx_e + head_r * a.cos(),
            y: head_cy_e + head_r * a.sin(),
        });
    }

    outline.push(left_wing_e);

    for i in (0..=steps).rev() {
        let t = t_neck_start + (i as f64 / steps as f64) * (t_neck_end - t_neck_start);
        let (pt, tvx, tvy) = if is_curved {
            (
                bezier_point(p0, p1, p2, t),
                bezier_tangent(p0, p1, p2, t).0,
                bezier_tangent(p0, p1, p2, t).1,
            )
        } else {
            (
                Point {
                    x: p0.x + vx_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                    y: p0.y + vy_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                },
                vx_s,
                vy_s,
            )
        };
        let tnx = -tvy;
        let tny = tvx;
        outline.push(Point {
            x: pt.x - tnx * (w / 2.0),
            y: pt.y - tny * (w / 2.0),
        });
    }

    outline.push(left_wing_s);

    let head_cx_s = p0.x + vx_s * head_r;
    let head_cy_s = p0.y + vy_s * head_r;
    let left_dx_s = head_cx_s - left_wing_s.x;
    let left_dy_s = head_cy_s - left_wing_s.y;
    let left_angle_s = left_dy_s.atan2(left_dx_s) + std::f64::consts::FRAC_PI_2;
    let right_dx_s = head_cx_s - right_wing_s.x;
    let right_dy_s = head_cy_s - right_wing_s.y;
    let right_angle_s = right_dy_s.atan2(right_dx_s) - std::f64::consts::FRAC_PI_2;
    for i in 0..=arc_steps {
        let a = left_angle_s + (i as f64 / arc_steps as f64) * (right_angle_s - left_angle_s);
        outline.push(Point {
            x: head_cx_s + head_r * a.cos(),
            y: head_cy_s + head_r * a.sin(),
        });
    }

    outline.push(right_wing_s);

    outline
}

fn draw_double_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    control_points: Option<Vec<Point>>,
) {
    if !build_double_arrow_path(context, start, end, stroke_size, &control_points) {
        return;
    }
    let stroke = stroke_size.max(0.5);

    // Drop shadow
    let shadow_offset = (stroke * 0.4).clamp(1.5, 4.0);
    let _ = context.save();
    context.translate(shadow_offset, shadow_offset + 1.0);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.35 * color.a);
    let _ = context.fill_preserve();
    let _ = context.restore();

    // Fill
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill_preserve();

    // Outline
    context.set_source_rgba(0.1, 0.1, 0.1, color.a);
    context.set_line_width(stroke * 0.2 + 1.0);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();
}
fn build_double_arrow_path(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    control_points: &Option<Vec<Point>>,
) -> bool {
    let outline = double_arrow_outline_points(start, end, stroke_size, control_points);
    if outline.is_empty() {
        return false;
    }
    context.new_path();
    context.move_to(outline[0].x, outline[0].y);
    for pt in &outline[1..] {
        context.line_to(pt.x, pt.y);
    }
    context.close_path();
    true
}

pub fn draw_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_arrow_internal(
            ctx,
            start,
            end,
            draw_color,
            stroke_size,
            style,
            control_points.clone(),
        );
    });
}

fn draw_arrow_internal(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
) {
    if matches!(
        style,
        ArrowStyle::Fancy | ArrowStyle::Standard | ArrowStyle::Curved | ArrowStyle::Double
    ) {
        if matches!(style, ArrowStyle::Double) {
            draw_double_arrow(context, start, end, color, stroke_size, control_points);
        } else {
            let is_smooth = matches!(style, ArrowStyle::Standard | ArrowStyle::Curved);
            draw_thorn_arrow(
                context,
                start,
                end,
                color,
                stroke_size,
                is_smooth,
                control_points,
            );
        }
        return;
    }

    let stroke = stroke_size.max(0.5);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke + 0.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < 0.1 && dy.abs() < 0.1 {
        return;
    }

    // Draw the line/curve
    match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            if let Some(ref v) = control_points {
                if let Some(mid) = v.get(1) {
                    context.move_to(start.x, start.y);
                    context.curve_to(mid.x, mid.y, mid.x, mid.y, end.x, end.y);
                } else {
                    context.move_to(start.x, start.y);
                    context.line_to(end.x, end.y);
                }
            } else {
                context.move_to(start.x, start.y);
                context.line_to(end.x, end.y);
            }
        }
        _ => {
            context.move_to(start.x, start.y);
            context.line_to(end.x, end.y);
        }
    }
    let _ = context.stroke();

    // Compute arrowhead dimensions
    let angle = dy.atan2(dx);
    let line_length = (dx * dx + dy * dy).sqrt().max(1.0);
    let head_length = (stroke * 4.8)
        .clamp(12.0, 120.0)
        .min((line_length * 0.75).max(8.0));

    let spread = match style {
        ArrowStyle::Fancy => 0.3,
        _ => 0.55,
    };

    // End arrowhead (all styles)
    let end_angle = match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            if let Some(ref v) = control_points {
                if let Some(mid) = v.get(1) {
                    (end.y - mid.y).atan2(end.x - mid.x)
                } else {
                    angle
                }
            } else {
                angle
            }
        }
        _ => angle,
    };
    draw_arrow_head(context, end, end_angle, head_length, spread, color);

    // Start arrowhead (Double only)
    if matches!(style, ArrowStyle::Double) {
        let start_angle = if let Some(ref v) = control_points {
            if let Some(mid) = v.get(1) {
                (start.y - mid.y).atan2(start.x - mid.x) + std::f64::consts::PI
            } else {
                angle + std::f64::consts::PI
            }
        } else {
            angle + std::f64::consts::PI
        };
        draw_arrow_head(context, start, start_angle, head_length, spread, color);
    }
}

pub fn draw_arrow_control_handles(
    context: &gtk4::cairo::Context,
    handles: Vec<Point>,
    color: DrawColor,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);

    if handles.len() >= 3 {
        // Curved/Double: Bezier with 3 handles
        let p0 = handles[0];
        let p1 = handles[1];
        let p2 = handles[2];

        // On-curve midpoint B(0.5) = 0.25*P0 + 0.5*P1 + 0.25*P2
        let mid_on_curve = Point {
            x: 0.25 * p0.x + 0.5 * p1.x + 0.25 * p2.x,
            y: 0.25 * p0.y + 0.5 * p1.y + 0.25 * p2.y,
        };

        // Draw dashed curve from start → mid → end (following the Bezier)
        context.set_source_rgba(color.r, color.g, color.b, 0.4);
        context.set_line_width(1.0 / scale);
        context.set_dash(&[4.0 / scale, 4.0 / scale], 0.0);
        context.move_to(p0.x, p0.y);
        context.curve_to(p1.x, p1.y, p1.x, p1.y, p2.x, p2.y);
        let _ = context.stroke();
        context.set_dash(&[], 0.0);

        // Draw handle circles: start, on-curve mid, end
        let display_handles = [p0, mid_on_curve, p2];
        let _ = context.save();
        for (i, handle) in display_handles.iter().enumerate() {
            let radius = (MOVE_HANDLE_RADIUS + if i == 1 { 1.0 } else { 0.0 }) / scale;
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
            context.arc(handle.x, handle.y, radius, 0.0, std::f64::consts::TAU);
            let _ = context.stroke();
            context.set_source_rgba(
                TEXT_EDIT_BORDER_COLOR.0,
                TEXT_EDIT_BORDER_COLOR.1,
                TEXT_EDIT_BORDER_COLOR.2,
                1.0,
            );
            context.arc(
                handle.x,
                handle.y,
                (radius - MOVE_HANDLE_OUTLINE_WIDTH / scale).max(1.0 / scale),
                0.0,
                std::f64::consts::TAU,
            );
            let _ = context.fill();
        }
        let _ = context.restore();
    } else if handles.len() == 2 {
        // Standard/Fancy: 2 handles at head and tail
        let p0 = handles[0];
        let p1 = handles[1];

        // Draw dashed line from start to end
        context.set_source_rgba(color.r, color.g, color.b, 0.4);
        context.set_line_width(1.0 / scale);
        context.set_dash(&[4.0 / scale, 4.0 / scale], 0.0);
        context.move_to(p0.x, p0.y);
        context.line_to(p1.x, p1.y);
        let _ = context.stroke();
        context.set_dash(&[], 0.0);

        // Draw handle circles at start and end
        let display_handles = [p0, p1];
        let _ = context.save();
        for handle in &display_handles {
            let radius = MOVE_HANDLE_RADIUS / scale;
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
            context.arc(handle.x, handle.y, radius, 0.0, std::f64::consts::TAU);
            let _ = context.stroke();
            context.set_source_rgba(
                TEXT_EDIT_BORDER_COLOR.0,
                TEXT_EDIT_BORDER_COLOR.1,
                TEXT_EDIT_BORDER_COLOR.2,
                1.0,
            );
            context.arc(
                handle.x,
                handle.y,
                (radius - MOVE_HANDLE_OUTLINE_WIDTH / scale).max(1.0 / scale),
                0.0,
                std::f64::consts::TAU,
            );
            let _ = context.fill();
        }
        let _ = context.restore();
    }
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

fn apply_font_settings(context: &gtk4::cairo::Context, font: &FontSettings) {
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
}

pub fn measure_text_width(context: &gtk4::cairo::Context, text: &str, font: &FontSettings) -> f64 {
    let _ = context.save();
    apply_font_settings(context, font);
    let width = context
        .text_extents(text)
        .map(|extents| extents.x_advance().max(extents.width()))
        .unwrap_or(0.0);
    let _ = context.restore();
    width
}

#[derive(Debug, Clone)]
pub struct TextLayoutLine {
    pub text: String,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone)]
pub struct TextLayout {
    pub lines: Vec<TextLayoutLine>,
    pub max_width: f64,
}

pub fn layout_wrapped_text(
    context: &gtk4::cairo::Context,
    text: &str,
    font: &FontSettings,
    max_width: f64,
) -> TextLayout {
    let allowed_width = max_width.max(font.size * 0.8).max(1.0);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_start = 0usize;
    let mut current_end = 0usize;
    let mut max_line_width: f64 = 0.0;

    for (char_idx, ch) in text.chars().enumerate() {
        if ch == '\n' {
            max_line_width = max_line_width.max(measure_text_width(context, &current, font));
            lines.push(TextLayoutLine {
                text: current.clone(),
                start_char: current_start,
                end_char: current_end,
            });
            current.clear();
            current_start = char_idx + 1;
            current_end = char_idx + 1;
            continue;
        }

        let mut candidate = current.clone();
        candidate.push(ch);
        let candidate_width = measure_text_width(context, &candidate, font);

        if !current.is_empty() && candidate_width > allowed_width {
            max_line_width = max_line_width.max(measure_text_width(context, &current, font));
            lines.push(TextLayoutLine {
                text: current.clone(),
                start_char: current_start,
                end_char: current_end,
            });
            current.clear();
            current.push(ch);
            current_start = char_idx;
            current_end = char_idx + 1;
        } else {
            current = candidate;
            current_end = char_idx + 1;
            max_line_width = max_line_width.max(candidate_width.min(allowed_width));
        }
    }

    if !current.is_empty() || lines.is_empty() {
        max_line_width = max_line_width.max(measure_text_width(context, &current, font));
        lines.push(TextLayoutLine {
            text: current,
            start_char: current_start,
            end_char: current_end,
        });
    }

    TextLayout {
        lines,
        max_width: max_line_width.min(allowed_width),
    }
}

pub fn draw_text(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    apply_font_settings(context, font);

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

pub fn draw_wrapped_text(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
    max_width: Option<f64>,
) {
    if let Some(max_width) = max_width {
        let layout = layout_wrapped_text(context, text, font, max_width.max(1.0));
        let line_height = (font.size * 1.2).max(font.size + 4.0);
        for (index, line) in layout.lines.iter().enumerate() {
            draw_text(
                context,
                Point {
                    x: position.x,
                    y: position.y + index as f64 * line_height,
                },
                &line.text,
                color,
                font,
            );
        }
    } else {
        draw_text(context, position, text, color, font);
    }
}

fn draw_text_with_shadow(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
    max_width: Option<f64>,
    shadow: bool,
) {
    draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_wrapped_text(ctx, position, text, draw_color, font, max_width);
    });
}

pub fn cursor_position_for_text_point(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    text: &str,
    font: &FontSettings,
    point: Point,
) -> usize {
    let padding_x = 10.0;
    let padding_y = 8.0;
    let line_height = (font.size * 1.2).max(font.size + 4.0);
    let content_width = (bounds.rect.width as f64 - padding_x * 2.0).max(1.0);
    let layout = layout_wrapped_text(context, text, font, content_width);

    if layout.lines.is_empty() {
        return 0;
    }

    let relative_y = (point.y - bounds.rect.y as f64 - padding_y).max(0.0);
    let line_index =
        ((relative_y / line_height).floor() as usize).min(layout.lines.len().saturating_sub(1));
    let line = &layout.lines[line_index];
    let relative_x = (point.x - bounds.rect.x as f64 - padding_x).max(0.0);

    let mut best_position = line.start_char;
    let mut best_distance = f64::INFINITY;
    let char_count = line.text.chars().count();

    for column in 0..=char_count {
        let prefix: String = line.text.chars().take(column).collect();
        let caret_x = measure_text_width(context, &prefix, font);
        let distance = (caret_x - relative_x).abs();
        if distance < best_distance {
            best_distance = distance;
            best_position = line.start_char + column;
        }
    }

    best_position.min(text.chars().count())
}

pub fn draw_active_text_input(
    context: &gtk4::cairo::Context,
    bounds: &TextEditBounds,
    text: &str,
    cursor_position: usize,
    cursor_visible: bool,
    color: DrawColor,
    font: &FontSettings,
) {
    let _ = context.save();

    // Inset the clip rect by the border half-width so text is always drawn
    // inside the border stroke, never underneath it.
    let inset = TEXT_EDIT_BORDER_WIDTH / 2.0 + 1.0;
    context.rectangle(
        bounds.rect.x as f64 + inset,
        bounds.rect.y as f64 + inset,
        (bounds.rect.width.max(1) as f64 - inset * 2.0).max(1.0),
        (bounds.rect.height.max(1) as f64 - inset * 2.0).max(1.0),
    );
    context.clip();

    let padding_x = 10.0;
    let padding_y = 8.0;
    let line_height = (font.size * 1.2).max(font.size + 4.0);
    let content_width = (bounds.rect.width as f64 - padding_x * 2.0).max(1.0);
    let layout = layout_wrapped_text(context, text, font, content_width);
    let clamped_cursor = cursor_position.min(text.chars().count());
    let mut cursor_line = 0usize;
    let mut cursor_column = 0usize;
    let text_block_height = (layout.lines.len().max(1) as f64 * line_height).max(line_height);
    // Center the text block vertically within the inner content area (inset from border).
    let inner_height = (bounds.rect.height as f64 - inset * 2.0).max(1.0);
    let vertical_offset = ((inner_height - text_block_height) / 2.0).max(padding_y);
    let baseline_offset =
        font.size + ((line_height - font.size) / 2.0).max(0.0) - (font.size * 0.12);

    for (index, line) in layout.lines.iter().enumerate() {
        let baseline_y = bounds.rect.y as f64
            + inset
            + vertical_offset
            + baseline_offset
            + index as f64 * line_height;
        draw_text(
            context,
            Point {
                x: bounds.rect.x as f64 + padding_x,
                y: baseline_y,
            },
            &line.text,
            color,
            font,
        );

        if clamped_cursor >= line.start_char && clamped_cursor <= line.end_char {
            cursor_line = index;
            cursor_column = clamped_cursor.saturating_sub(line.start_char);
        }
    }

    if cursor_visible {
        let line = layout
            .lines
            .get(cursor_line)
            .or_else(|| layout.lines.last());
        let prefix: String = line
            .map(|line| line.text.chars().take(cursor_column).collect())
            .unwrap_or_default();
        let cursor_x =
            bounds.rect.x as f64 + padding_x + measure_text_width(context, &prefix, font);
        let top = bounds.rect.y as f64 + inset + vertical_offset + cursor_line as f64 * line_height;
        let bottom = top + font.size.max(line_height - 2.0);
        context.set_source_rgba(color.r, color.g, color.b, color.a.max(0.8));
        context.set_line_width((font.size * 0.04).max(1.5));
        context.move_to(cursor_x, top);
        context.line_to(cursor_x, bottom);
        let _ = context.stroke();
    }

    let _ = context.restore();
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

const BLUR_PERFORMANCE_THRESHOLD: usize = 400 * 400; // 400x400 pixels
const BLUR_DOWNSAMPLE_FACTOR: usize = 4;

pub fn apply_blur_rect(image: &mut RgbaImage, rect: Rect, radius: f64, preserve_alpha: bool) {
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
        apply_blur_rect_downsampled(image, rect, radius, preserve_alpha);
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
            if !preserve_alpha {
                // Prevent alpha artifacts from making the checkerboard show through.
                pixel[3] = 255;
            }
            image.put_pixel(x as u32, y as u32, image::Rgba(pixel));
        }
    }
}

fn apply_blur_rect_downsampled(
    image: &mut RgbaImage,
    rect: Rect,
    radius: f64,
    preserve_alpha: bool,
) {
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
                    if !preserve_alpha {
                        // Prevent any alpha artifacts from making the checkerboard show through.
                        p[3] = 255;
                    }
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

/// Hybrid blur: low values look smoother, while high values become more
/// destructive by downsampling harder before blurring.
pub fn apply_hybrid_blur(image: &mut RgbaImage, rect: Rect, amount: f64) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if rect.width < 2 || rect.height < 2 {
        return;
    }

    let normalized = (amount / 25.0).clamp(0.0, 1.0);

    // Lower values preserve more structure with a gentle blur.
    // Higher values downsample more aggressively so detail is destroyed.
    let base_factor = 2.0 + normalized * 14.0; // 2x at min, 16x at max
    let factor = (base_factor as u32)
        .max(2)
        .min((rect.width.min(rect.height) as u32 / 2).max(2));

    let thumb_w = (rect.width as u32 / factor).max(2);
    let thumb_h = (rect.height as u32 / factor).max(2);

    // 1) Crop the region
    let cropped = image::imageops::crop_imm(
        image,
        rect.x.max(0) as u32,
        rect.y.max(0) as u32,
        rect.width as u32,
        rect.height as u32,
    )
    .to_image();

    // 2) Downsample based on intensity (higher = more destructive)
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

    // 4) Apply blur passes. Low intensity stays soft; high intensity gets
    //    stronger radius and more passes.
    let blur_radius = (1.2 + normalized * 7.8).max(1.2);
    let passes = if amount > 17.0 {
        3
    } else if amount > 8.0 {
        2
    } else {
        1
    };
    let mut blurred = upscaled;
    for _ in 0..passes {
        apply_blur_rect(
            &mut blurred,
            Rect {
                x: 0,
                y: 0,
                width: rect.width,
                height: rect.height,
            },
            blur_radius,
            false,
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

pub fn apply_focus_rect(image: &mut RgbaImage, rect: Rect, intensity: f64) {
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

    darken_region(image, 0, 0, image_width, y0, intensity);
    darken_region(image, 0, y1, image_width, image_height, intensity);
    darken_region(image, 0, y0, x0, y1, intensity);
    darken_region(image, x1, y0, image_width, y1, intensity);
}

fn darken_region(
    image: &mut RgbaImage,
    x_start: u32,
    y_start: u32,
    x_end: u32,
    y_end: u32,
    intensity: f64,
) {
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    let keep_ratio = (1.0 - (intensity / 100.0).clamp(0.10, 0.90)).clamp(0.10, 0.90);

    for y in y_start..y_end {
        for x in x_start..x_end {
            let pixel = image.get_pixel_mut(x, y);
            pixel[0] = (pixel[0] as f64 * keep_ratio).round() as u8;
            pixel[1] = (pixel[1] as f64 * keep_ratio).round() as u8;
            pixel[2] = (pixel[2] as f64 * keep_ratio).round() as u8;
        }
    }
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
        ((0.965, 0.965, 0.972), (0.910, 0.910, 0.922))
    } else {
        ((0.075, 0.075, 0.081), (0.095, 0.095, 0.102))
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
