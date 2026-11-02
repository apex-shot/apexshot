//! The right-hand inspector, shared with the Linux editor's dock.
//!
//! Static mode shows the shared Appearance builder; Motion swaps the dock's
//! page to the Motion inspector's tab set (Composition, Text, Appearance,
//! Watermark). Appearance uses the same controls and document in both modes.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, PolicyType, ScrolledWindow, Stack};

use super::motion_controls::MotionControls;
use super::sections;
use super::toolbar::Tool;
use super::ui_support::{px, INSPECTOR_WIDTH};
use crate::capture::editor::window::icon_names;
use crate::i18n::t;

pub(super) struct Inspector {
    pub root: GtkBox,
    scroller: ScrolledWindow,
    actions: GtkBox,
    motion_stack: Stack,
    motion_controls: MotionControls,
    transform: GtkBox,
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
        self.actions.set_visible(!motion);
        self.transform.set_visible(!motion);
        self.motion_stack.set_visible_child_name(if motion {
            "motion"
        } else {
            "motion-appearance"
        });
    }

    /// The static editor's tool switch knows the dock; the pages arrive
    /// with each tool's drawing backend.
    pub(super) fn show_tool(&self, _tool: Tool) {}

    /// The notch asked for a Motion tool page; switch the dock over.
    pub(super) fn show_motion_page(&self, page: &str) {
        self.motion_stack.set_visible_child_name(page);
        self.scroller.vadjustment().set_value(0.0);
    }

    /// Hand the Composition slider to the Motion timeline so a drag resizes
    /// the composition.
    pub(super) fn set_motion_duration_listener(&self, listener: Rc<dyn Fn(f64)>) {
        self.motion_controls.duration.on_change(listener);
    }

    pub(super) fn bind_motion(&self, timeline: &Rc<super::timeline::Timeline>) {
        self.motion_controls.bind(timeline);
        self.motion_controls.sync(&timeline.motion_state());
    }

    pub(super) fn sync_motion(&self, state: &crate::recording::editor::model::MotionState) {
        self.motion_controls.sync(state);
    }

    pub(super) fn refresh_motion_thumbnail(&self) {
        self.motion_controls.refresh_thumbnail();
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

    // Both modes use the same panel; only Static exposes its Transform.
    let (appearance_page, transform, background_sync) =
        sections::appearance_panel(scale, Some(host), &root, window);
    // The Motion dock: the notch's four pages, in the Linux tab order.
    let motion_stack = Stack::new();
    // Equal pages: a non-homogeneous stack would size each tool page to its
    // own content, so the dock would visibly change widths on every notch
    // click. One allocation matches the static Appearance panel's viewport.
    motion_stack.set_hhomogeneous(true);
    motion_stack.set_vhomogeneous(false);
    motion_stack.add_named(&appearance_page, Some("motion-appearance"));

    let motion_controls = MotionControls::build(scale, host);
    motion_stack.add_named(&motion_controls.page, Some("motion"));

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

    let watermark_page = GtkBox::new(Orientation::Vertical, px(12.0, scale));
    watermark_page.add_css_class("image-editor-inspector-body");
    watermark_page.append(&sections::watermark_panel(scale));
    motion_stack.add_named(&watermark_page, Some("motion-watermark"));

    // One homogeneous stack and viewport keep the dock's requisition
    // independent of which tool is active.
    motion_stack.set_visible_child_name("motion-appearance");
    scroller.set_child(Some(&motion_stack));
    root.append(&scroller);

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
        motion_stack,
        motion_controls,
        transform,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::EditorState;
    use crate::capture::editor::types::FrameStyle;
    use image::RgbaImage;

    fn collect_class(widget: &gtk4::Widget, class: &str, found: &mut Vec<gtk4::Widget>) {
        if widget.has_css_class(class) {
            found.push(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            collect_class(&current, class, found);
            child = current.next_sibling();
        }
    }

    #[test]
    fn motion_and_static_share_the_appearance_panel_without_static_transform() {
        let Some(()) = crate::test_support::with_gtk(|| {
            super::super::ui_support::install_css();
            let app = gtk4::Application::new(None::<&str>, Default::default());
            app.register(None::<&gtk4::gio::Cancellable>).unwrap();
            let window = gtk4::ApplicationWindow::new(&app);
            let host = super::super::annotate::EditorHost::new(
                Rc::new(|| {}),
                Rc::new(|_| {}),
                Rc::new(|| (0.0, 0.0)),
            );
            *host.editor.borrow_mut() = Some(EditorState::new(RgbaImage::new(100, 80)));
            let inspector = build(1.0, &host, &window);
            inspector.sync_background();
            let page = inspector
                .motion_stack
                .child_by_name("motion-appearance")
                .unwrap();
            let mut styles = Vec::new();
            collect_class(&page, "image-editor-frame-tile", &mut styles);
            assert!(styles.len() > 1);
            let style = styles[1].clone().downcast::<gtk4::Button>().unwrap();
            style.emit_clicked();
            inspector.sync_background();
            assert_eq!(
                host.read(|state| state.background.frame_style),
                Some(FrameStyle::ALL[1])
            );
            assert!(style.has_css_class("selected-style"));
            assert!(inspector.transform.is_visible());

            inspector.set_mode(true);
            inspector.show_motion_page("motion-appearance");
            assert_eq!(inspector.motion_stack.visible_child(), Some(page.clone()));
            assert!(inspector.scroller.is_visible());
            assert!(!inspector.transform.is_visible());
            styles[0]
                .clone()
                .downcast::<gtk4::Button>()
                .unwrap()
                .emit_clicked();
            inspector.sync_background();
            assert_eq!(
                host.read(|state| state.background.frame_style),
                Some(FrameStyle::ALL[0])
            );
            let width = inspector.root.measure(Orientation::Horizontal, -1);
            let appearance_width = page.measure(Orientation::Horizontal, -1).0;
            assert_eq!(width.0, appearance_width.max(px(INSPECTOR_WIDTH, 1.0)));
            for name in [
                "motion",
                "motion-text",
                "motion-watermark",
                "motion-appearance",
            ] {
                inspector.show_motion_page(name);
                assert_eq!(inspector.root.measure(Orientation::Horizontal, -1), width);
            }
            let mut motion_state = crate::recording::editor::model::MotionState::default();
            motion_state.add_segment_at(0.0);
            inspector.sync_motion(&motion_state);
            for name in [
                "motion",
                "motion-text",
                "motion-watermark",
                "motion-appearance",
            ] {
                inspector.show_motion_page(name);
                assert_eq!(inspector.root.measure(Orientation::Horizontal, -1), width);
            }
            assert!(inspector.scroller.is_visible());
            inspector.set_mode(false);
            assert!(inspector.scroller.is_visible());
            assert!(inspector.transform.is_visible());
            assert!(styles[0].has_css_class("selected-style"));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
