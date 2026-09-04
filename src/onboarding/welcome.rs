use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};
use std::cell::Cell;
use std::rc::Rc;

use super::ui::feature_card_list;
use crate::capture::editor::window::icon_names::custom;
use crate::config::{load_config, save_config};
use crate::i18n::{self, t};
use crate::settings::select::language_combo;

pub fn build(content: &GtkBox, on_language_changed: impl Fn() + 'static) {
    let config = load_config().sanitized();
    let lang_row = GtkBox::new(Orientation::Horizontal, 10);
    lang_row.set_halign(Align::Center);
    lang_row.set_margin_bottom(16);
    let lang_label = Label::new(Some(&t("Language")));
    lang_label.add_css_class("settings-sub-option");
    lang_label.set_valign(Align::Center);
    let lang_combo = language_combo(&config.ui_language);
    lang_row.append(&lang_label);
    lang_row.append(lang_combo.widget());
    content.append(&lang_row);

    let applying = Rc::new(Cell::new(false));
    lang_combo.connect_changed({
        let lang_combo = lang_combo.clone();
        move || {
            if applying.get() {
                return;
            }
            let Some(code) = lang_combo.active_id() else {
                return;
            };
            let code = i18n::sanitize_ui_language(&code);
            let mut config = load_config();
            if config.ui_language == code {
                return;
            }
            applying.set(true);
            config.ui_language = code.clone();
            if let Err(err) = save_config(&config.sanitized()) {
                eprintln!("[onboarding] Failed to save language: {err}");
                applying.set(false);
                return;
            }
            i18n::apply_language(&code);
            on_language_changed();
        }
    });

    // Logo (using the same curved arch as about page)
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(96);
    drawing_area.set_content_height(96);
    drawing_area.set_halign(Align::Center);
    drawing_area.set_margin_bottom(12);

    drawing_area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        // Scale from 24x24 viewBox to widget size, centered
        let scale = width as f64 / 24.0;
        cr.translate(cx - 12.0 * scale, cy - 12.0 * scale);
        cr.scale(scale, scale);

        // Path: M 2 21 C 6 21, 8 2, 12 2 C 16 2, 18 21, 22 21
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
    title.set_markup(&i18n::markup_title("Welcome to ApexShot"));
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let subtitle = Label::new(Some(&t(
        "Thanks for choosing ApexShot as your screenshot companion.\nLet's get you set up in just a few quick steps.",
    )));
    subtitle.set_halign(Align::Center);
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk4::Justification::Center);
    subtitle.set_width_request(500);
    subtitle.add_css_class("settings-sub-option");
    subtitle.set_margin_bottom(8);
    content.append(&subtitle);

    let area_title = t("Area & fullscreen capture");
    let area_body = t("Grab a region, monitor, or the whole desktop in one hotkey");
    let annotate_title = t("Built-in annotation editor");
    let annotate_body = t("Arrows, blur, text, and crop without leaving the app");
    let record_title = t("Screen recording with audio");
    let record_body = t("MP4 or GIF with mic and system audio when your desktop allows it");
    let ocr_title = t("OCR text extraction");
    let ocr_body = t("Pull text and QR codes straight from a capture");
    let features = feature_card_list(&[
        (
            custom::SCREENSHOOTER_SYMBOLIC,
            area_title.as_str(),
            area_body.as_str(),
        ),
        (
            custom::PENCIL_SYMBOLIC,
            annotate_title.as_str(),
            annotate_body.as_str(),
        ),
        (
            custom::RECORD_SCREEN_SYMBOLIC,
            record_title.as_str(),
            record_body.as_str(),
        ),
        (
            custom::FONT_X_GENERIC_SYMBOLIC,
            ocr_title.as_str(),
            ocr_body.as_str(),
        ),
    ]);
    features.set_margin_top(16);
    content.append(&features);
}
