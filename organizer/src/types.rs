//! Inputs shared by naming, scrubbing, and indexing.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// One recognised word with its bounding box in image pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextWord {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl TextWord {
    pub fn new(text: impl Into<String>, x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            width,
            height,
        }
    }
}

/// One line of recognised text with its bounding box in image pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The line's words in reading order, when the OCR engine reports them.
    /// Layout rebuilding (tables, code indentation) needs them; naming does not.
    #[serde(default)]
    pub words: Vec<TextWord>,
}

impl TextLine {
    pub fn new(text: impl Into<String>, x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            width,
            height,
            words: Vec::new(),
        }
    }

    /// The same line with its words attached.
    pub fn with_words(mut self, words: Vec<TextWord>) -> Self {
        self.words = words;
        self
    }
}

/// The OCR result for one screenshot, lines in reading order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrPage {
    pub image_width: u32,
    pub image_height: u32,
    pub lines: Vec<TextLine>,
}

impl OcrPage {
    /// Every line's text joined with newlines.
    pub fn full_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What was in front when the capture was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureContext {
    /// Application name as the platform reports it, e.g. "firefox" or
    /// "Code.exe". `None` when it could not be determined (Wayland).
    pub app: Option<String>,
    /// Title of the foreground window, e.g.
    /// "Pull request #283 · apex-shot/apexshot — Mozilla Firefox".
    pub window_title: Option<String>,
    pub captured_at: DateTime<Local>,
}
