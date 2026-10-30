//! The right-hand inspector frame.
//!
//! The frame names the selected tool and its purpose, and holds the output
//! actions. The tool options themselves belong to the annotation slices that
//! follow; until those land the frame says so plainly, the same way the
//! Settings pages describe surfaces the Windows build does not run yet.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};

use super::toolbar::Tool;
use super::ui_support::{px, INSPECTOR_WIDTH};
use crate::i18n::t;

pub(super) struct Inspector {
    pub root: GtkBox,
    title: Label,
    hint: Label,
}

pub(super) fn build(scale: f64) -> Inspector {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("image-editor-inspector");
    root.set_size_request(px(INSPECTOR_WIDTH, scale), -1);

    let head = GtkBox::new(Orientation::Vertical, px(4.0, scale));
    head.add_css_class("image-editor-inspector-head");

    let kicker = Label::new(Some(&t("ANNOTATE")));
    kicker.add_css_class("image-editor-kicker");
    kicker.set_xalign(0.0);

    let title = Label::new(None);
    title.add_css_class("image-editor-inspector-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    let hint = Label::new(None);
    hint.add_css_class("image-editor-inspector-hint");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_max_width_chars(34);

    head.append(&kicker);
    head.append(&title);
    head.append(&hint);

    let rule = GtkBox::new(Orientation::Horizontal, 0);
    rule.add_css_class("image-editor-rule");

    let body = GtkBox::new(Orientation::Vertical, px(10.0, scale));
    body.set_vexpand(true);
    body.set_margin_top(px(14.0, scale));
    body.set_margin_bottom(px(6.0, scale));
    body.set_margin_start(px(16.0, scale));
    body.set_margin_end(px(16.0, scale));

    let note = Label::new(Some(&t(
        "Annotation tools are not wired in the Windows build yet. This window opens, displays, and zooms screenshots.",
    )));
    note.add_css_class("image-editor-note");
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.set_max_width_chars(36);
    body.append(&note);

    let actions = GtkBox::new(Orientation::Vertical, px(8.0, scale));
    actions.add_css_class("image-editor-actions");
    actions.set_valign(Align::End);

    for (label, tooltip, primary) in [
        (
            t("Copy"),
            t("Copy is not available in the Windows image editor yet."),
            false,
        ),
        (
            t("Upload"),
            t("Cloud upload is not available in the Windows image editor yet."),
            false,
        ),
        (
            t("Export"),
            t("Export is not available in the Windows image editor yet."),
            true,
        ),
    ] {
        let button = Button::with_label(&label);
        button.add_css_class("image-editor-action-button");
        if primary {
            button.add_css_class("image-editor-action-primary");
        }
        button.set_sensitive(false);
        button.set_tooltip_text(Some(&tooltip));
        button.set_hexpand(true);
        actions.append(&button);
    }

    root.append(&head);
    root.append(&rule);
    root.append(&body);
    root.append(&actions);

    let inspector = Inspector { root, title, hint };
    inspector.show_tool(Tool::Select);
    inspector
}

impl Inspector {
    pub(super) fn show_tool(&self, tool: Tool) {
        self.title.set_text(&tool.label());
        self.hint.set_text(&tool.hint());
    }
}
