//! Gettext-based UI translations.
//!
//! English source strings are the message ids. Catalogs live in `po/` and are
//! compiled to `.mo` files at build time. The active language is
//! `AppConfig.ui_language` (`system` follows the desktop locale).

use gettextrs::{
    bind_textdomain_codeset, bindtextdomain, gettext, setlocale, textdomain, LocaleCategory,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const GETTEXT_PACKAGE: &str = "apexshot";
pub const SYSTEM_LANGUAGE: &str = "system";

#[derive(Clone, Copy, Debug)]
pub struct UiLanguage {
    /// Config / gettext code (`system`, `es`, `pt_BR`, …).
    pub code: &'static str,
    /// Native name of the language, shown untranslated in the picker.
    pub native_name: &'static str,
}

/// Languages we ship catalogs for, plus system default and English source.
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

static ORIGINAL_LANGUAGE: OnceLock<Option<String>> = OnceLock::new();

/// Translate `msgid`. Falls back to the English source string.
pub fn t(msgid: &str) -> String {
    gettext(msgid)
}

/// Translate `msgid` and replace `{key}` placeholders.
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

/// Bind catalogs and apply `ui_language`. Safe to call more than once.
pub fn init(ui_language: &str) {
    ORIGINAL_LANGUAGE.get_or_init(|| std::env::var("LANGUAGE").ok());
    let _ = setlocale(LocaleCategory::LcAll, "");
    apply_language_env(ui_language);
    let dir = locale_dir();
    let _ = bindtextdomain(GETTEXT_PACKAGE, dir.to_string_lossy().as_ref());
    let _ = bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
    let _ = textdomain(GETTEXT_PACKAGE);
    flush_gettext_cache();
}

pub fn init_from_config() {
    let language = crate::config::load_config().ui_language;
    init(&language);
}

/// Re-apply a language in an already-running process (onboarding picker).
pub fn apply_language(ui_language: &str) {
    apply_language_env(ui_language);
    let _ = textdomain(GETTEXT_PACKAGE);
    flush_gettext_cache();
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

fn apply_language_env(ui_language: &str) {
    let original = ORIGINAL_LANGUAGE.get_or_init(|| std::env::var("LANGUAGE").ok());
    let code = sanitize_ui_language(ui_language);
    if code == SYSTEM_LANGUAGE {
        match original {
            Some(value) => std::env::set_var("LANGUAGE", value),
            None => std::env::remove_var("LANGUAGE"),
        }
    } else {
        std::env::set_var("LANGUAGE", code);
    }
}

fn resolved_language(ui_language: &str) -> String {
    let code = sanitize_ui_language(ui_language);
    if code != SYSTEM_LANGUAGE {
        return code;
    }
    std::env::var("LANGUAGE")
        .ok()
        .and_then(|value| value.split(':').next().map(|part| part.replace('-', "_")))
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .map(|value| value.replace('-', "_"))
        .unwrap_or_else(|| "en".to_string())
}

fn locale_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("APEXSHOT_LOCALEDIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return path;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let next_to_binary = parent.join("locale");
            if looks_like_localedir(&next_to_binary) {
                return next_to_binary;
            }
            let share = parent.join("../share/locale");
            if looks_like_localedir(&share) {
                return share.canonicalize().unwrap_or(share);
            }
        }
    }
    PathBuf::from("/usr/share/locale")
}

fn looks_like_localedir(dir: &Path) -> bool {
    UI_LANGUAGES.iter().any(|lang| {
        lang.code != SYSTEM_LANGUAGE
            && lang.code != "en"
            && dir
                .join(lang.code)
                .join("LC_MESSAGES")
                .join(format!("{GETTEXT_PACKAGE}.mo"))
                .is_file()
    })
}

fn flush_gettext_cache() {
    // glibc gettext caches catalogs; bumping this counter forces a reload after
    // LANGUAGE changes in a long-lived process (onboarding / settings).
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        extern "C" {
            static mut _nl_msg_cat_cntr: libc::c_int;
        }
        _nl_msg_cat_cntr += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_falls_back_to_system() {
        assert_eq!(sanitize_ui_language(""), SYSTEM_LANGUAGE);
        assert_eq!(sanitize_ui_language("nope"), SYSTEM_LANGUAGE);
        assert_eq!(sanitize_ui_language("es"), "es");
        assert_eq!(sanitize_ui_language("pt_BR"), "pt_BR");
    }

    #[test]
    fn t_without_catalog_returns_msgid() {
        assert_eq!(t("Welcome to ApexShot"), "Welcome to ApexShot");
    }

    #[test]
    fn tfmt_replaces_named_placeholders() {
        assert_eq!(
            tfmt("Version {version}", &[("version", "1.2.3")]),
            "Version 1.2.3"
        );
    }

    #[test]
    fn empty_shortcut_sentinel_matches_english_source() {
        assert!(is_empty_shortcut_label("Record shortcut"));
        assert!(is_empty_shortcut_label(""));
        assert!(!is_empty_shortcut_label("Ctrl+A"));
    }
}
