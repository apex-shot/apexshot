use super::color::{
    highlighter_stroke_width, CENSOR_BLOCK_SIZE, HIGHLIGHTER_ALPHA_SCALE, NUMBER_FONT_SIZE,
    NUMBER_RADIUS, SELECT_HANDLE_SIZE,
};
use super::types::{AnnotationAction, DrawColor, Point, Rect, SelectHandle};
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
            font_size,
        } => draw_text(context, *position, text, *color, *font_size),
        AnnotationAction::Number {
            position,
            number,
            color,
        } => draw_number(context, *position, *number, *color),
        AnnotationAction::Blur { .. } => {}
        AnnotationAction::Focus { .. } => {}
        AnnotationAction::Censor { .. } => {}
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
            font_size,
        } => {
            draw_text(context, *position, text, color.with_alpha(0.9), *font_size);
        }
        AnnotationAction::Number {
            position,
            number,
            color,
        } => {
            draw_number(context, *position, *number, color.with_alpha(0.88));
        }
        AnnotationAction::Blur { rect } => {
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
        AnnotationAction::Censor { rect } => {
            draw_censor_draft_rect(context, *rect);
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
    image_width: f64,
    image_height: f64,
    rect: Rect,
    active: bool,
) {
    let Some(rect) = rect.clamp_to(image_width as u32, image_height as u32) else {
        return;
    };

    let x = rect.x as f64;
    let y = rect.y as f64;
    let width = rect.width as f64;
    let height = rect.height as f64;

    if width <= 1.0 || height <= 1.0 {
        return;
    }

    let _ = context.save();
    context.rectangle(0.0, 0.0, image_width, image_height);
    context.rectangle(x, y, width, height);
    context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
    context.set_source_rgba(0.0, 0.0, 0.0, if active { 0.45 } else { 0.35 });
    let _ = context.fill();
    let _ = context.restore();

    let _ = context.save();
    context.rectangle(x, y, width, height);
    context.set_line_width(if active { 2.0 } else { 1.4 });
    context.set_source_rgba(1.0, 1.0, 1.0, 0.95);
    let _ = context.stroke();

    context.set_source_rgba(1.0, 1.0, 1.0, 0.42);
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
    let _ = context.restore();
}

pub fn draw_selection_outline(context: &gtk4::cairo::Context, rect: Rect, view_scale: f64) {
    let scale = view_scale.max(0.01);
    let width = rect.width.max(1) as f64;
    let height = rect.height.max(1) as f64;
    let x = rect.x as f64;
    let y = rect.y as f64;

    let _ = context.save();
    context.set_line_width(1.8 / scale);
    context.rectangle(x, y, width, height);
    context.set_source_rgba(0.96, 0.98, 1.0, 0.96);
    let _ = context.stroke_preserve();

    context.set_line_width(1.2 / scale);
    context.set_dash(&[5.0 / scale, 4.0 / scale], 0.0);
    context.set_source_rgba(0.14, 0.58, 0.98, 0.95);
    let _ = context.stroke();
    context.set_dash(&[], 0.0);
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
        let size = if is_active {
            (SELECT_HANDLE_SIZE + 1.6) / scale
        } else {
            SELECT_HANDLE_SIZE / scale
        };
        let half = size / 2.0;

        context.rectangle(center.x - half, center.y - half, size, size);
        context.set_source_rgba(0.99, 1.0, 1.0, 0.98);
        let _ = context.fill_preserve();
        context.set_source_rgba(0.14, 0.58, 0.98, if is_active { 1.0 } else { 0.92 });
        context.set_line_width(if is_active { 2.0 / scale } else { 1.6 / scale });
        let _ = context.stroke();
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
    font_size: f64,
) {
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.select_font_face(
        "Sans",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    context.set_font_size(font_size.max(1.0));
    context.move_to(position.x, position.y);
    let _ = context.show_text(text);
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

pub fn apply_blur_rect(image: &mut RgbaImage, rect: Rect, radius: i32) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if radius <= 0 {
        return;
    }

    let image_width = image.width() as i32;
    let image_height = image.height() as i32;
    let radius = radius.max(1);

    let sample_x0 = (rect.x - radius).max(0);
    let sample_y0 = (rect.y - radius).max(0);
    let sample_x1 = (rect.x + rect.width - 1 + radius).min(image_width - 1);
    let sample_y1 = (rect.y + rect.height - 1 + radius).min(image_height - 1);

    let sample_width = (sample_x1 - sample_x0 + 1) as usize;
    let sample_height = (sample_y1 - sample_y0 + 1) as usize;
    let stride = sample_width + 1;

    let mut sample_pixels = Vec::with_capacity(sample_width * sample_height);
    for source_y in sample_y0..=sample_y1 {
        for source_x in sample_x0..=sample_x1 {
            sample_pixels.push(*image.get_pixel(source_x as u32, source_y as u32));
        }
    }

    let mut integral = vec![[0_u64; 4]; (sample_height + 1) * stride];

    for local_y in 0..sample_height {
        let mut row_sum = [0_u64; 4];

        for local_x in 0..sample_width {
            let pixel = sample_pixels[local_y * sample_width + local_x];
            row_sum[0] += pixel[0] as u64;
            row_sum[1] += pixel[1] as u64;
            row_sum[2] += pixel[2] as u64;
            row_sum[3] += pixel[3] as u64;

            let idx = (local_y + 1) * stride + (local_x + 1);
            let above = integral[idx - stride];
            integral[idx][0] = above[0] + row_sum[0];
            integral[idx][1] = above[1] + row_sum[1];
            integral[idx][2] = above[2] + row_sum[2];
            integral[idx][3] = above[3] + row_sum[3];
        }
    }

    for y in rect.y..(rect.y + rect.height) {
        let y0 = (y - radius).max(0);
        let y1 = (y + radius).min(image_height - 1);
        let local_y0 = (y0 - sample_y0) as usize;
        let local_y1 = (y1 - sample_y0) as usize;

        for x in rect.x..(rect.x + rect.width) {
            let x0 = (x - radius).max(0);
            let x1 = (x + radius).min(image_width - 1);
            let local_x0 = (x0 - sample_x0) as usize;
            let local_x1 = (x1 - sample_x0) as usize;

            let top_left = local_y0 * stride + local_x0;
            let top_right = local_y0 * stride + (local_x1 + 1);
            let bottom_left = (local_y1 + 1) * stride + local_x0;
            let bottom_right = (local_y1 + 1) * stride + (local_x1 + 1);

            let area = ((local_x1 - local_x0 + 1) * (local_y1 - local_y0 + 1)) as i64;
            let mut blurred = [0_u8; 4];
            for channel in 0..4 {
                let sum = integral[bottom_right][channel] as i64
                    + integral[top_left][channel] as i64
                    - integral[top_right][channel] as i64
                    - integral[bottom_left][channel] as i64;
                blurred[channel] = (sum / area).clamp(0, 255) as u8;
            }

            image.put_pixel(x as u32, y as u32, image::Rgba(blurred));
        }
    }
}

pub fn apply_censor_rect(image: &mut RgbaImage, rect: Rect, block_size: u32) {
    let Some(rect) = rect.clamp_to(image.width(), image.height()) else {
        return;
    };

    if block_size == 0 {
        return;
    }

    let source = image.clone();
    let block = block_size as i32;
    let max_y = rect.y + rect.height;
    let max_x = rect.x + rect.width;

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

            for y in by..(by + block_height) {
                for x in bx..(bx + block_width) {
                    let p = source.get_pixel(x as u32, y as u32);
                    r_sum += p[0] as u32;
                    g_sum += p[1] as u32;
                    b_sum += p[2] as u32;
                    a_sum += p[3] as u32;
                    count += 1;
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
                        image.put_pixel(x as u32, y as u32, color);
                    }
                }
            }

            bx += block;
        }

        by += block;
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
) {
    let tile_size = 14.0;
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;

    context.set_source_rgb(0.075, 0.075, 0.081);
    context.rectangle(0.0, 0.0, width, height);
    let _ = context.fill();

    context.set_source_rgb(0.095, 0.095, 0.102);
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

pub fn rgba_to_cairo_argb_bytes(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((image.width() * image.height() * 4) as usize);
    for pixel in image.pixels() {
        let r = pixel[0] as u32;
        let g = pixel[1] as u32;
        let b = pixel[2] as u32;
        let a = pixel[3] as u32;

        let pr = ((r * a + 127) / 255) as u8;
        let pg = ((g * a + 127) / 255) as u8;
        let pb = ((b * a + 127) / 255) as u8;

        out.push(pb);
        out.push(pg);
        out.push(pr);
        out.push(a as u8);
    }
    out
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
