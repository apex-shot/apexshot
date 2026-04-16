use gtk4::{prelude::*, Align, Label};

pub fn build(content: &gtk4::Box) {
    // Logo (same as welcome)
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(96);
    drawing_area.set_content_height(96);
    drawing_area.set_halign(Align::Center);
    drawing_area.set_margin_bottom(16);

    drawing_area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;
        let s = width as f64 / 96.0;

        // Background Rounded Rectangle
        cr.set_source_rgba(0.08, 0.08, 0.08, 1.0);
        let size_half = 36.0 * s;
        let radius = 10.0 * s;
        cr.arc(cx + size_half - radius, cy - size_half + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(cx + size_half - radius, cy + size_half - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(cx - size_half + radius, cy + size_half - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(cx - size_half + radius, cy - size_half + radius, radius, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2);
        cr.close_path();
        cr.fill().expect("Failed to render logo background");

        let logo_scale = 2.0 * s;
        cr.translate(cx - 12.0 * logo_scale, cy - 12.0 * logo_scale);
        cr.scale(logo_scale, logo_scale);

        // Left Wing
        cr.set_source_rgba(0.913, 0.329, 0.125, 1.0);
        cr.move_to(12.0, 2.0);
        cr.line_to(2.0, 22.0);
        cr.line_to(4.0, 22.0);
        cr.line_to(12.0, 18.0);
        cr.line_to(12.0, 14.0);
        cr.line_to(8.0, 14.0);
        cr.line_to(12.0, 6.0);
        cr.close_path();
        cr.fill().expect("Failed to draw logo left wing");

        // Right Wing
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.move_to(12.0, 2.0);
        cr.line_to(22.0, 22.0);
        cr.line_to(20.0, 22.0);
        cr.line_to(12.0, 18.0);
        cr.line_to(12.0, 14.0);
        cr.line_to(16.0, 14.0);
        cr.line_to(12.0, 6.0);
        cr.close_path();
        cr.fill().expect("Failed to draw logo right wing");

        // Focus Dot
        cr.set_source_rgba(0.913, 0.329, 0.125, 1.0);
        cr.arc(12.0, 10.5, 1.5, 0.0, 2.0 * std::f64::consts::PI);
        cr.fill().expect("Failed to draw logo focus dot");
    });

    content.append(&drawing_area);

    // Title
    let title = Label::new(Some("You're all set!"));
    title.add_css_class("settings-page-title");
    title.set_halign(Align::Center);
    content.append(&title);

    // Message
    let message = Label::new(Some("ApexShot is ready to use."));
    message.add_css_class("settings-sub-option-hint");
    message.set_halign(Align::Center);
    message.set_margin_top(8);
    content.append(&message);

    // Tip
    let tip_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    tip_box.set_margin_top(32);
    tip_box.set_halign(Align::Center);

    let tip_title = Label::new(Some("Pro Tip"));
    tip_title.add_css_class("settings-group-title");
    tip_title.set_halign(Align::Center);
    tip_box.append(&tip_title);

    let tip_text = Label::new(Some("Right-click the tray icon for quick capture actions."));
    tip_text.add_css_class("settings-sub-option-hint");
    tip_text.set_halign(Align::Center);
    tip_box.append(&tip_text);

    content.append(&tip_box);
}
