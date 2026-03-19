//! Pen weight types for highlighter freehand mode

/// Preset pen weights for freehand highlighting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenWeight {
    /// Small pen (8px)
    Small,
    /// Medium pen (16px)
    Medium,
    /// Large pen (24px)
    Large,
    /// Extra large pen (32px)
    ExtraLarge,
}

impl PenWeight {
    /// All available pen weights
    pub const ALL: [Self; 4] = [
        Self::Small,
        Self::Medium,
        Self::Large,
        Self::ExtraLarge,
    ];

    /// Get the stroke width in pixels
    pub fn stroke_width(self) -> f64 {
        match self {
            Self::Small => 8.0,
            Self::Medium => 16.0,
            Self::Large => 24.0,
            Self::ExtraLarge => 32.0,
        }
    }

    /// Get display label
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::ExtraLarge => "Extra Large",
        }
    }

    /// Get icon name for the pen (using built-in icons)
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Small => "pen-weight-small",
            Self::Medium => "pen-weight-medium",
            Self::Large => "pen-weight-large",
            Self::ExtraLarge => "pen-weight-extralarge",
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
}

impl Default for PenWeight {
    fn default() -> Self {
        Self::Medium
    }
}

/// Highlighter mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlighterMode {
    /// Text-aware: auto-detect and highlight text
    #[default]
    TextAware,
    /// Freehand: draw with selected pen weight
    Freehand,
}
