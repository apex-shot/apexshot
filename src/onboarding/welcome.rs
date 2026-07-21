use gtk4::{prelude::*, Align, Box as GtkBox, Label};

use super::ui::feature_card_list;

pub fn build(content: &GtkBox) {
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
    title.set_markup("<span size='x-large' weight='bold'>Welcome to ApexShot</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let subtitle = Label::new(Some(
        "Thanks for choosing ApexShot as your screenshot companion.\n\
         Let's get you set up in just a few quick steps.",
    ));
    subtitle.set_halign(Align::Center);
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk4::Justification::Center);
    subtitle.set_width_request(500);
    subtitle.add_css_class("settings-sub-option");
    subtitle.set_margin_bottom(8);
    content.append(&subtitle);

    let features = feature_card_list(&[
        (
            "▢",
            "Area & fullscreen capture",
            "Grab a region, monitor, or the whole desktop in one hotkey",
        ),
        (
            "✎",
            "Built-in annotation editor",
            "Arrows, blur, text, and crop without leaving the app",
        ),
        (
            "●",
            "Screen recording with audio",
            "MP4 or GIF with mic and system audio when your desktop allows it",
        ),
        (
            "Aa",
            "OCR text extraction",
            "Pull text and QR codes straight from a capture",
        ),
    ]);
    features.set_margin_top(16);
    content.append(&features);
}
