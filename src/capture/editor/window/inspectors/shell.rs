//! Shared inspector panel shell helpers (PR 10.11).

use gtk4::{prelude::*, Box as GtkBox, Label, Orientation, Widget};

use super::super::background_panel::BACKGROUND_SIDEBAR_WIDTH;

pub(super) fn build_tool_inspector() -> (GtkBox, GtkBox) {
    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    root.set_hexpand(false);
    root.set_halign(gtk4::Align::Fill);
    root.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_margin_top(4);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_hexpand(false);
    content.set_halign(gtk4::Align::Fill);

    root.append(&content);
    (root, content)
}

pub(super) fn append_inspector_section(content: &GtkBox, title: &str, widget: &Widget) {
    let section = GtkBox::new(Orientation::Vertical, 8);
    section.add_css_class("editor-inspector-section");
    section.set_hexpand(false);
    section.set_halign(gtk4::Align::Fill);

    let section_title = Label::new(Some(title));
    section_title.add_css_class("editor-background-section-title");
    section_title.set_xalign(0.0);
    section_title.set_hexpand(false);
    section_title.set_halign(gtk4::Align::Fill);

    let section_body = GtkBox::new(Orientation::Vertical, 0);
    section_body.set_hexpand(false);
    section_body.set_halign(gtk4::Align::Fill);
    section_body.append(widget);

    section.append(&section_title);
    section.append(&section_body);
    content.append(&section);
}
