use crate::recording::editor::model::VideoEditState;
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, AspectFrame, Box as GtkBox, Button, DrawingArea,
    EventControllerMotion, GestureClick, GestureDrag, Image, Label, MediaFile, Orientation,
    Overlay, Picture, Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const ACCENT: (f64, f64, f64) = (0.690, 0.361, 0.220);
const HANDLE_RADIUS: f64 = 28.0;
const EDGE_INSET: f64 = 40.0;
const HANDLE_DOT: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragOp {
    Move,
    Corner(u8), // 0 TL, 1 TR, 2 BR, 3 BL
    Edge(u8),   // 0 L, 1 R, 2 T, 3 B
    New,
}

impl DragOp {
    fn cursor_name(self) -> &'static str {
        match self {
            DragOp::Corner(0) => "nw-resize",
            DragOp::Corner(1) => "ne-resize",
            DragOp::Corner(2) => "se-resize",
            DragOp::Corner(3) => "sw-resize",
            DragOp::Corner(_) => "crosshair",
            DragOp::Edge(0) | DragOp::Edge(1) => "ew-resize",
            DragOp::Edge(2) | DragOp::Edge(3) => "ns-resize",
            DragOp::Edge(_) => "crosshair",
            DragOp::Move => "move",
            DragOp::New => "crosshair",
        }
    }
}

pub(super) fn show_crop(
    parent: &ApplicationWindow,
    state: &Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let (path, src_w, src_h, initial) = {
        let guard = state.lock().unwrap();
        let crop = guard.crop_or_full();
        (
            guard.metadata.path.clone(),
            guard.metadata.width.max(1) as f64,
            guard.metadata.height.max(1) as f64,
            crop,
        )
    };

    const DIALOG_W: i32 = 900;
    const CHROME_H: i32 = 136;
    let video_h = (DIALOG_W as f64 * src_h / src_w).round() as i32;

    let dialog = Window::builder()
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .default_width(DIALOG_W)
        .default_height(CHROME_H + video_h)
        .resizable(false)
        .build();
    dialog.add_css_class("recording-editor-dialog");
    dialog.add_css_class("recording-editor-crop-dialog");
    if !crate::capture::editor::ui_support::prefers_dark_glass_theme() {
        dialog.add_css_class("editor-theme-light");
    }

    let wrapper = GtkBox::new(Orientation::Vertical, 0);
    wrapper.add_css_class("recording-editor-dialog-bg");
    wrapper.set_overflow(gtk4::Overflow::Hidden);

    let close = Button::new();
    close.set_has_frame(false);
    close.set_can_focus(false);
    close.add_css_class("recording-editor-crop-close");
    close.set_tooltip_text(Some("Close"));
    close.set_valign(Align::Center);
    let close_icon = Image::from_icon_name("window-close-symbolic");
    close_icon.set_pixel_size(16);
    close.set_child(Some(&close_icon));
    let dialog_close = dialog.clone();
    close.connect_clicked(move |_| dialog_close.close());

    let title = Label::new(Some("Crop video"));
    title.add_css_class("recording-editor-crop-name");
    title.set_hexpand(true);
    title.set_halign(Align::Center);

    let header_pad = GtkBox::new(Orientation::Horizontal, 0);
    header_pad.set_size_request(32, 32);

    let header = GtkBox::new(Orientation::Horizontal, 0);
    header.add_css_class("recording-editor-crop-header");
    header.append(&header_pad);
    header.append(&title);
    header.append(&close);

    // Selection rect in source pixels.
    let selection: Rc<RefCell<(f64, f64, f64, f64)>> = Rc::new(RefCell::new(initial));

    let media = MediaFile::for_filename(path);
    media.set_muted(true);
    media.set_loop(true);
    let picture = Picture::for_paintable(&media);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_halign(Align::Fill);
    picture.set_valign(Align::Fill);
    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);

    let clip = GtkBox::new(Orientation::Vertical, 0);
    clip.add_css_class("recording-editor-crop-clip");
    clip.set_overflow(gtk4::Overflow::Hidden);
    clip.set_hexpand(true);
    clip.set_vexpand(true);
    clip.set_halign(Align::Fill);
    clip.set_valign(Align::Fill);
    clip.append(&picture);

    let overlay = Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_child(Some(&clip));

    // AspectFrame keeps the drawing surface glued to the letterboxed video,
    // so surface pixels map 1:1 onto source pixels.
    let frame = AspectFrame::new(0.5, 0.5, (src_w / src_h) as f32, false);
    frame.set_hexpand(true);
    frame.set_vexpand(false);
    frame.set_size_request(DIALOG_W, video_h);
    frame.set_child(Some(&overlay));

    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("recording-editor-crop-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(false);
    stage.set_size_request(DIALOG_W, video_h);
    stage.append(&frame);

    let size_label = Label::new(None);
    size_label.add_css_class("recording-editor-crop-size");
    size_label.set_xalign(0.0);

    let update_size_label = {
        let size_label = size_label.clone();
        let selection = selection.clone();
        Rc::new(move || {
            let (_, _, w, h) = *selection.borrow();
            size_label.set_text(&format!("{} × {}", w.round() as u32, h.round() as u32));
        }) as Rc<dyn Fn()>
    };
    update_size_label();

    let draw_area = DrawingArea::new();
    draw_area.set_hexpand(true);
    draw_area.set_vexpand(true);
    draw_area.set_can_target(true);
    draw_area.set_draw_func({
        let selection = selection.clone();
        move |_, cr, width, height| {
            let rect = *selection.borrow();
            draw_crop_overlay(cr, width as f64, height as f64, src_w, src_h, rect);
        }
    });
    overlay.add_overlay(&draw_area);

    let classify = {
        let src_w = src_w;
        let src_h = src_h;
        move |px: f64, py: f64, w: f64, h: f64, rect: (f64, f64, f64, f64)| -> DragOp {
            classify_crop_handle(px, py, w, h, rect, src_w, src_h)
        }
    };

    // Cursor feedback: change cursor when hovering the outline/handles/body.
    {
        let selection = selection.clone();
        let classify = classify.clone();
        let motion = EventControllerMotion::new();
        motion.connect_motion({
            let draw_area = draw_area.clone();
            move |_, px, py| {
                let w = draw_area.allocated_width().max(1) as f64;
                let h = draw_area.allocated_height().max(1) as f64;
                let op = classify(px, py, w, h, *selection.borrow());
                draw_area.set_cursor(gtk4::gdk::Cursor::from_name(op.cursor_name(), None).as_ref());
            }
        });
        motion.connect_leave({
            let draw_area = draw_area.clone();
            move |_| {
                draw_area.set_cursor(None);
            }
        });
        draw_area.add_controller(motion);
    }

    // ── Drag interaction ──
    let op: Rc<RefCell<Option<(DragOp, f64, f64, (f64, f64, f64, f64))>>> =
        Rc::new(RefCell::new(None));

    let press = GestureClick::new();
    press.set_button(1);
    press.connect_pressed({
        let selection = selection.clone();
        let op = op.clone();
        let classify = classify.clone();
        let draw_area = draw_area.clone();
        move |_, _, px, py| {
            let w = draw_area.allocated_width().max(1) as f64;
            let h = draw_area.allocated_height().max(1) as f64;
            let rect = *selection.borrow();
            let drag_op = classify(px, py, w, h, rect);
            *op.borrow_mut() = Some((drag_op, px, py, rect));
            if drag_op == DragOp::New {
                *selection.borrow_mut() =
                    (px / w.max(1.0) * src_w, py / h.max(1.0) * src_h, 2.0, 2.0);
            }
        }
    });

    let drag = GestureDrag::new();
    drag.set_button(1);
    press.group_with(&drag);
    drag.connect_drag_begin({
        let selection = selection.clone();
        let op = op.clone();
        let classify = classify.clone();
        let draw_area = draw_area.clone();
        move |_, px, py| {
            if op.borrow().is_some() {
                return;
            }
            let w = draw_area.allocated_width().max(1) as f64;
            let h = draw_area.allocated_height().max(1) as f64;
            let rect = *selection.borrow();
            let drag_op = classify(px, py, w, h, rect);
            *op.borrow_mut() = Some((drag_op, px, py, rect));
        }
    });
    drag.connect_drag_update({
        let selection = selection.clone();
        let op = op.clone();
        let draw_area = draw_area.clone();
        let update_size_label = update_size_label.clone();
        move |_, ox, oy| {
            let w = draw_area.allocated_width().max(1) as f64;
            let h = draw_area.allocated_height().max(1) as f64;
            let Some((drag_op, start_px, start_py, orig)) = *op.borrow() else {
                return;
            };
            let cur_x = (start_px + ox) / w.max(1.0) * src_w;
            let cur_y = (start_py + oy) / h.max(1.0) * src_h;
            let start_x = start_px / w.max(1.0) * src_w;
            let start_y = start_py / h.max(1.0) * src_h;
            let min_w = 24.0 / w.max(1.0) * src_w;
            let min_h = 24.0 / h.max(1.0) * src_h;
            let mut rect = *selection.borrow();
            match drag_op {
                DragOp::Move => {
                    rect.0 = (orig.0 + cur_x - start_x).clamp(0.0, src_w - rect.2);
                    rect.1 = (orig.1 + cur_y - start_y).clamp(0.0, src_h - rect.3);
                }
                DragOp::New => {
                    rect.0 = cur_x.min(start_x);
                    rect.1 = cur_y.min(start_y);
                    rect.2 = (cur_x - start_x).abs().max(min_w).min(src_w - rect.0);
                    rect.3 = (cur_y - start_y).abs().max(min_h).min(src_h - rect.1);
                }
                DragOp::Corner(c) => {
                    let (mut x0, mut y0, mut x1, mut y1) =
                        (orig.0, orig.1, orig.0 + orig.2, orig.1 + orig.3);
                    match c {
                        0 => {
                            x0 = cur_x.clamp(0.0, x1 - min_w);
                            y0 = cur_y.clamp(0.0, y1 - min_h);
                        }
                        1 => {
                            x1 = cur_x.clamp(x0 + min_w, src_w);
                            y0 = cur_y.clamp(0.0, y1 - min_h);
                        }
                        2 => {
                            x1 = cur_x.clamp(x0 + min_w, src_w);
                            y1 = cur_y.clamp(y0 + min_h, src_h);
                        }
                        _ => {
                            x0 = cur_x.clamp(0.0, x1 - min_w);
                            y1 = cur_y.clamp(y0 + min_h, src_h);
                        }
                    }
                    rect = (x0, y0, x1 - x0, y1 - y0);
                }
                DragOp::Edge(e) => {
                    let (mut x0, mut y0, mut x1, mut y1) =
                        (orig.0, orig.1, orig.0 + orig.2, orig.1 + orig.3);
                    match e {
                        0 => x0 = cur_x.clamp(0.0, x1 - min_w),
                        1 => x1 = cur_x.clamp(x0 + min_w, src_w),
                        2 => y0 = cur_y.clamp(0.0, y1 - min_h),
                        _ => y1 = cur_y.clamp(y0 + min_h, src_h),
                    }
                    rect = (x0, y0, x1 - x0, y1 - y0);
                }
            }
            *selection.borrow_mut() = rect;
            update_size_label();
            draw_area.queue_draw();
        }
    });
    drag.connect_drag_end({
        let op = op.clone();
        move |_, _, _| {
            *op.borrow_mut() = None;
        }
    });
    draw_area.add_controller(press);
    draw_area.add_controller(drag);

    let reset = Button::with_label("Reset");
    reset.set_has_frame(false);
    reset.set_can_focus(false);
    reset.add_css_class("recording-editor-secondary-button");

    let done = Button::with_label("Done");
    done.set_has_frame(false);
    done.set_can_focus(false);
    done.add_css_class("recording-editor-primary-button");

    reset.connect_clicked({
        let selection = selection.clone();
        let update_size_label = update_size_label.clone();
        let draw_area = draw_area.clone();
        move |_| {
            *selection.borrow_mut() = (0.0, 0.0, src_w, src_h);
            update_size_label();
            draw_area.queue_draw();
        }
    });

    done.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        let selection = selection.clone();
        let dialog = dialog.clone();
        move |_| {
            {
                let mut guard = state.lock().unwrap();
                let (x, y, w, h) = *selection.borrow();
                guard.set_crop(x, y, w, h);
            }
            on_change();
            dialog.close();
        }
    });

    size_label.set_halign(Align::Center);
    size_label.set_valign(Align::Center);
    size_label.set_hexpand(true);

    reset.set_halign(Align::Start);
    reset.set_valign(Align::Center);
    done.set_halign(Align::End);
    done.set_valign(Align::Center);

    let footer = GtkBox::new(Orientation::Horizontal, 0);
    footer.add_css_class("recording-editor-crop-footer");
    footer.append(&reset);
    footer.append(&size_label);
    footer.append(&done);

    wrapper.append(&header);
    wrapper.append(&stage);
    wrapper.append(&footer);
    dialog.set_child(Some(&wrapper));

    let media_close = media.clone();
    dialog.connect_close_request(move |_| {
        media_close.pause();
        glib::Propagation::Proceed
    });

    let escape = gtk4::EventControllerKey::new();
    escape.connect_key_pressed({
        let dialog = dialog.clone();
        move |_, keyval, _, _| {
            if keyval == gtk4::gdk::Key::Escape {
                dialog.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    dialog.add_controller(escape);

    dialog.present();
    media.play();
}

fn draw_crop_overlay(
    cr: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    src_w: f64,
    src_h: f64,
    rect: (f64, f64, f64, f64),
) {
    let x = rect.0 / src_w * width;
    let y = rect.1 / src_h * height;
    let w = rect.2 / src_w * width;
    let h = rect.3 / src_h * height;

    // Dim everything outside the selection.
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    cr.rectangle(0.0, 0.0, width, y);
    let _ = cr.fill();
    cr.rectangle(0.0, y + h, width, height - y - h);
    let _ = cr.fill();
    cr.rectangle(0.0, y, x, h);
    let _ = cr.fill();
    cr.rectangle(x + w, y, width - x - w, h);
    let _ = cr.fill();

    cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    cr.set_line_width(2.0);
    cr.rectangle(x, y, w, h);
    let _ = cr.stroke();

    let cx = |px: f64| px.clamp(HANDLE_DOT, width - HANDLE_DOT);
    let cy = |py: f64| py.clamp(HANDLE_DOT, height - HANDLE_DOT);
    for (hx, hy) in [
        (x, y),
        (x + w, y),
        (x + w, y + h),
        (x, y + h),
        (x, y + h / 2.0),
        (x + w, y + h / 2.0),
        (x + w / 2.0, y),
        (x + w / 2.0, y + h),
    ] {
        draw_handle(cr, cx(hx), cy(hy));
    }
}

fn draw_handle(cr: &gtk4::cairo::Context, x: f64, y: f64) {
    cr.rectangle(
        x - HANDLE_DOT,
        y - HANDLE_DOT,
        HANDLE_DOT * 2.0,
        HANDLE_DOT * 2.0,
    );
    cr.set_source_rgb(1.0, 1.0, 1.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(ACCENT.0, ACCENT.1, ACCENT.2);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
}

fn edge_hit(point: f64, line: f64, widget: f64, inward_positive: bool) -> bool {
    let on_edge = line <= HANDLE_RADIUS || line >= widget - HANDLE_RADIUS;
    let inward = if on_edge { EDGE_INSET } else { HANDLE_RADIUS };
    if inward_positive {
        point >= line - HANDLE_RADIUS && point <= line + inward
    } else {
        point >= line - inward && point <= line + HANDLE_RADIUS
    }
}

fn classify_crop_handle(
    px: f64,
    py: f64,
    w: f64,
    h: f64,
    rect: (f64, f64, f64, f64),
    src_w: f64,
    src_h: f64,
) -> DragOp {
    let (sx, sy, sw, sh) = (
        rect.0 / src_w * w,
        rect.1 / src_h * h,
        rect.2 / src_w * w,
        rect.3 / src_h * h,
    );
    let near_x = px >= sx - HANDLE_RADIUS && px <= sx + sw + HANDLE_RADIUS;
    let near_y = py >= sy - HANDLE_RADIUS && py <= sy + sh + HANDLE_RADIUS;
    let left = edge_hit(px, sx, w, true);
    let right = edge_hit(px, sx + sw, w, false);
    let top = edge_hit(py, sy, h, true);
    let bottom = edge_hit(py, sy + sh, h, false);
    if near_x && near_y && top && left {
        DragOp::Corner(0)
    } else if near_x && near_y && top && right {
        DragOp::Corner(1)
    } else if near_x && near_y && bottom && right {
        DragOp::Corner(2)
    } else if near_x && near_y && bottom && left {
        DragOp::Corner(3)
    } else if near_x && left {
        DragOp::Edge(0)
    } else if near_x && right {
        DragOp::Edge(1)
    } else if near_y && top {
        DragOp::Edge(2)
    } else if near_y && bottom {
        DragOp::Edge(3)
    } else if px > sx && px < sx + sw && py > sy && py < sy + sh {
        DragOp::Move
    } else {
        DragOp::New
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_frame_edge_is_easy_to_grab() {
        let rect = (0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(
            classify_crop_handle(24.0, 250.0, 900.0, 506.0, rect, 1920.0, 1080.0),
            DragOp::Edge(0)
        );
        assert_eq!(
            classify_crop_handle(10.0, 10.0, 900.0, 506.0, rect, 1920.0, 1080.0),
            DragOp::Corner(0)
        );
        assert_eq!(
            classify_crop_handle(200.0, 250.0, 900.0, 506.0, rect, 1920.0, 1080.0),
            DragOp::Move
        );
    }
}
