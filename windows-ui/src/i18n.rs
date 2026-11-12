//! UI translations from the shared `po/` catalogs.
//!
//! English source strings are the message ids. The Linux build compiles the
//! same catalogs to `.mo` files for gettext; this build embeds the `.po`
//! sources and parses the active one on first use, so the package needs no
//! locale directory. The active language is `AppConfig.ui_language`
//! (`system` follows the Windows display language).

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub const GETTEXT_PACKAGE: &str = "apexshot";
pub const SYSTEM_LANGUAGE: &str = "system";

#[derive(Clone, Copy, Debug)]
pub struct UiLanguage {
    pub code: &'static str,
    pub native_name: &'static str,
}

pub const UI_LANGUAGES: &[UiLanguage] = &[
    UiLanguage {
        code: SYSTEM_LANGUAGE,
        native_name: "System default",
    },
    UiLanguage {
        code: "en",
        native_name: "English",
    },
    UiLanguage {
        code: "es",
        native_name: "Español",
    },
    UiLanguage {
        code: "fr",
        native_name: "Français",
    },
    UiLanguage {
        code: "de",
        native_name: "Deutsch",
    },
    UiLanguage {
        code: "pt_BR",
        native_name: "Português (Brasil)",
    },
    UiLanguage {
        code: "zh_CN",
        native_name: "简体中文",
    },
    UiLanguage {
        code: "ja",
        native_name: "日本語",
    },
    UiLanguage {
        code: "ru",
        native_name: "Русский",
    },
    UiLanguage {
        code: "ar",
        native_name: "العربية",
    },
];

type Catalog = HashMap<String, String>;

const CATALOG_SOURCES: &[(&str, &str)] = &[
    ("es", include_str!("../../po/es.po")),
    ("fr", include_str!("../../po/fr.po")),
    ("de", include_str!("../../po/de.po")),
    ("pt_BR", include_str!("../../po/pt_BR.po")),
    ("zh_CN", include_str!("../../po/zh_CN.po")),
    ("ja", include_str!("../../po/ja.po")),
    ("ru", include_str!("../../po/ru.po")),
    ("ar", include_str!("../../po/ar.po")),
];

static ACTIVE: RwLock<Option<&'static Catalog>> = RwLock::new(None);

/// Translate `msgid`. Falls back to the English source string.
pub fn t(msgid: &str) -> String {
    let active = *ACTIVE.read().unwrap_or_else(|error| error.into_inner());
    active
        .and_then(|catalog| catalog.get(msgid))
        .cloned()
        .unwrap_or_else(|| msgid.to_string())
}

pub fn tfmt(msgid: &str, args: &[(&str, &str)]) -> String {
    let mut out = t(msgid);
    for (key, value) in args {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

pub fn escape_markup(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn markup_title(msgid: &str) -> String {
    format!(
        "<span size='x-large' weight='bold'>{}</span>",
        escape_markup(&t(msgid))
    )
}

pub fn markup_bold(msgid: &str) -> String {
    format!("<span weight='bold'>{}</span>", escape_markup(&t(msgid)))
}

pub fn sanitize_ui_language(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return SYSTEM_LANGUAGE.to_string();
    }
    if UI_LANGUAGES.iter().any(|lang| lang.code == trimmed) {
        return trimmed.to_string();
    }
    SYSTEM_LANGUAGE.to_string()
}

pub fn is_rtl(ui_language: &str) -> bool {
    resolved_language(ui_language).starts_with("ar")
}

/// Load the catalog for `ui_language`. Safe to call more than once.
pub fn init(ui_language: &str) {
    let catalog = catalog(resolved_language(ui_language));
    *ACTIVE.write().unwrap_or_else(|error| error.into_inner()) = catalog;
}

pub fn init_from_config() {
    init(&crate::config::load_config().ui_language);
}

/// Re-apply a language in an already-running process.
pub fn apply_language(ui_language: &str) {
    init(ui_language);
    apply_gtk_direction(ui_language);
}

pub fn apply_gtk_direction(ui_language: &str) {
    gtk4::Widget::set_default_direction(if is_rtl(ui_language) {
        gtk4::TextDirection::Rtl
    } else {
        gtk4::TextDirection::Ltr
    });
}

pub fn empty_shortcut_label() -> String {
    t("Record shortcut")
}

pub fn is_empty_shortcut_label(label: &str) -> bool {
    label.is_empty() || label == "Record shortcut" || label == t("Record shortcut")
}

fn resolved_language(ui_language: &str) -> &'static str {
    let code = sanitize_ui_language(ui_language);
    if code == SYSTEM_LANGUAGE {
        return system_language();
    }
    UI_LANGUAGES
        .iter()
        .find(|lang| lang.code == code)
        .map(|lang| lang.code)
        .unwrap_or("en")
}

/// The first preferred language with a catalog (or English), honouring a
/// `LANGUAGE` override before the Windows display-language list.
fn system_language() -> &'static str {
    let candidates: Vec<String> = match std::env::var("LANGUAGE")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(value) => value.split(':').map(str::to_owned).collect(),
        None => system_locale_names(),
    };
    candidates
        .iter()
        .find_map(|tag| match_locale(tag))
        .unwrap_or("en")
}

/// Map a locale tag (`de-DE`, `pt_PT`, `zh-Hans-CN`, `fr_CA.UTF-8`) to a
/// shipped language code.
fn match_locale(tag: &str) -> Option<&'static str> {
    let tag = tag.trim().split('.').next().unwrap_or("").replace('-', "_");
    let lower = tag.to_ascii_lowercase();
    if let Some(lang) = UI_LANGUAGES
        .iter()
        .find(|lang| lang.code != SYSTEM_LANGUAGE && lang.code.eq_ignore_ascii_case(&tag))
    {
        return Some(lang.code);
    }
    let mut parts = lower.split('_');
    let base = parts.next().unwrap_or("");
    if base == "zh" {
        // Only the Simplified catalog ships; Traditional readers get English.
        let traditional = parts.any(|part| matches!(part, "hant" | "tw" | "hk" | "mo"));
        return (!traditional).then_some("zh_CN");
    }
    UI_LANGUAGES
        .iter()
        .find(|lang| lang.code != SYSTEM_LANGUAGE && lang.code.split('_').next() == Some(base))
        .map(|lang| lang.code)
}

#[cfg(windows)]
fn system_locale_names() -> Vec<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

    let mut count = 0u32;
    let mut len = 0u32;
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if sized == 0 || len == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; len as usize];
    let filled = unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buffer.as_mut_ptr(), &mut len)
    };
    if filled == 0 {
        return Vec::new();
    }
    buffer
        .split(|&unit| unit == 0)
        .filter(|name| !name.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(not(windows))]
fn system_locale_names() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .into_iter()
        .collect()
}

fn catalog(code: &str) -> Option<&'static Catalog> {
    static PARSED: OnceLock<Vec<OnceLock<Catalog>>> = OnceLock::new();
    let index = CATALOG_SOURCES
        .iter()
        .position(|(source_code, _)| *source_code == code)?;
    let slots = PARSED.get_or_init(|| CATALOG_SOURCES.iter().map(|_| OnceLock::new()).collect());
    Some(slots[index].get_or_init(|| parse_po(CATALOG_SOURCES[index].1)))
}

enum PoField {
    None,
    Id,
    Str,
}

/// Parse the subset of PO the catalogs use: `msgid`/`msgstr` pairs with
/// continuation lines. Fuzzy and untranslated entries are skipped.
fn parse_po(source: &str) -> Catalog {
    let mut catalog = Catalog::new();
    let mut msgid = String::new();
    let mut msgstr = String::new();
    let mut field = PoField::None;
    let mut fuzzy = false;
    let mut next_fuzzy = false;
    for line in source.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("msgid ") {
            insert_entry(&mut catalog, &mut msgid, &mut msgstr, fuzzy);
            fuzzy = next_fuzzy;
            next_fuzzy = false;
            msgid.push_str(&unquote_po(rest));
            field = PoField::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            msgstr.push_str(&unquote_po(rest));
            field = PoField::Str;
        } else if line.starts_with('"') {
            match field {
                PoField::Id => msgid.push_str(&unquote_po(line)),
                PoField::Str => msgstr.push_str(&unquote_po(line)),
                PoField::None => {}
            }
        } else if line.starts_with("#,") {
            next_fuzzy = line.contains("fuzzy");
            field = PoField::None;
        } else if !line.is_empty() {
            field = PoField::None;
        }
    }
    insert_entry(&mut catalog, &mut msgid, &mut msgstr, fuzzy);
    catalog
}

fn insert_entry(catalog: &mut Catalog, msgid: &mut String, msgstr: &mut String, fuzzy: bool) {
    if !fuzzy && !msgid.is_empty() && !msgstr.is_empty() {
        catalog.insert(std::mem::take(msgid), std::mem::take(msgstr));
    }
    msgid.clear();
    msgstr.clear();
}

fn unquote_po(token: &str) -> String {
    let inner = token
        .trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or("");
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_language_has_a_parsed_catalog() {
        for lang in UI_LANGUAGES {
            if lang.code == SYSTEM_LANGUAGE || lang.code == "en" {
                continue;
            }
            let catalog = catalog(lang.code).expect(lang.code);
            assert!(
                catalog.len() > 1000,
                "{} has {} entries",
                lang.code,
                catalog.len()
            );
            assert!(catalog.contains_key("Display language"), "{}", lang.code);
        }
        assert!(catalog("en").is_none());
    }

    #[test]
    fn catalog_lookup_returns_the_translation() {
        assert_eq!(catalog("de").unwrap()["Settings"], "Einstellungen");
        assert_eq!(
            catalog("de").unwrap()["Record shortcut"],
            "Tastenkürzel aufnehmen"
        );
    }

    #[test]
    fn parse_po_joins_continuations_and_unescapes() {
        let source = concat!(
            "msgid \"\"\n",
            "msgstr \"\"\n",
            "\"Language: de\\n\"\n",
            "\n",
            "msgid \"\"\n",
            "\"Two \"\n",
            "\"lines\"\n",
            "msgstr \"Zwei \\\"Zeilen\\\"\\n\"\n",
            "\n",
            "#, fuzzy\n",
            "msgid \"Fuzzy\"\n",
            "msgstr \"Unsicher\"\n",
            "\n",
            "msgid \"Untranslated\"\n",
            "msgstr \"\"\n",
            "\n",
            "msgid \"Last\"\n",
            "msgstr \"Letzte\"\n",
        );
        let catalog = parse_po(source);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog["Two lines"], "Zwei \"Zeilen\"\n");
        assert_eq!(catalog["Last"], "Letzte");
    }

    #[test]
    fn locale_tags_map_to_shipped_languages() {
        assert_eq!(match_locale("de-DE"), Some("de"));
        assert_eq!(match_locale("fr_CA.UTF-8"), Some("fr"));
        assert_eq!(match_locale("pt-BR"), Some("pt_BR"));
        assert_eq!(match_locale("pt-PT"), Some("pt_BR"));
        assert_eq!(match_locale("zh-Hans-CN"), Some("zh_CN"));
        assert_eq!(match_locale("zh-CN"), Some("zh_CN"));
        assert_eq!(match_locale("zh-TW"), None);
        assert_eq!(match_locale("zh-Hant-HK"), None);
        assert_eq!(match_locale("en-US"), Some("en"));
        assert_eq!(match_locale("ko-KR"), None);
        assert_eq!(match_locale(""), None);
    }

    #[test]
    fn explicit_language_resolves_without_the_system_list() {
        assert_eq!(resolved_language("ja"), "ja");
        assert_eq!(resolved_language("pt_BR"), "pt_BR");
        assert!(is_rtl("ar"));
        assert!(!is_rtl("de"));
    }

    #[test]
    fn unknown_language_falls_back_to_system() {
        assert_eq!(sanitize_ui_language(""), SYSTEM_LANGUAGE);
        assert_eq!(sanitize_ui_language("nope"), SYSTEM_LANGUAGE);
        assert_eq!(sanitize_ui_language("es"), "es");
    }
}
