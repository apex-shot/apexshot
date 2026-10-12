use gtk4::cairo::ImageSurface;
use gtk4::{gdk, prelude::*, DrawingArea, GestureClick, GestureDrag, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::pad_handle::draw_soft_handle;
use crate::i18n::t;

/// Visual 2D control for a Motion segment's zoom anchor.
///
/// Replaces the old Anchor X/Y sliders: the actual screenshot thumbnail is
/// painted into the pad (aspect-fit) with a draggable puck, so users can see
/// where the zoom holds instead of guessing at percentages.
///
/// Normalized `0.0-1.0` coordinates, `(0.5, 0.5)` = image center. The pad is a
/// view/controller only: `set_anchor` / `set_surface` synchronize without
/// emitting, avoiding feedback loops with the timeline selection.
#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionAnchorPad {
    area: DrawingArea,
    anchor: Rc<Cell<(f64, f64)>>,
    surface: Rc<RefCell<Option<ImageSurface>>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64, f64)>>>>,
}

impl MotionAnchorPad {
    pub(super) fn new() -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, 152);
        area.set_hexpand(true);
        area.add_css_class("editor-motion-position-pad");
        area.add_css_class("editor-motion-anchor-pad");
        area.set_tooltip_text(Some(&t(
            "Click or drag on the image to place the zoom anchor; double-click to recenter",
        )));

        let pad = Self {
            area: area.clone(),
            anchor: Rc::new(Cell::new((0.5, 0.5))),
            surface: Rc::new(RefCell::new(None)),
            listeners: Rc::new(RefCell::new(Vec::new())),
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
                if n_press >= 2 {
                    pad.set_anchor(0.5, 0.5);
                    pad.notify();
                } else {
                    pad.apply_point(x, y);
                }
            }
        });
        area.add_controller(click);

        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let pad = pad.clone();
            move |gesture, _, _| {
                if let Some((x, y)) = gesture.start_point() {
                    pad.apply_point(x, y);
                }
            }
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
        area.add_controller(drag);
        area.set_cursor(gdk::Cursor::from_name("crosshair", None).as_ref());
        pad
    }

    pub(super) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    /// Update the puck without notifying listeners. Used when the timeline
    /// selection changes.
    pub(super) fn set_anchor(&self, x: f64, y: f64) {
        self.anchor.set((x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)));
        self.area.queue_draw();
    }

    /// Swap the thumbnail. Called from the inspector redraw so the picker
    /// always shows the current card; `card_preview` (downscaled) preferred.
    pub(super) fn set_surface(&self, surface: Option<ImageSurface>) {
        *self.surface.borrow_mut() = surface;
        self.area.queue_draw();
    }

    pub(super) fn connect_value_changed(&self, listener: impl Fn(f64, f64) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(listener));
    }

    fn notify(&self) {
        let (x, y) = self.anchor.get();
        for listener in self.listeners.borrow().iter().cloned() {
            listener(x, y);
        }
    }

    fn apply_point(&self, point_x: f64, point_y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let (x, y) = if let Some(surface) = self.surface.borrow().as_ref() {
            let (ix, iy, iw, ih) = image_rect_for(
                width,
                height,
                f64::from(surface.width().max(1)),
                f64::from(surface.height().max(1)),
            );
            if iw <= 0.0 || ih <= 0.0 {
                (0.5, 0.5)
            } else {
                (
                    ((point_x - ix) / iw).clamp(0.0, 1.0),
                    ((point_y - iy) / ih).clamp(0.0, 1.0),
                )
            }
        } else {
            (
                (point_x / width).clamp(0.0, 1.0),
                (point_y / height).clamp(0.0, 1.0),
            )
        };
        self.anchor.set((x, y));
        self.area.queue_draw();
        for listener in self.listeners.borrow().iter().cloned() {
            listener(x, y);
        }
    }

    fn draw(&self, widget: &DrawingArea, context: &gtk4::cairo::Context, width: i32, height: i32) {
        let width = f64::from(width.max(1));
        let height = f64::from(height.max(1));
        let light = widget_is_light(widget);
        rounded_rect(context, 0.0, 0.0, width, height, 10.0);
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.10);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.07);
        }
        context.fill_preserve().ok();
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        context.fill().ok();

        let surface = self.surface.borrow().clone();
        let Some(surface) = surface else {
            // No card yet: an empty field with the handle at its centre, so the
            // control still states its point before there is a thumbnail to
            // state it against.
            draw_soft_handle(context, width * 0.5, height * 0.5);
            return;
        };
        let surf_w = f64::from(surface.width().max(1));
        let surf_h = f64::from(surface.height().max(1));
        let (ix, iy, iw, ih) = image_rect_for(width, height, surf_w, surf_h);
        if iw <= 1.0 || ih <= 1.0 {
            return;
        }

        context.save().ok();
        rounded_rect(context, ix, iy, iw, ih, 8.0);
        context.clip();
        context.translate(ix, iy);
        context.scale(iw / surf_w, ih / surf_h);
        context.set_source_surface(&surface, 0.0, 0.0).ok();
        // Thumbnail is already downscaled (`card_preview`); Good is plenty and
        // keeps inspector scrub smooth.
        context.paint().ok();
        context.restore().ok();

        // Dim the thumbnail slightly so the white puck reads on bright shots,
        // then a rule-of-thirds overlay to aid placement.
        context.save().ok();
        rounded_rect(context, ix, iy, iw, ih, 8.0);
        context.clip();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.08);
        context.rectangle(ix, iy, iw, ih);
        context.fill().ok();
        context.set_source_rgba(1.0, 1.0, 1.0, 0.35);
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
        context.restore().ok();

        rounded_rect(context, ix, iy, iw, ih, 8.0);
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.35);
        } else {
            context.set_source_rgba(0.0, 0.0, 0.0, 0.45);
        }
        context.set_line_width(1.0);
        context.stroke().ok();

        let (ax, ay) = self.anchor.get();
        draw_soft_handle(context, ix + ax * iw, iy + ay * ih);
    }
}

/// Aspect-fit rectangle for the thumbnail inside the pad, with a small inset
/// so the marker ring never clips at the widget edge.
fn image_rect_for(alloc_w: f64, alloc_h: f64, surf_w: f64, surf_h: f64) -> (f64, f64, f64, f64) {
    const INSET: f64 = 6.0;
    let avail_w = (alloc_w - INSET * 2.0).max(1.0);
    let avail_h = (alloc_h - INSET * 2.0).max(1.0);
    let scale = (avail_w / surf_w.max(1.0)).min(avail_h / surf_h.max(1.0));
    let iw = surf_w * scale;
    let ih = surf_h * scale;
    ((alloc_w - iw) / 2.0, (alloc_h - ih) / 2.0, iw, ih)
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
    use super::image_rect_for;

    fn anchor_from_point(
        point_x: f64,
        point_y: f64,
        alloc_w: f64,
        alloc_h: f64,
        surf_w: f64,
        surf_h: f64,
    ) -> (f64, f64) {
        let (ix, iy, iw, ih) = image_rect_for(alloc_w, alloc_h, surf_w, surf_h);
        (
            ((point_x - ix) / iw).clamp(0.0, 1.0),
            ((point_y - iy) / ih).clamp(0.0, 1.0),
        )
    }

    #[test]
    fn anchor_mapping_round_trips_through_the_image_rect() {
        // Wide thumbnail in a 300x152 pad: image is letterboxed vertically.
        let (ix, iy, iw, ih) = image_rect_for(300.0, 152.0, 1200.0, 675.0);
        assert!(iw > 10.0 && ih > 10.0);
        // Corners map to the anchor extremes, center to center.
        assert_eq!(
            anchor_from_point(ix, iy, 300.0, 152.0, 1200.0, 675.0),
            (0.0, 0.0)
        );
        assert_eq!(
            anchor_from_point(ix + iw, iy + ih, 300.0, 152.0, 1200.0, 675.0),
            (1.0, 1.0)
        );
        let (cx, cy) = anchor_from_point(ix + iw * 0.5, iy + ih * 0.5, 300.0, 152.0, 1200.0, 675.0);
        assert!((cx - 0.5).abs() < 1e-9 && (cy - 0.5).abs() < 1e-9);
        // Clicks outside the image clamp instead of escaping 0-1.
        assert_eq!(
            anchor_from_point(0.0, 0.0, 300.0, 152.0, 1200.0, 675.0),
            (0.0, 0.0)
        );
    }
}
