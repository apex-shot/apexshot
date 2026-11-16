use gtk4::{glib, prelude::*, Align, Box as GtkBox, Button, Label, Orientation};

use crate::config::{is_cloud_logged_in, load_config, AppConfig};
use crate::i18n::t;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AccountChip {
    pub signed_in: bool,
    pub initials: String,
    pub title: String,
    pub detail: String,
    pub badge: Option<String>,
}

fn initials(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string())
}

fn capitalize(value: &str) -> String {
    let mut chars = value.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub(crate) fn chip_model(config: &AppConfig) -> AccountChip {
    if !is_cloud_logged_in(config) {
        return AccountChip {
            signed_in: false,
            initials: "?".to_string(),
            title: t("Sign in"),
            detail: t("ApexShot Cloud"),
            badge: None,
        };
    }
    let name = config.cloud_user_name.trim();
    let email = config.cloud_user_email.trim();
    let title = if !name.is_empty() {
        name.to_string()
    } else if !email.is_empty() {
        email.to_string()
    } else {
        t("ApexShot Cloud")
    };
    let detail = if !email.is_empty() && email != title {
        email.to_string()
    } else {
        t("Connected")
    };
    let tier = config.cloud_plan_tier.trim();
    AccountChip {
        signed_in: true,
        initials: initials(&title),
        title,
        detail,
        badge: (!tier.is_empty()).then(|| capitalize(tier)),
    }
}

pub fn build_account_footer(go_page: impl Fn(&'static str) + Clone + 'static) -> GtkBox {
    let footer = GtkBox::new(Orientation::Vertical, 0);
    footer.add_css_class("settings-account-box");
    footer.set_halign(Align::Fill);
    footer.set_margin_start(8);
    footer.set_margin_end(8);
    footer.set_margin_bottom(10);
    footer.set_margin_top(6);

    let chip = Button::new();
    chip.add_css_class("settings-account-chip");
    chip.set_has_frame(false);
    chip.set_hexpand(true);

    let content = GtkBox::new(Orientation::Horizontal, 8);
    content.set_hexpand(true);

    let avatar = Label::new(None);
    avatar.add_css_class("settings-account-avatar");
    avatar.set_size_request(28, 28);
    avatar.set_valign(Align::Center);
    content.append(&avatar);

    let text = GtkBox::new(Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    text.set_size_request(0, -1);
    let title = Label::new(None);
    title.add_css_class("settings-account-name");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(15);
    let detail = Label::new(None);
    detail.add_css_class("settings-account-detail");
    detail.set_xalign(0.0);
    detail.set_halign(Align::Start);
    detail.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    detail.set_max_width_chars(18);
    text.append(&title);
    text.append(&detail);
    content.append(&text);

    let badge = Label::new(None);
    badge.add_css_class("settings-status-pill");
    badge.set_valign(Align::Center);
    badge.set_visible(false);
    content.append(&badge);

    chip.set_child(Some(&content));
    footer.append(&chip);

    let widgets = ChipWidgets {
        avatar,
        title,
        detail,
        badge,
    };
    apply(&chip, &widgets, &load_config());
    chip.set_tooltip_text(Some(&widgets.title.text()));
    chip.connect_clicked(move |_| {
        let model = chip_model(&load_config());
        go_page(if model.signed_in { "7" } else { "6" });
    });

    {
        let widgets = widgets.clone();
        chip.connect_map(move |chip| apply(chip, &widgets, &load_config()));
    }
    {
        let chip = chip.downgrade();
        let widgets = widgets.clone();
        glib::timeout_add_seconds_local(2, move || {
            let Some(chip) = chip.upgrade() else {
                return glib::ControlFlow::Break;
            };
            apply(&chip, &widgets, &load_config());
            glib::ControlFlow::Continue
        });
    }

    footer
}

#[derive(Clone)]
struct ChipWidgets {
    avatar: Label,
    title: Label,
    detail: Label,
    badge: Label,
}

fn apply(chip: &Button, widgets: &ChipWidgets, config: &AppConfig) {
    let model = chip_model(config);
    widgets.avatar.set_text(&model.initials);
    widgets.title.set_text(&model.title);
    widgets.detail.set_text(&model.detail);
    chip.set_tooltip_text(Some(&if model.title == model.detail {
        model.title.clone()
    } else {
        format!("{} — {}", model.title, model.detail)
    }));
    match model.badge {
        Some(tier) => {
            widgets.badge.set_text(&tier);
            widgets.badge.set_visible(true);
        }
        None => {
            widgets.badge.set_text("");
            widgets.badge.set_visible(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(fields: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let mut config = AppConfig::default();
        fields(&mut config);
        config
    }

    #[test]
    fn signed_out_ghost_email_never_renders_as_an_account() {
        let model = chip_model(&config(|c| {
            c.cloud_user_email = "someone@example.com".into();
            c.cloud_user_name = "Someone".into();
            c.cloud_pro_plan = true;
            c.cloud_plan_tier = "pro".into();
        }));
        assert!(!model.signed_in);
        assert_eq!(model.title, "Sign in");
        assert_eq!(model.detail, "ApexShot Cloud");
        assert_eq!(model.badge, None);
    }

    #[test]
    fn signed_in_falls_back_through_name_email_placeholder() {
        let named = chip_model(&config(|c| {
            c.cloud_api_token = "t".into();
            c.cloud_user_name = "Godwin Mayodi".into();
            c.cloud_user_email = "g@example.com".into();
            c.cloud_plan_tier = "team".into();
        }));
        assert!(named.signed_in);
        assert_eq!(named.title, "Godwin Mayodi");
        assert_eq!(named.detail, "g@example.com");
        assert_eq!(named.badge.as_deref(), Some("Team"));
        assert_eq!(named.initials, "G");

        let email_only = chip_model(&config(|c| {
            c.cloud_api_token = "t".into();
            c.cloud_user_email = "g@example.com".into();
        }));
        assert_eq!(email_only.title, "g@example.com");
        assert_eq!(email_only.detail, "Connected");

        let bare = chip_model(&config(|c| {
            c.cloud_api_token = "t".into();
        }));
        assert_eq!(bare.title, "ApexShot Cloud");
        assert_eq!(bare.detail, "Connected");
    }

    #[test]
    fn badge_comes_only_from_cloud_plan_tier() {
        let legacy_pro = chip_model(&config(|c| {
            c.cloud_api_token = "t".into();
            c.cloud_pro_plan = true;
        }));
        assert_eq!(legacy_pro.badge, None);

        let empty_tier = chip_model(&config(|c| {
            c.cloud_api_token = "t".into();
            c.cloud_plan_tier = "  ".into();
        }));
        assert_eq!(empty_tier.badge, None);
    }

    #[test]
    fn initials_survive_missing_and_unicode_metadata() {
        assert_eq!(initials(""), "?");
        assert_eq!(initials("42"), "?");
        assert_eq!(initials("maya"), "M");
        let first = initials("Élodie");
        assert!(first.chars().count() == 1 && first != "?");
    }
}
