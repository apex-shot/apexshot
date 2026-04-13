//! Serializable schema for annotation storage
//!
//! This module defines the JSON schema for persisting annotations.
//! Annotations are stored in ~/.local/share/apexshot/annotations/

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level annotation file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationFile {
    /// Schema version for future migrations
    pub version: String,
    /// Original image path (for reference/recovery)
    pub image_path: String,
    /// SHA256 hash of the image when annotations were saved
    pub image_hash: String,
    /// Canvas size at time of save
    pub canvas_size: CanvasSize,
    /// All annotation objects
    pub annotations: Vec<SerializableAnnotation>,
    /// When annotations were first created
    pub created_at: DateTime<Utc>,
    /// When annotations were last modified
    pub modified_at: DateTime<Utc>,
}

/// Canvas dimensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

/// Serializable color (f64 -> u8 for JSON compactness)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn from_f64(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self {
            r: (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            g: (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            b: (b.clamp(0.0, 1.0) * 255.0).round() as u8,
            a: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }

    pub fn to_f64(&self) -> (f64, f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
            self.a as f64 / 255.0,
        )
    }
}

/// Serializable point
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn from_editor(p: crate::capture::editor::types::Point) -> Self {
        Self { x: p.x, y: p.y }
    }

    pub fn to_editor(&self) -> crate::capture::editor::types::Point {
        crate::capture::editor::types::Point {
            x: self.x,
            y: self.y,
        }
    }
}

/// Serializable rect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn from_editor(r: crate::capture::editor::types::Rect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }

    pub fn to_editor(&self) -> crate::capture::editor::types::Rect {
        crate::capture::editor::types::Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

/// Arrow style enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArrowStyle {
    Standard,
    Fancy,
    Curved,
    Double,
}

/// Obfuscate method enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObfuscateMethod {
    Pixelate,
    BlurSecure,
    BlurSmooth,
    Blackout,
}

/// Font style enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Bold,
    Italic,
    BoldItalic,
}

/// Text decoration enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextDecoration {
    None,
    Underline,
    Strikethrough,
    Both,
}

/// Text alignment enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

/// Font settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontSettings {
    pub family: String,
    pub size: f64,
    pub style: FontStyle,
    pub decoration: TextDecoration,
    pub alignment: TextAlignment,
}

/// Numbering style enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NumberingStyle {
    Numeric,
    Uppercase,
    Lowercase,
    Roman,
}

/// Number size enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NumberSize {
    Small,
    Medium,
    Large,
    ExtraLarge,
}

/// Serializable annotation - mirrors AnnotationAction but with serializable types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SerializableAnnotation {
    Pen {
        points: Vec<Point>,
        color: Color,
        stroke_size: f64,
    },
    Highlighter {
        points: Vec<Point>,
        color: Color,
        stroke_size: f64,
    },
    Circle {
        rect: Rect,
        color: Color,
        stroke_size: f64,
        shadow: bool,
    },
    Line {
        start: Point,
        end: Point,
        color: Color,
        stroke_size: f64,
        shadow: bool,
    },
    Arrow {
        start: Point,
        end: Point,
        color: Color,
        stroke_size: f64,
        style: ArrowStyle,
        control_points: Option<Vec<Point>>,
        shadow: bool,
    },
    Box {
        rect: Rect,
        color: Color,
        stroke_size: f64,
        shadow: bool,
    },
    Text {
        position: Point,
        text: String,
        color: Color,
        font: FontSettings,
        max_width: Option<f64>,
        shadow: bool,
    },
    Number {
        position: Point,
        number: u32,
        color: Color,
        style: NumberingStyle,
        size: NumberSize,
        shadow: bool,
    },
    Obfuscate {
        rect: Rect,
        method: ObfuscateMethod,
        amount: f64,
    },
    Focus {
        rect: Rect,
    },
}

impl AnnotationFile {
    /// Create a new annotation file with current timestamp
    pub fn new(image_path: &std::path::Path, image_hash: String, width: u32, height: u32) -> Self {
        let now = Utc::now();
        Self {
            version: "1.0".to_string(),
            image_path: image_path.to_string_lossy().to_string(),
            image_hash,
            canvas_size: CanvasSize { width, height },
            annotations: Vec::new(),
            created_at: now,
            modified_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_roundtrip() {
        let original = Color::from_f64(0.5, 0.25, 0.75, 1.0);
        let (r, g, b, a) = original.to_f64();
        assert!((r - 0.5).abs() < 0.01);
        assert!((g - 0.25).abs() < 0.01);
        assert!((b - 0.75).abs() < 0.01);
        assert!((a - 1.0).abs() < 0.01);
    }

    #[test]
    fn annotation_file_json_roundtrip() {
        let file = AnnotationFile::new(
            std::path::Path::new("/test/image.png"),
            "sha256:abc123".to_string(),
            1920,
            1080,
        );
        let json = serde_json::to_string_pretty(&file).unwrap();
        let parsed: AnnotationFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "1.0");
        assert_eq!(parsed.canvas_size.width, 1920);
    }
}
