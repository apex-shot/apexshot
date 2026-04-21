use crate::AppConfig;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, Label, Orientation,
};
use std::process::Command;

pub struct CloudSettingsWidgets {
    pub section: GtkBox,
}

pub fn build_cloud_section(_config: &AppConfig) -> CloudSettingsWidgets {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.set_hexpand(true);
    section.set_vexpand(true);

    // Center everything
    section.set_halign(Align::Center);
    section.set_valign(Align::Center);

    let content = GtkBox::new(Orientation::Vertical, 16);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_margin_top(100);
    content.set_margin_bottom(100);

    // Coming Soon title
    let title = Label::new(Some("Coming Soon"));
    title.add_css_class("settings-group-title");
    title.set_xalign(0.5);
    title.set_margin_bottom(8);
    content.append(&title);

    // Description text
    let description = Label::new(Some(
        "ApexShot Cloud is under development. Sign up for the waitlist to get early access",
    ));
    description.set_xalign(0.5);
    description.set_wrap(true);
    description.set_width_request(400);
    description.add_css_class("settings-sub-option");
    content.append(&description);

    // Waitlist button
    let waitlist_btn = Button::with_label("Join Waitlist");
    waitlist_btn.add_css_class("settings-primary-btn");
    waitlist_btn.set_margin_top(16);
    waitlist_btn.connect_clicked(|_| {
        std::thread::spawn(move || {
            let _ = Command::new("xdg-open").arg("https://apexshot.org/waitlist").spawn();
        });
    });

    content.append(&waitlist_btn);

    section.append(&content);

    CloudSettingsWidgets { section }
}
