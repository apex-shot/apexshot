//! Welcome: the headline and the one action that starts setup.

use gtk4::{prelude::*, Box as GtkBox, Button, Label};

use super::motion;
use super::ui::heading_parts;
use super::Nav;
use crate::i18n::t;

pub fn build(
    body: &GtkBox,
    actions: &GtkBox,
    nav: &Nav,
    animations: &mut Vec<gtk4::TickCallbackId>,
) {
    let heading = heading_parts(
        nav.scale,
        &t("Welcome to ApexShot"),
        &t("Capture your screen, mark it up, and export it."),
    );
    heading.block.remove(&heading.title);
    heading.block.remove(&heading.copy);
    for (label, delay, duration) in [
        (&heading.title, 360.0, 1000.0),
        (&heading.copy, 850.0, 850.0),
    ] {
        let (text, callback) = motion::reveal_text(label, delay, duration);
        heading.block.append(&text);
        if let Some(callback) = callback {
            animations.push(callback);
        }
    }
    body.append(&heading.block);

    let get_started = Button::with_label(&t("Get started"));
    get_started.add_css_class("settings-primary-btn");
    let advance = nav.advance.clone();
    get_started.connect_clicked(move |_| advance());
    if let Some(callback) = motion::fade_in(get_started.upcast_ref(), 1350.0, 350.0) {
        animations.push(callback);
    }
    actions.append(&get_started);

    let note = Label::new(Some(&t("No account needed.")));
    note.add_css_class("onboarding-action-note");
    note.set_wrap(true);
    note.set_max_width_chars(36);
    note.set_justify(gtk4::Justification::Center);
    if let Some(callback) = motion::fade_in(note.upcast_ref(), 1550.0, 400.0) {
        animations.push(callback);
    }
    actions.append(&note);
}
