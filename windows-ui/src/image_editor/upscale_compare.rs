//! The before / after sheet for neural export enhancement.
//!
//! It is an in-window scrim like the mode-switch warning. Left of the divider
//! is the conventional enlargement, right of it the network's, both of the
//! flattened export pixels, so the sheet shows exactly what enhancement
//! changes. Rendering runs on a worker and the result is cached per mode.

use gtk4::cairo::{Filter, ImageSurface};
use gtk4::glib::{self, Propagation};
use gtk4::{
    gdk, prelude::*, Align, Box as GtkBox, Button, DrawingArea, EventControllerKey,
    EventControllerMotion, GestureDrag, Image, Label, Orientation, Overflow, Overlay, PolicyType,
    ScrolledWindow, Spinner, ToggleButton,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::capture::editor::render;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::upscale::{self, SourceUpscale};
use crate::i18n::{t, tfmt};

use super::annotate::EditorHost;
use super::ui_support::px;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

struct Comparison {
    before: ImageSurface,
    after: ImageSurface,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zoom {
    Fit,
    Actual,
    Double,
}

pub(super) fn show(
    window: &gtk4::ApplicationWindow,
    host: &Rc<EditorHost>,
    scale: f64,
    initial: SourceUpscale,
    on_apply: Rc<dyn Fn(SourceUpscale)>,
) {
    if OPEN.with(|open| open.get()) {
        return;
    }
    OPEN.with(|open| open.set(true));
    let Some(overlay) = window.child().and_downcast::<gtk4::Overlay>() else {
        OPEN.with(|open| open.set(false));
        return;
    };
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let theme = if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    };

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("image-editor-confirm-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);

    let card = GtkBox::new(Orientation::Vertical, px(14.0, scale));
    card.add_css_class("image-editor-upscale-card");
    card.add_css_class(theme);
    card.set_halign(Align::Center);
    card.set_can_target(true);
    let width = (f64::from(window.width()) * 0.84)
        .clamp(f64::from(px(560.0, scale)), f64::from(px(980.0, scale)));
    let height = (f64::from(window.height()) * 0.84)
        .clamp(f64::from(px(420.0, scale)), f64::from(px(720.0, scale)));
    card.set_size_request(width as i32, height as i32);

    let header = GtkBox::new(Orientation::Horizontal, px(12.0, scale));
    let title_block = GtkBox::new(Orientation::Vertical, px(3.0, scale));
    title_block.set_hexpand(true);
    let title = Label::new(Some(&t("Compare enhancement")));
    title.add_css_class("image-editor-upscale-title");
    title.set_xalign(0.0);
    let subtitle = Label::new(Some(&t(if crate::studio_entitlement::studio_unlocked() {
        "Drag the divider to compare the standard enlargement with neural enhancement."
    } else {
        "Drag the divider to compare the standard enlargement with neural enhancement. Previewing is free; enhanced exports need Studio."
    })));
    subtitle.add_css_class("image-editor-upscale-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    title_block.append(&title);
    title_block.append(&subtitle);
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(px(14.0, scale));
    let close_button = Button::new();
    close_button.add_css_class("image-editor-upscale-close");
    close_button.set_has_frame(false);
    close_button.set_can_focus(false);
    close_button.set_valign(Align::Start);
    close_button.set_tooltip_text(Some(&t("Close")));
    close_button.set_child(Some(&close_icon));
    header.append(&title_block);
    header.append(&close_button);

    let toolbar = GtkBox::new(Orientation::Horizontal, px(12.0, scale));
    toolbar.add_css_class("image-editor-upscale-toolbar");
    let (modes, mode_tabs) = segmented(&["2×", "4×"]);
    mode_tabs[0].set_tooltip_text(Some(&t("Neural 2×")));
    mode_tabs[1].set_tooltip_text(Some(&t("Neural 4×")));
    if initial == SourceUpscale::Neural4x {
        mode_tabs[1].set_active(true);
    }
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let (zooms, zoom_tabs) = segmented(&["Fit", "100%", "200%"]);
    zooms.set_tooltip_text(Some(&t("Zoom")));
    toolbar.append(&modes);
    toolbar.append(&spacer);
    toolbar.append(&zooms);

    if let Some(reason) = host
        .read(|st| st.upscale_unavailable_reason(SourceUpscale::Neural4x))
        .flatten()
    {
        mode_tabs[1].set_sensitive(false);
        mode_tabs[1].set_tooltip_text(Some(&t(&reason)));
    }

    let drawing = DrawingArea::new();
    drawing.set_hexpand(true);
    drawing.set_vexpand(true);
    let scroll = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&drawing)
        .build();
    scroll.add_css_class("image-editor-upscale-scroll");

    let stage_overlay = Overlay::new();
    stage_overlay.set_hexpand(true);
    stage_overlay.set_vexpand(true);
    stage_overlay.set_child(Some(&scroll));

    let spinner = Spinner::new();
    let message = Label::new(None);
    message.add_css_class("image-editor-upscale-message");
    message.set_wrap(true);
    message.set_max_width_chars(52);
    message.set_justify(gtk4::Justification::Center);
    let status = GtkBox::new(Orientation::Vertical, px(10.0, scale));
    status.set_halign(Align::Center);
    status.set_valign(Align::Center);
    status.append(&spinner);
    status.append(&message);
    stage_overlay.add_overlay(&status);

    let before_badge = Label::new(Some(&t("Original")));
    before_badge.add_css_class("image-editor-upscale-badge");
    before_badge.set_halign(Align::Start);
    before_badge.set_valign(Align::Start);
    before_badge.set_margin_top(px(12.0, scale));
    before_badge.set_margin_start(px(12.0, scale));
    before_badge.set_can_target(false);
    before_badge.set_visible(false);
    let after_badge = Label::new(None);
    after_badge.add_css_class("image-editor-upscale-badge");
    after_badge.add_css_class("image-editor-upscale-badge-accent");
    after_badge.set_halign(Align::End);
    after_badge.set_valign(Align::Start);
    after_badge.set_margin_top(px(12.0, scale));
    after_badge.set_margin_end(px(12.0, scale));
    after_badge.set_can_target(false);
    after_badge.set_visible(false);
    stage_overlay.add_overlay(&before_badge);
    stage_overlay.add_overlay(&after_badge);

    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("image-editor-upscale-stage");
    stage.set_overflow(Overflow::Hidden);
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    stage.append(&stage_overlay);

    let footer = GtkBox::new(Orientation::Horizontal, px(8.0, scale));
    footer.add_css_class("image-editor-upscale-footer");
    let dims = Label::new(None);
    dims.add_css_class("image-editor-upscale-dims");
    dims.set_xalign(0.0);
    dims.set_hexpand(true);
    dims.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let cancel = Button::with_label(&t("Cancel"));
    cancel.add_css_class("image-editor-confirm-cancel");
    cancel.set_has_frame(false);
    let apply = Button::with_label(&t("Use Neural 2×"));
    apply.add_css_class("image-editor-confirm-continue");
    apply.set_has_frame(false);
    apply.set_sensitive(false);
    footer.append(&dims);
    footer.append(&cancel);
    footer.append(&apply);

    card.append(&header);
    card.append(&toolbar);
    card.append(&stage);
    card.append(&footer);

    let top_gap = GtkBox::new(Orientation::Vertical, 0);
    top_gap.set_vexpand(true);
    let bottom_gap = GtkBox::new(Orientation::Vertical, 0);
    bottom_gap.set_vexpand(true);
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.set_hexpand(true);
    let left_gap = GtkBox::new(Orientation::Horizontal, 0);
    left_gap.set_hexpand(true);
    let right_gap = GtkBox::new(Orientation::Horizontal, 0);
    right_gap.set_hexpand(true);
    row.append(&left_gap);
    row.append(&card);
    row.append(&right_gap);
    scrim.append(&top_gap);
    scrim.append(&row);
    scrim.append(&bottom_gap);

    let shown: Rc<RefCell<Option<Rc<Comparison>>>> = Rc::new(RefCell::new(None));
    let cache: Rc<RefCell<[Option<Rc<Comparison>>; 2]>> = Rc::new(RefCell::new([None, None]));
    let divider = Rc::new(Cell::new(0.5));
    let zoom = Rc::new(Cell::new(Zoom::Fit));
    let generation = Rc::new(Cell::new(0u64));
    let closed = Rc::new(Cell::new(false));
    let mode = Rc::new(Cell::new(initial));

    drawing.set_draw_func({
        let shown = shown.clone();
        let divider = divider.clone();
        let zoom = zoom.clone();
        move |area, context, width, height| {
            render::draw_canvas_checkerboard_background(context, width, height, None, !dark);
            let shown = shown.borrow();
            let Some(comparison) = shown.as_deref() else {
                return;
            };
            let (scale, x, y) = placement(area, comparison, zoom.get());
            context.translate(x, y);
            context.scale(scale, scale);
            let filter = if scale * f64::from(area.scale_factor().max(1)) >= 1.0 {
                Filter::Nearest
            } else {
                Filter::Good
            };
            let _ = context.set_source_surface(&comparison.before, 0.0, 0.0);
            context.source().set_filter(filter);
            let _ = context.paint();
            let split = divider.get() * f64::from(comparison.after.width());
            let _ = context.save();
            context.rectangle(
                split,
                0.0,
                f64::from(comparison.after.width()) - split,
                f64::from(comparison.after.height()),
            );
            context.clip();
            let _ = context.set_source_surface(&comparison.after, 0.0, 0.0);
            context.source().set_filter(filter);
            let _ = context.paint();
            let _ = context.restore();

            context.set_source_rgba(0.0, 0.0, 0.0, 0.28);
            context.set_line_width(3.0 / scale);
            context.move_to(split, 0.0);
            context.line_to(split, f64::from(comparison.after.height()));
            let _ = context.stroke();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.set_line_width(1.5 / scale);
            context.move_to(split, 0.0);
            context.line_to(split, f64::from(comparison.after.height()));
            let _ = context.stroke();

            // A grab handle makes the divider read as a control rather than
            // a seam between two renders. It is drawn this way so the drag
            // target is obvious before the pointer touches it.
            let middle = f64::from(comparison.after.height()) / 2.0;
            let radius = 15.0 / scale;
            context.set_source_rgba(0.0, 0.0, 0.0, 0.30);
            context.arc(
                split,
                middle,
                radius + 2.5 / scale,
                0.0,
                std::f64::consts::TAU,
            );
            let _ = context.fill();
            context.set_source_rgb(0.09, 0.09, 0.10);
            context.arc(split, middle, radius, 0.0, std::f64::consts::TAU);
            let _ = context.fill_preserve();
            context.set_source_rgba(1.0, 1.0, 1.0, 0.22);
            context.set_line_width(1.2 / scale);
            let _ = context.stroke();
            let arrow = 5.5 / scale;
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.set_line_width(2.0 / scale);
            context.set_line_cap(gtk4::cairo::LineCap::Round);
            context.set_line_join(gtk4::cairo::LineJoin::Round);
            context.move_to(split - arrow * 1.8, middle - arrow);
            context.line_to(split - arrow * 0.8, middle);
            context.line_to(split - arrow * 1.8, middle + arrow);
            let _ = context.stroke();
            context.move_to(split + arrow * 1.8, middle - arrow);
            context.line_to(split + arrow * 0.8, middle);
            context.line_to(split + arrow * 1.8, middle + arrow);
            let _ = context.stroke();
        }
    });

    // The divider is direct-manipulation: press anywhere on the preview to
    // put the seam there, then drag it. No separate slider below the canvas.
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let shown = shown.clone();
        let divider = divider.clone();
        let zoom = zoom.clone();
        move |gesture, x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let value = shown
                .borrow()
                .as_deref()
                .map(|comparison| divider_at(&area, comparison, zoom.get(), x));
            if let Some(value) = value {
                divider.set(value);
                area.queue_draw();
            }
        }
    });
    drag.connect_drag_update({
        let shown = shown.clone();
        let divider = divider.clone();
        let zoom = zoom.clone();
        move |gesture, offset_x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let value = shown
                .borrow()
                .as_deref()
                .map(|comparison| divider_at(&area, comparison, zoom.get(), start_x + offset_x));
            if let Some(value) = value {
                divider.set(value);
                area.queue_draw();
            }
        }
    });
    drawing.add_controller(drag);

    let hover = EventControllerMotion::new();
    hover.connect_motion({
        let shown = shown.clone();
        let divider = divider.clone();
        let zoom = zoom.clone();
        move |controller, x, _| {
            let Some(area) = controller.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let pointer = shown
                .borrow()
                .as_deref()
                .map(|comparison| divider_at(&area, comparison, zoom.get(), x));
            let near_divider =
                pointer.is_some_and(|pointer| (pointer - divider.get()).abs() <= 0.03);
            let cursor = if near_divider {
                gdk::Cursor::from_name("ew-resize", None)
            } else {
                None
            };
            area.set_cursor(cursor.as_ref());
        }
    });
    hover.connect_leave(move |controller| {
        if let Some(area) = controller.widget().and_downcast::<DrawingArea>() {
            area.set_cursor(None);
        }
    });
    drawing.add_controller(hover);

    for (index, tab) in zoom_tabs.iter().enumerate() {
        tab.connect_toggled({
            let zoom = zoom.clone();
            let shown = shown.clone();
            let drawing = drawing.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                let value = match index {
                    1 => Zoom::Actual,
                    2 => Zoom::Double,
                    _ => Zoom::Fit,
                };
                zoom.set(value);
                resize_canvas(&drawing, shown.borrow().as_deref(), value);
            }
        });
    }

    let present: Rc<dyn Fn(Rc<Comparison>)> = {
        let shown = shown.clone();
        let spinner = spinner.clone();
        let status = status.clone();
        let before_badge = before_badge.clone();
        let after_badge = after_badge.clone();
        let apply = apply.clone();
        let dims = dims.clone();
        let drawing = drawing.clone();
        let zoom = zoom.clone();
        Rc::new(move |comparison| {
            *shown.borrow_mut() = Some(comparison.clone());
            spinner.stop();
            spinner.set_visible(false);
            status.set_visible(false);
            before_badge.set_visible(true);
            after_badge.set_visible(true);
            apply.set_sensitive(true);
            dims.set_text(&tfmt(
                "Export preview: {width} × {height} pixels. Check text carefully; missing detail may not be recovered.",
                &[
                    ("width", &comparison.after.width().to_string()),
                    ("height", &comparison.after.height().to_string()),
                ],
            ));
            resize_canvas(&drawing, Some(&comparison), zoom.get());
            drawing.queue_draw();
        })
    };

    let request: Rc<dyn Fn()> = {
        let host = host.clone();
        let mode = mode.clone();
        let after_badge = after_badge.clone();
        let before_badge = before_badge.clone();
        let status = status.clone();
        let message = message.clone();
        let spinner = spinner.clone();
        let apply = apply.clone();
        let dims = dims.clone();
        let drawing = drawing.clone();
        let shown = shown.clone();
        let cache = cache.clone();
        let generation = generation.clone();
        let closed = closed.clone();
        let zoom = zoom.clone();
        let present = present.clone();
        Rc::new(move || {
            let selected = mode.get();
            let index = if selected == SourceUpscale::Neural4x {
                1
            } else {
                0
            };
            after_badge.set_text(&t(if index == 1 {
                "Neural 4×"
            } else {
                "Neural 2×"
            }));
            apply.set_label(&t(if index == 1 {
                "Use Neural 4×"
            } else {
                "Use Neural 2×"
            }));
            if let Some(comparison) = cache.borrow()[index].clone() {
                present(comparison);
                return;
            }
            *shown.borrow_mut() = None;
            before_badge.set_visible(false);
            after_badge.set_visible(false);
            apply.set_sensitive(false);
            dims.set_text("");
            status.set_visible(true);
            spinner.set_visible(true);
            spinner.start();
            message.set_text(&t("Preparing before / after preview…"));
            resize_canvas(&drawing, None, zoom.get());
            if let Some(reason) = host
                .read(|st| st.upscale_unavailable_reason(selected))
                .flatten()
            {
                spinner.stop();
                spinner.set_visible(false);
                message.set_text(&t(&reason));
                return;
            }
            let Some((base, actions)) = host.read(|st| st.export_source()) else {
                spinner.stop();
                spinner.set_visible(false);
                message.set_text(&t("Load an image before previewing upscaling."));
                return;
            };
            generation.set(generation.get() + 1);
            let id = generation.get();
            let (sender, receiver) = mpsc::channel::<Result<(RgbaImage, Arc<RgbaImage>), String>>();
            std::thread::spawn(move || {
                let source = EditorState::flatten_effects(&base, &actions);
                let before = upscale::standard_enlarge(&source, selected);
                let result =
                    upscale::enhance_cached(&source, selected).map(|after| (before, after));
                let _ = sender.send(result);
            });
            let spinner = spinner.clone();
            let message = message.clone();
            let cache = cache.clone();
            let generation = generation.clone();
            let closed = closed.clone();
            let present = present.clone();
            glib::timeout_add_local(Duration::from_millis(16), move || {
                match receiver.try_recv() {
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if closed.get() || generation.get() != id {
                            return glib::ControlFlow::Break;
                        }
                        spinner.stop();
                        spinner.set_visible(false);
                        message.set_text(&t("The upscaling preview worker stopped unexpectedly."));
                        glib::ControlFlow::Break
                    }
                    Ok(result) => {
                        if closed.get() || generation.get() != id {
                            return glib::ControlFlow::Break;
                        }
                        match result {
                            Ok((before, after)) => match (
                                render::rgba_image_to_surface(&before),
                                render::rgba_image_to_surface(&after),
                            ) {
                                (Some(before), Some(after)) => {
                                    let comparison = Rc::new(Comparison { before, after });
                                    cache.borrow_mut()[index] = Some(comparison.clone());
                                    present(comparison);
                                }
                                _ => {
                                    spinner.stop();
                                    spinner.set_visible(false);
                                    message.set_text(&t("Could not prepare the comparison image."));
                                }
                            },
                            Err(error) => {
                                spinner.stop();
                                spinner.set_visible(false);
                                message.set_text(&t(&error));
                            }
                        }
                        glib::ControlFlow::Break
                    }
                }
            });
        })
    };

    for (index, tab) in mode_tabs.iter().enumerate() {
        tab.connect_toggled({
            let request = request.clone();
            let mode = mode.clone();
            move |button| {
                if !button.is_active() {
                    return;
                }
                mode.set(if index == 1 {
                    SourceUpscale::Neural4x
                } else {
                    SourceUpscale::Neural2x
                });
                request();
            }
        });
    }

    let keys = EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let close: Rc<dyn Fn()> = {
        let scrim = scrim.downgrade();
        let overlay = overlay.downgrade();
        let window = window.downgrade();
        let keys = keys.downgrade();
        let closed = closed.clone();
        Rc::new(move || {
            closed.set(true);
            OPEN.with(|open| open.set(false));
            if let Some(scrim) = scrim.upgrade() {
                if let Some(overlay) = overlay.upgrade() {
                    overlay.remove_overlay(&scrim);
                }
            }
            if let Some(window) = window.upgrade() {
                if let Some(keys) = keys.upgrade() {
                    window.remove_controller(&keys);
                }
            }
        })
    };
    keys.connect_key_pressed({
        let close = close.clone();
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                close();
                return Propagation::Stop;
            }
            Propagation::Proceed
        }
    });
    window.add_controller(keys.clone());

    close_button.connect_clicked({
        let close = close.clone();
        move |_| close()
    });
    cancel.connect_clicked({
        let close = close.clone();
        move |_| close()
    });
    apply.connect_clicked({
        let close = close.clone();
        let mode = mode.clone();
        move |_| {
            on_apply(mode.get());
            close();
        }
    });

    overlay.add_overlay(&scrim);
    request();
}

/// A flat segmented tray of grouped tabs, first one active.
fn segmented(labels: &[&str]) -> (GtkBox, Vec<ToggleButton>) {
    let tray = GtkBox::new(Orientation::Horizontal, 0);
    tray.add_css_class("image-editor-bg-tabs");
    tray.add_css_class("image-editor-upscale-segmented");
    tray.set_homogeneous(true);
    let mut tabs: Vec<ToggleButton> = Vec::new();
    for label in labels {
        let tab = ToggleButton::with_label(&t(label));
        tab.add_css_class("image-editor-bg-tab");
        tab.set_has_frame(false);
        if let Some(first) = tabs.first() {
            tab.set_group(Some(first));
        }
        tray.append(&tab);
        tabs.push(tab);
    }
    if let Some(first) = tabs.first() {
        first.set_active(true);
    }
    (tray, tabs)
}

/// Display scale and top-left offset of the export inside the stage.
fn placement(area: &DrawingArea, comparison: &Comparison, zoom: Zoom) -> (f64, f64, f64) {
    let device = f64::from(area.scale_factor().max(1));
    let (width, height) = (
        f64::from(area.width().max(1)),
        f64::from(area.height().max(1)),
    );
    let (image_width, image_height) = (
        f64::from(comparison.after.width()),
        f64::from(comparison.after.height()),
    );
    let margin = 16.0;
    let scale = match zoom {
        Zoom::Fit => ((width - 2.0 * margin) / image_width)
            .min((height - 2.0 * margin) / image_height)
            .min(1.0 / device)
            .max(0.01),
        Zoom::Actual => 1.0 / device,
        Zoom::Double => 2.0 / device,
    };
    let x = ((width - image_width * scale) / 2.0).max(0.0);
    let y = ((height - image_height * scale) / 2.0).max(0.0);
    (scale, x, y)
}

fn resize_canvas(area: &DrawingArea, comparison: Option<&Comparison>, zoom: Zoom) {
    let device = f64::from(area.scale_factor().max(1));
    let scale = match zoom {
        Zoom::Fit => None,
        Zoom::Actual => Some(1.0 / device),
        Zoom::Double => Some(2.0 / device),
    };
    match (comparison, scale) {
        (Some(comparison), Some(scale)) => {
            area.set_content_width((f64::from(comparison.after.width()) * scale).ceil() as i32);
            area.set_content_height((f64::from(comparison.after.height()) * scale).ceil() as i32);
        }
        _ => {
            area.set_content_width(0);
            area.set_content_height(0);
        }
    }
    area.queue_draw();
}

/// The seam is a fraction of the rendered export, not of the whole canvas,
/// so the handle tracks the pointer whether the export is centred or wider
/// than the stage.
fn divider_at(area: &DrawingArea, comparison: &Comparison, zoom: Zoom, x: f64) -> f64 {
    let (scale, offset_x, _) = placement(area, comparison, zoom);
    ((x - offset_x) / (f64::from(comparison.after.width()) * scale)).clamp(0.0, 1.0)
}
