use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};

use super::ui::{feature_card_list, option_card, status_pill};

pub fn build(content: &GtkBox) {
    let title = Label::new(None);
    title.set_markup("<span size='x-large' weight='bold'>Cloud Upload</span>");
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let pill = status_pill("Available now · optional");
    pill.set_margin_bottom(12);
    content.append(&pill);

    let desc = Label::new(Some(
        "Share captures and recordings with a link, or keep everything on your own server.",
    ));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_justify(gtk4::Justification::Center);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    let features = feature_card_list(&[
        (
            "⇄",
            "Sync across devices",
            "Pull recent uploads when you're signed in",
        ),
        (
            "↗",
            "Instant share links",
            "Copy a URL after capture without leaving the workflow",
        ),
        (
            "☁",
            "Storage you control",
            "Hosted ApexShot Cloud or your own XBackBone instance",
        ),
    ]);
    features.set_margin_top(18);
    content.append(&features);

    let options_title = Label::new(None);
    options_title.set_markup("<span weight='bold'>Choose your cloud</span>");
    options_title.set_halign(Align::Center);
    options_title.set_margin_top(22);
    options_title.set_margin_bottom(10);
    content.append(&options_title);

    let options_row = GtkBox::new(Orientation::Horizontal, 12);
    options_row.set_halign(Align::Center);
    options_row.set_hexpand(true);

    options_row.append(&option_card(
        "A",
        "ApexShot Cloud",
        "Hosted by us, ready out of the box with device login.",
    ));
    options_row.append(&option_card(
        "X",
        "XBackBone",
        "Self-host for full control of storage and URLs.",
    ));
    content.append(&options_row);

    let hint = Label::new(Some(
        "Skip for now if you want. Configure later in Settings → Cloud.",
    ));
    hint.set_halign(Align::Center);
    hint.set_wrap(true);
    hint.set_width_request(500);
    hint.add_css_class("settings-sub-option");
    hint.set_margin_top(18);
    content.append(&hint);
}
