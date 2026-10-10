use gtk4::{prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Stack};

use super::Nav;
use crate::config::{load_config, save_config};
use crate::i18n::{self, t};
use crate::settings::select::language_combo;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav, mascot: &Stack) {
    body.set_vexpand(true);
    let centered = GtkBox::new(Orientation::Vertical, 12);
    centered.add_css_class("onboarding-welcome-content");
    centered.set_width_request(560);
    centered.set_halign(Align::Center);
    centered.set_valign(Align::Center);
    centered.append(mascot);

    let heading = GtkBox::new(Orientation::Vertical, 8);
    heading.set_halign(Align::Center);
    let title = Label::new(Some(&t("Welcome to ApexShot")));
    title.add_css_class("onboarding-headline");
    title.set_halign(Align::Center);
    title.set_xalign(0.5);
    title.set_wrap(true);
    heading.append(&title);
    let copy = Label::new(Some(&t(
        "Your screenshot companion for Linux. Set it up in a few quick steps.",
    )));
    copy.add_css_class("onboarding-copy");
    copy.set_halign(Align::Center);
    copy.set_xalign(0.5);
    copy.set_wrap(true);
    heading.append(&copy);
    centered.append(&heading);

    let config = load_config().sanitized();
    let lang_row = GtkBox::new(Orientation::Horizontal, 12);
    lang_row.add_css_class("onboarding-language-row");
    lang_row.set_halign(Align::Center);
    lang_row.set_margin_top(20);
    let lang_label = Label::new(Some(&t("Language")));
    lang_label.add_css_class("settings-sub-option");
    lang_label.set_valign(Align::Center);
    let lang_combo = language_combo(&config.ui_language);
    lang_row.append(&lang_label);
    lang_row.append(lang_combo.widget());
    centered.append(&lang_row);
    body.append(&centered);

    let rebuild = nav.rebuild.clone();
    lang_combo.connect_changed({
        let lang_combo = lang_combo.clone();
        move || {
            let Some(code) = lang_combo.active_id() else {
                return;
            };
            let code = i18n::sanitize_ui_language(&code);
            let mut config = load_config();
            if config.ui_language == code {
                return;
            }
            config.ui_language = code.clone();
            if let Err(err) = save_config(&config.sanitized()) {
                eprintln!("[onboarding] Failed to save language: {err}");
                return;
            }
            i18n::apply_language(&code);
            rebuild();
        }
    });

    let get_started = Button::with_label(&t("Get started"));
    get_started.add_css_class("settings-primary-btn");
    get_started.set_size_request(220, -1);
    let advance = nav.advance.clone();
    get_started.connect_clicked(move |_| advance());
    actions.append(&get_started);
}
