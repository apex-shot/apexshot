use gtk4::{prelude::*, Align, Button, Label, show_uri};

const WAITLIST_URL: &str = "https://apexshot.org/waitlist";

fn open_url(url: &str) {
    let _ = show_uri(None::<&gtk4::Window>, url, 0);
}

pub fn build(content: &gtk4::Box) {
    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>Cloud Sync</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(4);
    content.append(&title);

    let subtitle = Label::new(Some("Coming Soon"));
    subtitle.add_css_class("settings-sub-option");
    subtitle.set_halign(Align::Center);
    subtitle.set_margin_bottom(8);
    content.append(&subtitle);

    // Description
    let desc = Label::new(Some("We're building something great:"));
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
        "Sync captures across devices",
        "Instant share links",
        "Cloud storage integration",
    ];

    for feature in features {
        let label = Label::new(Some(feature));
        label.set_halign(Align::Start);
        label.set_margin_start(40);
        label.set_margin_end(40);
        features_box.append(&label);
    }
    content.append(&features_box);

    // Waitlist message
    let waitlist_msg = Label::new(Some("Be the first to know when it launches."));
    waitlist_msg.set_halign(Align::Center);
    waitlist_msg.set_wrap(true);
    waitlist_msg.set_width_request(500);
    waitlist_msg.add_css_class("settings-sub-option");
    waitlist_msg.set_margin_top(32);
    content.append(&waitlist_msg);

    // Waitlist button
    let waitlist_btn = Button::with_label("Join Waitlist");
    waitlist_btn.add_css_class("settings-primary-btn");
    waitlist_btn.set_halign(Align::Center);
    waitlist_btn.set_margin_top(32);
    waitlist_btn.connect_clicked(|_| {
        open_url(WAITLIST_URL);
    });
    content.append(&waitlist_btn);
}
