//! The Watermark tool's Motion page.
//!
//! A picture picker, the Remove action, the size and inset dials and a
//! placement pad, backed by the Motion state the timeline owns. The chosen
//! image is decoded once for the preview and composited in card space, so it
//! follows the camera and the zoom exactly the way the titles do.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::cairo::{Context as CairoContext, Filter, ImageSurface};
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, DrawingArea, FileChooserAction, FileChooserNative, FileFilter,
    Label, Orientation, ResponseType,
};

use super::annotate::EditorHost;
use super::motion_controls::{MotionPad, PadLayer};
use super::motion_text::CardLayout;
use super::sections::section_card;
use super::timeline::Timeline;
use super::ui_support::px;
use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::recording::editor::model::{MotionState, MotionWatermark};
use crate::video_editor::side_panel::FilledSlider;

/// The longest edge a chosen mark is decoded at. It is drawn over a fraction
/// of the card, so a bounded decode loses nothing on screen.
const PREVIEW_MAX_EDGE: u32 = 1024;

/// The card, the picker, the size and inset dials and the placement pad,
/// plus the decoded mark the preview painter reads.
pub(super) struct Controls {
    pub page: GtkBox,
    surface: Rc<RefCell<Option<ImageSurface>>>,
    /// The path the decoded surface belongs to, so a state sync only touches
    /// the disk when the selection really changed.
    selected: Rc<RefCell<Option<String>>>,
    window: gtk4::glib::WeakRef<gtk4::ApplicationWindow>,
    name: Label,
    picker: Button,
    remove: Button,
    thumbnail: DrawingArea,
    size: FilledSlider,
    inset: FilledSlider,
    pad: MotionPad,
    mark_size: Rc<Cell<f64>>,
    mark_inset: Rc<Cell<f64>>,
}

impl Controls {
    pub(super) fn build(
        scale: f64,
        window: &gtk4::ApplicationWindow,
        host: &Rc<EditorHost>,
    ) -> Self {
        let page = section_card("Watermark", scale);
        let surface: Rc<RefCell<Option<ImageSurface>>> = Rc::new(RefCell::new(None));

        // The picker row: preview chip, chosen name and the folder affordance.
        let picker = Button::new();
        picker.add_css_class("image-editor-bg-image-row");
        picker.set_has_frame(false);
        picker.set_hexpand(true);
        picker.set_tooltip_text(Some(&t("Choose Watermark Image")));
        let inner = GtkBox::new(Orientation::Horizontal, px(10.0, scale));
        let thumbnail = DrawingArea::new();
        thumbnail.set_content_width(px(28.0, scale));
        thumbnail.set_content_height(px(28.0, scale));
        thumbnail.set_valign(Align::Center);
        thumbnail.set_can_target(false);
        thumbnail.add_css_class("image-editor-bg-custom-swatch");
        thumbnail.add_css_class("image-editor-bg-image-thumb");
        thumbnail.set_draw_func({
            let surface = surface.clone();
            move |_, context, width, height| {
                let surface = surface.borrow().clone();
                let Some(surface) = surface else {
                    return;
                };
                let width = f64::from(width);
                let height = f64::from(height);
                if width < 1.0 || height < 1.0 {
                    return;
                }
                let source_w = f64::from(surface.width().max(1));
                let source_h = f64::from(surface.height().max(1));
                let fit = (width / source_w).min(height / source_h);
                let draw_w = source_w * fit;
                let draw_h = source_h * fit;
                crate::capture::editor::render::rounded_rect_path(
                    context, 0.0, 0.0, width, height, 7.0,
                );
                context.clip();
                context.translate((width - draw_w) * 0.5, (height - draw_h) * 0.5);
                context.scale(fit, fit);
                context.set_source_surface(&surface, 0.0, 0.0).ok();
                let _ = context.paint();
            }
        });
        let name = Label::new(Some(&t("Select image...")));
        name.set_hexpand(true);
        name.set_xalign(0.0);
        name.set_valign(Align::Center);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        let icon = gtk4::Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
        icon.set_pixel_size(px(13.0, scale));
        icon.set_valign(Align::Center);
        icon.add_css_class("image-editor-bg-image-icon");
        inner.append(&thumbnail);
        inner.append(&name);
        inner.append(&icon);
        picker.set_child(Some(&inner));
        page.append(&picker);

        // Remove is the panel's quiet secondary action: the flat sidebar row
        // the Linux inspector uses, not a filled slab competing with the
        // picker above it.
        let remove = Button::with_label(&t("Remove"));
        remove.add_css_class("image-editor-watermark-remove");
        remove.set_has_frame(false);
        remove.set_halign(Align::Fill);
        remove.set_hexpand(true);
        remove.set_sensitive(false);
        remove.set_tooltip_text(Some(&t("Remove the watermark")));
        page.append(&remove);

        // Size and inset are dials; the placement is a pad over the card, the
        // same box the Motion inspector uses for positions, so the mark can be
        // dragged where it belongs instead of nudged by two numbers.
        let size = FilledSlider::new(&t("Watermark size"), 2.0, 80.0, 18.0, 1.0, 0, scale)
            .with_suffix("%");
        let inset = FilledSlider::new(&t("Watermark inset"), 0.0, 45.0, 4.0, 1.0, 0, scale)
            .with_suffix("%");
        page.append(&size.widget());
        page.append(&inset.widget());

        let pad = MotionPad::new(
            scale,
            Some(host.clone()),
            &t("Click or drag to place the watermark; double-click to center it"),
        );
        page.append(&pad.area);
        let mark_size = Rc::new(Cell::new(0.18));
        let mark_inset = Rc::new(Cell::new(0.04));
        let layer: PadLayer = {
            let surface = surface.clone();
            let mark_size = mark_size.clone();
            let mark_inset = mark_inset.clone();
            Rc::new(move |context, rect, point| {
                let surface = surface.borrow().clone();
                let Some(surface) = surface else {
                    return;
                };
                let source_w = f64::from(surface.width().max(1));
                let source_h = f64::from(surface.height().max(1));
                let (x, y, width, height) = mark_rect(
                    rect,
                    point,
                    mark_size.get(),
                    mark_inset.get(),
                    source_w,
                    source_h,
                );
                let _ = context.save();
                context.translate(x, y);
                context.scale(width / source_w, height / source_h);
                context.set_source_surface(&surface, 0.0, 0.0).ok();
                context.source().set_filter(Filter::Bilinear);
                context.paint().ok();
                let _ = context.restore();
            })
        };
        pad.set_layer(layer);

        Self {
            page,
            surface,
            selected: Rc::new(RefCell::new(None)),
            window: window.downgrade(),
            name,
            picker,
            remove,
            thumbnail,
            size,
            inset,
            pad,
            mark_size,
            mark_inset,
        }
    }

    /// The decoded mark the Motion preview composites, or nothing before a
    /// picture is chosen.
    pub(super) fn surface(&self) -> Rc<RefCell<Option<ImageSurface>>> {
        self.surface.clone()
    }

    pub(super) fn bind(&self, timeline: &Rc<Timeline>) {
        let chooser_slot: Rc<RefCell<Option<FileChooserNative>>> = Rc::new(RefCell::new(None));
        self.picker.connect_clicked({
            let timeline = Rc::downgrade(timeline);
            let window = self.window.clone();
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
                let timeline = timeline.clone();
                let chooser_slot_response = Rc::downgrade(&chooser_slot);
                chooser.connect_response(move |dialog, response| {
                    let path = (response == ResponseType::Accept)
                        .then(|| dialog.file().and_then(|file| file.path()))
                        .flatten();
                    dialog.destroy();
                    if let Some(slot) = chooser_slot_response.upgrade() {
                        slot.borrow_mut().take();
                    }
                    let (Some(path), Some(timeline)) = (path, timeline.upgrade()) else {
                        return;
                    };
                    let path = path.to_string_lossy().into_owned();
                    timeline
                        .edit_motion(|state| state.watermark.image_file_name = Some(path), false);
                });
                *chooser_slot.borrow_mut() = Some(chooser.clone());
                chooser.show();
            }
        });

        self.remove.connect_clicked({
            let timeline = Rc::downgrade(timeline);
            move |_| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(|state| state.watermark.image_file_name = None, false);
                }
            }
        });

        bind_slider(&self.size, timeline, |watermark, value| {
            watermark.size = value
        });
        bind_slider(&self.inset, timeline, |watermark, value| {
            watermark.inset = value
        });
        self.pad.on_change({
            let timeline = Rc::downgrade(timeline);
            Rc::new(move |x, y| {
                if let Some(timeline) = timeline.upgrade() {
                    timeline.edit_motion(|state| state.watermark.position = (x, y), false);
                }
            })
        });
        self.pad.on_interaction(
            interaction_callback(timeline, true),
            interaction_callback(timeline, false),
        );
    }

    pub(super) fn sync(&self, state: &MotionState) {
        let watermark = &state.watermark;
        self.size.set_value(watermark.size * 100.0);
        self.inset.set_value(watermark.inset * 100.0);
        self.mark_size.set(watermark.size);
        self.mark_inset.set(watermark.inset);
        self.pad
            .set_point(watermark.position.0, watermark.position.1);
        self.pad.area.queue_draw();
        self.show_selection(watermark.image_file_name.as_deref());
    }

    /// Reflect the model's selection. The decoded surface is rebuilt only
    /// when the path changed, so a slider tick never reopens the file.
    fn show_selection(&self, path: Option<&str>) {
        if self.selected.borrow().as_deref() == path {
            return;
        }
        *self.selected.borrow_mut() = path.map(str::to_owned);
        *self.surface.borrow_mut() = path.and_then(load_surface);
        let file_name = path.and_then(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        });
        match &file_name {
            Some(file_name) => {
                self.name.set_text(file_name);
                self.picker.set_tooltip_text(Some(file_name));
            }
            None => {
                self.name.set_text(&t("Select image..."));
                self.picker
                    .set_tooltip_text(Some(&t("Choose Watermark Image")));
            }
        }
        self.remove.set_sensitive(path.is_some());
        self.thumbnail.queue_draw();
    }
}

fn bind_slider(
    slider: &FilledSlider,
    timeline: &Rc<Timeline>,
    apply: fn(&mut MotionWatermark, f64),
) {
    slider.enable_keyboard(10.0);
    slider.on_interaction(
        interaction_callback(timeline, true),
        interaction_callback(timeline, false),
    );
    let timeline = Rc::downgrade(timeline);
    slider.on_change(Rc::new(move |value| {
        if let Some(timeline) = timeline.upgrade() {
            timeline.edit_motion(|state| apply(&mut state.watermark, value / 100.0), false);
        }
    }));
}

fn interaction_callback(timeline: &Rc<Timeline>, start: bool) -> Rc<dyn Fn()> {
    let timeline = Rc::downgrade(timeline);
    Rc::new(move || {
        if let Some(timeline) = timeline.upgrade() {
            if start {
                timeline.begin_edit();
            } else {
                timeline.finish_edit();
            }
        }
    })
}

fn load_surface(path: &str) -> Option<ImageSurface> {
    let image = image::open(path).ok()?;
    let image = if image.width().max(image.height()) > PREVIEW_MAX_EDGE {
        image.thumbnail(PREVIEW_MAX_EDGE, PREVIEW_MAX_EDGE)
    } else {
        image
    };
    crate::capture::editor::render::rgba_image_to_surface(&image.to_rgba8())
}

/// Where the mark lands inside a card rect, in that rect's own pixels. Shared
/// by the preview's card-space painter and the pad's overlay so both show the
/// same placement at their own scale.
fn mark_rect(
    (left, top, width, height): (f64, f64, f64, f64),
    position: (f64, f64),
    size: f64,
    inset: f64,
    source_w: f64,
    source_h: f64,
) -> (f64, f64, f64, f64) {
    let mark_w = (width * size.clamp(0.02, 0.8)).max(1.0);
    let mark_h = (mark_w * source_h / source_w).min(height * 0.8).max(1.0);
    let inset_x = (width * inset.clamp(0.0, 0.45)).min((width - mark_w).max(0.0) * 0.5);
    let inset_y = inset_x.min((height - mark_h).max(0.0) * 0.5);
    let x = left
        + (position.0.clamp(0.0, 1.0) * width - mark_w * 0.5)
            .clamp(inset_x, (width - inset_x - mark_w).max(inset_x));
    let y = top
        + (position.1.clamp(0.0, 1.0) * height - mark_h * 0.5)
            .clamp(inset_y, (height - inset_y - mark_h).max(inset_y));
    (x, y, mark_w, mark_h)
}

/// Paint the mark in source-card coordinates, above the card content and the
/// titles. The projection makes it track zoom and perspective with the card.
pub(super) fn paint(
    context: &CairoContext,
    card: CardLayout,
    motion: &MotionState,
    watermark: &ImageSurface,
) {
    if motion.watermark.image_file_name.is_none() {
        return;
    }
    let source_w = f64::from(watermark.width().max(1));
    let source_h = f64::from(watermark.height().max(1));
    let (x, y, width, height) = mark_rect(
        (0.0, 0.0, card.img_w(), card.img_h()),
        motion.watermark.position,
        motion.watermark.size,
        motion.watermark.inset,
        source_w,
        source_h,
    );
    let Some(matrix) = card.local_matrix(x, y) else {
        return;
    };
    let _ = context.save();
    context.transform(matrix);
    context.translate(x, y);
    context.scale(width / source_w, height / source_h);
    context.set_source_surface(watermark, 0.0, 0.0).ok();
    context.source().set_filter(Filter::Bilinear);
    context.paint().ok();
    let _ = context.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::MotionTransform;
    use gtk4::cairo::{Context, Format};

    #[test]
    fn watermark_is_composited_in_card_space_at_the_chosen_position() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let layout =
                crate::capture::editor::state::background::Appearance::default().layout(64, 64);
            let card = super::super::motion_text::card_layout(
                layout,
                64.0,
                64.0,
                MotionTransform::default(),
                (0.5, 0.5),
            );
            let mark = ImageSurface::create(Format::ARgb32, 8, 8).unwrap();
            {
                let context = Context::new(&mark).unwrap();
                context.set_source_rgb(1.0, 0.0, 0.0);
                context.paint().unwrap();
            }
            let mut frame = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                context.set_source_rgb(1.0, 1.0, 1.0);
                context.paint().unwrap();
            }
            let mut motion = MotionState::default();
            motion.watermark.image_file_name = Some("mark.png".into());
            motion.watermark.size = 0.25;
            motion.watermark.inset = 0.0;
            motion.watermark.position = (0.5, 0.5);
            {
                let context = Context::new(&frame).unwrap();
                paint(&context, card, &motion, &mark);
            }
            frame.flush();
            let data = frame.data().unwrap();
            let offset = (32 * 64 + 32) * 4;
            assert_eq!(&data[offset..offset + 4], &[0, 0, 255, 255]);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn the_panel_edits_the_timeline_and_reflects_it_back() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let app = gtk4::Application::new(
                Some("org.apexshot.test.watermark"),
                gtk4::gio::ApplicationFlags::NON_UNIQUE,
            );
            app.register(None::<&gtk4::gio::Cancellable>).unwrap();
            let window = gtk4::ApplicationWindow::new(&app);
            let host = EditorHost::new(Rc::new(|| {}), Rc::new(|_| {}), Rc::new(|| (0.0, 0.0)));
            *host.editor.borrow_mut() = Some(crate::capture::editor::state::EditorState::new(
                image::RgbaImage::new(80, 80),
            ));
            let controls = Controls::build(1.0, &window, &host);
            let timeline = Rc::new(super::super::timeline::build(1.0, true));
            controls.bind(&timeline);

            controls.sync(&timeline.motion_state());
            assert!(!controls.remove.is_sensitive());
            assert_eq!(controls.name.text(), t("Select image..."));

            let path = std::env::temp_dir().join("apexshot-watermark-panel-test.png");
            image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 30, 30, 255]))
                .save(&path)
                .unwrap();
            let path = path.to_string_lossy().into_owned();
            timeline.edit_motion(
                |state| state.watermark.image_file_name = Some(path.clone()),
                false,
            );
            controls.sync(&timeline.motion_state());
            assert!(controls.remove.is_sensitive());
            assert_eq!(controls.name.text(), "apexshot-watermark-panel-test.png");
            assert!(controls.surface.borrow().is_some());

            controls.size.set_value_from_test(42.0);
            assert!((timeline.motion_state().watermark.size - 0.42).abs() < 1e-9);

            // The pad is the placement control now: a click at its centre
            // moves the mark to the card's middle.
            let area = controls.pad.area.clone();
            area.measure(Orientation::Horizontal, -1);
            let height = area.measure(Orientation::Vertical, 240).0.max(32);
            area.allocate(240, height, -1, None);
            let controllers = area.observe_controllers();
            let click = (0..controllers.n_items())
                .filter_map(|index| controllers.item(index))
                .find_map(|item| item.downcast::<gtk4::GestureClick>().ok())
                .filter(|click| click.button() == 1)
                .unwrap();
            let (x, y) = (120.0_f64, f64::from(height) / 2.0);
            click.emit_by_name::<()>("pressed", &[&1_i32, &x, &y]);
            click.emit_by_name::<()>("released", &[&1_i32, &x, &y]);
            let position = timeline.motion_state().watermark.position;
            assert!((position.0 - 0.5).abs() < 1e-6);
            assert!((position.1 - 0.5).abs() < 1e-6);
            assert_eq!(controls.pad.point(), (0.5, 0.5));

            controls.remove.emit_clicked();
            assert_eq!(timeline.motion_state().watermark.image_file_name, None);
            controls.sync(&timeline.motion_state());
            assert!(!controls.remove.is_sensitive());
            assert!(controls.surface.borrow().is_none());
            let _ = std::fs::remove_file(&path);
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
