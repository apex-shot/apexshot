use gtk4::{prelude::*, Align, Box as GtkBox, Label, Orientation};

use super::ui::{feature_card_list, option_card, status_pill};
use crate::capture::editor::window::icon_names::{self, custom};
use crate::i18n::{self, t};

pub fn build(content: &GtkBox) {
    let title = Label::new(None);
    title.set_markup(&i18n::markup_title("Cloud Upload"));
    title.set_halign(Align::Center);
    title.set_margin_bottom(8);
    content.append(&title);

    let pill = status_pill(&t("Available now · optional"));
    pill.set_margin_bottom(12);
    content.append(&pill);

    let desc = Label::new(Some(&t(
        "Share captures and recordings with a link, or keep everything on your own server.",
    )));
    desc.set_halign(Align::Center);
    desc.set_wrap(true);
    desc.set_justify(gtk4::Justification::Center);
    desc.set_width_request(500);
    desc.add_css_class("settings-sub-option");
    content.append(&desc);

    let sync_title = t("Sync across devices");
    let sync_body = t("Pull recent uploads when you're signed in");
    let links_title = t("Instant share links");
    let links_body = t("Copy a URL after capture without leaving the workflow");
    let storage_title = t("Storage you control");
    let storage_body = t("Hosted ApexShot Cloud or your own XBackBone instance");
    let features = feature_card_list(&[
        (
            custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
            sync_title.as_str(),
            sync_body.as_str(),
        ),
        (
            custom::ARROW2_TOP_RIGHT_SYMBOLIC,
            links_title.as_str(),
            links_body.as_str(),
        ),
        (
            icon_names::FOLDER_OPEN_REGULAR,
            storage_title.as_str(),
            storage_body.as_str(),
        ),
    ]);
    features.set_margin_top(18);
    content.append(&features);

    let options_title = Label::new(None);
    options_title.set_markup(&i18n::markup_bold("Choose your cloud"));
    options_title.set_halign(Align::Center);
    options_title.set_margin_top(22);
    options_title.set_margin_bottom(10);
    content.append(&options_title);

    let options_row = GtkBox::new(Orientation::Horizontal, 12);
    options_row.set_halign(Align::Center);
    options_row.set_hexpand(true);

    let hosted_title = t("ApexShot Cloud");
    let hosted_body = t("Hosted by us, ready out of the box with device login.");
    let xb_title = t("XBackBone");
    let xb_body = t("Self-host for full control of storage and URLs.");
    options_row.append(&option_card(
        custom::APEXSHOT_CLOUD,
        &hosted_title,
        &hosted_body,
    ));
    options_row.append(&option_card(custom::XBACKBONE, &xb_title, &xb_body));
    content.append(&options_row);

    let hint = Label::new(Some(&t(
        "Skip for now if you want. After you Connect, your next screenshot gets a share link. Configure later in Settings → Cloud.",
    )));
    hint.set_halign(Align::Center);
    hint.set_wrap(true);
    hint.set_width_request(500);
    hint.add_css_class("settings-sub-option");
    hint.set_margin_top(18);
    content.append(&hint);
}
