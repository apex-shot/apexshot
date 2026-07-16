//! Pen weight types for pen and highlighter freehand modes.

use serde::{Deserialize, Serialize};

/// Preset thickness levels for freehand pen / highlighter strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PenWeight {
    /// Thin stroke
    Small,
    /// Medium stroke
    #[default]
    Medium,
    /// Thick stroke
    Large,
    /// Very thick stroke
    ExtraLarge,
}

impl PenWeight {
    /// All available pen weights
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::ExtraLarge];

    /// Stroke width used by the pen tool (image pixels).
    ///
    /// Kept intentionally moderate so freehand ink matches the thickness
    /// previews and feels natural next to shape tools. Highlighter uses a
    /// separate, thicker scale via [`Self::highlighter_stroke_width`].
    pub fn pen_stroke_width(self) -> f64 {
        match self {
            Self::Small => 3.0,
            Self::Medium => 5.0,
            Self::Large => 8.0,
            Self::ExtraLarge => 12.0,
        }
    }

    /// Stroke width used by freehand highlighter (image pixels).
    ///
    /// Markers need more body than ink, so this scale stays thicker than
    /// [`Self::pen_stroke_width`].
    pub fn highlighter_stroke_width(self) -> f64 {
        match self {
            Self::Small => 10.0,
            Self::Medium => 16.0,
            Self::Large => 24.0,
            Self::ExtraLarge => 32.0,
        }
    }

    /// Back-compat alias: previous callers treated this as the freehand width.
    /// Prefer [`Self::pen_stroke_width`] or [`Self::highlighter_stroke_width`].
    pub fn stroke_width(self) -> f64 {
        self.pen_stroke_width()
    }

    /// Get display label
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Thin",
            Self::Medium => "Medium",
            Self::Large => "Thick",
            Self::ExtraLarge => "Very Thick",
        }
    }

    /// Get icon name for the pen/highlighter control.
    pub fn icon_name(self) -> &'static str {
        "document-edit-symbolic"
    }

    /// Get icon pixel size for visually representing thickness.
    pub fn icon_pixel_size(self) -> i32 {
        match self {
            Self::Small => 14,
            Self::Medium => 17,
            Self::Large => 20,
            Self::ExtraLarge => 23,
        }
    }

    /// Get index for UI list
    pub fn index(self) -> usize {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
            Self::ExtraLarge => 3,
        }
    }

    /// Create from index
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Small,
            1 => Self::Medium,
            2 => Self::Large,
            3 => Self::ExtraLarge,
            _ => Self::Medium, // Default
        }
    }

    /// Nearest weight for a stored pen stroke size (supports legacy widths).
    pub fn nearest_for_pen_stroke(stroke_size: f64) -> Self {
        Self::ALL
            .into_iter()
            .min_by(|a, b| {
                (a.pen_stroke_width() - stroke_size)
                    .abs()
                    .total_cmp(&(b.pen_stroke_width() - stroke_size).abs())
            })
            .unwrap_or_default()
    }

    /// Nearest weight for a stored highlighter stroke size (supports legacy widths).
    pub fn nearest_for_highlighter_stroke(stroke_size: f64) -> Self {
        Self::ALL
            .into_iter()
            .min_by(|a, b| {
                (a.highlighter_stroke_width() - stroke_size)
                    .abs()
                    .total_cmp(&(b.highlighter_stroke_width() - stroke_size).abs())
            })
            .unwrap_or_default()
    }
}

/// Highlighter mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HighlighterMode {
    /// Text-aware: auto-detect and highlight text
    #[default]
    TextAware,
    /// Freehand: draw with selected pen weight
    Freehand,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_widths_are_thinner_than_highlighter_widths() {
        for weight in PenWeight::ALL {
            assert!(
                weight.pen_stroke_width() < weight.highlighter_stroke_width(),
                "{weight:?}: pen should be thinner than highlighter"
            );
        }
    }

    #[test]
    fn nearest_pen_maps_legacy_medium_width() {
        // Legacy Medium was 16px; nearest new pen width is ExtraLarge (12).
        assert_eq!(
            PenWeight::nearest_for_pen_stroke(16.0),
            PenWeight::ExtraLarge
        );
        assert_eq!(PenWeight::nearest_for_pen_stroke(5.0), PenWeight::Medium);
    }

    #[test]
    fn nearest_highlighter_keeps_legacy_scale() {
        assert_eq!(
            PenWeight::nearest_for_highlighter_stroke(16.0),
            PenWeight::Medium
        );
        assert_eq!(
            PenWeight::nearest_for_highlighter_stroke(32.0),
            PenWeight::ExtraLarge
        );
    }
}
