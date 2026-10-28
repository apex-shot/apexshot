//! Welcome: the mascot, the headline, and the one action that starts setup.

use gtk4::{prelude::*, Align, Box as GtkBox, Button, Label, Orientation, Stack};

use super::{px, Nav};
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav, mascot: &Stack) {
    body.set_vexpand(true);
    let centered = GtkBox::new(Orientation::Vertical, px(nav.scale, 12.0));
    centered.add_css_class("onboarding-welcome-content");
    centered.set_width_request(px(nav.scale, 560.0));
    centered.set_halign(Align::Center);
    centered.set_valign(Align::Center);
    centered.append(mascot);

    let heading = GtkBox::new(Orientation::Vertical, px(nav.scale, 8.0));
    heading.set_halign(Align::Center);
    let title = Label::new(Some(&t("Welcome to ApexShot")));
    title.add_css_class("onboarding-headline");
    title.set_halign(Align::Center);
    title.set_xalign(0.5);
    title.set_wrap(true);
    heading.append(&title);
    let copy = Label::new(Some(&t(
        "Your screenshot companion for Windows. Set it up in a few quick steps.",
    )));
    copy.add_css_class("onboarding-copy");
    copy.set_halign(Align::Center);
    copy.set_xalign(0.5);
    copy.set_justify(gtk4::Justification::Center);
    copy.set_wrap(true);
    heading.append(&copy);
    centered.append(&heading);
    body.append(&centered);

    let get_started = Button::with_label(&t("Get started"));
    get_started.add_css_class("settings-primary-btn");
    let advance = nav.advance.clone();
    get_started.connect_clicked(move |_| advance());
    actions.append(&get_started);
}
