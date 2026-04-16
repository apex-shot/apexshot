use gtk4::{prelude::*, Align, Button, Label, show_uri};

const WAITLIST_URL: &str = "https://apexshot.org/waitlist";

fn open_url(url: &str) {
    let _ = show_uri(None::<&gtk4::Window>, url, 0);
}

pub fn build(content: &gtk4::Box) {
    // Title
    let title = Label::new(Some("Cloud Sync"));
    let subtitle = Label::new(Some("Coming Soon"));
    subtitle.add_css_class("settings-sub-option-hint");
    subtitle.set_margin_start(8);

    let title_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    title_box.set_halign(Align::Center);
    title_box.append(&title);
    title_box.append(&subtitle);
    content.append(&title_box);

    // Description
    let desc = Label::new(Some("We're building something great:"));
    desc.add_css_class("settings-sub-option-hint");
    desc.set_halign(Align::Center);
    desc.set_margin_top(8);
    content.append(&desc);

    // Features
    let features_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    features_box.set_margin_top(16);
    features_box.set_halign(Align::Center);

    let features = [
        "Sync captures across devices",
        "Instant share links",
        "Cloud storage integration",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Waitlist message
    let waitlist_msg = Label::new(Some("Be the first to know when it launches."));
    waitlist_msg.add_css_class("settings-sub-option-hint");
    waitlist_msg.set_halign(Align::Center);
    waitlist_msg.set_margin_top(16);
    content.append(&waitlist_msg);

    // Waitlist button
    let waitlist_btn = Button::with_label("Join Waitlist");
    waitlist_btn.add_css_class("settings-primary-btn");
    waitlist_btn.set_halign(Align::Center);
    waitlist_btn.set_margin_top(16);
    waitlist_btn.connect_clicked(|_| {
        open_url(WAITLIST_URL);
    });
    content.append(&waitlist_btn);
}
