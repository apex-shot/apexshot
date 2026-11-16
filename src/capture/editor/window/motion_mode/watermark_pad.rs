use gtk4::cairo::Filter;
use gtk4::{gdk, prelude::*, DrawingArea, GestureClick, GestureDrag, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::pad_handle::draw_soft_handle;
use super::MotionRuntime;
use crate::capture::editor::window::motion_render::motion_watermark_rect;
use crate::i18n::t;

/// Placement pad for the Motion watermark.
///
/// Replaces the old Position X/Y sliders: the pad shows the card thumbnail
/// with the chosen mark drawn where it will land, so the mark is dragged into
/// place instead of nudged by two percentages. Coordinates are normalized to
/// the card, `(0.5, 0.5)` its centre, the same space the compositor reads.
///
/// The pad paints straight from the Motion runtime, so the thumbnail, the
/// mark, its size and its inset are always the current ones. It notifies on
/// a click or drag only; `queue_draw` is all a model change needs.
#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionWatermarkPad {
    area: DrawingArea,
    runtime: Rc<RefCell<MotionRuntime>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64, f64)>>>>,
    interaction_active: Rc<Cell<bool>>,
    interaction_ended: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl MotionWatermarkPad {
    pub(super) fn new(runtime: Rc<RefCell<MotionRuntime>>) -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, 152);
        area.set_hexpand(true);
        area.add_css_class("editor-motion-position-pad");
        area.add_css_class("editor-motion-watermark-pad");
        area.set_tooltip_text(Some(&t(
            "Click or drag to place the watermark; double-click to center it",
        )));

        let pad = Self {
            area: area.clone(),
            runtime,
            listeners: Rc::new(RefCell::new(Vec::new())),
            interaction_active: Rc::new(Cell::new(false)),
            interaction_ended: Rc::new(RefCell::new(Vec::new())),
        };
        area.set_draw_func({
            let pad = pad.clone();
            move |widget, context, width, height| pad.draw(widget, context, width, height)
        });

        let click = GestureClick::new();
        click.set_button(1);
        click.connect_pressed({
            let pad = pad.clone();
            move |_, n_press, x, y| {
                pad.interaction_active.set(true);
                if n_press >= 2 {
                    pad.notify(0.5, 0.5);
                } else {
                    pad.apply_point(x, y);
                }
            }
        });
        click.connect_released({
            let pad = pad.clone();
            move |_, _, _, _| pad.end_interaction()
        });
        area.add_controller(click);

        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let pad = pad.clone();
            move |_, _, _| pad.interaction_active.set(true)
        });
        drag.connect_drag_update({
            let pad = pad.clone();
            move |gesture, dx, dy| {
                let Some((start_x, start_y)) = gesture.start_point() else {
                    return;
                };
                pad.apply_point(start_x + dx, start_y + dy);
            }
        });
        drag.connect_drag_end({
            let pad = pad.clone();
            move |_, _, _| pad.end_interaction()
        });
        area.add_controller(drag);
        area.set_cursor(gdk::Cursor::from_name("crosshair", None).as_ref());
        pad
    }

    pub(super) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    pub(super) fn connect_value_changed(&self, listener: impl Fn(f64, f64) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(listener));
    }

    /// Called once a click or drag lets go, so the caller can repaint the
    /// sharp preview frame the drag skipped.
    pub(super) fn connect_interaction_ended(&self, listener: impl Fn() + 'static) {
        self.interaction_ended.borrow_mut().push(Rc::new(listener));
    }

    fn end_interaction(&self) {
        if !self.interaction_active.replace(false) {
            return;
        }
        for listener in self.interaction_ended.borrow().iter().cloned() {
            listener();
        }
    }

    fn notify(&self, x: f64, y: f64) {
        for listener in self.listeners.borrow().iter().cloned() {
            listener(x, y);
        }
        self.area.queue_draw();
    }

    fn card_size(&self) -> Option<(f64, f64)> {
        let runtime = self.runtime.try_borrow().ok()?;
        let card = runtime.card_preview.as_ref().or(runtime.card.as_ref())?;
        Some((
            f64::from(card.width().max(1)),
            f64::from(card.height().max(1)),
        ))
    }

    fn apply_point(&self, point_x: f64, point_y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let (card_w, card_h) = self.card_size().unwrap_or((width, height));
        let rect = card_rect_for(width, height, card_w, card_h);
        let (x, y) = point_to_position(point_x, point_y, rect);
        self.notify(x, y);
    }

    fn draw(&self, widget: &DrawingArea, context: &gtk4::cairo::Context, width: i32, height: i32) {
        let width = f64::from(width.max(1));
        let height = f64::from(height.max(1));
        let light = widget_is_light(widget);
        rounded_rect(context, 0.0, 0.0, width, height, 10.0);
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        context.fill().ok();

        let Ok(runtime) = self.runtime.try_borrow() else {
            return;
        };
        let watermark = runtime.motion.watermark.clone();
        let card = runtime
            .card_preview
            .clone()
            .or_else(|| runtime.card.clone());
        let mark = runtime
            .watermark_surface
            .clone()
            .filter(|_| watermark.image_file_name.is_some());
        drop(runtime);

        let (card_w, card_h) = card
            .as_ref()
            .map(|card| {
                (
                    f64::from(card.width().max(1)),
                    f64::from(card.height().max(1)),
                )
            })
            .unwrap_or((width, height));
        let (ix, iy, iw, ih) = card_rect_for(width, height, card_w, card_h);
        if iw <= 1.0 || ih <= 1.0 {
            return;
        }

        if let Some(card) = card.as_ref() {
            context.save().ok();
            rounded_rect(context, ix, iy, iw, ih, 8.0);
            context.clip();
            context.translate(ix, iy);
            context.scale(iw / card_w, ih / card_h);
            context.set_source_surface(card, 0.0, 0.0).ok();
            context.paint().ok();
            context.restore().ok();
        }

        // A rule-of-thirds overlay to aid placement, as on the anchor pad.
        context.save().ok();
        rounded_rect(context, ix, iy, iw, ih, 8.0);
        context.clip();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.08);
        context.rectangle(ix, iy, iw, ih);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 0.28);
        context.set_line_width(1.0);
        for third in 1..3 {
            let x = ix + iw * f64::from(third) / 3.0;
            context.move_to(x, iy);
            context.line_to(x, iy + ih);
            context.stroke().ok();
            let y = iy + ih * f64::from(third) / 3.0;
            context.move_to(ix, y);
            context.line_to(ix + iw, y);
            context.stroke().ok();
        }

        // The mark itself, where the compositor will put it on the card.
        if let Some(mark) = mark.as_ref() {
            let source_w = f64::from(mark.width().max(1));
            let source_h = f64::from(mark.height().max(1));
            let (x, y, w, h) = motion_watermark_rect(
                (ix, iy, iw, ih),
                watermark.position,
                watermark.size,
                watermark.inset,
                (source_w, source_h),
            );
            context.translate(x, y);
            context.scale(w / source_w, h / source_h);
            context.set_source_surface(mark, 0.0, 0.0).ok();
            context.source().set_filter(Filter::Bilinear);
            context.paint().ok();
        }
        context.restore().ok();

        let (px, py) = position_to_point(watermark.position, (ix, iy, iw, ih));
        draw_soft_handle(context, px, py);
    }
}

/// Aspect-fit rectangle for the card thumbnail inside the pad, with a small
/// inset so the handle never clips at the widget edge.
fn card_rect_for(alloc_w: f64, alloc_h: f64, card_w: f64, card_h: f64) -> (f64, f64, f64, f64) {
    const INSET: f64 = 6.0;
    let avail_w = (alloc_w - INSET * 2.0).max(1.0);
    let avail_h = (alloc_h - INSET * 2.0).max(1.0);
    let scale = (avail_w / card_w.max(1.0)).min(avail_h / card_h.max(1.0));
    let iw = card_w * scale;
    let ih = card_h * scale;
    ((alloc_w - iw) / 2.0, (alloc_h - ih) / 2.0, iw, ih)
}

/// Pointer position to the normalized card position, clamped to the card.
fn point_to_position(
    point_x: f64,
    point_y: f64,
    (ix, iy, iw, ih): (f64, f64, f64, f64),
) -> (f64, f64) {
    (
        ((point_x - ix) / iw.max(1.0)).clamp(0.0, 1.0),
        ((point_y - iy) / ih.max(1.0)).clamp(0.0, 1.0),
    )
}

fn position_to_point((x, y): (f64, f64), (ix, iy, iw, ih): (f64, f64, f64, f64)) -> (f64, f64) {
    (ix + x.clamp(0.0, 1.0) * iw, iy + y.clamp(0.0, 1.0) * ih)
}

fn widget_is_light(widget: &impl gtk4::glib::object::IsA<Widget>) -> bool {
    let mut current = Some(widget.clone().upcast::<Widget>());
    while let Some(node) = current {
        if node.has_css_class("editor-theme-light") {
            return true;
        }
        current = node.parent();
    }
    false
}

fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    crate::capture::editor::render::rounded_rect_path(context, x, y, width, height, radius);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_positions_round_trip_through_the_card_rect() {
        let rect = card_rect_for(288.0, 152.0, 1920.0, 1200.0);
        for value in [(0.0, 0.0), (0.5, 0.5), (0.9, 0.9), (1.0, 1.0)] {
            let (x, y) = position_to_point(value, rect);
            let back = point_to_position(x, y, rect);
            assert!((back.0 - value.0).abs() < 1e-9 && (back.1 - value.1).abs() < 1e-9);
        }
        // Clicks outside the card clamp to its edge.
        assert_eq!(point_to_position(-50.0, 999.0, rect), (0.0, 1.0));
    }

    #[test]
    fn the_pad_draws_the_mark_where_the_compositor_puts_it() {
        // The same placement at two scales differs only by that scale.
        let mark = (200.0, 100.0);
        let full = motion_watermark_rect((0.0, 0.0, 1920.0, 1200.0), (0.9, 0.9), 0.18, 0.04, mark);
        let rect = card_rect_for(288.0, 152.0, 1920.0, 1200.0);
        let thumb = motion_watermark_rect(rect, (0.9, 0.9), 0.18, 0.04, mark);
        let scale = rect.2 / 1920.0;
        assert!((thumb.0 - (rect.0 + full.0 * scale)).abs() < 1e-9);
        assert!((thumb.1 - (rect.1 + full.1 * scale)).abs() < 1e-9);
        assert!((thumb.2 - full.2 * scale).abs() < 1e-9);
    }
}
