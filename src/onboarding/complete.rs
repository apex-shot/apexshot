use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};

use super::ui::{feature_card_list, tip_block};
use crate::capture::editor::window::icon_names::custom;
use crate::i18n::{self, t};

pub fn build(content: &GtkBox) {
    // Logo (using the same curved arch as welcome page)
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(96);
    drawing_area.set_content_height(96);
    drawing_area.set_halign(Align::Center);
    drawing_area.set_margin_bottom(12);

    drawing_area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        let scale = width as f64 / 24.0;
        cr.translate(cx - 12.0 * scale, cy - 12.0 * scale);
        cr.scale(scale, scale);

        cr.set_source_rgba(0.913, 0.329, 0.125, 1.0); // #E95420
        cr.set_line_width(2.5);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        cr.move_to(2.0, 21.0);
        cr.curve_to(6.0, 21.0, 8.0, 2.0, 12.0, 2.0);
        cr.curve_to(16.0, 2.0, 18.0, 21.0, 22.0, 21.0);
        cr.stroke().expect("Failed to draw logo");
    });

    content.append(&drawing_area);

    let title = Label::new(None);
    title.set_markup(&i18n::markup_title("You're all set!"));
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let message = Label::new(Some(&t(
        "The tray daemon starts when you finish so hotkeys and captures work right away.",
    )));
    message.set_halign(Align::Center);
    message.set_wrap(true);
    message.set_justify(gtk4::Justification::Center);
    message.set_width_request(500);
    message.add_css_class("settings-sub-option");
    content.append(&message);

    let tray_title = t("Tray icon");
    let tray_body = t("Right-click for Area, Screen, and Record");
    let hotkeys_title = t("Hotkeys");
    let hotkeys_body = t("Capture without opening Settings every time");
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
    checklist.set_margin_top(18);
    content.append(&checklist);

    let tip_wrap = GtkBox::new(Orientation::Vertical, 0);
    tip_wrap.set_margin_top(18);
    tip_wrap.set_halign(Align::Center);
    let tip_title = t("Pro tip");
    let tip_body =
        t("If a hotkey conflicts with your desktop, change it under Settings → Shortcuts.");
    tip_wrap.append(&tip_block(&tip_title, &tip_body));
    content.append(&tip_wrap);
}
