//! Fixing OCR reading mistakes with a local model, only when the result
//! provably says the same thing.

use std::collections::HashSet;

use crate::text_layout::{LayoutText, TextKind};

/// Longest text, in words, the model is asked to correct.
pub const MAX_AI_WORDS: usize = 150;

pub const CLEANUP_PROMPT: &str = "You fix text read from a screenshot by optical character recognition. Correct only clear reading mistakes: wrong letters or digits (l and 1, O and 0, rn and m), words split or joined by mistake, and stray symbols. Do not rephrase, reorder, translate, summarize, add, or remove anything. Keep numbers, names, links, code, and punctuation exactly as they are unless they are clearly misread. Keep line breaks. Reply with the corrected text only.";

/// True when this layout may go to the model: prose or a list, 1..=MAX_AI_WORDS words.
pub fn ai_eligible(layout: &LayoutText) -> bool {
    let kind = matches!(layout.kind, TextKind::Prose | TextKind::List);
    let words = layout.text.split_whitespace().count();
    kind && (1..=MAX_AI_WORDS).contains(&words)
}

/// The model's corrected text, or None when it changed more than reading mistakes.
pub fn accept_correction(original: &str, corrected: &str) -> Option<String> {
    let corrected = unwrap_reply(corrected);
    if corrected.is_empty() {
        return None;
    }
    let longest = original.chars().count().max(corrected.chars().count());
    if edit_distance(original, corrected) as f32 > 0.08 * longest as f32 {
        return None;
    }
    if digit_runs(original) != digit_runs(corrected) {
        return None;
    }
    if link_tokens(original) != link_tokens(corrected) {
        return None;
    }
    Some(corrected.to_string())
}

/// The reply without its fence, wrapping quotes, or surrounding whitespace.
fn unwrap_reply(reply: &str) -> &str {
    let mut text = reply.trim();
    if text.starts_with("```") {
        text = match text.find('\n') {
            Some(index) => &text[index + 1..],
            None => "",
        };
        if let Some(body) = text.trim_end().strip_suffix("```") {
            text = body;
        }
    }
    strip_wrapping(text)
}

/// Drop matching quotes or backticks around the whole reply.
fn strip_wrapping(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim();
        let bytes = trimmed.as_bytes();
        if bytes.len() < 2 {
            return trimmed;
        }
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if first == last && matches!(first, b'"' | b'\'' | b'`') {
            text = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

/// Character-level Levenshtein distance, one row of state at a time.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, a_char) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.iter().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The digits of every number-like token, in order. A token counts when at
/// least half of its letters and digits are digits ("1,240.00", "INV-2026"),
/// so a misread letter inside a word ("0rder") may still be fixed while a
/// changed number never passes.
fn digit_runs(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|token| {
            let digits = token.chars().filter(char::is_ascii_digit).count();
            let alnum = token.chars().filter(|c| c.is_alphanumeric()).count();
            digits > 0 && digits * 2 >= alnum
        })
        .map(|token| token.chars().filter(char::is_ascii_digit).collect())
        .collect()
}

/// Whitespace-separated tokens that carry a link or an address.
fn link_tokens(text: &str) -> HashSet<&str> {
    text.split_whitespace()
        .filter(|token| token.contains("://") || token.contains('@'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(kind: TextKind, text: &str) -> LayoutText {
        LayoutText {
            kind,
            text: text.to_string(),
            html: None,
        }
    }

    #[test]
    fn accepts_fixed_letters() {
        let fixed = accept_correction("Tbe quick brown fox", "The quick brown fox");
        assert_eq!(fixed.as_deref(), Some("The quick brown fox"));
    }

    #[test]
    fn accepts_fixed_letters_before_digits() {
        let fixed = accept_correction("0rder 1234 shipped", "Order 1234 shipped");
        assert_eq!(fixed.as_deref(), Some("Order 1234 shipped"));
    }

    #[test]
    fn rejects_reordered_digits() {
        let fixed = accept_correction("0rder 1234 shipped", "Order 1243 shipped");
        assert_eq!(fixed, None);
    }

    #[test]
    fn rejects_a_rewrite() {
        let fixed = accept_correction("The quick brown fox", "The fast brown fox jumps");
        assert_eq!(fixed, None);
    }

    #[test]
    fn rejects_a_changed_link() {
        let fixed = accept_correction("see https://a.com/x", "see https://a.com/y");
        assert_eq!(fixed, None);
    }

    #[test]
    fn strips_a_fence() {
        let reply = "```\nThe quick brown fox\n```";
        let fixed = accept_correction("The quick brown fox", reply);
        assert_eq!(fixed.as_deref(), Some("The quick brown fox"));
    }

    #[test]
    fn prose_of_the_prompted_length_is_eligible() {
        let text = "one two three four five six seven eight nine ten";
        assert!(ai_eligible(&layout(TextKind::Prose, text)));
        assert!(ai_eligible(&layout(TextKind::List, text)));
    }

    #[test]
    fn code_is_not_eligible() {
        let text = "one two three four five six seven eight nine ten";
        assert!(!ai_eligible(&layout(TextKind::Code, text)));
        assert!(!ai_eligible(&layout(TextKind::Table, text)));
    }

    #[test]
    fn long_prose_is_not_eligible() {
        let text = "word ".repeat(200);
        assert!(!ai_eligible(&layout(TextKind::Prose, &text)));
    }

    #[test]
    fn empty_text_is_not_eligible() {
        assert!(!ai_eligible(&layout(TextKind::Prose, "")));
    }
}
