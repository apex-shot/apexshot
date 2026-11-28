//! Finish: what the install now has, and the action that closes setup.

use gtk4::{prelude::*, Box as GtkBox, Button};

use super::ui::{feature_card_list, heading};
use super::{Nav, SECTION_GAP};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let message = if crate::app_identity::portal_only() {
        t("The tray daemon starts when you finish. Capture from its menu; global hotkeys depend on desktop portal support and approval.")
    } else {
        t("The tray daemon starts when you finish so hotkeys and captures work right away.")
    };
    body.append(&heading(&t("You're all set!"), &message));

    let tray_title = t("Tray icon");
    let tray_body = t("Right-click for Area, Screen, and Record");
    let naming_title = t("Screenshots name themselves");
    // One line, like the other items: the page must fit without scrolling.
    let naming_body = t("Search them by their text in History.");
    let menu_title = t("App menu");
    let menu_body = t("Open ApexShot anytime for Settings, shortcuts, and preferences");
    // The shortcuts page just before this one already covers hotkeys.
    let search_icon = if crate::settings::windowing::prefers_dark_glass_theme() {
        custom::SEARCH
    } else {
        custom::SEARCH_LIGHT
    };
    let checklist = feature_card_list(&[
        (
            custom::OVERLAPPING_WINDOWS_SYMBOLIC,
            tray_title.as_str(),
            tray_body.as_str(),
        ),
        (search_icon, naming_title.as_str(), naming_body.as_str()),
        (
            custom::SETTINGS_SYMBOLIC,
            menu_title.as_str(),
            menu_body.as_str(),
        ),
    ]);
    checklist.set_margin_top(SECTION_GAP);
    body.append(&checklist);

    let finish_btn = Button::with_label(&t("Start Using ApexShot"));
    finish_btn.add_css_class("settings-primary-btn");
    let finish = nav.finish.clone();
    finish_btn.connect_clicked(move |_| finish());
    actions.append(&finish_btn);
}
