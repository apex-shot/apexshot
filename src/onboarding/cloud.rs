use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};

pub fn build(content: &GtkBox) {
    // Title
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>Cloud Upload</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(4);
    content.append(&title);

    let subtitle = Label::new(Some("Available now"));
    subtitle.add_css_class("settings-sub-option");
    subtitle.set_halign(Align::Center);
    subtitle.set_margin_bottom(8);
    content.append(&subtitle);

    // Description
    let desc = Label::new(Some("Upload captures and recordings to the cloud:"));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    // Features
    let features_box = GtkBox::new(Orientation::Vertical, 12);
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

    // Destination options
    let options_box = GtkBox::new(Orientation::Vertical, 12);
    options_box.set_margin_top(32);
    options_box.set_halign(Align::Center);

    let options_title = Label::new(None);
    options_title.set_markup("<span weight='bold'>Choose your cloud</span>");
    options_title.set_halign(Align::Center);
    options_title.set_margin_bottom(4);
    options_box.append(&options_title);

    let apexshot_label = Label::new(Some("ApexShot Cloud — hosted by us, ready out of the box"));
    apexshot_label.set_halign(Align::Start);
    apexshot_label.set_margin_start(40);
    apexshot_label.set_margin_end(40);
    apexshot_label.set_wrap(true);
    apexshot_label.set_width_request(460);
    options_box.append(&apexshot_label);

    let xb_label = Label::new(Some(
        "XBackBone — self-host your own instance for full control",
    ));
    xb_label.set_halign(Align::Start);
    xb_label.set_margin_start(40);
    xb_label.set_margin_end(40);
    xb_label.set_wrap(true);
    xb_label.set_width_request(460);
    options_box.append(&xb_label);

    content.append(&options_box);

    // Pointer to settings
    let hint = Label::new(Some(
        "Use whichever you want — configure it later in Settings → Cloud.",
    ));
    hint.set_halign(Align::Center);
    hint.set_wrap(true);
    hint.set_width_request(500);
    hint.add_css_class("settings-sub-option");
    hint.set_margin_top(32);
    content.append(&hint);
}
