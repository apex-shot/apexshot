//! The per-tool option bars that dock under the floating tool bar.
//!
//! The Linux editor docks a small floating bar below the toolbar when a tool
//! with options is selected: crop gets its ratio pill and confirm buttons,
//! arrow gets its style picker, and so on. This slice rebuilds the bars'
//! shape — the controls read and swap selections locally, but nothing reaches
//! a backend until the annotation model lands.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Entry, Image, Label, Orientation, Popover, Stack};
use std::rc::Rc;

use super::toolbar::Tool;
use super::ui_support::px;
use crate::i18n::t;

pub(super) struct SubBars {
    pub root: Stack,
    /// Called when a bar appears or hides so the canvas can make room for it.
    on_dock_change: Rc<dyn Fn(bool)>,
}

/// A pill button that opens a popover of option rows; picking a row updates
/// the pill's label and the checked row, like the Settings select menus. The
/// selection stays local — no backend reads it yet.
fn option_pill(scale: f64, initial: &str, options: &[&str]) -> Button {
    use std::cell::RefCell;

    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.add_css_class("image-editor-subbar-pill");
    button.set_tooltip_text(Some(initial));

    let text = Label::new(Some(initial));
    text.add_css_class("image-editor-subbar-pill-label");
    let chevron = Image::from_icon_name("chevron-down-regular");
    chevron.set_pixel_size(px(10.0, scale));
    chevron.add_css_class("image-editor-subbar-pill-arrow");
    let content = GtkBox::new(Orientation::Horizontal, px(4.0, scale));
    content.append(&text);
    content.append(&chevron);
    button.set_child(Some(&content));

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.add_css_class("image-editor-subbar-popover");
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("image-editor-subbar-menu");

    let selected = Rc::new(RefCell::new(0usize));
    let checks = Rc::new(RefCell::new(Vec::<Label>::new()));
    for (index, option) in options.iter().enumerate() {
        let check = Label::new(Some("✓"));
        check.add_css_class("image-editor-subbar-check");
        check.set_visible(index == 0);
        let option_label = Label::new(Some(option));
        option_label.set_xalign(0.0);
        option_label.set_hexpand(true);
        let row_box = GtkBox::new(Orientation::Horizontal, px(6.0, scale));
        row_box.append(&option_label);
        row_box.append(&check);
        let row = Button::new();
        row.set_has_frame(false);
        row.add_css_class("image-editor-subbar-option");
        row.set_child(Some(&row_box));
        row.connect_clicked({
            let popover = popover.clone();
            let text = text.clone();
            let selected = selected.clone();
            let checks = checks.clone();
            let label = (*option).to_owned();
            move |_| {
                *selected.borrow_mut() = index;
                for (i, check) in checks.borrow().iter().enumerate() {
                    check.set_visible(i == index);
                }
                text.set_text(&label);
                popover.popdown();
            }
        });
        checks.borrow_mut().push(check);
        list.append(&row);
    }
    popover.set_child(Some(&list));
    popover.set_parent(&button);
    button.connect_clicked(move |_| popover.popup());
    button
}

fn bar() -> GtkBox {
    let root = GtkBox::new(Orientation::Horizontal, 6);
    root.add_css_class("image-editor-sub-bar");
    root
}

/// The `- [n] +` stepper the number bar carries for the starting marker.
fn number_stepper(_scale: f64) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("image-editor-subbar-stepper");

    let entry = Entry::new();
    entry.set_text("1");
    entry.set_width_chars(3);
    entry.set_max_width_chars(4);
    gtk4::prelude::EditableExt::set_alignment(&entry, 0.5);
    entry.add_css_class("image-editor-subbar-entry");

    for (label, delta) in [("−", -1i32), ("+", 1)] {
        let step = Button::with_label(label);
        step.add_css_class("image-editor-subbar-step");
        step.set_focusable(false);
        step.connect_clicked({
            let entry = entry.clone();
            move |_| {
                let value = entry.text().parse::<i32>().unwrap_or(1) + delta;
                entry.set_text(&value.max(0).to_string());
            }
        });
        // Keep the order `-`, entry, `+`.
        if delta < 0 {
            row.append(&step);
            row.append(&entry);
        } else {
            row.append(&step);
        }
    }
    row
}

fn build_crop(scale: f64) -> GtkBox {
    let root = bar();
    root.append(&option_pill(
        scale,
        &t("Freeform"),
        &[&t("Freeform"), &t("Original"), "1:1", "4:3", "16:9"],
    ));
    let dimensions = Label::new(Some("—"));
    dimensions.add_css_class("image-editor-subbar-dimensions");
    root.append(&dimensions);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("image-editor-subbar-button");
    cancel.set_sensitive(false);
    cancel.set_tooltip_text(Some(&t(
        "Crop is not available in the Windows image editor yet",
    )));
    let apply = Button::with_label(&t("Apply"));
    apply.add_css_class("image-editor-subbar-button");
    apply.add_css_class("image-editor-subbar-apply");
    apply.set_sensitive(false);
    apply.set_tooltip_text(Some(&t(
        "Crop is not available in the Windows image editor yet",
    )));
    root.append(&cancel);
    root.append(&apply);
    root
}

fn intensity_slider(scale: f64, tooltip: &str) -> gtk4::Scale {
    let slider = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    slider.set_value(50.0);
    slider.set_size_request(px(140.0, scale), -1);
    slider.set_valign(Align::Center);
    slider.set_tooltip_text(Some(tooltip));
    slider.add_css_class("image-editor-subbar-slider");
    slider
}

pub(super) fn build(scale: f64, on_dock_change: Rc<dyn Fn(bool)>) -> SubBars {
    let root = Stack::new();
    root.add_css_class("image-editor-sub-bars");
    root.set_hhomogeneous(false);
    root.set_vhomogeneous(false);
    root.set_halign(Align::Center);
    root.set_valign(Align::Start);
    // Just under the floating toolbar: its 8px top gap plus the rail's height.
    root.set_margin_top(px(56.0, scale));
    root.set_visible(false);

    root.add_named(&build_crop(scale), Some("crop"));

    let text_bar = bar();
    text_bar.append(&option_pill(scale, "Sans", &["Sans", "Serif", "Monospace"]));
    text_bar.append(&option_pill(
        scale,
        "24pt",
        &["12pt", "16pt", "24pt", "32pt", "48pt"],
    ));
    root.add_named(&text_bar, Some("text"));

    let arrow_bar = bar();
    arrow_bar.append(&option_pill(
        scale,
        &t("Standard"),
        &[&t("Standard"), &t("Fancy"), &t("Curved"), &t("Double")],
    ));
    root.add_named(&arrow_bar, Some("arrow"));

    let highlighter_bar = bar();
    highlighter_bar.append(&option_pill(
        scale,
        &t("Text-aware"),
        &[&t("Text-aware"), &t("Freehand")],
    ));
    root.add_named(&highlighter_bar, Some("highlighter"));

    let number_bar = bar();
    number_bar.append(&option_pill(
        scale,
        "1, 2, 3, 4…",
        &[
            "1, 2, 3, 4…",
            "A, B, C, D…",
            "a, b, c, d…",
            "i, ii, iii, iv…",
        ],
    ));
    number_bar.append(&number_stepper(scale));
    number_bar.append(&option_pill(
        scale,
        &t("Medium"),
        &[&t("Small"), &t("Medium"), &t("Large"), &t("Extra Large")],
    ));
    root.add_named(&number_bar, Some("number"));

    let obfuscate_bar = bar();
    obfuscate_bar.append(&option_pill(
        scale,
        &t("Blur"),
        &[&t("Pixelate"), &t("Blur"), &t("Blackout")],
    ));
    obfuscate_bar.append(&intensity_slider(scale, &t("Obfuscate intensity")));
    root.add_named(&obfuscate_bar, Some("obfuscate"));

    let focus_bar = bar();
    focus_bar.append(&intensity_slider(scale, &t("Focus background intensity")));
    root.add_named(&focus_bar, Some("focus"));

    SubBars {
        root,
        on_dock_change,
    }
}

impl SubBars {
    /// Swap the docked bar for `tool`; tools without options show nothing.
    pub(super) fn show_tool(&self, tool: Tool) {
        let name = match tool {
            Tool::Crop => Some("crop"),
            Tool::Text => Some("text"),
            Tool::Arrow => Some("arrow"),
            Tool::Highlighter => Some("highlighter"),
            Tool::Number => Some("number"),
            Tool::Obfuscate => Some("obfuscate"),
            Tool::Focus => Some("focus"),
            _ => None,
        };
        match name {
            Some(name) => {
                self.root.set_visible_child_name(name);
                self.root.set_visible(true);
                (self.on_dock_change)(true);
            }
            None => {
                self.root.set_visible(false);
                (self.on_dock_change)(false);
            }
        }
    }
}
