use super::super::types::{
    DrawColor, FontSettings, FontStyle, MoveHandle, Point, TextAlignment, TextDecoration,
    TextEditBounds,
};
use super::{
    MOVE_HANDLE_OUTLINE_WIDTH, MOVE_HANDLE_RADIUS, RESIZE_HANDLE_SIZE, TEXT_EDIT_BORDER_COLOR,
    TEXT_EDIT_BORDER_RADIUS, TEXT_EDIT_BORDER_WIDTH,
};

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

    let ends_with_newline = text.ends_with('\n');
    if !current.is_empty() || lines.is_empty() || ends_with_newline {
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
fn draw_text_background(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    font: &FontSettings,
    max_width: Option<f64>,
    bg_color: DrawColor,
) {
    let allowed_width = max_width
        .unwrap_or(f64::INFINITY)
        .max(font.size * 0.8)
        .max(1.0);
    let layout = layout_wrapped_text(context, text, font, allowed_width);
    let line_height = (font.size * 1.2).max(font.size + 4.0);
    let total_height = layout.lines.len() as f64 * line_height;
    let pad = 4.0_f64;
    let corner = 3.0_f64;
    let bg_x = position.x - pad;
    let bg_y = position.y - font.size - pad;
    let bg_w = layout.max_width + pad * 2.0;
    let bg_h = total_height + pad * 2.0;
    let r = corner.min(bg_w / 2.0).min(bg_h / 2.0);
    let _ = context.save();
    context.set_source_rgba(bg_color.r, bg_color.g, bg_color.b, bg_color.a);
    context.new_path();
    context.move_to(bg_x + r, bg_y);
    context.line_to(bg_x + bg_w - r, bg_y);
    context.arc(
        bg_x + bg_w - r,
        bg_y + r,
        r,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        bg_x + bg_w - r,
        bg_y + bg_h - r,
        r,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        bg_x + r,
        bg_y + bg_h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        bg_x + r,
        bg_y + r,
        r,
        std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
    let _ = context.fill();
    let _ = context.restore();
}
pub fn draw_wrapped_text(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
    max_width: Option<f64>,
) {
    let allowed_width = max_width
        .unwrap_or(f64::INFINITY)
        .max(font.size * 0.8)
        .max(1.0);
    let layout = layout_wrapped_text(context, text, font, allowed_width);
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
}
pub(super) fn draw_text_with_shadow(
    context: &gtk4::cairo::Context,
    position: Point,
    text: &str,
    color: DrawColor,
    font: &FontSettings,
    max_width: Option<f64>,
    shadow: bool,
    background_color: Option<DrawColor>,
) {
    if let Some(bg) = background_color {
        draw_text_background(context, position, text, font, max_width, bg);
    }
    super::draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
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
