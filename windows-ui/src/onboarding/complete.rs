//! Finish: what the install now has, and the action that closes setup.

use gtk4::{prelude::*, Box as GtkBox, Button};

use super::ui::{feature_card_list, heading};
use super::{px, Nav, SECTION_GAP};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    let scale = nav.scale;
    body.append(&heading(
        scale,
        &t("You’re all set."),
        &t("Here’s where to find everything."),
    ));

    let tray_title = t("Tray icon");
    let tray_body = t("Right-click for your capture menu.");
    let hotkeys_title = t("Hotkeys");
    let hotkeys_body = t("Your shortcuts work while ApexShot runs.");
    let menu_title = t("Settings");
    let menu_body = t("Change any of this later.");
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
    checklist.set_margin_top(px(scale, SECTION_GAP));
    body.append(&checklist);

    let finish_btn = Button::with_label(&t("Open ApexShot"));
    finish_btn.add_css_class("settings-primary-btn");
    let finish = nav.finish.clone();
    finish_btn.connect_clicked(move |_| finish());
    actions.append(&finish_btn);
}
