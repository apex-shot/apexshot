use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::color::{
    draw_color_to_rgba_u8, DEFAULT_COLOR_INDEX, DEFAULT_FOCUS_INTENSITY, DEFAULT_OBFUSCATE_AMOUNT,
    DRAW_COLORS, STROKE_WIDTH, TEXT_SIZE,
};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::pen_weight::{HighlighterMode, PenWeight};
use super::state::EditorState;
use super::types::{
    ArrowStyle, BackgroundAlignment, CropAspectRatio, DrawColor, ObfuscateMethod, Tool,
};

const PREFS_FILE: &str = "editor_prefs.yml";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPreferences {
    tool: Tool,
    color: PersistedColor,
    stroke_size: f64,
    text_size: f64,
    text_font_family: String,
    obfuscate_method: ObfuscateMethod,
    obfuscate_pixelate_amount: f64,
    obfuscate_blur_amount: f64,
    focus_intensity: f64,
    arrow_style: ArrowStyle,
    background_padding: f64,
    background_shadow: f64,
    background_insert: f64,
    auto_balance: bool,
    background_alignment: BackgroundAlignment,
    background_corner_radius: f64,
    background_aspect_ratio: CropAspectRatio,
    highlighter_mode: HighlighterMode,
    pen_weight: PenWeight,
    numbering_style: NumberingStyle,
    numbering_start: u32,
    number_size: NumberSize,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        let color = DRAW_COLORS[DEFAULT_COLOR_INDEX];
        let (r, g, b, a) = draw_color_to_rgba_u8(color);
        Self {
            tool: Tool::Background,
            color: PersistedColor { r, g, b, a },
            stroke_size: STROKE_WIDTH,
            text_size: TEXT_SIZE,
            text_font_family: String::from("Sans"),
            obfuscate_method: ObfuscateMethod::Pixelate,
            obfuscate_pixelate_amount: DEFAULT_OBFUSCATE_AMOUNT,
            obfuscate_blur_amount: DEFAULT_OBFUSCATE_AMOUNT,
            focus_intensity: DEFAULT_FOCUS_INTENSITY,
            arrow_style: ArrowStyle::Standard,
            background_padding: 24.0,
            background_shadow: 15.0,
            background_insert: 0.0,
            auto_balance: false,
            background_alignment: BackgroundAlignment::Center,
            background_corner_radius: 18.0,
            background_aspect_ratio: CropAspectRatio::Original,
            highlighter_mode: HighlighterMode::default(),
            pen_weight: PenWeight::default(),
            numbering_style: NumberingStyle::default(),
            numbering_start: 1,
            number_size: NumberSize::default(),
        }
    }
}

impl EditorPreferences {
    pub fn from_state(state: &EditorState) -> Self {
        let (r, g, b, a) = draw_color_to_rgba_u8(state.selected_color);
        Self {
            tool: state.selected_tool,
            color: PersistedColor { r, g, b, a },
            stroke_size: state.stroke_size,
            text_size: state.text_size,
            text_font_family: state.text_font_family.clone(),
            obfuscate_method: state.obfuscate_method,
            obfuscate_pixelate_amount: state.obfuscate_pixelate_amount,
            obfuscate_blur_amount: state.obfuscate_blur_amount,
            focus_intensity: state.focus_intensity,
            arrow_style: state.arrow_style,
            background_padding: state.background_padding,
            background_shadow: state.background_shadow,
            background_insert: state.background_insert,
            auto_balance: state.auto_balance,
            background_alignment: state.background_alignment,
            background_corner_radius: state.background_corner_radius,
            background_aspect_ratio: state.background_aspect_ratio,
            highlighter_mode: state.highlighter_mode,
            pen_weight: state.pen_weight,
            numbering_style: state.numbering_style,
            numbering_start: state.numbering_start,
            number_size: state.number_size,
        }
    }

    pub fn apply_to_state(&self, state: &mut EditorState) {
        state.selected_tool = self.tool;
        state.selected_color = DrawColor::new(
            self.color.r as f64 / 255.0,
            self.color.g as f64 / 255.0,
            self.color.b as f64 / 255.0,
            self.color.a as f64 / 255.0,
        );
        state.stroke_size = self.stroke_size;
        state.text_size = self.text_size;
        state.text_font_family = self.text_font_family.clone();
        state.obfuscate_method = self.obfuscate_method;
        state.obfuscate_pixelate_amount = self.obfuscate_pixelate_amount;
        state.obfuscate_blur_amount = self.obfuscate_blur_amount;
        state.focus_intensity = self.focus_intensity;
        state.arrow_style = self.arrow_style;
        state.background_padding = self.background_padding;
        state.background_shadow = self.background_shadow;
        state.background_insert = self.background_insert;
        state.auto_balance = self.auto_balance;
        state.background_alignment = self.background_alignment;
        state.background_corner_radius = self.background_corner_radius;
        state.background_aspect_ratio = self.background_aspect_ratio;
        state.highlighter_mode = self.highlighter_mode;
        state.pen_weight = self.pen_weight;
        state.numbering_style = self.numbering_style;
        state.numbering_start = self.numbering_start;
        state.number_size = self.number_size;
    }
}

pub fn editor_prefs_path() -> Option<PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("apexshot");
    path.push(PREFS_FILE);
    Some(path)
}

pub fn load_editor_prefs() -> EditorPreferences {
    let Some(path) = editor_prefs_path() else {
        return EditorPreferences::default();
    };

    let Ok(raw) = std::fs::read_to_string(path) else {
        return EditorPreferences::default();
    };

    serde_yml::from_str::<EditorPreferences>(&raw).unwrap_or_default()
}

pub fn save_editor_prefs(prefs: &EditorPreferences) {
    let Some(path) = editor_prefs_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let Ok(raw) = serde_yml::to_string(prefs) else {
        return;
    };

    let _ = std::fs::write(path, raw);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn default_prefs_match_editor_state_defaults() {
        let prefs = EditorPreferences::default();
        let state = EditorState::new(RgbaImage::new(10, 10));

        assert_eq!(prefs.tool, state.selected_tool);
        assert!((prefs.stroke_size - state.stroke_size).abs() < f64::EPSILON);
        assert!((prefs.text_size - state.text_size).abs() < f64::EPSILON);
        assert_eq!(prefs.text_font_family, state.text_font_family);
        assert_eq!(prefs.obfuscate_method, state.obfuscate_method);
        assert_eq!(prefs.arrow_style, state.arrow_style);
        assert_eq!(prefs.background_alignment, state.background_alignment);
        assert_eq!(prefs.background_aspect_ratio, state.background_aspect_ratio);
        assert_eq!(prefs.highlighter_mode, state.highlighter_mode);
        assert_eq!(prefs.pen_weight, state.pen_weight);
        assert_eq!(prefs.numbering_style, state.numbering_style);
        assert_eq!(prefs.number_size, state.number_size);
    }

    #[test]
    fn round_trip_apply_and_extract() {
        let mut state = EditorState::new(RgbaImage::new(10, 10));
        state.selected_tool = Tool::Arrow;
        state.stroke_size = 7.0;
        state.text_size = 32.0;
        state.text_font_family = String::from("Monospace");
        state.obfuscate_method = ObfuscateMethod::Blur;
        state.arrow_style = ArrowStyle::Curved;
        state.background_padding = 40.0;
        state.background_corner_radius = 0.0;
        state.pen_weight = PenWeight::Large;
        state.numbering_style = NumberingStyle::Uppercase;
        state.numbering_start = 5;
        state.number_size = NumberSize::Large;

        let prefs = EditorPreferences::from_state(&state);

        let mut restored = EditorState::new(RgbaImage::new(10, 10));
        prefs.apply_to_state(&mut restored);

        assert_eq!(restored.selected_tool, Tool::Arrow);
        assert!((restored.stroke_size - 7.0).abs() < f64::EPSILON);
        assert!((restored.text_size - 32.0).abs() < f64::EPSILON);
        assert_eq!(restored.text_font_family, "Monospace");
        assert_eq!(restored.obfuscate_method, ObfuscateMethod::Blur);
        assert_eq!(restored.arrow_style, ArrowStyle::Curved);
        assert!((restored.background_padding - 40.0).abs() < f64::EPSILON);
        assert!((restored.background_corner_radius - 0.0).abs() < f64::EPSILON);
        assert_eq!(restored.pen_weight, PenWeight::Large);
        assert_eq!(restored.numbering_style, NumberingStyle::Uppercase);
        assert_eq!(restored.numbering_start, 5);
        assert_eq!(restored.number_size, NumberSize::Large);
    }

    #[test]
    fn serde_round_trip() {
        let mut state = EditorState::new(RgbaImage::new(10, 10));
        state.selected_tool = Tool::Pen;
        state.stroke_size = 4.0;
        state.focus_intensity = 75.0;

        let prefs = EditorPreferences::from_state(&state);
        let yaml = serde_yml::to_string(&prefs).unwrap();
        let deserialized: EditorPreferences = serde_yml::from_str(&yaml).unwrap();

        assert_eq!(deserialized.tool, Tool::Pen);
        assert!((deserialized.stroke_size - 4.0).abs() < f64::EPSILON);
        assert!((deserialized.focus_intensity - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn serde_with_missing_fields_uses_defaults() {
        let yaml = "tool: Text\ntext_size: 48.0\n";
        let prefs: EditorPreferences = serde_yml::from_str(yaml).unwrap();

        assert_eq!(prefs.tool, Tool::Text);
        assert!((prefs.text_size - 48.0).abs() < f64::EPSILON);
        assert!((prefs.stroke_size - STROKE_WIDTH).abs() < f64::EPSILON);
        assert_eq!(prefs.arrow_style, ArrowStyle::Standard);
    }
}
