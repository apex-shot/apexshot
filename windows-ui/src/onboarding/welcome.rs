//! Welcome: the headline and the one action that starts setup.

use gtk4::{prelude::*, Box as GtkBox, Button, Label};

use super::ui::heading;
use super::Nav;
use crate::i18n::t;

pub fn build(body: &GtkBox, actions: &GtkBox, nav: &Nav) {
    body.append(&heading(
        nav.scale,
        &t("Welcome to ApexShot"),
        &t("Capture your screen, mark it up, and export it."),
    ));

    let get_started = Button::with_label(&t("Get started"));
    get_started.add_css_class("settings-primary-btn");
    let advance = nav.advance.clone();
    get_started.connect_clicked(move |_| advance());
    actions.append(&get_started);
    let note = Label::new(Some(&t("No account needed.")));
    note.add_css_class("onboarding-action-note");
    note.set_wrap(true);
    note.set_max_width_chars(36);
    note.set_justify(gtk4::Justification::Center);
    actions.append(&note);
}
