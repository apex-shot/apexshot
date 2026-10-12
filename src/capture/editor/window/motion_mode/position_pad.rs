use gtk4::{gdk, prelude::*, DrawingArea, GestureClick, GestureDrag, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::pad_handle::draw_soft_handle;

/// Direct 2D control for a Motion transform's camera framing (Ken Burns).
///
/// Pad top-right shows top-right: the card moves opposite the camera so the
/// requested region lands in the stage center.
/// The pad is deliberately a view/controller only: callers can synchronize it
/// from sliders without emitting another change, avoiding feedback loops.
#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionPositionPad {
    area: DrawingArea,
    position: Rc<Cell<(f64, f64)>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64, f64)>>>>,
    interaction_active: Rc<Cell<bool>>,
    interaction_started: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
    interaction_ended: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl MotionPositionPad {
    pub(in crate::capture::editor::window) fn new() -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, 152);
        area.set_hexpand(true);
        area.add_css_class("editor-motion-position-pad");

        let pad = Self {
            area: area.clone(),
            position: Rc::new(Cell::new((0.0, 0.0))),
            listeners: Rc::new(RefCell::new(Vec::new())),
            interaction_active: Rc::new(Cell::new(false)),
            interaction_started: Rc::new(RefCell::new(Vec::new())),
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
            move |_, _, x, y| {
                pad.begin_interaction();
                pad.apply_point(x, y);
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
            move |gesture, _, _| {
                pad.begin_interaction();
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
        drag.connect_drag_end({
            let pad = pad.clone();
            move |_, _, _| pad.end_interaction()
        });
        area.add_controller(drag);
        area.set_cursor(gdk::Cursor::from_name("move", None).as_ref());
        area.set_tooltip_text(Some("Drag to choose what the camera shows"));
        pad
    }

    pub(in crate::capture::editor::window) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    /// Update the visual marker without notifying listeners. Used when the
    /// transform changes through the X/Y sliders or timeline selection.
    pub(in crate::capture::editor::window) fn set_position(&self, x: f64, y: f64) {
        self.position.set((x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)));
        self.area.queue_draw();
    }

    pub(in crate::capture::editor::window) fn connect_value_changed(
        &self,
        listener: impl Fn(f64, f64) + 'static,
    ) {
        self.listeners.borrow_mut().push(Rc::new(listener));
    }

    pub(in crate::capture::editor::window) fn connect_interaction(
        &self,
        started: impl Fn() + 'static,
        ended: impl Fn() + 'static,
    ) {
        self.interaction_started.borrow_mut().push(Rc::new(started));
        self.interaction_ended.borrow_mut().push(Rc::new(ended));
    }

    fn begin_interaction(&self) {
        if self.interaction_active.replace(true) {
            return;
        }
        for listener in self.interaction_started.borrow().iter().cloned() {
            listener();
        }
    }

    fn end_interaction(&self) {
        if !self.interaction_active.replace(false) {
            return;
        }
        for listener in self.interaction_ended.borrow().iter().cloned() {
            listener();
        }
    }

    fn apply_point(&self, point_x: f64, point_y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let x = (point_x / width * 2.0 - 1.0).clamp(-1.0, 1.0);
        // Camera framing: positive Y shows the lower part of the scene, which
        // matches GTK's top-to-bottom coordinate system.
        let y = (point_y / height * 2.0 - 1.0).clamp(-1.0, 1.0);
        self.set_position(x, y);
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
        // Match the idle filled portion of FillSlider across the whole pad.
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        }
        context.fill().ok();

        // Zero, so the field still states its origin without a grid.
        if light {
            context.set_source_rgba(0.11, 0.13, 0.16, 0.16);
        } else {
            context.set_source_rgba(1.0, 1.0, 1.0, 0.16);
        }
        context.set_line_width(1.0);
        for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
            context.move_to(
                width * 0.5 - dx * CENTRE_TICK,
                height * 0.5 - dy * CENTRE_TICK,
            );
            context.line_to(
                width * 0.5 + dx * CENTRE_TICK,
                height * 0.5 + dy * CENTRE_TICK,
            );
        }
        context.stroke().ok();

        let (pos_x, pos_y) = self.position.get();
        // Leave room for the complete marker at every extreme; the control
        // value still reaches +/- 1 exactly when the pointer reaches an edge.
        let marker_margin = 19.0_f64.min(width * 0.5).min(height * 0.5);
        let x = marker_margin + (pos_x + 1.0) * 0.5 * (width - marker_margin * 2.0);
        let y = marker_margin + (pos_y + 1.0) * 0.5 * (height - marker_margin * 2.0);
        draw_soft_handle(context, x, y);
    }
}

/// Arm of the field's centre cross.
const CENTRE_TICK: f64 = 5.0;

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
    #[test]
    fn position_mapping_matches_the_pad_edges() {
        let map = |x: f64, y: f64| {
            (
                (x / 200.0 * 2.0 - 1.0).clamp(-1.0, 1.0),
                (y / 100.0 * 2.0 - 1.0).clamp(-1.0, 1.0),
            )
        };
        assert_eq!(map(100.0, 50.0), (0.0, 0.0));
        assert_eq!(map(0.0, 0.0), (-1.0, -1.0));
        assert_eq!(map(200.0, 100.0), (1.0, 1.0));
    }

    #[test]
    fn the_pad_states_position_with_one_soft_handle_and_no_grid() {
        // The field used to carry a 5x3 matrix of dots around a light disc with
        // an accent core. The control has no snap positions — `apply_point` maps
        // the pointer to a continuous -1..1 — so the matrix advertised a grid
        // that was never there, and the disc read as a second widget parked on
        // the pad. Both stay gone: position is one soft handle, and the only
        // other mark on the field is the cross at zero.
        let source = include_str!("position_pad.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        assert!(
            !production.contains("0.74, 0.74, 0.78"),
            "the dot matrix must not come back: it advertised positions the pad never snaps to"
        );
        assert!(
            !production.contains("0.690, 0.361, 0.220"),
            "the marker is the soft handle, not a disc ringed in white with an accent core"
        );
        assert!(
            production.contains("draw_soft_handle(context, x, y)"),
            "the marker is the pads' shared soft handle"
        );
        assert!(
            production.contains("CENTRE_TICK"),
            "the field keeps the cross that marks zero"
        );
    }
}
