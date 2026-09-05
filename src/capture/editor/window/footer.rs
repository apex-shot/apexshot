use gtk4::{prelude::*, Box as GtkBox, Button, Label, Orientation};

use super::super::ui_support::footer_icon_button;
use crate::i18n::t;

pub(super) struct FooterParts {
    pub zoom_button: Button,
    pub zoom_label: Label,
    pub zoom_header_label: Label,
    pub zoom_popup: GtkBox,
    pub zoom_minus_btn: Button,
    pub zoom_plus_btn: Button,
    pub zoom_in_btn: Button,
    pub zoom_out_btn: Button,
    pub fit_to_screen_btn: Button,
    pub zoom_to_selection_btn: Button,
    pub copy_btn: Button,
    pub upload_btn: Button,
    pub save_btn: Button,
}

fn build_zoom_row(label: &str, shortcut: &str) -> (Button, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("editor-footer-zoom-row");

    let label_widget = Label::new(Some(label));
    label_widget.set_hexpand(true);
    label_widget.set_xalign(0.0);

    let shortcut_box = GtkBox::new(Orientation::Horizontal, 4);
    shortcut_box.add_css_class("editor-footer-zoom-shortcut-box");

    for part in shortcut.split(' ') {
        let part_label = Label::new(Some(part));
        part_label.add_css_class("editor-footer-zoom-shortcut-part");
        shortcut_box.append(&part_label);
    }

    row.append(&label_widget);
    row.append(&shortcut_box);

    let btn = Button::builder()
        .has_frame(false)
        .css_classes(["flat", "editor-footer-zoom-action-btn"])
        .child(&row)
        .build();

    (btn, row)
}

pub(super) fn build_footer(copy_icon_name: &str, upload_icon_name: &str) -> FooterParts {
    let zoom_button = Button::new();
    zoom_button.set_has_frame(false);
    zoom_button.set_tooltip_text(Some(&t("Zoom controls")));
    zoom_button.add_css_class("editor-footer-zoom-button");

    let zoom_label = Label::new(Some("100%"));
    zoom_label.add_css_class("editor-footer-zoom-label");
    zoom_button.set_child(Some(&zoom_label));

    let zoom_minus_btn = Button::with_label("-");
    zoom_minus_btn.add_css_class("editor-floating-zoom-step");
    zoom_minus_btn.add_css_class("flat");

    let zoom_plus_btn = Button::with_label("+");
    zoom_plus_btn.add_css_class("editor-floating-zoom-step");
    zoom_plus_btn.add_css_class("flat");

    let zoom_popup = GtkBox::new(Orientation::Vertical, 0);
    zoom_popup.add_css_class("editor-footer-zoom-popup");
    zoom_popup.set_halign(gtk4::Align::Start);
    zoom_popup.set_valign(gtk4::Align::End);
    zoom_popup.set_margin_start(16);
    zoom_popup.set_margin_bottom(16);
    zoom_popup.set_visible(false);

    // Header
    let zoom_header = GtkBox::new(Orientation::Horizontal, 0);
    zoom_header.add_css_class("editor-footer-zoom-header");

    let zoom_header_label = Label::new(Some("100%"));
    zoom_header_label.set_hexpand(true);
    zoom_header_label.set_xalign(0.5);
    zoom_header_label.add_css_class("editor-footer-zoom-header-label");

    zoom_header.append(&zoom_header_label);

    // List of actions
    let zoom_list = GtkBox::new(Orientation::Vertical, 0);
    zoom_list.add_css_class("editor-footer-zoom-list");

    let (zoom_in_btn, _) = build_zoom_row(&t("Zoom In"), "Ctrl +");
    let (zoom_out_btn, _) = build_zoom_row(&t("Zoom Out"), "Ctrl -");
    let (fit_to_screen_btn, _) = build_zoom_row("Fit to Screen", "Ctrl 0");
    let (zoom_to_selection_btn, _) = build_zoom_row(&t("Zoom to Selection"), "Ctrl 2");

    let sep1 = GtkBox::new(Orientation::Horizontal, 0);
    sep1.add_css_class("editor-footer-zoom-separator");

    zoom_list.append(&zoom_in_btn);
    zoom_list.append(&zoom_out_btn);
    zoom_list.append(&fit_to_screen_btn);
    zoom_list.append(&zoom_to_selection_btn);

    zoom_popup.append(&zoom_header);
    zoom_popup.append(&sep1);
    zoom_popup.append(&zoom_list);

    let (copy_btn, _) = footer_icon_button(copy_icon_name, &t("Copy file URI"));
    let (upload_btn, _) = footer_icon_button(upload_icon_name, &t("Upload to cloud"));

    let save_btn = Button::with_label(&t("Done"));
    save_btn.set_has_frame(false);
    save_btn.add_css_class("editor-done-button");
    save_btn.add_css_class("body");
    save_btn.set_valign(gtk4::Align::Center);

    FooterParts {
        zoom_button,
        zoom_label,
        zoom_header_label,
        zoom_popup,
        zoom_minus_btn,
        zoom_plus_btn,
        zoom_in_btn,
        zoom_out_btn,
        fit_to_screen_btn,
        zoom_to_selection_btn,
        copy_btn,
        upload_btn,
        save_btn,
    }
}
