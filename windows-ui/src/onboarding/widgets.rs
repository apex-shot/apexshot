//! Small builders shared by the onboarding pages.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};

use super::{Shell, Step};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;

/// A design value at 100% scaling, converted to this display's logical pixels.
pub(crate) fn px(scale: f64, value: f64) -> i32 {
    (value * scale).round() as i32
}

pub(crate) fn label(text: &str, class: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label.set_halign(Align::Start);
    label
}

pub(crate) fn wrapped(text: &str, class: &str, max_chars: i32) -> Label {
    let label = label(text, class);
    label.set_wrap(true);
    label.set_max_width_chars(max_chars);
    label.set_justify(gtk4::Justification::Left);
    label
}

pub(crate) fn vbox(spacing: i32) -> GtkBox {
    GtkBox::new(Orientation::Vertical, spacing)
}

pub(crate) fn hbox(spacing: i32) -> GtkBox {
    GtkBox::new(Orientation::Horizontal, spacing)
}

pub(crate) fn spacer() -> GtkBox {
    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    spacer
}

pub(crate) fn icon(name: &str, size: i32) -> Image {
    let image = Image::from_icon_name(name);
    image.set_pixel_size(size);
    image.set_halign(Align::Center);
    image.set_valign(Align::Center);
    image
}

/// Brand mark and wordmark, as shown at the top of the account pages and the
/// final checklist.
pub(crate) fn brand(shell: &Shell) -> GtkBox {
    let row = hbox(px(shell.scale(), 10.0));
    row.add_css_class("ob-brand");
    row.append(&icon(icon_names::custom::APEXSHOT, px(shell.scale(), 24.0)));
    let name = label(&t("ApexShot"), "ob-brand-name");
    name.set_valign(Align::Center);
    row.append(&name);
    row
}

/// A full-width primary button, optionally with the Return-key hint.
pub(crate) fn primary_button(shell: &Shell, text: &str, return_hint: bool) -> Button {
    action_button(shell, text, "ob-primary", return_hint)
}

pub(crate) fn secondary_button(shell: &Shell, text: &str) -> Button {
    action_button(shell, text, "ob-secondary", false)
}

pub(crate) fn ghost_button(shell: &Shell, text: &str) -> Button {
    action_button(shell, text, "ob-ghost", false)
}

fn action_button(shell: &Shell, text: &str, class: &str, return_hint: bool) -> Button {
    let button = Button::new();
    button.add_css_class(class);
    let content = hbox(px(shell.scale(), 8.0));
    content.set_halign(Align::Center);
    let label = Label::new(Some(&t(text)));
    label.add_css_class("ob-button");
    content.append(&label);
    if return_hint {
        let hint = Label::new(Some("↵"));
        hint.add_css_class("ob-return-glyph");
        hint.set_valign(Align::Center);
        hint.set_opacity(0.55);
        content.append(&hint);
    }
    button.set_child(Some(&content));
    button
}

/// Footer "Next" target: the bright action on the right of every setup page.
pub(crate) fn next_button(shell: &Shell, text: &str) -> Button {
    let button = Button::new();
    button.add_css_class("ob-next");
    let content = hbox(px(shell.scale(), 9.0));
    content.set_halign(Align::Center);
    let label = Label::new(Some(&t(text)));
    label.add_css_class("ob-button");
    content.append(&label);
    let hint = Label::new(Some("↵"));
    hint.add_css_class("ob-return-glyph");
    hint.set_valign(Align::Center);
    hint.set_opacity(0.5);
    content.append(&hint);
    button.set_child(Some(&content));
    button
}

/// Footer "Back" target: a quiet pill with a chevron.
pub(crate) fn back_button(shell: &Shell) -> Button {
    let button = Button::new();
    button.add_css_class("ob-back");
    let content = hbox(px(shell.scale(), 7.0));
    content.set_halign(Align::Center);
    let glyph = Label::new(Some("‹"));
    glyph.add_css_class("ob-back-glyph");
    glyph.set_valign(Align::Center);
    content.append(&glyph);
    let label = Label::new(Some(&t("Back")));
    label.add_css_class("ob-button-small");
    label.set_valign(Align::Center);
    content.append(&label);
    button.set_child(Some(&content));
    button
}

/// The round back target on the authorize page.
pub(crate) fn round_back_button(shell: &Shell) -> Button {
    let button = Button::new();
    button.add_css_class("ob-round-back");
    button.set_size_request(px(shell.scale(), 34.0), px(shell.scale(), 34.0));
    button.set_halign(Align::Start);
    button.set_valign(Align::Center);
    let glyph = Label::new(Some("‹"));
    glyph.add_css_class("ob-back-glyph");
    glyph.set_halign(Align::Center);
    glyph.set_valign(Align::Center);
    button.set_child(Some(&glyph));
    button
}

/// Four progress dots with the active one drawn as a pill.
pub(crate) fn dots(shell: &Shell, active: usize) -> GtkBox {
    let row = hbox(px(shell.scale(), 8.0));
    row.add_css_class("ob-dots");
    row.set_halign(Align::Center);
    row.set_valign(Align::Center);
    for index in 0..4 {
        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("ob-dot");
        if index == active {
            dot.add_css_class("ob-dot-active");
            dot.set_size_request(px(shell.scale(), 22.0), px(shell.scale(), 6.0));
        } else {
            dot.set_size_request(px(shell.scale(), 6.0), px(shell.scale(), 6.0));
            if index < active {
                dot.add_css_class("ob-dot-done");
            }
        }
        row.append(&dot);
    }
    row
}

/// The bottom bar of a setup page: back on the left, progress in the centre,
/// skip and next on the right.
pub(crate) fn footer(
    shell: &Shell,
    step: Step,
    next_label: &str,
    on_next: Rc<dyn Fn()>,
    with_skip: bool,
) -> GtkBox {
    let bar = hbox(0);
    bar.add_css_class("ob-footer");
    bar.set_halign(Align::Fill);
    bar.set_valign(Align::End);

    let left = hbox(0);
    left.set_hexpand(true);
    left.set_halign(Align::Start);
    if let Some(previous) = step.prev() {
        let back = back_button(shell);
        let shell_c = shell.clone();
        back.connect_clicked(move |_| shell_c.go(previous));
        left.append(&back);
    }
    bar.append(&left);

    if let Some(index) = step.dot_index() {
        bar.append(&dots(shell, index));
    }

    let right = hbox(px(shell.scale(), 10.0));
    right.set_hexpand(true);
    right.set_halign(Align::End);
    right.set_valign(Align::Center);
    if with_skip {
        let skip = ghost_button(shell, "Skip");
        let shell_c = shell.clone();
        skip.connect_clicked(move |_| shell_c.advance());
        right.append(&skip);
    }
    let next = next_button(shell, next_label);
    {
        let action = Rc::clone(&on_next);
        next.connect_clicked(move |_| action());
    }
    right.append(&next);
    bar.append(&right);

    shell.set_primary(on_next);
    bar
}

/// Centered page heading: title over a one- or two-line subtitle.
pub(crate) fn heading(shell: &Shell, title: &str, subtitle: &str) -> GtkBox {
    let block = vbox(px(shell.scale(), 10.0));
    block.set_halign(Align::Center);
    let title = wrapped(title, "ob-title", 48);
    title.set_halign(Align::Center);
    title.set_xalign(0.5);
    title.set_justify(gtk4::Justification::Center);
    block.append(&title);
    let subtitle = wrapped(subtitle, "ob-subtitle", 58);
    subtitle.set_halign(Align::Center);
    subtitle.set_xalign(0.5);
    subtitle.set_justify(gtk4::Justification::Center);
    block.append(&subtitle);
    block
}

/// Three short benefit rows on a framed panel, used under the account copy.
pub(crate) fn feature_panel(shell: &Shell, items: &[&str]) -> GtkBox {
    let panel = vbox(px(shell.scale(), 10.0));
    panel.add_css_class("ob-feature-panel");
    panel.set_halign(Align::Fill);
    for item in items {
        let row = hbox(px(shell.scale(), 10.0));
        let check = Label::new(Some("✓"));
        check.add_css_class("ob-check");
        check.set_valign(Align::Center);
        row.append(&check);
        let text = Label::new(Some(&t(item)));
        text.add_css_class("ob-feature");
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_halign(Align::Start);
        row.append(&text);
        panel.append(&row);
    }
    panel
}

/// Replace the text of a button built by [`action_button`] without replacing
/// its child (which would drop the button's own typography and hint).
pub(crate) fn set_button_text(button: &Button, text: &str) {
    fn first_label(widget: &gtk4::Widget) -> Option<Label> {
        if let Some(label) = widget.downcast_ref::<Label>() {
            return Some(label.clone());
        }
        let mut child = widget.first_child();
        while let Some(inner) = child {
            if let Some(label) = first_label(&inner) {
                return Some(label);
            }
            child = inner.next_sibling();
        }
        None
    }
    if let Some(label) = first_label(button.upcast_ref()) {
        label.set_text(text);
    }
}

/// One settings row inside a framed card: icon, title, optional caption, and a
/// control at the end.
pub(crate) fn settings_row(
    shell: &Shell,
    icon_name: &str,
    title: &str,
    caption: Option<&str>,
    control: &impl IsA<gtk4::Widget>,
) -> GtkBox {
    let row = hbox(px(shell.scale(), 14.0));
    row.add_css_class("ob-settings-row");
    row.set_hexpand(true);

    let badge = GtkBox::new(Orientation::Vertical, 0);
    badge.add_css_class("ob-row-icon");
    badge.set_halign(Align::Center);
    badge.set_valign(Align::Center);
    badge.append(&icon(icon_name, px(shell.scale(), 17.0)));
    row.append(&badge);

    let text = vbox(px(shell.scale(), 3.0));
    text.set_hexpand(true);
    text.set_valign(Align::Center);
    let title = Label::new(Some(&t(title)));
    title.add_css_class("ob-choice-title");
    title.set_xalign(0.0);
    title.set_halign(Align::Start);
    text.append(&title);
    if let Some(caption) = caption {
        let caption = Label::new(Some(&t(caption)));
        caption.add_css_class("ob-choice-caption");
        caption.set_xalign(0.0);
        caption.set_halign(Align::Start);
        caption.set_wrap(true);
        text.append(&caption);
    }
    row.append(&text);

    control.as_ref().set_valign(Align::Center);
    row.append(control);
    row
}

/// A small email pill for the signed-in state and the final page.
pub(crate) fn email_pill(shell: &Shell, email: &str) -> GtkBox {
    let pill = hbox(px(shell.scale(), 7.0));
    pill.add_css_class("ob-email-pill");
    pill.set_halign(Align::Start);
    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.add_css_class("ob-online-dot");
    dot.set_size_request(px(shell.scale(), 6.0), px(shell.scale(), 6.0));
    dot.set_valign(Align::Center);
    pill.append(&dot);
    let label = Label::new(Some(email));
    label.add_css_class("ob-email");
    label.set_valign(Align::Center);
    pill.append(&label);
    pill
}
