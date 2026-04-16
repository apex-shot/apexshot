use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};

pub fn build(content: &GtkBox) {
    // Logo (using the same procedural drawing as about page)
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

        // Left Wing - Apex Energy (#E95420)
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

        // Right Wing - White
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

        // Lens Focus Dot
        cr.set_source_rgba(0.913, 0.329, 0.125, 1.0);
        cr.arc(12.0, 10.5, 1.5, 0.0, 2.0 * std::f64::consts::PI);
        cr.fill().expect("Failed to draw logo focus dot");
    });

    content.append(&drawing_area);

    // Title
    let title = Label::new(Some("Welcome to ApexShot"));
    title.add_css_class("settings-page-title");
    title.set_halign(Align::Center);
    content.append(&title);

    // Subtitle
    let subtitle = Label::new(Some(
        "Thanks for choosing ApexShot as your screenshot companion.\nLet's get you set up in just a few quick steps.",
    ));
    subtitle.add_css_class("settings-sub-option-hint");
    subtitle.set_halign(Align::Center);
    subtitle.set_margin_top(8);
    content.append(&subtitle);

    // Feature highlights
    let features_box = GtkBox::new(Orientation::Vertical, 8);
    features_box.set_margin_top(24);
    features_box.set_halign(Align::Center);

    let features = [
        "Area & fullscreen capture",
        "Built-in annotation editor",
        "Screen recording with audio",
        "OCR text extraction",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        features_box.append(&label);
    }

    content.append(&features_box);
}
