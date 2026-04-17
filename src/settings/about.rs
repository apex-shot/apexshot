use gtk4::{prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Separator, show_uri};

pub struct AboutSettingsWidgets {
    pub section: GtkBox,
}

pub fn build_about_section() -> AboutSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);
    section.set_vexpand(true);
    section.set_halign(Align::Center);
    section.set_valign(Align::Center);
    section.set_width_request(400);

    // --- 1. ICON & TITLE ---
    let header_vbox = GtkBox::new(Orientation::Vertical, 8);

    // Procedural Logo Drawing
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(128);
    drawing_area.set_content_height(128);
    drawing_area.set_halign(Align::Center);
    drawing_area.set_margin_bottom(16);

    drawing_area.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        // Scale from 24x24 viewBox to 128x128, centered
        let scale = width as f64 / 24.0;
        cr.translate(cx - 12.0 * scale, cy - 12.0 * scale);
        cr.scale(scale, scale);

        // Draw the new logo: curved arch shape
        // Path: M 2 21 C 6 21, 8 2, 12 2 C 16 2, 18 21, 22 21
        cr.set_source_rgba(0.913, 0.329, 0.125, 1.0); // #E95420
        cr.set_line_width(2.5);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        cr.move_to(2.0, 21.0);
        cr.curve_to(6.0, 21.0, 8.0, 2.0, 12.0, 2.0);
        cr.curve_to(16.0, 2.0, 18.0, 21.0, 22.0, 21.0);
        cr.stroke().expect("Failed to draw logo");
    });

    let title = Label::new(None);
    title.set_markup("Apex<span weight='light' alpha='50%'>s h o t</span>");
    title.add_css_class("about-app-name");

    let version = Label::new(Some(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("about-version-label");

    header_vbox.append(&drawing_area);
    header_vbox.append(&title);
    header_vbox.append(&version);
    section.append(&header_vbox);

    // --- 2. UPDATE ACTION ---
    let update_vbox = GtkBox::new(Orientation::Vertical, 12);
    update_vbox.set_margin_top(40);

    let check_btn = Button::with_label("Check for Updates");
    check_btn.add_css_class("primary-settings-button");
    check_btn.set_width_request(200);
    check_btn.set_halign(Align::Center);
    check_btn.connect_clicked(|_| {
        let _ = show_uri(None::<&gtk4::Window>, "https://github.com/apex-shot/apexshot/releases", 0);
    });

    let whats_new_btn = Button::with_label("What's New");
    whats_new_btn.add_css_class("secondary-settings-button");
    whats_new_btn.set_width_request(200);
    whats_new_btn.set_halign(Align::Center);
    whats_new_btn.connect_clicked(|_| {
        let _ = show_uri(None::<&gtk4::Window>, "https://github.com/apex-shot/apexshot/releases/latest", 0);
    });

    update_vbox.append(&check_btn);
    update_vbox.append(&whats_new_btn);
    section.append(&update_vbox);

    // --- 3. LINKS ---
    let links_grid = gtk4::Grid::new();
    links_grid.set_margin_top(48);
    links_grid.set_column_spacing(24);
    links_grid.set_row_spacing(12);
    links_grid.set_halign(Align::Center);

    let create_link = |label: &str| -> Button {
        let btn = Button::with_label(label);
        btn.add_css_class("about-link-button");
        btn
    };

    links_grid.attach(&create_link("Help Center"), 0, 0, 1, 1);
    links_grid.attach(&create_link("Send Feedback"), 1, 0, 1, 1);
    links_grid.attach(&create_link("Follow on Twitter"), 0, 1, 1, 1);
    links_grid.attach(&create_link("Website"), 1, 1, 1, 1);
    section.append(&links_grid);

    // --- 4. FOOTER ---
    let footer_vbox = GtkBox::new(Orientation::Vertical, 8);
    footer_vbox.set_margin_top(60);
    footer_vbox.set_opacity(0.5);

    let copyright = Label::new(Some("Copyright © 2026 ApexShot. All rights reserved."));
    copyright.add_css_class("settings-sub-option-hint");
    copyright.set_halign(Align::Center);

    let legal_hbox = GtkBox::new(Orientation::Horizontal, 12);
    legal_hbox.set_halign(Align::Center);
    let tos = Label::new(Some("Terms of Service"));
    tos.add_css_class("settings-sub-option-hint");
    let privacy = Label::new(Some("Privacy Policy"));
    privacy.add_css_class("settings-sub-option-hint");

    legal_hbox.append(&tos);
    legal_hbox.append(&Separator::new(Orientation::Vertical));
    legal_hbox.append(&privacy);

    footer_vbox.append(&copyright);
    footer_vbox.append(&legal_hbox);
    section.append(&footer_vbox);

    AboutSettingsWidgets { section }
}
