//! Finish: what the install now has, and the action that closes setup.

use gtk4::{prelude::*, Box as GtkBox, Button};

use super::ui::{feature_card_list, heading};
use super::{px, Nav};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let scale = nav.scale;
    body.append(&heading(
        scale,
        &t("You're all set!"),
        &t("ApexShot is ready. It runs from the tray from now on."),
    ));

    let tray_title = t("Tray icon");
    let tray_body = t("ApexShot stays in the tray; right-click it for the capture menu.");
    let hotkeys_title = t("Hotkeys");
    let hotkeys_body = t("Use your configured shortcuts while ApexShot runs in the background.");
    let menu_title = t("Settings");
    let menu_body = t("Open ApexShot anytime for Settings and preferences");
    let checklist = feature_card_list(
        scale,
        &[
            (
                custom::OVERLAPPING_WINDOWS_SYMBOLIC,
                tray_title.as_str(),
                tray_body.as_str(),
            ),
            (
                custom::KEYBOARD_SHORTCUTS_SYMBOLIC,
                hotkeys_title.as_str(),
                hotkeys_body.as_str(),
            ),
            (
                custom::SETTINGS_SYMBOLIC,
                menu_title.as_str(),
                menu_body.as_str(),
            ),
        ],
    );
    checklist.set_margin_top(px(scale, 28.0));
    body.append(&checklist);

    let finish_btn = Button::with_label(&t("Start Using ApexShot"));
    finish_btn.add_css_class("settings-primary-btn");
    let finish = nav.finish.clone();
    finish_btn.connect_clicked(move |_| finish());
    actions.append(&finish_btn);
}
