use std::cell::RefCell;
use std::rc::Rc;

use gtk4::{
    prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, DrawingArea, FileChooserAction,
    FileChooserNative, FileFilter, Image, Label, Orientation, ResponseType,
};

use crate::capture::editor::window::icon_names;
use crate::i18n::t;

use super::session::MotionSession;
use super::widgets::motion_appearance_slider;

/// Watermark state is a dedicated Motion layer, not part of the background
/// or card Appearance snapshot. Coordinates are normalized to the card so
/// the chosen placement remains stable between preview and MP4 export.
pub(in crate::capture::editor::window) fn build_motion_watermark_panel(
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

    let selected_name = image_file_name
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned);

    let picker = Button::new();
    picker.add_css_class("recording-editor-bg-image-row");
    picker.set_has_frame(false);
    picker.set_hexpand(true);
    picker.set_tooltip_text(Some(
        selected_name
            .as_deref()
            .unwrap_or(&t("Choose Watermark Image")),
    ));
    let picker_inner = GtkBox::new(Orientation::Horizontal, 10);
    let thumbnail = DrawingArea::new();
    thumbnail.add_css_class("recording-editor-bg-custom-swatch");
    thumbnail.set_content_width(28);
    thumbnail.set_content_height(28);
    thumbnail.set_valign(Align::Center);
    thumbnail.set_can_target(false);
    thumbnail.set_draw_func({
        let runtime = session.runtime.clone();
        move |_, cr, width, height| {
            let surface = runtime.borrow().watermark_surface.clone();
            let Some(surface) = surface else {
                return;
            };
            let source_w = surface.width().max(1) as f64;
            let source_h = surface.height().max(1) as f64;
            let scale = (f64::from(width) / source_w).min(f64::from(height) / source_h);
            let _ = cr.save();
            cr.translate(
                (f64::from(width) - source_w * scale) * 0.5,
                (f64::from(height) - source_h * scale) * 0.5,
            );
            cr.scale(scale, scale);
            let _ = cr.set_source_surface(&surface, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
    });
    let name_label = Label::new(Some(
        selected_name.as_deref().unwrap_or(&t("Select image...")),
    ));
    name_label.set_hexpand(true);
    name_label.set_xalign(0.0);
    name_label.set_valign(Align::Center);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let icon = Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    icon.set_pixel_size(13);
    icon.set_valign(Align::Center);
    picker_inner.append(&thumbnail);
    picker_inner.append(&name_label);
    picker_inner.append(&icon);
    picker.set_child(Some(&picker_inner));
    section.append(&picker);

    let show_selection: Rc<dyn Fn(Option<&str>)> = {
        let name_label = name_label.downgrade();
        let picker = picker.downgrade();
        let thumbnail = thumbnail.downgrade();
        Rc::new(move |name: Option<&str>| {
            if let Some(name_label) = name_label.upgrade() {
                name_label.set_text(name.unwrap_or(&t("Select image...")));
            }
            if let Some(picker) = picker.upgrade() {
                picker.set_tooltip_text(Some(name.unwrap_or(&t("Choose Watermark Image"))));
            }
            if let Some(thumbnail) = thumbnail.upgrade() {
                thumbnail.queue_draw();
            }
        })
    };

    let clear = Button::with_label(&t("Remove"));
    clear.set_has_frame(false);
    clear.add_css_class("editor-sidebar-action-button");
    section.append(&clear);

    let chooser_slot = Rc::new(RefCell::new(None::<FileChooserNative>));
    picker.connect_clicked({
        let window = window.downgrade();
        let session = session.clone();
        let preview = preview.clone();
        let show_selection = show_selection.clone();
        let chooser_slot = chooser_slot.clone();
        move |_| {
            if chooser_slot.borrow().is_some() {
                return;
            }
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
            let show_selection = show_selection.clone();
            let chooser_slot_response = Rc::downgrade(&chooser_slot);
            chooser.connect_response(move |dialog, response| {
                let path = (response == ResponseType::Accept)
                    .then(|| dialog.file().and_then(|file| file.path()))
                    .flatten();
                dialog.destroy();
                if let Some(slot) = chooser_slot_response.upgrade() {
                    slot.borrow_mut().take();
                }
                let Some(path) = path else {
                    return;
                };
                let path = path.to_string_lossy().into_owned();
                let surface = super::super::motion_render::load_motion_background_preview_surface(
                    &path,
                    super::super::background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
                );
                let Some(surface) = surface else {
                    return;
                };
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&path)
                    .to_owned();
                let mut runtime = session.runtime.borrow_mut();
                runtime.begin_motion_edit();
                runtime.motion.watermark.image_file_name = Some(path);
                runtime.watermark_surface = Some(surface);
                drop(runtime);
                show_selection(Some(&name));
                preview.queue_draw();
            });
            *chooser_slot.borrow_mut() = Some(chooser.clone());
            chooser.show();
        }
    });

    clear.connect_clicked({
        let session = session.clone();
        let preview = preview.clone();
        let show_selection = show_selection.clone();
        move |_| {
            let mut runtime = session.runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.watermark.image_file_name = None;
            runtime.watermark_surface = None;
            drop(runtime);
            show_selection(None);
            preview.queue_draw();
        }
    });

    let size_slider = motion_appearance_slider("Watermark size", 0.02, 0.80, size, "%");
    size_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.watermark.size = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&size_slider.widget());

    let inset_slider = motion_appearance_slider("Watermark inset", 0.0, 0.45, inset, "%");
    inset_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.watermark.inset = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&inset_slider.widget());

    let x_slider = motion_appearance_slider("Watermark position X", 0.0, 1.0, position.0, "%");
    x_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.watermark.position.0 = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&x_slider.widget());

    let y_slider = motion_appearance_slider("Watermark position Y", 0.0, 1.0, position.1, "%");
    y_slider.connect_value_changed({
        let runtime = session.runtime.clone();
        let preview = preview.clone();
        move |slider| {
            let mut runtime = runtime.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.watermark.position.1 = slider.value();
            preview.queue_draw();
        }
    });
    section.append(&y_slider.widget());
    section
}
