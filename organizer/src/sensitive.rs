//! Decides when a capture's text must be kept out of its name and the index.
//!
//! The organizer never describes pixels, so explicit or private material can
//! only reach a filename or the search index through text: the window title,
//! the site, or the on-screen text. When this module flags a capture, callers
//! give it a generic dated name and a date-only index entry.

use crate::{CaptureContext, OcrPage};

/// Whether a capture's text may be used for its name and the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sensitivity {
    #[default]
    Normal,
    /// Taken from a private or incognito browser window.
    PrivateWindow,
    /// The title, site, or on-screen text is explicit.
    Explicit,
}

impl Sensitivity {
    /// True when the capture must get a generic name and a date-only index entry.
    pub fn withholds_text(self) -> bool {
        self != Sensitivity::Normal
    }
}

/// Names that only appear in a private or incognito browser window.
const PRIVATE_WINDOW_MARKERS: &[&str] = &[
    "private browsing",
    "inprivate",
    "incognito",
    "private window",
];

/// Hosts known to carry explicit material.
const EXPLICIT_HOSTS: &[&str] = &[
    "pornhub.com",
    "xvideos.com",
    "xnxx.com",
    "xhamster.com",
    "redtube.com",
    "youporn.com",
    "onlyfans.com",
    "fansly.com",
    "chaturbate.com",
    "stripchat.com",
    "spankbang.com",
    "eporner.com",
    "brazzers.com",
    "rule34.xxx",
    "e-hentai.org",
    "nhentai.net",
];

/// Top-level domains that mark a site as explicit.
const EXPLICIT_TLDS: &[&str] = &["xxx", "porn", "adult", "sex"];

/// Whole words that mark a title or page text as explicit.
const EXPLICIT_WORDS: &[&str] = &[
    "porn",
    "porno",
    "pornography",
    "xxx",
    "nsfw",
    "nude",
    "nudes",
    "nudity",
    "naked",
    "sex",
    "sexy",
    "hentai",
    "milf",
    "onlyfans",
    "camgirl",
    "erotic",
    "erotica",
    "fetish",
    "bdsm",
    "blowjob",
    "cumshot",
    "anal",
    "orgasm",
    "dildo",
    "horny",
    "slut",
    "pussy",
    "cock",
    "boobs",
    "tits",
    "nipples",
    "escort",
    "stripper",
    "xvideos",
    "pornhub",
    "xhamster",
];

/// True when the window title belongs to a private or incognito window.
pub fn is_private_window(title: &str) -> bool {
    let lower = title.to_lowercase();
    PRIVATE_WINDOW_MARKERS
        .iter()
        .any(|marker| lower.contains(*marker))
}

/// True when `host` is a known explicit site or reads like one.
pub fn is_explicit_site(host: &str) -> bool {
    let lower = host.trim().to_lowercase();
    let host = lower.strip_prefix("www.").unwrap_or(&lower);
    if host.is_empty() {
        return false;
    }
    if is_known_host(host) {
        return true;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let tld = labels[labels.len() - 1];
    if EXPLICIT_TLDS.contains(&tld) {
        return true;
    }
    labels
        .iter()
        .any(|label| label.contains("porn") || label.contains("hentai"))
}

/// True when the text holds a whole explicit word or an explicit host.
pub fn contains_explicit(text: &str) -> bool {
    let lower = text.to_lowercase();
    for word in lower.split(|c: char| !c.is_alphanumeric()) {
        if EXPLICIT_WORDS.contains(&word) {
            return true;
        }
    }
    for token in lower.split_whitespace() {
        let token = token
            .strip_prefix("https://")
            .or_else(|| token.strip_prefix("http://"))
            .unwrap_or(token);
        let end = token.find('/').unwrap_or(token.len());
        let host = &token[..end];
        if host.contains('.') && is_explicit_site(host) {
            return true;
        }
    }
    false
}

/// Classify a capture, checking the private window first.
pub fn classify(page: &OcrPage, ctx: &CaptureContext, site: Option<&str>) -> Sensitivity {
    if let Some(title) = ctx.window_title.as_deref() {
        if is_private_window(title) {
            return Sensitivity::PrivateWindow;
        }
    }
    if let Some(site) = site {
        if is_explicit_site(site) {
            return Sensitivity::Explicit;
        }
    }
    if let Some(title) = ctx.window_title.as_deref() {
        if contains_explicit(title) {
            return Sensitivity::Explicit;
        }
    }
    if contains_explicit(&page.full_text()) {
        return Sensitivity::Explicit;
    }
    Sensitivity::Normal
}

fn is_known_host(host: &str) -> bool {
    EXPLICIT_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
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

    fn titled(title: &str) -> CaptureContext {
        let mut ctx = context();
        ctx.window_title = Some(title.to_string());
        ctx
    }

    fn page(text: &str) -> OcrPage {
        OcrPage {
            image_width: 800,
            image_height: 600,
            lines: vec![TextLine::new(text, 0, 0, 100, 20)],
        }
    }

    #[test]
    fn private_window_titles_are_recognised() {
        assert!(is_private_window("Docs — Mozilla Firefox Private Browsing"));
        assert!(is_private_window("New Tab - Google Chrome (Incognito)"));
        assert!(is_private_window("[InPrivate] Bing - Microsoft Edge"));
        assert!(!is_private_window("Private equity report - Word"));
    }

    #[test]
    fn explicit_sites_are_recognised() {
        assert!(is_explicit_site("www.pornhub.com"));
        assert!(is_explicit_site("de.xhamster.com"));
        assert!(is_explicit_site("example.xxx"));
        assert!(!is_explicit_site("github.com"));
        assert!(!is_explicit_site("essex.ac.uk"));
    }

    #[test]
    fn explicit_text_is_recognised() {
        assert!(contains_explicit("NSFW thread"));
        assert!(!contains_explicit("Data analysis.xlsx"));
        assert!(!contains_explicit("Essex cocktail bar"));
        assert!(contains_explicit("visit https://xvideos.com/abc"));
        assert!(!contains_explicit(""));
    }

    #[test]
    fn classify_normal_capture() {
        let page = page("hello");
        let ctx = titled("Inbox - Outlook");
        assert_eq!(classify(&page, &ctx, None), Sensitivity::Normal);
    }

    #[test]
    fn classify_explicit_site() {
        let page = page("hello");
        let ctx = titled("Inbox - Outlook");
        assert_eq!(
            classify(&page, &ctx, Some("onlyfans.com")),
            Sensitivity::Explicit
        );
    }

    #[test]
    fn classify_explicit_page_text() {
        let page = page("nude photos");
        let ctx = titled("Inbox - Outlook");
        assert_eq!(classify(&page, &ctx, None), Sensitivity::Explicit);
    }

    #[test]
    fn classify_private_window_wins_over_explicit_text() {
        let page = page("nude photos");
        let ctx = titled("Docs — Mozilla Firefox Private Browsing");
        assert_eq!(classify(&page, &ctx, None), Sensitivity::PrivateWindow);
    }
}
