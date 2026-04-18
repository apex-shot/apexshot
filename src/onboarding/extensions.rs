use gtk4::{prelude::*, Align, Button, Label, show_uri};

// TODO: Update these URLs when extensions are published
const GNOME_EXTENSION_URL: &str = "https://extensions.gnome.org/extension/XXXXX/apexshot/";
const CHROME_EXTENSION_URL: &str = "https://chromewebstore.google.com/detail/apexshot/XXXXX";

fn open_url(url: &str) {
    let _ = show_uri(None::<&gtk4::Window>, url, 0);
}

pub fn build_gnome(content: &gtk4::Box) {
    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>GNOME Shell Extension</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description
    let desc = Label::new(Some(
        "Unlock the full ApexShot experience with the GNOME extension:",
    ));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    // Features
    let features_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    features_box.set_margin_top(32);
    features_box.set_halign(Align::Center);

    let features = [
        "Floating preview windows",
        "Quick access overlay",
        "Recording status indicator",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        label.set_margin_start(40);
        label.set_margin_end(40);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Install button
    let install_btn = Button::with_label("Install GNOME Extension");
    install_btn.add_css_class("settings-primary-btn");
    install_btn.set_halign(Align::Center);
    install_btn.set_margin_top(32);
    install_btn.connect_clicked(|_| {
        open_url(GNOME_EXTENSION_URL);
    });
    content.append(&install_btn);
}

pub fn build_chrome(content: &gtk4::Box) {
    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>Browser Extension</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description
    let desc = Label::new(Some(
        "Capture full-page screenshots from any website with our Chrome/Chromium extension:",
    ));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    // Features
    let features_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    features_box.set_margin_top(32);
    features_box.set_halign(Align::Center);

    let features = [
        "Full-page scroll capture",
        "Sends directly to ApexShot",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        label.set_margin_start(40);
        label.set_margin_end(40);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Install button
    let install_btn = Button::with_label("Get Chrome Extension");
    install_btn.add_css_class("settings-primary-btn");
    install_btn.set_halign(Align::Center);
    install_btn.set_margin_top(32);
    install_btn.connect_clicked(|_| {
        open_url(CHROME_EXTENSION_URL);
    });
    content.append(&install_btn);
}
