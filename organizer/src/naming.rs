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

/// How a window title separates its subject from the app name.
const SEPARATORS: [&str; 3] = [" \u{2014} ", " \u{2013} ", " - "];

/// Titles that say nothing about the content.
const GENERIC_TITLES: &[&str] = &[
    "new tab",
    "untitled",
    "desktop",
    "program manager",
    "start",
    "home",
    "new incognito tab",
    "private browsing",
];

/// Words that only appear in menus and toolbars, space separated.
const CHROME_WORDS: &str = "file edit view help window tools search settings home back \
    forward menu share close cancel ok save open new sign in log out more history bookmarks \
    profiles tab selection go run terminal";

const MONTHS: &[&str] = &[
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Top-level labels that look like a file extension, not a site.
const FILE_LABELS: &[&str] = &[
    "rs", "js", "ts", "py", "md", "txt", "png", "jpg", "exe", "json", "toml", "html", "css",
];

/// Suggest a filename stem for a capture.
pub fn suggest_name(page: &OcrPage, ctx: &CaptureContext, style: NameStyle) -> NameSuggestion {
    let title_parts = ctx.window_title.as_deref().map(split_window_title);
    let (title_subject, title_app) = title_parts.unwrap_or_default();
    let app = title_app.or_else(|| ctx.app.as_deref().map(display_app_from_process));
    let site = detect_site(page, ctx);
    let day = ctx.captured_at.format("%Y-%m-%d").to_string();

    let subject = crate::secrets::scrub(&title_subject).trim().to_string();
    if is_usable_subject(&subject) {
        return suggestion(&subject, app, site, NameSource::WindowTitle, style, &day);
    }
    if let Some(heading) = best_heading(page) {
        return suggestion(&heading, app, site, NameSource::Heading, style, &day);
    }
    let app_subject = app.as_ref().map(|name| format!("{name} screenshot"));
    if let Some(subject) = app_subject {
        return suggestion(&subject, app, site, NameSource::AppOnly, style, &day);
    }

    let fallback = format!(
        "Screenshot {}",
        ctx.captured_at.format("%Y-%m-%d at %H.%M.%S")
    );
    NameSuggestion {
        stem: sanitize_stem(&fallback, MAX_STEM_CHARS),
        app,
        site,
        source: NameSource::Fallback,
    }
}

/// Layout the subject for `style` and sanitize it into a stem.
fn suggestion(
    subject: &str,
    app: Option<String>,
    site: Option<String>,
    source: NameSource,
    style: NameStyle,
    day: &str,
) -> NameSuggestion {
    let styled = match style {
        NameStyle::Clean => subject.to_string(),
        NameStyle::Dated => format!("{day} {subject}"),
        NameStyle::ByApp => match app.as_deref() {
            Some(name) if source != NameSource::AppOnly => format!("{name} - {subject}"),
            _ => subject.to_string(),
        },
    };
    NameSuggestion {
        stem: sanitize_stem(&styled, MAX_STEM_CHARS),
        app,
        site,
        source,
    }
}

fn is_usable_subject(subject: &str) -> bool {
    let lower = subject.to_lowercase();
    subject.chars().count() >= 2 && !GENERIC_TITLES.contains(&lower.as_str())
}

/// Split a window title into its subject and the app name it ends with.
pub fn split_window_title(title: &str) -> (String, Option<String>) {
    let title = strip_notification_count(title.trim());
    let separator = SEPARATORS.into_iter().find(|sep| title.contains(*sep));
    let Some(sep) = separator else {
        return (title.to_string(), None);
    };
    let Some(index) = title.rfind(sep) else {
        return (title.to_string(), None);
    };
    let left = title[..index].trim();
    let last = title[index + sep.len()..].trim();

    if let Some(display) = display_app_from_title_suffix(last) {
        return (left.to_string(), Some(display));
    }
    if !left.is_empty() && !last.is_empty() && last.split_whitespace().count() <= 3 {
        return (left.to_string(), Some(last.to_string()));
    }
    (title.to_string(), None)
}

/// Drop a leading "(3) " style unread count.
fn strip_notification_count(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('(') else {
        return text;
    };
    let Some(close) = rest.find(')') else {
        return text;
    };
    let digits = &rest[..close];
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return text;
    }
    rest[close + 1..].strip_prefix(' ').unwrap_or(text)
}

fn display_app_from_title_suffix(seg: &str) -> Option<String> {
    let display = match seg.trim().to_lowercase().as_str() {
        "mozilla firefox" | "firefox" => "Firefox",
        "google chrome" | "chrome" => "Chrome",
        "chromium" => "Chromium",
        "microsoft edge" | "microsoft\u{200b} edge" | "edge" => "Edge",
        "brave" => "Brave",
        "opera" => "Opera",
        "vivaldi" => "Vivaldi",
        "visual studio code" => "VS Code",
        "slack" => "Slack",
        "discord" => "Discord",
        "microsoft teams" => "Teams",
        "notepad" => "Notepad",
        "word" => "Word",
        "excel" => "Excel",
        "powerpoint" => "PowerPoint",
        "outlook" => "Outlook",
        "windows powershell" | "powershell" => "PowerShell",
        "terminal" => "Terminal",
        "gimp" => "GIMP",
        "inkscape" => "Inkscape",
        "libreoffice writer" => "Writer",
        "libreoffice calc" => "Calc",
        "thunderbird" => "Thunderbird",
        "spotify" => "Spotify",
        "figma" => "Figma",
        "notion" => "Notion",
        "obsidian" => "Obsidian",
        "sublime text" => "Sublime Text",
        "zed" => "Zed",
        _ => return None,
    };
    Some(display.to_string())
}

fn display_app_from_process(app: &str) -> String {
    let lower = app.trim().to_lowercase();
    let name = lower.strip_suffix(".exe").unwrap_or(lower.as_str());
    let display = match name {
        "firefox" => "Firefox",
        "chrome" | "google-chrome" | "google-chrome-stable" => "Chrome",
        "chromium" | "chromium-browser" => "Chromium",
        "msedge" | "microsoft-edge" => "Edge",
        "brave" | "brave-browser" => "Brave",
        "opera" => "Opera",
        "vivaldi" => "Vivaldi",
        "code" => "VS Code",
        "explorer" => "File Explorer",
        "nautilus" | "org.gnome.nautilus" => "Files",
        "windowsterminal" => "Terminal",
        "slack" => "Slack",
        "discord" => "Discord",
        "ms-teams" | "teams" => "Teams",
        "notepad" => "Notepad",
        "winword" => "Word",
        "excel" => "Excel",
        "powerpnt" => "PowerPoint",
        "outlook" => "Outlook",
        "gimp" => "GIMP",
        "thunderbird" => "Thunderbird",
        _ => return capitalize_first(name),
    };
    display.to_string()
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn is_browser(display: &str) -> bool {
    matches!(
        display,
        "Firefox" | "Chrome" | "Chromium" | "Edge" | "Brave" | "Opera" | "Vivaldi"
    )
}

/// The bare host shown in the capture, if any.
pub fn detect_site(page: &OcrPage, ctx: &CaptureContext) -> Option<String> {
    let title_parts = ctx.window_title.as_deref().map(split_window_title);
    let title_app = title_parts.and_then(|(_, app)| app);
    let app = title_app.or_else(|| ctx.app.as_deref().map(display_app_from_process));
    if app.as_deref().is_some_and(|app| !is_browser(app)) {
        return None;
    }

    let cutoff = page.image_height as i32 / 5;
    for line in &page.lines {
        if line.y >= cutoff {
            continue;
        }
        for token in line.text.split_whitespace() {
            if let Some(host) = host_from_token(token) {
                return Some(host);
            }
        }
    }
    None
}

fn host_from_token(token: &str) -> Option<String> {
    let token = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))
        .unwrap_or(token);
    let end = token
        .find(|c| matches!(c, '/' | '?' | '#' | ':'))
        .unwrap_or(token.len());
    let host = &token[..end];
    let host = host.strip_prefix("www.").unwrap_or(host).to_lowercase();
    if is_host(&host) {
        Some(host)
    } else {
        None
    }
}

fn is_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    if !labels.iter().all(|label| is_host_label(label)) {
        return false;
    }
    let last = labels[labels.len() - 1];
    let len = last.chars().count();
    if !(2..=24).contains(&len) || !last.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    !FILE_LABELS.contains(&last)
}

fn is_host_label(label: &str) -> bool {
    !label.is_empty() && label.chars().all(is_label_char)
}

fn is_label_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

/// The line that reads most like the page's title.
fn best_heading(page: &OcrPage) -> Option<String> {
    let mut heights: Vec<i32> = page.lines.iter().map(|line| line.height).collect();
    heights.sort_unstable();
    let middle = heights.get(heights.len() / 2).copied().unwrap_or(0);
    let median = middle.max(1) as f32;
    let image_height = page.image_height.max(1) as f32;

    let mut best: Option<(f32, String)> = None;
    for line in &page.lines {
        let trimmed = line.text.trim();
        let text = crate::secrets::scrub(trimmed).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let word_count = text.split_whitespace().count();
        if !(2..=12).contains(&word_count) {
            continue;
        }
        let chars = text.chars().count();
        let texty = text.chars().filter(|c| is_texty_char(*c)).count();
        if texty * 5 < chars * 3 {
            continue;
        }
        if is_ui_chrome(&text) || is_time_or_date_only(&text) {
            continue;
        }

        let rel = line.height as f32 / median;
        let pos = 1.0 - (line.y as f32 / image_height);
        let words = word_count.min(6) as f32 / 6.0;
        let score = rel * 2.0 + pos + words;
        if best.as_ref().map_or(true, |(top, _)| score > *top) {
            best = Some((score, text));
        }
    }
    best.map(|(_, text)| text)
}

fn is_texty_char(c: char) -> bool {
    c.is_alphabetic() || c.is_whitespace()
}

fn trim_punctuation(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric())
}

fn is_chrome_word(word: &str) -> bool {
    CHROME_WORDS.split(' ').any(|stop| stop == word)
}

fn is_ui_chrome(text: &str) -> bool {
    text.split_whitespace().all(|word| {
        let word = trim_punctuation(word).to_lowercase();
        is_chrome_word(&word)
    })
}

fn is_time_word(word: &str) -> bool {
    matches!(word, "am" | "pm") || MONTHS.contains(&word)
}

fn is_time_or_date_only(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|chunk| !chunk.is_empty())
        .all(|chunk| {
            let letters = chunk
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_lowercase();
            letters.is_empty() || is_time_word(&letters)
        })
}

/// Make `text` safe as a filename stem on Windows and Linux, at most
/// `max_chars` characters.
pub fn sanitize_stem(text: &str, max_chars: usize) -> String {
    let mut cleaned = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }
    let mut stem = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    stem = trim_trailing_dots(&stem);
    if is_reserved_name(&stem) {
        stem.push_str(" screenshot");
    }
    if stem.chars().count() > max_chars {
        stem = truncate_at_word(&stem, max_chars);
    }
    if stem.is_empty() {
        return "Screenshot".to_string();
    }
    stem
}

fn trim_trailing_dots(text: &str) -> String {
    let mut out = text.trim_end();
    while let Some(rest) = out.strip_suffix('.') {
        out = rest.trim_end();
    }
    out.to_string()
}

fn is_reserved_name(text: &str) -> bool {
    let base = text.to_lowercase();
    if matches!(base.as_str(), "con" | "prn" | "aux" | "nul") {
        return true;
    }
    let Some(rest) = base
        .strip_prefix("com")
        .or_else(|| base.strip_prefix("lpt"))
    else {
        return false;
    };
    rest.len() == 1 && matches!(rest, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
}

fn truncate_at_word(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let head = &chars[..max_chars];
    let half = max_chars / 2;
    let cut = head
        .iter()
        .enumerate()
        .filter(|(index, c)| **c == ' ' && *index >= half)
        .map(|(index, _)| index)
        .last();
    let end = cut.unwrap_or(head.len());
    let prefix: String = head[..end].iter().collect();
    trim_trailing_dots(prefix.trim_end())
}

/// `dir/stem.ext`, or `dir/stem 2.ext`, `dir/stem 3.ext`, ... when taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    let mut counter = 2u32;
    loop {
        let candidate = dir.join(format!("{stem} {counter}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextLine;
    use chrono::{Local, TimeZone};

    fn context() -> CaptureContext {
        CaptureContext {
            app: None,
            window_title: None,
            captured_at: Local.with_ymd_and_hms(2026, 11, 27, 14, 3, 9).unwrap(),
        }
    }

    fn ocr_page(width: u32, height: u32, lines: &[(&str, i32, i32)]) -> OcrPage {
        OcrPage {
            image_width: width,
            image_height: height,
            lines: lines
                .iter()
                .map(|(text, y, h)| TextLine::new(*text, 0, *y, 600, *h))
                .collect(),
        }
    }

    fn pr_window_title() -> &'static str {
        "Pull request #283 · apex-shot/apexshot — Mozilla Firefox"
    }

    fn pr_context() -> CaptureContext {
        let mut ctx = context();
        ctx.window_title = Some(pr_window_title().to_string());
        ctx
    }

    #[test]
    fn split_window_title_cases() {
        let (subject, app) = split_window_title(pr_window_title());
        assert_eq!(subject, "Pull request #283 · apex-shot/apexshot");
        assert_eq!(app, Some("Firefox".to_string()));

        let (subject, app) = split_window_title("main.rs - apexshot - Visual Studio Code");
        assert_eq!(subject, "main.rs - apexshot");
        assert_eq!(app, Some("VS Code".to_string()));

        let (subject, app) = split_window_title("(3) Inbox - Outlook");
        assert_eq!(subject, "Inbox");
        assert_eq!(app, Some("Outlook".to_string()));

        let (subject, app) = split_window_title("Downloads");
        assert_eq!(subject, "Downloads");
        assert_eq!(app, None);
    }

    #[test]
    fn split_window_title_keeps_unknown_apps_short() {
        let (subject, app) = split_window_title("Notes - Acme Widget");
        assert_eq!(subject, "Notes");
        assert_eq!(app, Some("Acme Widget".to_string()));

        let long = "Notes - a long description of several words here";
        let (subject, app) = split_window_title(long);
        assert_eq!(subject, long);
        assert_eq!(app, None);
    }

    #[test]
    fn detect_site_reads_top_host() {
        let mut ctx = context();
        ctx.window_title = Some("Pull request — Mozilla Firefox".to_string());
        let url = "https://github.com/apex-shot/apexshot/pull/283";
        let page = ocr_page(1200, 800, &[(url, 10, 16)]);
        assert_eq!(detect_site(&page, &ctx), Some("github.com".to_string()));
    }

    #[test]
    fn detect_site_strips_www() {
        let mut ctx = context();
        ctx.window_title = Some("Example — Mozilla Firefox".to_string());
        let page = ocr_page(1200, 800, &[("www.example.org", 12, 16)]);
        assert_eq!(detect_site(&page, &ctx), Some("example.org".to_string()));
    }

    #[test]
    fn detect_site_ignores_editor_app() {
        let mut ctx = context();
        ctx.app = Some("Code.exe".to_string());
        let page = ocr_page(1200, 800, &[("www.example.org", 12, 16)]);
        assert_eq!(detect_site(&page, &ctx), None);
    }

    #[test]
    fn detect_site_rejects_filenames() {
        let mut ctx = context();
        ctx.window_title = Some("naming.rs — Mozilla Firefox".to_string());
        let page = ocr_page(1200, 800, &[("main.rs", 12, 16)]);
        assert_eq!(detect_site(&page, &ctx), None);
    }

    #[test]
    fn detect_site_ignores_bottom_lines() {
        let mut ctx = context();
        ctx.window_title = Some("Docs — Mozilla Firefox".to_string());
        let url = "https://github.com/apex-shot/apexshot";
        let page = ocr_page(1200, 800, &[(url, 500, 16)]);
        assert_eq!(detect_site(&page, &ctx), None);
    }

    #[test]
    fn suggest_name_clean_uses_title() {
        let ctx = pr_context();
        let suggestion = suggest_name(&OcrPage::default(), &ctx, NameStyle::Clean);
        assert_eq!(suggestion.stem, "Pull request #283 · apex-shot apexshot");
        assert_eq!(suggestion.app, Some("Firefox".to_string()));
        assert_eq!(suggestion.source, NameSource::WindowTitle);
    }

    #[test]
    fn suggest_name_by_app_prefixes_app() {
        let ctx = pr_context();
        let suggestion = suggest_name(&OcrPage::default(), &ctx, NameStyle::ByApp);
        assert!(suggestion.stem.starts_with("Firefox - Pull request #283"));
    }

    #[test]
    fn suggest_name_dated_prefixes_day() {
        let ctx = pr_context();
        let suggestion = suggest_name(&OcrPage::default(), &ctx, NameStyle::Dated);
        assert!(suggestion.stem.starts_with("2026-11-27 Pull request"));
    }

    #[test]
    fn suggest_name_falls_back_to_heading() {
        let mut ctx = context();
        ctx.window_title = Some("New Tab — Mozilla Firefox".to_string());
        let lines = [
            ("File Edit View History", 5, 14),
            ("Welcome to the project docs", 80, 40),
            ("Last updated 12:30", 400, 14),
        ];
        let page = ocr_page(1200, 800, &lines);
        let suggestion = suggest_name(&page, &ctx, NameStyle::Clean);
        assert_eq!(suggestion.stem, "Welcome to the project docs");
        assert_eq!(suggestion.source, NameSource::Heading);
    }

    #[test]
    fn suggest_name_uses_app_only() {
        let mut ctx = context();
        ctx.app = Some("slack.exe".to_string());
        let suggestion = suggest_name(&OcrPage::default(), &ctx, NameStyle::Clean);
        assert_eq!(suggestion.stem, "Slack screenshot");
        assert_eq!(suggestion.source, NameSource::AppOnly);
        assert_eq!(suggestion.app, Some("Slack".to_string()));
    }

    #[test]
    fn suggest_name_fallback_is_timestamped() {
        let ctx = context();
        let suggestion = suggest_name(&OcrPage::default(), &ctx, NameStyle::Clean);
        assert!(suggestion.stem.starts_with("Screenshot 2026-11-27 at "));
        assert_eq!(suggestion.source, NameSource::Fallback);
        assert_eq!(suggestion.app, None);
    }

    #[test]
    fn sanitize_stem_replaces_forbidden_chars() {
        assert_eq!(sanitize_stem("a/b:c?d", MAX_STEM_CHARS), "a b c d");
    }

    #[test]
    fn sanitize_stem_trims_trailing_dots() {
        assert_eq!(sanitize_stem("report...", MAX_STEM_CHARS), "report");
    }

    #[test]
    fn sanitize_stem_renames_reserved_names() {
        assert_eq!(sanitize_stem("con", MAX_STEM_CHARS), "con screenshot");
    }

    #[test]
    fn sanitize_stem_cuts_long_text_at_a_word() {
        let text = "abcde ".repeat(20);
        let stem = sanitize_stem(&text, MAX_STEM_CHARS);
        assert!(stem.chars().count() <= MAX_STEM_CHARS);
        assert!(!stem.ends_with(' '));
        assert!(stem.split_whitespace().all(|word| word.len() == 5));
    }

    #[test]
    fn sanitize_stem_falls_back_when_empty() {
        assert_eq!(sanitize_stem("", MAX_STEM_CHARS), "Screenshot");
    }

    #[test]
    fn unique_path_skips_taken_names() {
        let nanos = std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "apexshot-organizer-naming-test-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Shot.png"), b"one").unwrap();
        std::fs::write(dir.join("Shot 2.png"), b"two").unwrap();

        let path = unique_path(&dir, "Shot", "png");
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(path, dir.join("Shot 3.png"));
    }
}
