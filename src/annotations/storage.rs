//! Storage functions for annotation persistence
//!
//! Annotations are stored in ~/.local/share/apexshot/annotations/
//! Each file is named by the SHA256 hash of the image path.

use super::schema::{
    AnnotationFile, BackgroundAlignment, BackgroundSettings, BackgroundStyle, CropAspectRatio,
    Point, Rect, SerializableAnnotation,
};
use crate::capture::editor::types::{
    AnnotationAction, ArrowStyle as EditorArrowStyle,
    BackgroundAlignment as EditorBackgroundAlignment, BackgroundStyle as EditorBackgroundStyle,
    CropAspectRatio as EditorCropAspectRatio, DrawColor, FontSettings as EditorFontSettings,
};
use image::RgbaImage;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnnotationError {
    #[error("Failed to create annotation directory: {0}")]
    DirectoryError(String),

    #[error("Failed to write annotation file: {0}")]
    WriteError(String),

    #[error("Failed to read annotation file: {0}")]
    ReadError(String),

    #[error("Failed to parse annotation file: {0}")]
    ParseError(String),

    #[error("Image hash mismatch - image may have been modified externally")]
    HashMismatch,

    #[error("Failed to compute image hash: {0}")]
    HashError(String),
}

/// Get the annotation storage directory
fn annotation_directory() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("apexshot")
        .join("annotations")
}

/// Get the originals storage directory
fn originals_directory() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("apexshot")
        .join("originals")
}

/// Get the annotation file path for an image
pub fn annotation_path_for_image(image_path: &Path) -> std::path::PathBuf {
    let path_str = image_path.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    annotation_directory().join(format!("{}.json", hash))
}

/// Get the original image path for an image
pub fn original_path_for_image(image_path: &Path) -> std::path::PathBuf {
    let path_str = image_path.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    originals_directory().join(format!("{}.png", hash))
}

/// Compute SHA256 hash of an image file's contents
pub fn compute_image_hash(image_path: &Path) -> Result<String, AnnotationError> {
    let bytes = std::fs::read(image_path).map_err(|e| AnnotationError::HashError(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let hash = format!("{:x}", hasher.finalize());
    Ok(format!("sha256:{}", hash))
}

/// Save annotations for an image, along with the original base image
pub fn save_annotations(
    image_path: &Path,
    canvas_width: u32,
    canvas_height: u32,
    annotations: &[AnnotationAction],
    original_base_image: &RgbaImage,
    background_style: &EditorBackgroundStyle,
    background_padding: f64,
    background_shadow: f64,
    background_insert: f64,
    auto_balance: bool,
    background_alignment: EditorBackgroundAlignment,
    background_corner_radius: f64,
    background_aspect_ratio: EditorCropAspectRatio,
) -> Result<(), AnnotationError> {
    let annotation_path = annotation_path_for_image(image_path);
    let image_hash = compute_image_hash(image_path)?;

    let mut file = AnnotationFile::new(image_path, image_hash, canvas_width, canvas_height);
    file.background = BackgroundSettings {
        style: background_style_to_serializable(background_style),
        padding: background_padding,
        shadow: background_shadow,
        insert: background_insert,
        auto_balance,
        alignment: background_alignment_to_serializable(background_alignment),
        corner_radius: background_corner_radius,
        aspect_ratio: crop_aspect_ratio_to_serializable(background_aspect_ratio),
    };
    file.annotations = annotations
        .iter()
        .filter_map(action_to_serializable)
        .collect();
    file.modified_at = chrono::Utc::now();

    // Ensure directory exists
    let dir = annotation_directory();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| AnnotationError::DirectoryError(e.to_string()))?;
    }

    // Atomic write: write to temp file, then rename
    let temp_path = annotation_path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&file)
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;

    let mut file_handle = std::fs::File::create(&temp_path)
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;
    file_handle
        .write_all(json.as_bytes())
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;
    file_handle
        .sync_all()
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;

    std::fs::rename(&temp_path, &annotation_path)
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;

    // Also save the original base image for non-destructive re-editing
    save_original_image(image_path, original_base_image)?;

    Ok(())
}

pub fn background_style_from_serializable(style: &BackgroundStyle) -> EditorBackgroundStyle {
    match style {
        BackgroundStyle::None => EditorBackgroundStyle::None,
        BackgroundStyle::Gradient { index } => EditorBackgroundStyle::Gradient(*index),
        BackgroundStyle::Wallpaper { path } => EditorBackgroundStyle::Wallpaper(path.into()),
        BackgroundStyle::Blurred { index } => EditorBackgroundStyle::Blurred(*index),
        BackgroundStyle::PlainColor { color } => {
            EditorBackgroundStyle::PlainColor(color_from_serializable(*color))
        }
    }
}

pub fn background_alignment_from_serializable(
    alignment: BackgroundAlignment,
) -> EditorBackgroundAlignment {
    match alignment {
        BackgroundAlignment::TopLeft => EditorBackgroundAlignment::TopLeft,
        BackgroundAlignment::TopCenter => EditorBackgroundAlignment::TopCenter,
        BackgroundAlignment::TopRight => EditorBackgroundAlignment::TopRight,
        BackgroundAlignment::CenterLeft => EditorBackgroundAlignment::CenterLeft,
        BackgroundAlignment::Center => EditorBackgroundAlignment::Center,
        BackgroundAlignment::CenterRight => EditorBackgroundAlignment::CenterRight,
        BackgroundAlignment::BottomLeft => EditorBackgroundAlignment::BottomLeft,
        BackgroundAlignment::BottomCenter => EditorBackgroundAlignment::BottomCenter,
        BackgroundAlignment::BottomRight => EditorBackgroundAlignment::BottomRight,
    }
}

pub fn crop_aspect_ratio_from_serializable(ratio: CropAspectRatio) -> EditorCropAspectRatio {
    match ratio {
        CropAspectRatio::Freeform => EditorCropAspectRatio::Freeform,
        CropAspectRatio::Original => EditorCropAspectRatio::Original,
        CropAspectRatio::Square => EditorCropAspectRatio::Square,
        CropAspectRatio::FourThree => EditorCropAspectRatio::FourThree,
        CropAspectRatio::SixteenNine => EditorCropAspectRatio::SixteenNine,
        CropAspectRatio::TwentyOneNine => EditorCropAspectRatio::TwentyOneNine,
        CropAspectRatio::ThreeTwo => EditorCropAspectRatio::ThreeTwo,
        CropAspectRatio::NineSixteen => EditorCropAspectRatio::NineSixteen,
    }
}

/// Load annotations for an image
///
/// Returns:
/// - Ok(Some(file)) if annotations exist and hash matches
/// - Ok(None) if no annotation file exists
/// - Err(HashMismatch) if the image was modified externally
/// - Err(...) for other errors
pub fn load_annotations(image_path: &Path) -> Result<Option<AnnotationFile>, AnnotationError> {
    let annotation_path = annotation_path_for_image(image_path);
    if !annotation_path.exists() {
        return Ok(None);
    }

    let json = std::fs::read_to_string(&annotation_path)
        .map_err(|e| AnnotationError::ReadError(e.to_string()))?;
    let file: AnnotationFile =
        serde_json::from_str(&json).map_err(|e| AnnotationError::ParseError(e.to_string()))?;

    // Verify hash matches current image
    let current_hash = compute_image_hash(image_path)?;
    if file.image_hash != current_hash {
        return Err(AnnotationError::HashMismatch);
    }

    Ok(Some(file))
}

/// Check if annotations exist for an image (without loading them)
pub fn annotations_exist(image_path: &Path) -> bool {
    annotation_path_for_image(image_path).exists()
}

/// Delete annotations for an image
pub fn delete_annotations(image_path: &Path) -> Result<(), AnnotationError> {
    let annotation_path = annotation_path_for_image(image_path);
    if annotation_path.exists() {
        std::fs::remove_file(&annotation_path)
            .map_err(|e| AnnotationError::WriteError(e.to_string()))?;
    }
    // Also delete the original if it exists
    let original_path = original_path_for_image(image_path);
    if original_path.exists() {
        let _ = std::fs::remove_file(&original_path);
    }
    Ok(())
}

/// Save the original base image (before annotations)
pub fn save_original_image(
    image_path: &Path,
    original: &image::RgbaImage,
) -> Result<(), AnnotationError> {
    let original_path = original_path_for_image(image_path);

    // Ensure directory exists
    let dir = originals_directory();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| AnnotationError::DirectoryError(e.to_string()))?;
    }

    // Save as PNG to preserve quality
    original
        .save_with_format(&original_path, image::ImageFormat::Png)
        .map_err(|e| AnnotationError::WriteError(e.to_string()))?;

    Ok(())
}

/// Load the original base image (without annotations)
pub fn load_original_image(image_path: &Path) -> Result<Option<image::RgbaImage>, AnnotationError> {
    let original_path = original_path_for_image(image_path);
    if !original_path.exists() {
        return Ok(None);
    }

    let img = image::open(&original_path)
        .map_err(|e| AnnotationError::ReadError(e.to_string()))?
        .to_rgba8();

    Ok(Some(img))
}

/// Check if an original image exists
pub fn original_exists(image_path: &Path) -> bool {
    original_path_for_image(image_path).exists()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageSessionSnapshot {
    pub annotations: Vec<SerializableAnnotation>,
    pub background: BackgroundSettings,
    pub canvas_width: u32,
    pub canvas_height: u32,
}

impl ImageSessionSnapshot {
    pub fn from_editor(
        canvas_width: u32,
        canvas_height: u32,
        actions: &[AnnotationAction],
        background_style: &EditorBackgroundStyle,
        background_padding: f64,
        background_shadow: f64,
        background_insert: f64,
        auto_balance: bool,
        background_alignment: EditorBackgroundAlignment,
        background_corner_radius: f64,
        background_aspect_ratio: EditorCropAspectRatio,
    ) -> Self {
        Self {
            annotations: actions.iter().filter_map(action_to_serializable).collect(),
            background: BackgroundSettings {
                style: background_style_to_serializable(background_style),
                padding: background_padding,
                shadow: background_shadow,
                insert: background_insert,
                auto_balance,
                alignment: background_alignment_to_serializable(background_alignment),
                corner_radius: background_corner_radius,
                aspect_ratio: crop_aspect_ratio_to_serializable(background_aspect_ratio),
            },
            canvas_width,
            canvas_height,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.annotations.is_empty() && self.background == BackgroundSettings::default()
    }
}

/// Convert editor AnnotationAction to serializable form
fn action_to_serializable(action: &AnnotationAction) -> Option<SerializableAnnotation> {
    match action {
        AnnotationAction::Pen {
            points,
            color,
            stroke_size,
        } => Some(SerializableAnnotation::Pen {
            points: points.iter().map(|p| Point::from_editor(*p)).collect(),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
        }),

        AnnotationAction::Highlighter {
            points,
            color,
            stroke_size,
        } => Some(SerializableAnnotation::Highlighter {
            points: points.iter().map(|p| Point::from_editor(*p)).collect(),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
        }),

        AnnotationAction::Circle {
            rect,
            color,
            stroke_size,
            shadow,
        } => Some(SerializableAnnotation::Circle {
            rect: Rect::from_editor(*rect),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        }),

        AnnotationAction::Line {
            start,
            end,
            color,
            stroke_size,
            shadow,
        } => Some(SerializableAnnotation::Line {
            start: Point::from_editor(*start),
            end: Point::from_editor(*end),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        }),

        AnnotationAction::Arrow {
            start,
            end,
            color,
            stroke_size,
            style,
            control_points,
            shadow,
        } => Some(SerializableAnnotation::Arrow {
            start: Point::from_editor(*start),
            end: Point::from_editor(*end),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
            style: arrow_style_to_serializable(*style),
            control_points: control_points
                .as_ref()
                .map(|pts| pts.iter().map(|p| Point::from_editor(*p)).collect()),
            shadow: *shadow,
        }),

        AnnotationAction::Box {
            rect,
            color,
            stroke_size,
            shadow,
        } => Some(SerializableAnnotation::Box {
            rect: Rect::from_editor(*rect),
            color: color_to_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        }),

        AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            shadow,
            background_color,
        } => Some(SerializableAnnotation::Text {
            position: Point::from_editor(*position),
            text: text.clone(),
            color: color_to_serializable(*color),
            font: font_settings_to_serializable(font),
            max_width: *max_width,
            shadow: *shadow,
            background_color: background_color.map(color_to_serializable),
        }),

        AnnotationAction::Number {
            position,
            number,
            color,
            style,
            size,
            shadow,
        } => Some(SerializableAnnotation::Number {
            position: Point::from_editor(*position),
            number: *number,
            color: color_to_serializable(*color),
            style: numbering_style_to_serializable(*style),
            size: number_size_to_serializable(*size),
            shadow: *shadow,
        }),

        AnnotationAction::Obfuscate {
            rect,
            method,
            amount,
        } => Some(SerializableAnnotation::Obfuscate {
            rect: Rect::from_editor(*rect),
            method: obfuscate_method_to_serializable(*method),
            amount: *amount,
        }),

        AnnotationAction::Focus { rect, intensity } => Some(SerializableAnnotation::Focus {
            rect: Rect::from_editor(*rect),
            intensity: *intensity,
        }),
    }
}

/// Convert serializable annotation to editor AnnotationAction
pub fn serializable_to_action(ann: &SerializableAnnotation) -> AnnotationAction {
    match ann {
        SerializableAnnotation::Pen {
            points,
            color,
            stroke_size,
        } => AnnotationAction::Pen {
            points: points.iter().map(|p| p.to_editor()).collect(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
        },

        SerializableAnnotation::Highlighter {
            points,
            color,
            stroke_size,
        } => AnnotationAction::Highlighter {
            points: points.iter().map(|p| p.to_editor()).collect(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
        },

        SerializableAnnotation::Circle {
            rect,
            color,
            stroke_size,
            shadow,
        } => AnnotationAction::Circle {
            rect: rect.to_editor(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        },

        SerializableAnnotation::Line {
            start,
            end,
            color,
            stroke_size,
            shadow,
        } => AnnotationAction::Line {
            start: start.to_editor(),
            end: end.to_editor(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        },

        SerializableAnnotation::Arrow {
            start,
            end,
            color,
            stroke_size,
            style,
            control_points,
            shadow,
        } => AnnotationAction::Arrow {
            start: start.to_editor(),
            end: end.to_editor(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
            style: serializable_to_arrow_style(*style),
            control_points: control_points
                .as_ref()
                .map(|pts| pts.iter().map(|p| p.to_editor()).collect()),
            shadow: *shadow,
        },

        SerializableAnnotation::Box {
            rect,
            color,
            stroke_size,
            shadow,
        } => AnnotationAction::Box {
            rect: rect.to_editor(),
            color: color_from_serializable(*color),
            stroke_size: *stroke_size,
            shadow: *shadow,
        },

        SerializableAnnotation::Text {
            position,
            text,
            color,
            font,
            max_width,
            shadow,
            background_color,
        } => AnnotationAction::Text {
            position: position.to_editor(),
            text: text.clone(),
            color: color_from_serializable(*color),
            font: serializable_to_font_settings(font),
            max_width: *max_width,
            shadow: *shadow,
            background_color: background_color.map(color_from_serializable),
        },

        SerializableAnnotation::Number {
            position,
            number,
            color,
            style,
            size,
            shadow,
        } => AnnotationAction::Number {
            position: position.to_editor(),
            number: *number,
            color: color_from_serializable(*color),
            style: serializable_to_numbering_style(*style),
            size: serializable_to_number_size(*size),
            shadow: *shadow,
        },

        SerializableAnnotation::Obfuscate {
            rect,
            method,
            amount,
        } => AnnotationAction::Obfuscate {
            rect: rect.to_editor(),
            method: serializable_to_obfuscate_method(*method),
            amount: *amount,
        },

        SerializableAnnotation::Focus { rect, intensity } => AnnotationAction::Focus {
            rect: rect.to_editor(),
            intensity: *intensity,
        },
    }
}

// Helper conversion functions

fn color_to_serializable(c: DrawColor) -> super::schema::Color {
    super::schema::Color::from_f64(c.r, c.g, c.b, c.a)
}

fn color_from_serializable(c: super::schema::Color) -> DrawColor {
    let (r, g, b, a) = c.to_f64();
    DrawColor::new(r, g, b, a)
}

fn background_style_to_serializable(style: &EditorBackgroundStyle) -> BackgroundStyle {
    match style {
        EditorBackgroundStyle::None => BackgroundStyle::None,
        EditorBackgroundStyle::Gradient(index) => BackgroundStyle::Gradient { index: *index },
        EditorBackgroundStyle::Wallpaper(path) => BackgroundStyle::Wallpaper {
            path: path.to_string_lossy().to_string(),
        },
        EditorBackgroundStyle::Blurred(index) => BackgroundStyle::Blurred { index: *index },
        EditorBackgroundStyle::PlainColor(color) => BackgroundStyle::PlainColor {
            color: color_to_serializable(*color),
        },
    }
}

fn background_alignment_to_serializable(
    alignment: EditorBackgroundAlignment,
) -> BackgroundAlignment {
    match alignment {
        EditorBackgroundAlignment::TopLeft => BackgroundAlignment::TopLeft,
        EditorBackgroundAlignment::TopCenter => BackgroundAlignment::TopCenter,
        EditorBackgroundAlignment::TopRight => BackgroundAlignment::TopRight,
        EditorBackgroundAlignment::CenterLeft => BackgroundAlignment::CenterLeft,
        EditorBackgroundAlignment::Center => BackgroundAlignment::Center,
        EditorBackgroundAlignment::CenterRight => BackgroundAlignment::CenterRight,
        EditorBackgroundAlignment::BottomLeft => BackgroundAlignment::BottomLeft,
        EditorBackgroundAlignment::BottomCenter => BackgroundAlignment::BottomCenter,
        EditorBackgroundAlignment::BottomRight => BackgroundAlignment::BottomRight,
    }
}

fn crop_aspect_ratio_to_serializable(ratio: EditorCropAspectRatio) -> CropAspectRatio {
    match ratio {
        EditorCropAspectRatio::Freeform => CropAspectRatio::Freeform,
        EditorCropAspectRatio::Original => CropAspectRatio::Original,
        EditorCropAspectRatio::Square => CropAspectRatio::Square,
        EditorCropAspectRatio::FourThree => CropAspectRatio::FourThree,
        EditorCropAspectRatio::SixteenNine => CropAspectRatio::SixteenNine,
        EditorCropAspectRatio::TwentyOneNine => CropAspectRatio::TwentyOneNine,
        EditorCropAspectRatio::ThreeTwo => CropAspectRatio::ThreeTwo,
        EditorCropAspectRatio::NineSixteen => CropAspectRatio::NineSixteen,
    }
}

fn arrow_style_to_serializable(s: EditorArrowStyle) -> super::schema::ArrowStyle {
    match s {
        EditorArrowStyle::Standard => super::schema::ArrowStyle::Standard,
        EditorArrowStyle::Fancy => super::schema::ArrowStyle::Fancy,
        EditorArrowStyle::Curved => super::schema::ArrowStyle::Curved,
        EditorArrowStyle::Double => super::schema::ArrowStyle::Double,
    }
}

fn serializable_to_arrow_style(s: super::schema::ArrowStyle) -> EditorArrowStyle {
    match s {
        super::schema::ArrowStyle::Standard => EditorArrowStyle::Standard,
        super::schema::ArrowStyle::Fancy => EditorArrowStyle::Fancy,
        super::schema::ArrowStyle::Curved => EditorArrowStyle::Curved,
        super::schema::ArrowStyle::Double => EditorArrowStyle::Double,
    }
}

fn obfuscate_method_to_serializable(
    m: crate::capture::editor::types::ObfuscateMethod,
) -> super::schema::ObfuscateMethod {
    match m {
        crate::capture::editor::types::ObfuscateMethod::Pixelate => {
            super::schema::ObfuscateMethod::Pixelate
        }
        crate::capture::editor::types::ObfuscateMethod::Blur => {
            super::schema::ObfuscateMethod::Blur
        }
        crate::capture::editor::types::ObfuscateMethod::Blackout => {
            super::schema::ObfuscateMethod::Blackout
        }
    }
}

fn serializable_to_obfuscate_method(
    m: super::schema::ObfuscateMethod,
) -> crate::capture::editor::types::ObfuscateMethod {
    match m {
        super::schema::ObfuscateMethod::Pixelate => {
            crate::capture::editor::types::ObfuscateMethod::Pixelate
        }
        super::schema::ObfuscateMethod::Blur => {
            crate::capture::editor::types::ObfuscateMethod::Blur
        }
        super::schema::ObfuscateMethod::Blackout => {
            crate::capture::editor::types::ObfuscateMethod::Blackout
        }
    }
}

fn font_settings_to_serializable(f: &EditorFontSettings) -> super::schema::FontSettings {
    use crate::capture::editor::types::{
        FontStyle as EFontStyle, TextAlignment as ETextAlignment, TextDecoration as ETextDecoration,
    };

    super::schema::FontSettings {
        family: f.family.clone(),
        size: f.size,
        style: match f.style {
            EFontStyle::Normal => super::schema::FontStyle::Normal,
            EFontStyle::Bold => super::schema::FontStyle::Bold,
            EFontStyle::Italic => super::schema::FontStyle::Italic,
            EFontStyle::BoldItalic => super::schema::FontStyle::BoldItalic,
        },
        decoration: match f.decoration {
            ETextDecoration::None => super::schema::TextDecoration::None,
            ETextDecoration::Underline => super::schema::TextDecoration::Underline,
            ETextDecoration::Strikethrough => super::schema::TextDecoration::Strikethrough,
            ETextDecoration::Both => super::schema::TextDecoration::Both,
        },
        alignment: match f.alignment {
            ETextAlignment::Left => super::schema::TextAlignment::Left,
            ETextAlignment::Center => super::schema::TextAlignment::Center,
            ETextAlignment::Right => super::schema::TextAlignment::Right,
        },
    }
}

fn serializable_to_font_settings(f: &super::schema::FontSettings) -> EditorFontSettings {
    use crate::capture::editor::types::{
        FontStyle as EFontStyle, TextAlignment as ETextAlignment, TextDecoration as ETextDecoration,
    };

    EditorFontSettings {
        family: f.family.clone(),
        size: f.size,
        style: match f.style {
            super::schema::FontStyle::Normal => EFontStyle::Normal,
            super::schema::FontStyle::Bold => EFontStyle::Bold,
            super::schema::FontStyle::Italic => EFontStyle::Italic,
            super::schema::FontStyle::BoldItalic => EFontStyle::BoldItalic,
        },
        decoration: match f.decoration {
            super::schema::TextDecoration::None => ETextDecoration::None,
            super::schema::TextDecoration::Underline => ETextDecoration::Underline,
            super::schema::TextDecoration::Strikethrough => ETextDecoration::Strikethrough,
            super::schema::TextDecoration::Both => ETextDecoration::Both,
        },
        alignment: match f.alignment {
            super::schema::TextAlignment::Left => ETextAlignment::Left,
            super::schema::TextAlignment::Center => ETextAlignment::Center,
            super::schema::TextAlignment::Right => ETextAlignment::Right,
        },
    }
}

fn numbering_style_to_serializable(
    s: crate::capture::editor::numbering_style::NumberingStyle,
) -> super::schema::NumberingStyle {
    use crate::capture::editor::numbering_style::NumberingStyle as ENumberingStyle;
    match s {
        ENumberingStyle::Numeric => super::schema::NumberingStyle::Numeric,
        ENumberingStyle::Uppercase => super::schema::NumberingStyle::Uppercase,
        ENumberingStyle::Lowercase => super::schema::NumberingStyle::Lowercase,
        ENumberingStyle::Roman => super::schema::NumberingStyle::Roman,
    }
}

fn serializable_to_numbering_style(
    s: super::schema::NumberingStyle,
) -> crate::capture::editor::numbering_style::NumberingStyle {
    use crate::capture::editor::numbering_style::NumberingStyle as ENumberingStyle;
    match s {
        super::schema::NumberingStyle::Numeric => ENumberingStyle::Numeric,
        super::schema::NumberingStyle::Uppercase => ENumberingStyle::Uppercase,
        super::schema::NumberingStyle::Lowercase => ENumberingStyle::Lowercase,
        super::schema::NumberingStyle::Roman => ENumberingStyle::Roman,
    }
}

fn number_size_to_serializable(
    s: crate::capture::editor::numbering_style::NumberSize,
) -> super::schema::NumberSize {
    use crate::capture::editor::numbering_style::NumberSize as ENumberSize;
    match s {
        ENumberSize::Small => super::schema::NumberSize::Small,
        ENumberSize::Medium => super::schema::NumberSize::Medium,
        ENumberSize::Large => super::schema::NumberSize::Large,
        ENumberSize::ExtraLarge => super::schema::NumberSize::ExtraLarge,
    }
}

fn serializable_to_number_size(
    s: super::schema::NumberSize,
) -> crate::capture::editor::numbering_style::NumberSize {
    use crate::capture::editor::numbering_style::NumberSize as ENumberSize;
    match s {
        super::schema::NumberSize::Small => ENumberSize::Small,
        super::schema::NumberSize::Medium => ENumberSize::Medium,
        super::schema::NumberSize::Large => ENumberSize::Large,
        super::schema::NumberSize::ExtraLarge => ENumberSize::ExtraLarge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn annotation_path_is_deterministic() {
        let path1 = PathBuf::from("/home/user/test.png");
        let path2 = PathBuf::from("/home/user/test.png");
        assert_eq!(
            annotation_path_for_image(&path1),
            annotation_path_for_image(&path2)
        );
    }

    #[test]
    fn different_paths_get_different_annotations() {
        let path1 = PathBuf::from("/home/user/test1.png");
        let path2 = PathBuf::from("/home/user/test2.png");
        assert_ne!(
            annotation_path_for_image(&path1),
            annotation_path_for_image(&path2)
        );
    }

    #[test]
    fn background_settings_roundtrip_through_serializable_schema() {
        let style = EditorBackgroundStyle::Wallpaper(PathBuf::from("/tmp/background.png"));
        let serializable_style = background_style_to_serializable(&style);

        assert_eq!(
            background_style_from_serializable(&serializable_style),
            style
        );
        assert_eq!(
            background_alignment_from_serializable(background_alignment_to_serializable(
                EditorBackgroundAlignment::BottomCenter
            )),
            EditorBackgroundAlignment::BottomCenter
        );
        assert_eq!(
            crop_aspect_ratio_from_serializable(crop_aspect_ratio_to_serializable(
                EditorCropAspectRatio::TwentyOneNine
            )),
            EditorCropAspectRatio::TwentyOneNine
        );
    }

    fn scratch_png(name: &str) -> (PathBuf, image::RgbaImage) {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-image-session-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        image.save(&path).unwrap();
        (path, image)
    }

    fn cleanup_png(path: &Path) {
        let _ = delete_annotations(path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn save_session(path: &Path, image: &image::RgbaImage, actions: &[AnnotationAction]) {
        save_annotations(
            path,
            image.width(),
            image.height(),
            actions,
            image,
            &EditorBackgroundStyle::None,
            24.0,
            15.0,
            0.0,
            false,
            EditorBackgroundAlignment::Center,
            18.0,
            EditorCropAspectRatio::Original,
        )
        .unwrap();
    }

    #[test]
    fn save_annotations_does_not_change_png_bytes() {
        let (path, image) = scratch_png("no-flatten");
        let before = std::fs::read(&path).unwrap();
        let actions = [AnnotationAction::Line {
            start: crate::capture::editor::types::Point { x: 1.0, y: 1.0 },
            end: crate::capture::editor::types::Point { x: 4.0, y: 4.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        }];
        save_session(&path, &image, &actions);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        cleanup_png(&path);
    }

    #[test]
    fn persist_then_load_restores_actions_and_original() {
        let (path, image) = scratch_png("roundtrip");
        let actions = [AnnotationAction::Box {
            rect: crate::capture::editor::types::Rect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            },
            color: DrawColor::new(0.0, 1.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: true,
        }];
        save_annotations(
            &path,
            image.width(),
            image.height(),
            &actions,
            &image,
            &EditorBackgroundStyle::None,
            42.0,
            15.0,
            0.0,
            false,
            EditorBackgroundAlignment::Center,
            18.0,
            EditorCropAspectRatio::Original,
        )
        .unwrap();

        let file = load_annotations(&path).unwrap().expect("sidecar");
        assert_eq!(file.annotations.len(), 1);
        assert!((file.background.padding - 42.0).abs() < f64::EPSILON);
        let original = load_original_image(&path).unwrap().expect("original");
        assert_eq!(original.dimensions(), image.dimensions());
        cleanup_png(&path);
    }

    #[test]
    fn empty_session_deletes_existing_sidecar() {
        let (path, image) = scratch_png("delete-empty");
        let actions = [AnnotationAction::Line {
            start: crate::capture::editor::types::Point { x: 0.0, y: 0.0 },
            end: crate::capture::editor::types::Point { x: 1.0, y: 1.0 },
            color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 1.0,
            shadow: false,
        }];
        save_session(&path, &image, &actions);
        assert!(annotations_exist(&path));

        let empty = ImageSessionSnapshot::from_editor(
            image.width(),
            image.height(),
            &[],
            &EditorBackgroundStyle::None,
            24.0,
            15.0,
            0.0,
            false,
            EditorBackgroundAlignment::Center,
            18.0,
            EditorCropAspectRatio::Original,
        );
        assert!(empty.is_empty());
        delete_annotations(&path).unwrap();
        assert!(!annotations_exist(&path));
        assert!(load_original_image(&path).unwrap().is_none());
        cleanup_png(&path);
    }

    #[test]
    fn hash_mismatch_does_not_panic() {
        let (path, image) = scratch_png("hash");
        save_session(&path, &image, &[]);
        std::fs::write(&path, b"not-the-same-png-bytes").unwrap();
        match load_annotations(&path) {
            Err(AnnotationError::HashMismatch) => {}
            other => panic!("expected hash mismatch, got {other:?}"),
        }
        cleanup_png(&path);
    }
}
