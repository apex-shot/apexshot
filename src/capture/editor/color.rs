use super::types::{DrawColor, PersistedCustomColor, PersistedCustomColorSlots};
use std::path::PathBuf;

pub const STROKE_WIDTH: f64 = 3.0;
pub const HIGHLIGHTER_STROKE_WIDTH: f64 = 11.0;
pub const HIGHLIGHTER_ALPHA_SCALE: f64 = 0.42;
pub const MIN_STROKE_SIZE: f64 = 1.0;
pub const MAX_STROKE_SIZE: f64 = 24.0;
pub const STROKE_SIZE_STEP: f64 = 1.0;
pub const TEXT_SIZE: f64 = 26.0;
pub const MIN_TEXT_SIZE: f64 = 10.0;
pub const MAX_TEXT_SIZE: f64 = 120.0;
pub const TEXT_SIZE_STEP: f64 = 2.0;
pub const NUMBER_RADIUS: f64 = 15.0;
pub const NUMBER_FONT_SIZE: f64 = 14.0;
pub const BLUR_RADIUS: i32 = 6;
pub const CENSOR_BLOCK_SIZE: u32 = 12;
pub const DRAG_REDRAW_INTERVAL_US: i64 = 16_000;
pub const DEFAULT_COLOR_INDEX: usize = 0;
pub const SELECT_HIT_PADDING: f64 = 8.0;
pub const SELECT_HANDLE_SIZE: f64 = 8.0;
pub const SELECT_HANDLE_HIT_RADIUS: f64 = 9.0;
pub const SELECT_MIN_RESIZE_SIZE: f64 = 2.0;
pub const CUSTOM_COLORS_CONFIG_FILE: &str = "editor_custom_colors.yml";

pub const DRAW_COLORS: [DrawColor; 12] = [
    DrawColor::new(0.07, 0.07, 0.07, 1.00), // Black
    DrawColor::new(0.04, 0.52, 1.00, 0.95), // Blue
    DrawColor::new(0.00, 0.35, 0.20, 0.95), // Dark Green
    DrawColor::new(0.92, 0.14, 0.14, 0.95), // Red
    DrawColor::new(1.00, 0.60, 0.00, 0.95), // Orange
    DrawColor::new(1.00, 0.84, 0.04, 0.95), // Yellow
    DrawColor::new(0.16, 0.73, 0.36, 0.95), // Green
    DrawColor::new(0.00, 0.81, 0.78, 0.95), // Cyan
    DrawColor::new(0.20, 0.56, 0.98, 0.95), // Blue Bright
    DrawColor::new(0.62, 0.36, 0.98, 0.95), // Purple
    DrawColor::new(1.00, 0.08, 0.47, 0.95), // Pink
    DrawColor::new(0.96, 0.96, 0.96, 0.98), // White
];

pub fn color_distance_squared(a: DrawColor, b: DrawColor) -> f64 {
    let dr = a.r - b.r;
    let dg = a.g - b.g;
    let db = a.b - b.b;
    let da = a.a - b.a;
    dr * dr + dg * dg + db * db + da * da
}

pub fn palette_index_for_color(color: DrawColor) -> usize {
    DRAW_COLORS
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            color_distance_squared(color, **left).total_cmp(&color_distance_squared(color, **right))
        })
        .map(|(index, _)| index)
        .unwrap_or(DEFAULT_COLOR_INDEX)
}

pub fn clamp_text_size(size: f64) -> f64 {
    size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE)
}

pub fn clamp_stroke_size(size: f64) -> f64 {
    size.clamp(MIN_STROKE_SIZE, MAX_STROKE_SIZE)
}

pub fn highlighter_stroke_width(stroke_size: f64) -> f64 {
    let base = clamp_stroke_size(stroke_size);
    base * (HIGHLIGHTER_STROKE_WIDTH / STROKE_WIDTH)
}

pub fn draw_color_to_rgba_u8(color: DrawColor) -> (u8, u8, u8, u8) {
    (
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

pub fn draw_color_to_hex(color: DrawColor) -> String {
    let (r, g, b, _) = draw_color_to_rgba_u8(color);
    format!("{r:02X}{g:02X}{b:02X}")
}

pub fn parse_hex_rgb(input: &str) -> Option<(u8, u8, u8)> {
    let value = input.trim().trim_start_matches('#');
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }

    let r = u8::from_str_radix(&value[0..2], 16).ok()?;
    let g = u8::from_str_radix(&value[2..4], 16).ok()?;
    let b = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some((r, g, b))
}

pub fn parse_channel_u8(input: &str) -> Option<u8> {
    input
        .trim()
        .parse::<u16>()
        .ok()
        .and_then(|value| u8::try_from(value).ok())
}

pub fn parse_alpha_percent(input: &str) -> Option<f64> {
    let value = input.trim().parse::<f64>().ok()?;
    if !(0.0..=100.0).contains(&value) {
        return None;
    }
    Some(value / 100.0)
}

pub fn picker_dynamic_css(color: DrawColor) -> String {
    let (r, g, b, _) = draw_color_to_rgba_u8(color);
    let alpha = color.a.clamp(0.0, 1.0);

    format!(
        "
        #editor-picker-preview-hue {{
            background: rgba({r}, {g}, {b}, {alpha:.3});
        }}

        #editor-picker-universal-wheel {{
            background-image: radial-gradient(circle at 30% 28%,
                rgba(255, 255, 255, 0.72) 0%,
                rgba(255, 255, 255, 0.18) 24%,
                rgba({r}, {g}, {b}, 1.0) 100%);
        }}

        #editor-picker-opacity-slider trough {{
            background-image:
                linear-gradient(45deg,
                    rgba(255, 255, 255, 0.14) 25%,
                    rgba(0, 0, 0, 0.0) 25%,
                    rgba(0, 0, 0, 0.0) 75%,
                    rgba(255, 255, 255, 0.14) 75%,
                    rgba(255, 255, 255, 0.14) 100%),
                linear-gradient(45deg,
                    rgba(0, 0, 0, 0.10) 25%,
                    rgba(0, 0, 0, 0.0) 25%,
                    rgba(0, 0, 0, 0.0) 75%,
                    rgba(0, 0, 0, 0.10) 75%,
                    rgba(0, 0, 0, 0.10) 100%),
                linear-gradient(to right,
                    rgba({r}, {g}, {b}, 0.0) 0%,
                    rgba({r}, {g}, {b}, 1.0) 100%);
            background-size: 8px 8px, 8px 8px, 100% 100%;
            background-position: 0 0, 4px 4px, 0 0;
        }}
        "
    )
}

pub fn custom_color_slots_css(colors: &[Option<DrawColor>]) -> String {
    let mut css = String::new();

    for (index, color) in colors.iter().enumerate() {
        let Some(color) = color else {
            continue;
        };

        let (r, g, b, _) = draw_color_to_rgba_u8(*color);
        let alpha = color.a.clamp(0.0, 1.0);
        css.push_str(&format!(
            "
            #editor-custom-color-dot-{index} {{
                background: rgba({r}, {g}, {b}, {alpha:.3});
                border: 1px solid rgba(0, 0, 0, 0.22);
            }}
            "
        ));
    }

    css
}

pub fn persisted_custom_colors_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("cleanshitx");
    path.push(CUSTOM_COLORS_CONFIG_FILE);
    Some(path)
}

pub fn draw_color_to_persisted(color: DrawColor) -> PersistedCustomColor {
    let (r, g, b, a) = draw_color_to_rgba_u8(color);
    PersistedCustomColor { r, g, b, a }
}

pub fn persisted_to_draw_color(color: PersistedCustomColor) -> DrawColor {
    DrawColor::new(
        color.r as f64 / 255.0,
        color.g as f64 / 255.0,
        color.b as f64 / 255.0,
        color.a as f64 / 255.0,
    )
}

pub fn load_persisted_custom_slot_colors(slot_count: usize) -> Vec<Option<DrawColor>> {
    let mut slots = vec![None; slot_count];

    let Some(path) = persisted_custom_colors_path() else {
        return slots;
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return slots;
    };

    let Ok(stored) = serde_yml::from_str::<PersistedCustomColorSlots>(&raw) else {
        return slots;
    };

    for (index, color) in stored.slots.into_iter().take(slot_count).enumerate() {
        slots[index] = color.map(persisted_to_draw_color);
    }

    slots
}

pub fn save_persisted_custom_slot_colors(slots: &[Option<DrawColor>]) {
    let Some(path) = persisted_custom_colors_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let stored = PersistedCustomColorSlots {
        slots: slots
            .iter()
            .copied()
            .map(|color| color.map(draw_color_to_persisted))
            .collect(),
    };

    let Ok(raw) = serde_yml::to_string(&stored) else {
        return;
    };

    let _ = std::fs::write(path, raw);
}

pub fn move_custom_color_between_slots(
    slots: &mut [Option<DrawColor>],
    from_index: usize,
    to_index: usize,
) -> bool {
    if from_index == to_index || from_index >= slots.len() || to_index >= slots.len() {
        return false;
    }

    if slots[from_index].is_none() {
        return false;
    }

    slots.swap(from_index, to_index);
    true
}

pub fn image_space_distance_for_view(view_distance: f64, view_scale: f64) -> f64 {
    view_distance / view_scale.max(0.01)
}

pub fn selection_hit_padding_for_scale(view_scale: f64) -> f64 {
    image_space_distance_for_view(SELECT_HIT_PADDING, view_scale).clamp(SELECT_HIT_PADDING, 96.0)
}

pub fn selection_handle_hit_radius_for_scale(view_scale: f64) -> f64 {
    image_space_distance_for_view(SELECT_HANDLE_HIT_RADIUS, view_scale)
        .clamp(SELECT_HANDLE_HIT_RADIUS, 128.0)
}
