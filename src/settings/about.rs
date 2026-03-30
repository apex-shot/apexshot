use gtk4::{prelude::*, Align, Box as GtkBox, Button, Image, Label, Orientation, Separator};

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
    let icon = Image::from_icon_name("application-x-executable"); // Replace with app icon later
    icon.set_pixel_size(96);
    icon.set_margin_bottom(16);

    let title = Label::new(Some("ApexShot"));
    title.add_css_class("about-app-name");

    let version = Label::new(Some("Version 1.2.3 (789)"));
    version.add_css_class("about-version-label");

    header_vbox.append(&icon);
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

    let whats_new_btn = Button::with_label("What's New");
    whats_new_btn.add_css_class("secondary-settings-button");
    whats_new_btn.set_width_request(200);
    whats_new_btn.set_halign(Align::Center);

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
