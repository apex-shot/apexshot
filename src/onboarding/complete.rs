use gtk4::{prelude::*, Align, Label};

pub fn build(content: &gtk4::Box) {
    // Logo (using the same curved arch as welcome page)
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(128);
    drawing_area.set_content_height(128);
    drawing_area.set_halign(Align::Center);
    drawing_area.set_margin_bottom(24);

    drawing_area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        // Scale from 24x24 viewBox to 128x128, centered
        let scale = width as f64 / 24.0;
        cr.translate(cx - 12.0 * scale, cy - 12.0 * scale);
        cr.scale(scale, scale);

        // Draw the curved arch shape
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

    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>You're all set!</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Message
    let message = Label::new(Some("ApexShot is ready to use."));
    message.set_halign(Align::Center);
    message.set_wrap(true);
    message.set_width_request(500);
    message.add_css_class("settings-sub-option");
    content.append(&message);

    // Tip
    let tip_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    tip_box.set_margin_top(32);
    tip_box.set_halign(Align::Center);

    let tip_title = Label::new(None);
    tip_title.set_markup("<span weight='bold'>Pro Tip</span>");
    tip_title.set_halign(Align::Center);
    tip_title.set_margin_bottom(4);
    tip_box.append(&tip_title);

    let tip_text = Label::new(Some("Right-click the tray icon for quick capture actions."));
    tip_text.set_halign(Align::Center);
    tip_text.set_wrap(true);
    tip_text.set_width_request(500);
    tip_text.add_css_class("settings-sub-option");
    tip_box.append(&tip_text);

    content.append(&tip_box);
}
