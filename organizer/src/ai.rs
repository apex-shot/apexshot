//! Optional AI naming: the prompt a small local model gets and the cleanup of
//! its reply. The model only ever sees text that naming already scrubbed,
//! never the image.

use crate::naming::{sanitize_stem, NameSuggestion, MAX_STEM_CHARS};
use crate::secrets::{find_secrets, scrub};
use crate::sensitive::contains_explicit;
use crate::OcrPage;

/// Something that can answer one naming prompt, such as a local model server.
pub trait AiNamer {
    /// The model's raw reply, or None when it failed or ran out of time.
    fn complete(&self, system: &str, user: &str) -> Option<String>;
}

pub const SYSTEM_PROMPT: &str = "You write file names for screenshots. From the window title and the text on screen, find the main thing the screenshot is about and name it in 4 to 8 words.
Rules:
- Start with the subject: who wrote it, or what document, product, error, or topic it is. Then what it is about.
- Use only names and facts that appear in the given text. Never invent names.
- Ignore ads, menus, sidebars, and trending lists.
- Never answer with only the app or website name.
- No file extension, no quotes, no final punctuation.
- Never include passwords, keys, email addresses, phone numbers, or account numbers.
The shape of a good answer: \"<who or what> <about what>\", like \"<sender> invoice reminder due <date>\" or \"<author> post about <topic>\".
Reply with the name only.";

/// Longest on-screen text sent to the model, in characters.
pub const MAX_PROMPT_TEXT_CHARS: usize = 1500;

/// Build the user message from the rule-based suggestion and the page.
pub fn build_user_prompt(
    suggestion: &NameSuggestion,
    window_title: Option<&str>,
    page: &OcrPage,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(app) = &suggestion.app {
        lines.push(format!("App: {app}"));
    }
    if let Some(site) = &suggestion.site {
        lines.push(format!("Site: {site}"));
    }
    if let Some(title) = window_title {
        let title = scrub(title);
        if !title.is_empty() {
            lines.push(format!("Window title: {title}"));
        }
    }
    lines.push(format!("Current name: {}", suggestion.stem));
    lines.push("Text on screen, top to bottom:".to_string());
    lines.push(page_text(page));
    lines.join("\n")
}

/// The page's scrubbed lines in reading order, capped for the prompt.
fn page_text(page: &OcrPage) -> String {
    let mut lines: Vec<&crate::TextLine> = page.lines.iter().collect();
    lines.sort_by_key(|line| (line.y, line.x));
    let text = lines
        .iter()
        .map(|line| scrub(line.text.trim()))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return "(none)".to_string();
    }
    cap_text(&text)
}

/// Keep at most `MAX_PROMPT_TEXT_CHARS` characters, ending on a line break
/// when there is one.
fn cap_text(text: &str) -> String {
    if text.chars().count() <= MAX_PROMPT_TEXT_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_PROMPT_TEXT_CHARS).collect();
    match cut.rfind('\n') {
        Some(at) => cut[..at].to_string(),
        None => cut,
    }
}

/// The usable name in a model reply, or None when it cannot be used.
pub fn clean_reply(raw: &str) -> Option<String> {
    let mut text = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    text = strip_label(text);
    text = strip_wrapping(text);
    text = strip_extension(text);
    if !find_secrets(text).is_empty() || contains_explicit(text) {
        return None;
    }
    let words = text.split_whitespace().count();
    if !(2..=12).contains(&words) {
        return None;
    }
    let stem = sanitize_stem(text, MAX_STEM_CHARS);
    if stem == "Screenshot" {
        None
    } else {
        Some(stem)
    }
}

/// Drop a leading "Filename:"-style label, ignoring case.
fn strip_label(text: &str) -> &str {
    for label in ["file name:", "filename:", "name:", "title:"] {
        if let Some(prefix) = text.get(..label.len()) {
            if prefix.eq_ignore_ascii_case(label) {
                return text[label.len()..].trim();
            }
        }
    }
    text
}

/// Drop quotes, backticks, and asterisks around the reply.
fn strip_wrapping(mut text: &str) -> &str {
    loop {
        let next = text.trim_matches(|c| {
            matches!(
                c,
                '"' | '\'' | '`' | '*' | '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}'
            )
        });
        if next == text {
            return text;
        }
        text = next;
    }
}

/// Drop a trailing file extension, ignoring case.
fn strip_extension(text: &str) -> &str {
    for extension in [".png", ".jpg", ".jpeg", ".webp"] {
        if let Some(head) = strip_suffix_ignore_case(text, extension) {
            if !head.is_empty() {
                return head;
            }
        }
    }
    text
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let start = text.len().checked_sub(suffix.len())?;
    let tail = text.get(start..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &text[..start])
}

/// Ask `namer` for a name; None when it fails or the reply is unusable.
pub fn ai_name(
    namer: &dyn AiNamer,
    suggestion: &NameSuggestion,
    window_title: Option<&str>,
    page: &OcrPage,
) -> Option<String> {
    if suggestion.sensitivity.withholds_text() {
        return None;
    }
    let user = build_user_prompt(suggestion, window_title, page);
    let reply = namer.complete(SYSTEM_PROMPT, &user)?;
    clean_reply(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::NameSource;
    use crate::sensitive::Sensitivity;
    use crate::TextLine;
    use std::cell::Cell;

    struct Fixed(Option<String>);

    impl AiNamer for Fixed {
        fn complete(&self, _system: &str, _user: &str) -> Option<String> {
            self.0.clone()
        }
    }

    struct Counting {
        reply: Option<String>,
        calls: Cell<u32>,
    }

    impl AiNamer for Counting {
        fn complete(&self, _system: &str, _user: &str) -> Option<String> {
            self.calls.set(self.calls.get() + 1);
            self.reply.clone()
        }
    }

    fn suggestion() -> NameSuggestion {
        NameSuggestion {
            stem: "Home X".to_string(),
            app: Some("Chrome".to_string()),
            site: Some("x.com".to_string()),
            source: NameSource::WindowTitle,
            sensitivity: Sensitivity::Normal,
        }
    }

    fn page(lines: &[(&str, i32, i32)]) -> OcrPage {
        OcrPage {
            image_width: 1200,
            image_height: 800,
            lines: lines
                .iter()
                .map(|(text, x, y)| TextLine::new(*text, *x, *y, 600, 16))
                .collect(),
        }
    }

    #[test]
    fn builds_a_prompt_from_the_suggestion_and_page() {
        let page = page(&[("Second line", 0, 80), ("First line", 0, 20)]);
        let prompt = build_user_prompt(&suggestion(), Some("Home / X"), &page);
        assert!(prompt.contains("App: Chrome"));
        assert!(prompt.contains("Site: x.com"));
        assert!(prompt.contains("Window title: Home / X"));
        assert!(prompt.contains("Current name: Home X"));
        let first = prompt.find("First line").unwrap();
        let second = prompt.find("Second line").unwrap();
        assert!(first < second);
    }

    #[test]
    fn an_empty_page_says_none() {
        let prompt = build_user_prompt(&suggestion(), None, &OcrPage::default());
        assert!(prompt.ends_with("Text on screen, top to bottom:\n(none)"));
        assert!(!prompt.contains("Window title:"));
    }

    #[test]
    fn caps_the_page_text() {
        let long = "a".repeat(MAX_PROMPT_TEXT_CHARS + 200);
        let page = page(&[(long.as_str(), 0, 0)]);
        let prompt = build_user_prompt(&suggestion(), None, &page);
        let text = prompt
            .split_once("Text on screen, top to bottom:\n")
            .unwrap()
            .1;
        assert_eq!(text.chars().count(), MAX_PROMPT_TEXT_CHARS);
    }

    #[test]
    fn cleans_a_reply() {
        assert_eq!(
            clean_reply("WhalePanda post on earning ohms").as_deref(),
            Some("WhalePanda post on earning ohms")
        );
        assert_eq!(
            clean_reply("Filename: \"Quarterly sales report.png\"").as_deref(),
            Some("Quarterly sales report")
        );
        assert_eq!(
            clean_reply("\n\n  **Release notes for v2**  \nextra").as_deref(),
            Some("Release notes for v2")
        );
        assert_eq!(clean_reply("Hi"), None);
        assert_eq!(clean_reply("Login for jane@example.com page"), None);
        assert_eq!(clean_reply("nude photo set"), None);
        assert_eq!(clean_reply(""), None);
    }

    #[test]
    fn skips_the_model_for_a_private_window() {
        let namer = Counting {
            reply: Some("Quarterly sales review deck".to_string()),
            calls: Cell::new(0),
        };
        let mut private = suggestion();
        private.sensitivity = Sensitivity::PrivateWindow;
        let name = ai_name(&namer, &private, Some("Home / X"), &OcrPage::default());
        assert_eq!(name, None);
        assert_eq!(namer.calls.get(), 0);
    }

    #[test]
    fn returns_the_cleaned_reply() {
        let namer = Fixed(Some("Quarterly sales review deck".into()));
        let name = ai_name(&namer, &suggestion(), Some("Home / X"), &OcrPage::default());
        assert_eq!(name.as_deref(), Some("Quarterly sales review deck"));
    }

    #[test]
    fn returns_none_when_the_model_fails() {
        let namer = Fixed(None);
        let name = ai_name(&namer, &suggestion(), Some("Home / X"), &OcrPage::default());
        assert_eq!(name, None);
    }
}
