use gtk4::{prelude::*, Box as GtkBox, Button};

use super::ui::{feature_card_list, heading, tip_block};
use super::Nav;
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
    let hotkeys_title = t("Hotkeys");
    let hotkeys_body = if crate::app_identity::portal_only() {
        t("Available when your desktop supports the GlobalShortcuts portal")
    } else {
        t("Capture without opening Settings every time")
    };
    let menu_title = t("App menu");
    let menu_body = t("Open ApexShot anytime for Settings and preferences");
    let checklist = feature_card_list(&[
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
    ]);
    checklist.set_margin_top(28);
    body.append(&checklist);

    let tip_title = t("Pro tip");
    let tip_body =
        t("If a hotkey conflicts with your desktop, change it under Settings → Shortcuts.");
    let tip = tip_block(&tip_title, &tip_body);
    tip.set_margin_top(24);
    body.append(&tip);

    let finish_btn = Button::with_label(&t("Start Using ApexShot"));
    finish_btn.add_css_class("settings-primary-btn");
    let finish = nav.finish.clone();
    finish_btn.connect_clicked(move |_| finish());
    actions.append(&finish_btn);
}
