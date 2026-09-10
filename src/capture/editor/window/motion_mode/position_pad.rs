use gtk4::{gdk, prelude::*, DrawingArea, GestureClick, GestureDrag, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Direct 2D control for a Motion transform's signed X/Y position.
///
/// The pad is deliberately a view/controller only: callers can synchronize it
/// from sliders without emitting another change, avoiding feedback loops.
#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionPositionPad {
    area: DrawingArea,
    position: Rc<Cell<(f64, f64)>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64, f64)>>>>,
}

impl MotionPositionPad {
    pub(super) fn new() -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, 152);
        area.set_hexpand(true);
        area.add_css_class("editor-motion-position-pad");

        let pad = Self {
            area: area.clone(),
            position: Rc::new(Cell::new((0.0, 0.0))),
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
            move |_, _, x, y| pad.apply_point(x, y)
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
        area.set_cursor(gdk::Cursor::from_name("move", None).as_ref());
        pad
    }

    pub(super) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    /// Update the visual marker without notifying listeners. Used when the
    /// transform changes through the X/Y sliders or timeline selection.
    pub(super) fn set_position(&self, x: f64, y: f64) {
        self.position.set((x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)));
        self.area.queue_draw();
    }

    pub(super) fn connect_value_changed(&self, listener: impl Fn(f64, f64) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(listener));
    }

    fn apply_point(&self, point_x: f64, point_y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let x = (point_x / width * 2.0 - 1.0).clamp(-1.0, 1.0);
        // Motion's positive Y moves the card down, which matches GTK's
        // top-to-bottom coordinate system.
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

        context.set_source_rgba(0.74, 0.74, 0.78, 0.56);
        for column in 0..5 {
            let x = width * (f64::from(column) + 0.5) / 5.0;
            for row in 1..=3 {
                let y = height * f64::from(row) / 4.0;
                context.arc(x, y, 1.7, 0.0, std::f64::consts::TAU);
                context.fill().ok();
            }
        }

        let (pos_x, pos_y) = self.position.get();
        // Leave room for the complete marker at every extreme; the control
        // value still reaches +/- 1 exactly when the pointer reaches an edge.
        let marker_margin = 19.0_f64.min(width * 0.5).min(height * 0.5);
        let x = marker_margin + (pos_x + 1.0) * 0.5 * (width - marker_margin * 2.0);
        let y = marker_margin + (pos_y + 1.0) * 0.5 * (height - marker_margin * 2.0);
        context.set_source_rgba(0.0, 0.0, 0.0, 0.20);
        context.arc(x, y + 2.0, 17.0, 0.0, std::f64::consts::TAU);
        context.fill().ok();
        context.set_source_rgb(0.94, 0.94, 0.94);
        context.arc(x, y, 16.0, 0.0, std::f64::consts::TAU);
        context.fill().ok();
        context.set_source_rgb(0.690, 0.361, 0.220);
        context.arc(x, y, 10.0, 0.0, std::f64::consts::TAU);
        context.fill().ok();
    }
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
    let radius = radius.min(width * 0.5).min(height * 0.5);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2 * 3.0,
    );
    context.close_path();
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
}
