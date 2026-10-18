use gtk4::cairo::{Context, Filter, Format, ImageSurface};
use gtk4::{
    gdk, gio, glib, prelude::*, Align, Box as GtkBox, Button, DrawingArea, DropDown,
    EventControllerMotion, GestureDrag, Image, Label, Orientation, Overflow, Overlay, PolicyType,
    ScrolledWindow, Spinner, Window,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::super::motion_render;
use super::MotionSession;
use crate::capture::editor::{render, state::EditorState, upscale::SourceUpscale};
use crate::i18n::{t, tfmt};
use crate::recording::editor::model::{MotionBackgroundFillType, MotionState};

struct Comparison {
    before: ImageSurface,
    after: ImageSurface,
}

pub(super) fn show(parent: &gtk4::ApplicationWindow, session: &MotionSession, motion_export: bool) {
    let window = Window::builder()
        .title(t("Compare neural upscaling"))
        .transient_for(parent)
        .modal(true)
        .default_width(1040)
        .default_height(760)
        .build();
    window.add_css_class("editor-upscale-window");
    if session.prefers_dark {
        window.add_css_class("editor-theme-dark");
    } else {
        window.add_css_class("editor-theme-light");
    }

    let titlebar = GtkBox::new(Orientation::Horizontal, 16);
    titlebar.add_css_class("editor-upscale-titlebar");
    titlebar.set_hexpand(true);
    let title_block = GtkBox::new(Orientation::Vertical, 2);
    title_block.set_hexpand(true);
    title_block.set_valign(Align::Center);
    let title = Label::new(Some(&t("Compare neural upscaling")));
    title.add_css_class("editor-upscale-title");
    title.set_xalign(0.0);
    let subtitle = Label::new(Some(&t("Compare before / after")));
    subtitle.add_css_class("editor-upscale-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_block.append(&title);
    title_block.append(&subtitle);

    let close_icon_button = Button::new();
    close_icon_button.add_css_class("editor-upscale-close");
    close_icon_button.set_has_frame(false);
    close_icon_button.set_can_focus(false);
    close_icon_button.set_valign(Align::Center);
    close_icon_button.set_tooltip_text(Some(&t("Close")));
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(16);
    close_icon_button.set_child(Some(&close_icon));

    titlebar.append(&title_block);
    titlebar.append(&close_icon_button);
    window.set_titlebar(Some(&titlebar));
    install_titlebar_drag(&title_block, &window);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("editor-root");
    body.add_css_class("editor-upscale-body");
    if session.prefers_dark {
        body.add_css_class("editor-theme-dark");
    } else {
        body.add_css_class("editor-theme-light");
    }

    let note = GtkBox::new(Orientation::Horizontal, 10);
    note.add_css_class("editor-upscale-note");
    let note_icon = Image::from_icon_name("dialog-information-symbolic");
    note_icon.set_pixel_size(16);
    note_icon.set_valign(Align::Start);
    let note_text = Label::new(Some(&t("Standard enlargement on the left, neural enhancement on the right. Move the divider and scroll to inspect rendered export pixels before file compression. This preview is free; enhanced exports require Studio.")));
    note_text.set_wrap(true);
    note_text.set_xalign(0.0);
    note_text.set_hexpand(true);
    note.append(&note_icon);
    note.append(&note_text);
    body.append(&note);

    let mode = DropDown::from_strings(&[&t("Neural 2×"), &t("Neural 4×")]);
    mode.set_selected(u32::from(
        session
            .runtime
            .borrow()
            .motion
            .appearance
            .source_upscale
            .factor()
            == 4,
    ));
    mode.add_css_class("editor-upscale-dropdown");
    let zoom = DropDown::from_strings(&["100%", "200%"]);
    zoom.add_css_class("editor-upscale-dropdown");
    zoom.set_tooltip_text(Some(&t(
        "100% displays export pixels at native screen scale",
    )));
    let controls = GtkBox::new(Orientation::Horizontal, 18);
    controls.add_css_class("editor-upscale-controls");
    controls.append(&labeled_control(&t("Neural upscaling"), &mode));
    controls.append(&labeled_control(&t("Zoom"), &zoom));
    body.append(&controls);

    let images: Rc<RefCell<Option<Comparison>>> = Rc::new(RefCell::new(None));
    let divider = Rc::new(Cell::new(0.5));
    let zoom_factor = Rc::new(Cell::new(1.0));
    let drawing = DrawingArea::new();
    drawing.set_content_width(640);
    drawing.set_content_height(400);
    drawing.set_hexpand(true);
    drawing.set_vexpand(true);
    drawing.set_draw_func({
        let images = images.clone();
        let divider = divider.clone();
        let zoom_factor = zoom_factor.clone();
        move |area, context, width, height| {
            render::draw_canvas_checkerboard_background(context, width, height, None, false);
            let images = images.borrow();
            let Some(images) = images.as_ref() else {
                return;
            };
            let scale = zoom_factor.get() / f64::from(area.scale_factor());
            let image_width = f64::from(images.after.width()) * scale;
            let image_height = f64::from(images.after.height()) * scale;
            let offset_x = ((f64::from(width) - image_width) / 2.0).max(0.0);
            let offset_y = ((f64::from(height) - image_height) / 2.0).max(0.0);
            context.translate(offset_x, offset_y);
            context.scale(scale, scale);
            let _ = context.set_source_surface(&images.before, 0.0, 0.0);
            context.source().set_filter(Filter::Nearest);
            let _ = context.paint();
            let split = divider.get() * f64::from(images.after.width());
            let _ = context.save();
            context.rectangle(
                split,
                0.0,
                f64::from(images.after.width()) - split,
                f64::from(images.after.height()),
            );
            context.clip();
            let _ = context.set_source_surface(&images.after, 0.0, 0.0);
            context.source().set_filter(Filter::Nearest);
            let _ = context.paint();
            let _ = context.restore();
            context.set_source_rgba(0.0, 0.0, 0.0, 0.28);
            context.set_line_width(3.0 / scale);
            context.move_to(split, 0.0);
            context.line_to(split, f64::from(images.after.height()));
            let _ = context.stroke();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.set_line_width(1.5 / scale);
            context.move_to(split, 0.0);
            context.line_to(split, f64::from(images.after.height()));
            let _ = context.stroke();

            // A grab handle makes the divider read as a control rather than
            // a seam between two renders. It is drawn this way so the drag
            // target is obvious before the pointer touches it.
            let middle = f64::from(images.after.height()) / 2.0;
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
    let scroll = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .child(&drawing)
        .build();
    scroll.add_css_class("editor-upscale-scroll");
    drawing.set_tooltip_text(Some(&t("Before / after divider")));

    // The divider is direct-manipulation: press anywhere on the preview to
    // put the seam there, then drag it. No separate slider below the canvas.
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let divider = divider.clone();
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        move |gesture, x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            divider.set(divider_at_pointer(
                &area,
                &images.borrow(),
                zoom_factor.get(),
                x,
            ));
            area.queue_draw();
        }
    });
    drag.connect_drag_update({
        let divider = divider.clone();
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        move |gesture, offset_x, _| {
            let Some(area) = gesture.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            divider.set(divider_at_pointer(
                &area,
                &images.borrow(),
                zoom_factor.get(),
                start_x + offset_x,
            ));
            area.queue_draw();
        }
    });
    drawing.add_controller(drag);

    let hover = EventControllerMotion::new();
    hover.connect_motion({
        let divider = divider.clone();
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        move |controller, x, _| {
            let Some(area) = controller.widget().and_downcast::<DrawingArea>() else {
                return;
            };
            let pointer = divider_at_pointer(&area, &images.borrow(), zoom_factor.get(), x);
            let near_divider = (pointer - divider.get()).abs() <= 0.03;
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

    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("editor-upscale-stage");
    stage.set_overflow(Overflow::Hidden);
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    let stage_overlay = Overlay::new();
    stage_overlay.set_hexpand(true);
    stage_overlay.set_vexpand(true);
    stage_overlay.set_child(Some(&scroll));
    let before_badge = stage_badge(&t("Original"));
    before_badge.set_halign(Align::Start);
    before_badge.set_valign(Align::Start);
    let after_badge = stage_badge(&mode_label(selected_mode(&mode)));
    after_badge.set_halign(Align::End);
    after_badge.set_valign(Align::Start);
    stage_overlay.add_overlay(&before_badge);
    stage_overlay.add_overlay(&after_badge);
    stage.append(&stage_overlay);
    body.append(&stage);

    let spinner = Spinner::new();
    spinner.set_visible(false);
    let status = Label::new(None);
    status.add_css_class("editor-upscale-status");
    status.set_wrap(true);
    status.set_xalign(0.0);
    status.set_hexpand(true);
    let status_row = GtkBox::new(Orientation::Horizontal, 8);
    status_row.add_css_class("editor-upscale-status-row");
    status_row.append(&spinner);
    status_row.append(&status);
    body.append(&status_row);

    zoom.connect_selected_notify({
        let drawing = drawing.clone();
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        move |dropdown| {
            let scale = if dropdown.selected() == 1 { 2.0 } else { 1.0 };
            zoom_factor.set(scale);
            resize_canvas(&drawing, &images.borrow(), scale);
        }
    });
    drawing.connect_scale_factor_notify({
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        move |area| resize_canvas(area, &images.borrow(), zoom_factor.get())
    });

    let footer = GtkBox::new(Orientation::Horizontal, 10);
    footer.add_css_class("editor-upscale-footer");
    footer.set_halign(Align::Fill);
    let footer_spacer = GtkBox::new(Orientation::Horizontal, 0);
    footer_spacer.set_hexpand(true);
    let close_window: Rc<dyn Fn()> = Rc::new({
        let window = window.downgrade();
        move || {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        }
    });
    let close = Button::with_label(&t("Close"));
    close.set_has_frame(false);
    close.add_css_class("editor-sidebar-action-button");
    close.add_css_class("editor-upscale-secondary");
    close.connect_clicked({
        let close_window = close_window.clone();
        move |_| close_window()
    });
    close_icon_button.connect_clicked(move |_| close_window());
    let apply = Button::with_label(&t("Use enhancement"));
    apply.set_has_frame(false);
    apply.add_css_class("editor-done-button");
    apply.add_css_class("editor-upscale-primary");
    apply.set_sensitive(false);
    apply.connect_clicked({
        let runtime = session.runtime.clone();
        let source_state = session.source_state.clone();
        let mode = mode.clone();
        let window = window.downgrade();
        move |_| {
            let selected = selected_mode(&mode);
            let mut runtime = runtime.borrow_mut();
            if runtime.motion.appearance.source_upscale != selected {
                runtime.begin_motion_edit();
                runtime.motion.appearance.source_upscale = selected;
            }
            drop(runtime);
            if let Some(state) = source_state.borrow().as_ref() {
                state.lock().unwrap().source_upscale = selected;
            }
            if let Some(window) = window.upgrade() {
                window.close();
            }
        }
    });
    footer.append(&close);
    footer.append(&footer_spacer);
    footer.append(&apply);
    body.append(&footer);

    let active: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));
    window.connect_close_request({
        let active = active.clone();
        move |_| {
            if let Some(cancel) = active.borrow().as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
            glib::Propagation::Proceed
        }
    });
    let request: Rc<dyn Fn()> = Rc::new({
        let session = session.clone();
        let window = window.downgrade();
        let mode = mode.clone();
        let after_badge = after_badge.clone();
        let spinner = spinner.clone();
        let status = status.clone();
        let apply = apply.clone();
        let drawing = drawing.clone();
        let images = images.clone();
        let zoom_factor = zoom_factor.clone();
        let active = active.clone();
        move || {
            let selected = selected_mode(&mode);
            after_badge.set_text(&mode_label(selected));
            let Some(source) = session.source_snapshot() else {
                status.set_text(&t("Load an image before previewing upscaling."));
                return;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            if let Some(previous) = active.borrow_mut().replace(Arc::clone(&cancel)) {
                previous.store(true, Ordering::Relaxed);
            }
            let motion = motion_export.then(|| session.motion_state());
            let prefers_dark = session.prefers_dark;
            mode.set_sensitive(false);
            apply.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            status.set_text(&t("Preparing before / after preview…"));
            *images.borrow_mut() = None;
            drawing.queue_draw();
            let worker = {
                let cancel = Arc::clone(&cancel);
                gio::spawn_blocking(move || {
                    render_comparison(source(), selected, motion.as_ref(), prefers_dark, &cancel)
                })
            };
            let window = window.clone();
            let mode = mode.clone();
            let spinner = spinner.clone();
            let status = status.clone();
            let apply = apply.clone();
            let drawing = drawing.clone();
            let images = images.clone();
            let zoom_factor = zoom_factor.clone();
            glib::MainContext::default().spawn_local(async move {
                let result = worker.await.unwrap_or_else(|_| Err(t("The upscaling preview worker stopped unexpectedly.")));
                if cancel.load(Ordering::Relaxed) || window.upgrade().is_none() { return; }
                mode.set_sensitive(true);
                spinner.stop();
                spinner.set_visible(false);
                let result = result.and_then(|(before, after)| {
                    let before = render::rgba_image_to_surface(&before).ok_or_else(|| t("Could not prepare the comparison image."))?;
                    let after = render::rgba_image_to_surface(&after).ok_or_else(|| t("Could not prepare the comparison image."))?;
                    Ok(Comparison { before, after })
                });
                match result {
                    Ok(comparison) => {
                        let (width, height) = (comparison.after.width(), comparison.after.height());
                        status.set_text(&tfmt("Export preview: {width} × {height} pixels. Check text carefully; missing detail may not be recovered.", &[("width", &width.to_string()), ("height", &height.to_string())]));
                        *images.borrow_mut() = Some(comparison);
                        resize_canvas(&drawing, &images.borrow(), zoom_factor.get());
                        apply.set_sensitive(true);
                        drawing.queue_draw();
                    }
                    Err(error) => status.set_text(&error),
                }
            });
        }
    });
    mode.connect_selected_notify({
        let request = Rc::downgrade(&request);
        move |_| {
            if let Some(request) = request.upgrade() {
                request();
            }
        }
    });
    window.connect_map(move |_| request());
    window.set_child(Some(&body));
    window.present();
}

/// The seam is a fraction of the rendered export, not of the whole canvas.
/// Mapping against the allocated width made the handle jump left whenever the
/// drawing area was wider than a short export.
fn divider_at_pointer(area: &DrawingArea, images: &Option<Comparison>, zoom: f64, x: f64) -> f64 {
    let allocated_width = f64::from(area.allocated_width().max(1));
    let (display_width, offset_x) = images
        .as_ref()
        .map(|images| {
            let display_width =
                f64::from(images.after.width()) * zoom / f64::from(area.scale_factor());
            let offset_x = ((allocated_width - display_width) / 2.0).max(0.0);
            (display_width, offset_x)
        })
        .filter(|(width, _)| *width > 0.0)
        .unwrap_or((allocated_width, 0.0));
    ((x - offset_x) / display_width).clamp(0.0, 1.0)
}

fn labeled_control(title: &str, control: &impl IsA<gtk4::Widget>) -> GtkBox {
    let field = GtkBox::new(Orientation::Vertical, 6);
    let label = Label::new(Some(title));
    label.add_css_class("editor-upscale-field-label");
    label.set_xalign(0.0);
    field.append(&label);
    field.append(control);
    field
}

fn stage_badge(text: &str) -> Label {
    let badge = Label::new(Some(text));
    badge.add_css_class("editor-upscale-stage-badge");
    badge.set_margin_top(10);
    badge.set_margin_start(10);
    badge.set_margin_end(10);
    badge
}

fn mode_label(mode: SourceUpscale) -> String {
    match mode {
        SourceUpscale::Neural4x => t("Neural 4×"),
        _ => t("Neural 2×"),
    }
}

fn install_titlebar_drag(zone: &GtkBox, window: &Window) {
    let drag = gtk4::GestureClick::new();
    drag.set_button(1);
    let window = window.downgrade();
    drag.connect_pressed(move |gesture, _, _, _| {
        let Some(window) = window.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };
        let Some((x, y)) = event.position() else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    zone.add_controller(drag);
}

fn selected_mode(dropdown: &DropDown) -> SourceUpscale {
    if dropdown.selected() == 1 {
        SourceUpscale::Neural4x
    } else {
        SourceUpscale::Neural2x
    }
}

fn resize_canvas(area: &DrawingArea, images: &Option<Comparison>, zoom: f64) {
    if let Some(images) = images.as_ref() {
        let scale = zoom / f64::from(area.scale_factor());
        area.set_content_width((f64::from(images.after.width()) * scale).ceil() as i32);
        area.set_content_height((f64::from(images.after.height()) * scale).ceil() as i32);
    }
    area.queue_draw();
}

const MAX_MOTION_COMPARISON_PIXELS: i64 = 8_388_608;
const MAX_MOTION_COMPARISON_EDGE: i32 = 7_680;

fn checkpoint(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("The upscaling preview was cancelled.".into());
    }
    Ok(())
}

fn checked_motion_comparison_size(width: i32, height: i32) -> Result<(i32, i32), String> {
    if width <= 0 || height <= 0 {
        return Err("The Motion comparison frame has no pixels to render.".into());
    }
    if width.max(height) > MAX_MOTION_COMPARISON_EDGE
        || i64::from(width) * i64::from(height) > MAX_MOTION_COMPARISON_PIXELS
    {
        return Err(
            "The Motion comparison frame is too large to preview. Reduce the Frame size.".into(),
        );
    }
    Ok((width, height))
}

fn render_comparison(
    mut state: EditorState,
    mode: SourceUpscale,
    motion: Option<&MotionState>,
    prefers_dark: bool,
    cancel: &AtomicBool,
) -> Result<(RgbaImage, RgbaImage), String> {
    checkpoint(cancel)?;
    let frame = motion
        .map(|motion| {
            let (width, height) = motion_render::motion_frame_output_size(
                f64::from(state.working_image.width()),
                f64::from(state.working_image.height()),
                motion,
            );
            checked_motion_comparison_size(width, height).map(|size| (motion, size))
        })
        .transpose()?;
    state.source_upscale = mode;
    let before = state
        .upscaled_export_state(false)
        .map_err(|error| error.to_string())?;
    checkpoint(cancel)?;
    let after = state
        .upscaled_export_state(true)
        .map_err(|error| error.to_string())?;
    checkpoint(cancel)?;
    let Some((motion, (width, height))) = frame else {
        return Ok((
            before.to_final_image().map_err(|error| error.to_string())?,
            after.to_final_image().map_err(|error| error.to_string())?,
        ));
    };
    let background_path = match motion.appearance.background_fill_type {
        MotionBackgroundFillType::Wallpaper => motion.appearance.wallpaper_image_name.as_deref(),
        MotionBackgroundFillType::Image => motion.appearance.custom_background_image.as_deref(),
        _ => None,
    };
    let background = background_path.and_then(motion_render::load_motion_background_surface);
    let watermark = motion
        .watermark
        .image_file_name
        .as_deref()
        .and_then(motion_render::load_motion_background_surface);
    let render_card = |state: EditorState| -> Result<RgbaImage, String> {
        let card = state
            .to_motion_card_image()
            .map_err(|error| error.to_string())?;
        let card = render::rgba_image_to_surface(&card)
            .ok_or_else(|| t("Could not prepare the comparison image."))?;
        let mut surface = ImageSurface::create(Format::ARgb32, width, height)
            .map_err(|error| error.to_string())?;
        {
            let context = Context::new(&surface).map_err(|error| error.to_string())?;
            motion_render::draw_motion_output_frame(
                &context,
                width,
                height,
                &card,
                motion,
                background.as_ref(),
                watermark.as_ref(),
                motion.playhead,
                prefers_dark,
                f64::from(mode.factor()),
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().map_err(|error| error.to_string())?;
        Ok(render::cairo_argb_to_rgba_image(
            width as u32,
            height as u32,
            stride,
            &data,
        ))
    };
    Ok((render_card(before)?, render_card(after)?))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use image::RgbaImage;

    use super::{
        checked_motion_comparison_size, checkpoint, render_comparison, MAX_MOTION_COMPARISON_EDGE,
        MAX_MOTION_COMPARISON_PIXELS,
    };
    use crate::capture::editor::{state::EditorState, upscale::SourceUpscale};

    #[test]
    fn motion_comparison_accepts_the_pixel_budget_boundary_without_allocating() {
        assert_eq!(checked_motion_comparison_size(2048, 4096), Ok((2048, 4096)));
        assert!(checked_motion_comparison_size(2049, 4096).is_err());
        assert_eq!(
            i64::from(2048) * i64::from(4096),
            MAX_MOTION_COMPARISON_PIXELS
        );
    }

    #[test]
    fn motion_comparison_accepts_the_edge_budget_boundary() {
        assert_eq!(
            checked_motion_comparison_size(MAX_MOTION_COMPARISON_EDGE, 1),
            Ok((MAX_MOTION_COMPARISON_EDGE, 1))
        );
        assert!(checked_motion_comparison_size(MAX_MOTION_COMPARISON_EDGE + 1, 1).is_err());
        assert!(checked_motion_comparison_size(1, MAX_MOTION_COMPARISON_EDGE + 1).is_err());
    }

    #[test]
    fn motion_comparison_rejects_empty_and_negative_frames() {
        assert!(checked_motion_comparison_size(0, 1080).is_err());
        assert!(checked_motion_comparison_size(1920, 0).is_err());
        assert!(checked_motion_comparison_size(-2, 1080).is_err());
    }

    #[test]
    fn cancelled_previews_stop_before_any_work() {
        assert!(checkpoint(&AtomicBool::new(false)).is_ok());
        assert!(checkpoint(&AtomicBool::new(true)).is_err());
        let state = EditorState::new(RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])));
        let result = render_comparison(
            state,
            SourceUpscale::Neural2x,
            None,
            false,
            &AtomicBool::new(true),
        );
        assert_eq!(
            result.err().as_deref(),
            Some("The upscaling preview was cancelled.")
        );
    }
}
