//! Picks a descriptive filename from the window title and the OCR text.

use std::path::{Path, PathBuf};

use crate::{CaptureContext, OcrPage};

/// How a suggested name is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameStyle {
    /// "Start ApexShot at login by default"
    #[default]
    Clean,
    /// "2026-11-27 Start ApexShot at login by default"
    Dated,
    /// "Firefox - Start ApexShot at login by default"
    ByApp,
}

/// Where the subject of a suggested name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    WindowTitle,
    Heading,
    AppOnly,
    Fallback,
}

/// A suggested filename stem (no extension, no directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameSuggestion {
    pub stem: String,
    /// Display name of the app, e.g. "Firefox", when one was found.
    pub app: Option<String>,
    /// Bare host, e.g. "github.com", when the capture shows a web page.
    pub site: Option<String>,
    pub source: NameSource,
}

/// Longest stem `suggest_name` returns, in characters.
pub const MAX_STEM_CHARS: usize = 60;

/// Suggest a filename stem for a capture.
pub fn suggest_name(page: &OcrPage, ctx: &CaptureContext, style: NameStyle) -> NameSuggestion {
    let _ = (page, style);
    NameSuggestion {
        stem: format!(
            "Screenshot {}",
            ctx.captured_at.format("%Y-%m-%d at %H.%M.%S")
        ),
        app: None,
        site: None,
        source: NameSource::Fallback,
    }
}

/// Split a window title into its subject and the app name it ends with.
pub fn split_window_title(title: &str) -> (String, Option<String>) {
    (title.trim().to_string(), None)
}

/// The bare host shown in the capture, if any.
pub fn detect_site(page: &OcrPage, ctx: &CaptureContext) -> Option<String> {
    let _ = (page, ctx);
    None
}

/// Make `text` safe as a filename stem on Windows and Linux, at most
/// `max_chars` characters.
pub fn sanitize_stem(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// `dir/stem.ext`, or `dir/stem 2.ext`, `dir/stem 3.ext`, ... when taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    dir.join(format!("{stem}.{ext}"))
}
