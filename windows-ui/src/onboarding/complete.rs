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
    let [tray_icon, hotkeys_icon, settings_icon] =
        feature_icons(crate::settings::windowing::prefers_dark_glass_theme());
    let checklist = feature_card_list(
        scale,
        &[
            (tray_icon, tray_title.as_str(), tray_body.as_str()),
            (hotkeys_icon, hotkeys_title.as_str(), hotkeys_body.as_str()),
            (settings_icon, menu_title.as_str(), menu_body.as_str()),
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

fn feature_icons(dark: bool) -> [&'static str; 3] {
    if dark {
        [
            custom::PANELS_TOP_LEFT,
            custom::KEYBOARD,
            custom::SETTINGS_2,
        ]
    } else {
        [
            custom::PANELS_TOP_LEFT_LIGHT,
            custom::KEYBOARD_LIGHT,
            custom::SETTINGS_2_LIGHT,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_reuses_the_settings_outline_icons_in_both_themes() {
        let _ = crate::test_support::with_gtk(|| {
            crate::icons::install();
            let display = gtk4::gdk::Display::default().expect("GTK display");
            let theme = gtk4::IconTheme::for_display(&display);
            for dark in [true, false] {
                for name in feature_icons(dark) {
                    assert!(theme.has_icon(name), "missing bundled icon: {name}");
                    assert_eq!(name.ends_with("-light"), !dark);
                    assert!(!name.ends_with("-symbolic"));
                }
            }
        });
    }
}
