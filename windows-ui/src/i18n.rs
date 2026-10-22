//! English-only translation facade.
//!
//! The Windows package ships no gettext catalogs yet, so `t` returns the source
//! string. The picker still persists `ui_language`, which keeps the config
//! compatible with the Linux build and makes a later catalog drop-in possible.

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

pub fn t(msgid: &str) -> String {
    msgid.to_string()
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
    let code = sanitize_ui_language(ui_language);
    if code != SYSTEM_LANGUAGE {
        return code.starts_with("ar");
    }
    std::env::var("LANGUAGE")
        .ok()
        .map(|value| value.starts_with("ar"))
        .unwrap_or(false)
}

/// Bind catalogs. A no-op Windows build keeps the call sites identical.
pub fn init(_ui_language: &str) {}

pub fn init_from_config() {
    init(&crate::config::load_config().ui_language);
}

pub fn apply_language(ui_language: &str) {
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
