//! Per-tool colour memory: each annotate tool keeps the colour last picked
//! for it, and the image editor saves the map in the app config so the next
//! session opens every tool with that colour.

use std::collections::BTreeMap;

use super::super::color::draw_color_to_rgba_u8;
use super::super::types::{DrawColor, Tool};
use super::EditorState;

/// The config key for a tool that draws with a colour; `None` for tools
/// that have no colour of their own.
pub(crate) fn tool_color_key(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Pen => Some("pen"),
        Tool::Highlighter => Some("highlighter"),
        Tool::Circle => Some("ellipse"),
        Tool::Arrow => Some("arrow"),
        Tool::Line => Some("line"),
        Tool::Box => Some("rectangle"),
        Tool::Text => Some("text"),
        Tool::Number => Some("number"),
        Tool::Crop | Tool::Select | Tool::Background | Tool::Obfuscate | Tool::Focus => None,
    }
}

/// `#rrggbbaa`, the form the config stores.
pub(crate) fn color_to_hex(color: DrawColor) -> String {
    let (r, g, b, a) = draw_color_to_rgba_u8(color);
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

/// Parse `#rrggbb` or `#rrggbbaa`; anything else is `None`.
pub(crate) fn color_from_hex(value: &str) -> Option<DrawColor> {
    let hex = value.trim().strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    let alpha = if hex.len() == 8 { channel(6)? } else { 255 };
    Some(DrawColor::new(
        f64::from(channel(0)?) / 255.0,
        f64::from(channel(2)?) / 255.0,
        f64::from(channel(4)?) / 255.0,
        f64::from(alpha) / 255.0,
    ))
}

impl EditorState {
    /// Seed the memory from the saved map and arm the current tool's
    /// colour. Unknown tools and malformed values are skipped.
    pub fn load_tool_colors(&mut self, saved: &BTreeMap<String, String>) {
        for (key, value) in saved {
            let known = [
                Tool::Pen,
                Tool::Highlighter,
                Tool::Circle,
                Tool::Arrow,
                Tool::Line,
                Tool::Box,
                Tool::Text,
                Tool::Number,
            ]
            .into_iter()
            .any(|tool| tool_color_key(tool) == Some(key.as_str()));
            if let (true, Some(color)) = (known, color_from_hex(value)) {
                self.tool_colors.insert(key.clone(), color);
            }
        }
        if let Some(color) = self.remembered_tool_color(self.selected_tool) {
            self.selected_color = color;
        }
    }

    /// The colour last picked for `tool`, if any.
    pub fn remembered_tool_color(&self, tool: Tool) -> Option<DrawColor> {
        tool_color_key(tool).and_then(|key| self.tool_colors.get(key).copied())
    }

    /// Remember `color` for the armed tool. Returns the config key and
    /// `#rrggbbaa` value to save, or `None` when the armed tool has no
    /// colour of its own.
    pub fn remember_tool_color(&mut self, color: DrawColor) -> Option<(&'static str, String)> {
        let key = tool_color_key(self.selected_tool)?;
        self.tool_colors.insert(key.to_string(), color);
        Some((key, color_to_hex(color)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn hex_round_trips_and_rejects_malformed_values() {
        let color = DrawColor::new(1.0, 0.0, 0.5, 1.0);
        let hex = color_to_hex(color);
        assert_eq!(hex, "#ff0080ff");
        assert_eq!(color_to_hex(color_from_hex(&hex).unwrap()), hex);
        assert_eq!(
            color_to_hex(color_from_hex("#00ff00").unwrap()),
            "#00ff00ff"
        );
        assert!(color_from_hex("00ff00").is_none());
        assert!(color_from_hex("#zzzzzz").is_none());
        assert!(color_from_hex("#12345").is_none());
    }

    #[test]
    fn each_tool_opens_with_its_own_remembered_color() {
        let blue = DrawColor::new(0.0, 0.0, 1.0, 1.0);
        let green = DrawColor::new(0.0, 1.0, 0.0, 1.0);
        let mut saved = BTreeMap::new();
        saved.insert("arrow".to_string(), color_to_hex(blue));
        saved.insert("pen".to_string(), color_to_hex(green));
        saved.insert("unknown".to_string(), "#ffffffff".to_string());
        saved.insert("line".to_string(), "not a colour".to_string());

        let mut state = EditorState::new(RgbaImage::new(8, 8));
        state.load_tool_colors(&saved);
        assert!(!state.tool_colors.contains_key("unknown"));
        assert!(!state.tool_colors.contains_key("line"));

        state.set_tool(Tool::Arrow);
        assert_eq!(color_to_hex(state.selected_color), color_to_hex(blue));
        state.set_tool(Tool::Pen);
        assert_eq!(color_to_hex(state.selected_color), color_to_hex(green));
    }

    #[test]
    fn picking_a_color_is_remembered_only_for_tools_that_draw() {
        let red = DrawColor::new(1.0, 0.0, 0.0, 1.0);
        let mut state = EditorState::new(RgbaImage::new(8, 8));
        state.set_tool(Tool::Box);
        assert_eq!(
            state.remember_tool_color(red),
            Some(("rectangle", "#ff0000ff".to_string()))
        );
        state.set_tool(Tool::Select);
        assert_eq!(state.remember_tool_color(red), None);
        assert_eq!(
            state.remembered_tool_color(Tool::Box).map(color_to_hex),
            Some("#ff0000ff".to_string())
        );
    }
}
