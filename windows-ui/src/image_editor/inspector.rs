//! The right-hand inspector, shared with the Linux editor's dock.
//!
//! Static mode shows the shared Appearance builder; Motion swaps the dock's
//! page to the Motion inspector's tab set (Composition, Text, Appearance,
//! Watermark). The static Background card is connected to the document; the
//! remaining Appearance and Motion controls are still shells.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, PolicyType, ScrolledWindow, Stack};

use super::sections;
use super::toolbar::Tool;
use super::ui_support::{px, INSPECTOR_WIDTH};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::video_editor::side_panel::FilledSlider;

pub(super) struct Inspector {
    pub root: GtkBox,
    scroller: ScrolledWindow,
    actions: GtkBox,
    motion_scroller: ScrolledWindow,
    motion_stack: Stack,
    motion_duration: FilledSlider,
    background_sync: Rc<dyn Fn()>,
    /// Clipboard copy and the flattened export the window wires to the document.
    pub copy_button: gtk4::Button,
    pub export_button: gtk4::Button,
}

impl Inspector {
    pub(super) fn sync_background(&self) {
        (self.background_sync)();
    }
    /// Swap the dock's content for Motion or back: the Motion pages hide the
    /// Appearance scroll and its bottom actions, keeping the strip the
    /// caption buttons live in. Entry always lands on the Composition page,
    /// like the Linux editor applies its mode.
    pub(super) fn set_mode(&self, motion: bool) {
        self.scroller.set_visible(!motion);
        self.actions.set_visible(!motion);
        self.motion_scroller.set_visible(motion);
        self.motion_stack.set_visible(motion);
        if motion {
            self.motion_stack.set_visible_child_name("motion");
        }
    }

    /// The static editor's tool switch knows the dock; the pages arrive
    /// with each tool's drawing backend.
    pub(super) fn show_tool(&self, _tool: Tool) {}

    /// The notch asked for a Motion tool page; switch the dock over.
    pub(super) fn show_motion_page(&self, page: &str) {
        self.motion_stack.set_visible_child_name(page);
    }

    /// Hand the Composition slider to the Motion timeline so a drag resizes
    /// the composition.
    pub(super) fn set_motion_duration_listener(&self, listener: Rc<dyn Fn(f64)>) {
        self.motion_duration.on_change(listener);
    }
}

pub(super) fn build(
    scale: f64,
    host: &Rc<super::annotate::EditorHost>,
    window: &gtk4::ApplicationWindow,
) -> Inspector {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("image-editor-inspector");
    root.set_hexpand(false);
    root.set_size_request(px(INSPECTOR_WIDTH, scale), -1);

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .overlay_scrolling(true)
        .vexpand(true)
        .build();
    scroller.add_css_class("image-editor-inspector-scroll");
    // The wallpaper catalog reports a big natural size; keep the panel at its
    // fixed width and let the content scroll inside.
    scroller.set_propagate_natural_width(false);
    scroller.set_propagate_natural_height(false);
    scroller.set_hexpand(false);

    // The static Appearance dock: the Linux builder attaches its Transform
    // extension only here, so the port does the same.
    let (static_page, background_sync) =
        sections::appearance_panel(scale, true, Some(host), &root, window);
    scroller.set_child(Some(&static_page));
    root.append(&scroller);

    // The Motion dock: the notch's four pages, in the Linux tab order.
    let motion_stack = Stack::new();
    // Equal pages: a non-homogeneous stack would size each tool page to its
    // own content, so the dock would visibly change widths on every notch
    // click. One allocation matches the static Appearance panel's viewport.
    motion_stack.set_hhomogeneous(true);
    motion_stack.set_vhomogeneous(false);

    let composition_page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    composition_page.add_css_class("image-editor-inspector-body");
    let composition_hint = Label::new(Some(&t(
        "Studio preview. MP4 export needs activation; local editing and project saves remain free.",
    )));
    composition_hint.add_css_class("image-editor-hint");
    composition_hint.set_wrap(true);
    composition_hint.set_xalign(0.0);
    composition_page.append(&composition_hint);
    let composition_card = sections::section_card("Composition", scale);
    let duration =
        FilledSlider::new(&t("Duration"), 1.0, 10.0, 6.0, 0.5, 1, scale).with_suffix("s");
    composition_card.append(&duration.widget());
    composition_page.append(&composition_card);
    motion_stack.add_named(&composition_page, Some("motion"));

    // The Linux Text page carries four sections of its own; this slice
    // leaves the panel's port to a next one and says so where it would be.
    let text_page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    text_page.add_css_class("image-editor-inspector-body");
    text_page.append(&sections::section_card("Text", scale));
    let text_hint = Label::new(Some(&t(
        "The Text panel is not available in the Windows image editor yet",
    )));
    text_hint.add_css_class("image-editor-hint");
    text_hint.set_wrap(true);
    text_hint.set_xalign(0.0);
    text_page.append(&text_hint);
    motion_stack.add_named(&text_page, Some("motion-text"));

    motion_stack.add_named(
        &sections::appearance_panel(scale, false, None, &root, window).0,
        Some("motion-appearance"),
    );

    let watermark_page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    watermark_page.add_css_class("image-editor-inspector-body");
    watermark_page.append(&sections::watermark_panel(scale));
    motion_stack.add_named(&watermark_page, Some("motion-watermark"));

    // Hidden until Motion is open; the dock shows one mode at a time.
    motion_stack.set_visible_child_name("motion");
    motion_stack.set_visible(false);

    // The Motion pages are long (the Appearance card especially), and a
    // stack reports its widest page's natural size — which would stretch
    // the dock to window width and starve the preview. The same clipped
    // scroll the static dock uses keeps the dock at its fixed width.
    let motion_scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .overlay_scrolling(true)
        .vexpand(true)
        .build();
    motion_scroller.add_css_class("image-editor-inspector-scroll");
    motion_scroller.set_propagate_natural_width(false);
    motion_scroller.set_propagate_natural_height(false);
    motion_scroller.set_hexpand(false);
    motion_scroller.set_child(Some(&motion_stack));
    motion_scroller.set_visible(false);
    root.append(&motion_scroller);

    // Bottom actions: copy and upload are icon buttons on the left, Export
    // owns the right edge as the panel's one accent pill. They belong to the
    // static editor; Motion's tools export through their own paths.
    let actions = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    actions.add_css_class("image-editor-actions");
    let mut action_buttons = Vec::new();
    for (icon, tooltip) in [
        (icon_names::custom::COPY_SYMBOLIC, "Copy to clipboard"),
        (
            icon_names::custom::CLOUD_OUTLINE_THIN_SYMBOLIC,
            "Upload to cloud",
        ),
    ] {
        let button = action_icon_button(scale, icon, &t(tooltip));
        actions.append(&button);
        action_buttons.push(button);
    }
    let copy_button = action_buttons[0].clone();
    let upload_button = action_buttons[1].clone();
    // Upload stays a shell until the Windows cloud backend exists.
    upload_button.set_sensitive(false);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&spacer);
    let export = action_export_button(scale);
    actions.append(&export);
    root.append(&actions);

    Inspector {
        root,
        scroller,
        actions,
        motion_scroller,
        motion_stack,
        motion_duration: duration,
        background_sync,
        copy_button,
        export_button: export,
    }
}

/// A flat 18px icon button, the shape the Linux sidebar gives Copy and Upload.
fn action_icon_button(scale: f64, icon: &str, tooltip: &str) -> gtk4::Button {
    let image = gtk4::Image::from_icon_name(icon);
    image.set_pixel_size(px(18.0, scale));
    image.add_css_class("image-editor-action-icon-glyph");
    let button = gtk4::Button::builder()
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .child(&image)
        .sensitive(false)
        .build();
    button.add_css_class("image-editor-action-icon");
    button
}

/// Export is the panel's one accent pill: it flattens the document and saves
/// it through the platform dialog.
fn action_export_button(scale: f64) -> gtk4::Button {
    let export = gtk4::Button::with_label(&t("Export"));
    export.add_css_class("image-editor-export");
    export.set_has_frame(false);
    export.set_valign(gtk4::Align::Center);
    export.set_sensitive(false);
    export.set_tooltip_text(Some(&t("Save the annotated image")));
    let _ = scale;
    export
}
