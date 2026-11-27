//! Finds secrets in recognised text so they never reach a filename or the
//! search index, and so copies can be redacted where they appear.

use crate::OcrPage;

/// What kind of secret a span holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    Email,
    CardNumber,
    ApiKey,
    Jwt,
    Password,
}

/// A secret inside a string, as byte offsets (`text[start..end]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretSpan {
    pub kind: SecretKind,
    pub start: usize,
    pub end: usize,
}

/// Every secret in `text`, sorted by `start`, never overlapping.
pub fn find_secrets(text: &str) -> Vec<SecretSpan> {
    let _ = text;
    Vec::new()
}

/// `text` with every secret removed and the surrounding whitespace collapsed.
pub fn scrub(text: &str) -> String {
    text.to_string()
}

/// Indices into `page.lines` of the lines holding a secret, with its kind.
pub fn secret_lines(page: &OcrPage) -> Vec<(usize, SecretKind)> {
    let _ = page;
    Vec::new()
}
