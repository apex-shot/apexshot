use gtk4::{
    prelude::*, ApplicationWindow, Box as GtkBox, Button, DrawingArea, FileChooserAction,
    FileChooserNative, FileFilter, Label, Orientation, ResponseType,
};

use crate::i18n::t;

use super::session::MotionSession;
use super::widgets::motion_appearance_slider;

/// Watermark state is a dedicated Motion layer, not part of the background
/// or card Appearance snapshot. Coordinates are normalized to the card so
/// the chosen placement remains stable between preview and MP4 export.
pub(super) fn build_motion_watermark_panel(
    window: &ApplicationWindow,
    session: &MotionSession,
    preview: &DrawingArea,
) -> GtkBox {
    let (image_file_name, size, inset, position) = {
        let runtime = session.runtime.borrow();
        let watermark = &runtime.motion.watermark;
        (
            watermark.image_file_name.clone(),
            watermark.size,
            watermark.inset,
            watermark.position,
        )
    };
    let section = GtkBox::new(Orientation::Vertical, 6);
    section.add_css_class("editor-inspector-placeholder-shell");
    section.add_css_class("editor-motion-inspector");
    section.add_css_class("editor-motion-watermark-section");
    section.set_hexpand(false);
    section.set_vexpand(false);
    let title = Label::new(Some(&t("Watermark")));
    title.add_css_class("editor-background-section-title");
    title.set_xalign(0.0);
    section.append(&title);

    let selected_file = Label::new(Some(
        image_file_name
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or(&t("No image selected")),
    ));
    selected_file.set_xalign(0.0);
    selected_file.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    selected_file.add_css_class("editor-motion-watermark-file");
    section.append(&selected_file);

    let choose = Button::with_label(&t("Choose…"));
    choose.set_has_frame(false);
    choose.add_css_class("editor-sidebar-action-button");
    choose.connect_clicked({
        let window = window.downgrade();
        let session = session.clone();
        let preview = preview.clone();
        let selected_file = selected_file.clone();
        move |_| {
            let chooser = FileChooserNative::new(
                Some(&t("Choose Watermark Image")),
                window.upgrade().as_ref(),
                FileChooserAction::Open,
                Some(&t("Choose")),
                Some(&t("Cancel")),
            );
            let filter = FileFilter::new();
            filter.set_name(Some(&t("Images")));
            for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                filter.add_pattern(pattern);
            }
            chooser.add_filter(&filter);
            let session = session.clone();
            let preview = preview.clone();
            let selected_file = selected_file.clone();
            chooser.connect_response(move |dialog, response| {
                if response != ResponseType::Accept {
                    return;
                }
                let Some(path) = dialog.file().and_then(|file| file.path()) else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                let surface = super::super::motion_render::load_motion_background_surface(&path);
                let Some(surface) = surface else {
                    return;
                };
                selected_file.set_text(
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&path),
                );
                let mut runtime = session.runtime.borrow_mut();
                runtime.motion.watermark.image_file_name = Some(path);
                runtime.watermark_surface = Some(surface);
                preview.queue_draw();
            });
            chooser.show();
        }
    });
    section.append(&choose);

    let clear = Button::with_label(&t("Remove"));
    clear.set_has_frame(false);
    clear.add_css_class("editor-sidebar-action-button");
    clear.connect_clicked({
        let session = session.clone();
        let preview = preview.clone();
        let selected_file = selected_file.clone();
        move |_| {
            let mut runtime = session.runtime.borrow_mut();
            runtime.motion.watermark.image_file_name = None;
            runtime.watermark_surface = None;
            selected_file.set_text(&t("No image selected"));
            preview.queue_draw();
        }
    });
    section.append(&clear);

    let size_slider = motion_appearance_slider("Watermark size", 0.02, 0.80, size, "%");
    size_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.watermark.size = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&size_slider.widget());

    let inset_slider = motion_appearance_slider("Watermark inset", 0.0, 0.45, inset, "%");
    inset_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.watermark.inset = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&inset_slider.widget());

    let x_slider = motion_appearance_slider("Watermark position X", 0.0, 1.0, position.0, "%");
    x_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.watermark.position.0 = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&x_slider.widget());

    let y_slider = motion_appearance_slider("Watermark position Y", 0.0, 1.0, position.1, "%");
    y_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            runtime.borrow_mut().motion.watermark.position.1 = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&y_slider.widget());
    section
}
