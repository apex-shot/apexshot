use gtk4::{gdk, prelude::*, DrawingArea, GestureClick, GestureDrag, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::model::{
    DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y, MAX_MOTION_TEXT_POS, MIN_MOTION_TEXT_POS,
};

/// Marker radius, plus the breathing room that keeps the puck fully inside
/// the pad at every value.
const MARKER_RADIUS: f64 = 16.0;
const PAD_MARGIN: f64 = MARKER_RADIUS + 3.0;

/// Direct 2D control for a Motion title's placement.
///
/// Replaces the old Placement X/Y sliders: the pad is the card and the
/// draggable puck is the title, so "where does it land" is answered by
/// pointing at it instead of by reading two percentages.
///
/// Normalized `0.0-1.0` coordinates across the card, clamped to the model's
/// `MIN_MOTION_TEXT_POS..MAX_MOTION_TEXT_POS` band so the puck cannot show a
/// position the model would refuse to store. The pad is a view/controller
/// only: `set_text_pos` synchronizes without emitting, so the timeline
/// selection and the preview drag cannot loop back through it.
#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionTextPad {
    area: DrawingArea,
    position: Rc<Cell<(f64, f64)>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(f64, f64)>>>>,
}

impl MotionTextPad {
    pub(super) fn new() -> Self {
        let area = DrawingArea::new();
        area.set_size_request(-1, 152);
        area.set_hexpand(true);
        area.add_css_class("editor-motion-position-pad");
        area.add_css_class("editor-motion-text-pad");
        area.set_tooltip_text(Some(&t("Drag to place the title; double-click to reset")));

        let pad = Self {
            area: area.clone(),
            position: Rc::new(Cell::new((
                DEFAULT_MOTION_TEXT_POS_X,
                DEFAULT_MOTION_TEXT_POS_Y,
            ))),
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
                    pad.set_text_pos(DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y);
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
        area.set_cursor(gdk::Cursor::from_name("move", None).as_ref());
        pad
    }

    pub(super) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    /// Update the puck without notifying listeners. Used when the timeline
    /// selection changes or the preview drag writes a position.
    pub(super) fn set_text_pos(&self, x: f64, y: f64) {
        self.position.set((
            x.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS),
            y.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS),
        ));
        self.area.queue_draw();
    }

    pub(super) fn connect_value_changed(&self, listener: impl Fn(f64, f64) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(listener));
    }

    fn notify(&self) {
        let (x, y) = self.position.get();
        for listener in self.listeners.borrow().iter().cloned() {
            listener(x, y);
        }
    }

    fn apply_point(&self, point_x: f64, point_y: f64) {
        let width = f64::from(self.area.allocated_width().max(1));
        let height = f64::from(self.area.allocated_height().max(1));
        let (x, y) = point_to_text_pos(point_x, point_y, width, height);
        self.set_text_pos(x, y);
        self.notify();
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
        let (x, y) = text_pos_to_point(pos_x, pos_y, width, height);
        context.set_source_rgba(0.0, 0.0, 0.0, 0.20);
        context.arc(x, y + 2.0, MARKER_RADIUS + 1.0, 0.0, std::f64::consts::TAU);
        context.fill().ok();
        context.set_source_rgb(0.94, 0.94, 0.94);
        context.arc(x, y, MARKER_RADIUS, 0.0, std::f64::consts::TAU);
        context.fill().ok();
        // Theme accent orange (matches Position's marker and the crop dialog).
        context.set_source_rgb(0.690, 0.361, 0.220);
        context.arc(x, y, 10.0, 0.0, std::f64::consts::TAU);
        context.fill().ok();
    }
}

/// The pad's live area: the allocation inset by the marker radius so the
/// puck is never clipped at an extreme.
fn pad_inner_rect(width: f64, height: f64) -> (f64, f64, f64, f64) {
    let inset_x = PAD_MARGIN.min(width * 0.5);
    let inset_y = PAD_MARGIN.min(height * 0.5);
    (
        inset_x,
        inset_y,
        (width - inset_x * 2.0).max(1.0),
        (height - inset_y * 2.0).max(1.0),
    )
}

/// Pointer position to normalized title position. Both directions of the
/// mapping go through the same inner rect as the paint, so a click lands
/// exactly under the pointer instead of drifting from the drawn puck.
///
/// The pad covers the whole card, so a click past the model's band clamps
/// into it rather than escaping.
fn point_to_text_pos(point_x: f64, point_y: f64, width: f64, height: f64) -> (f64, f64) {
    let (inset_x, inset_y, inner_w, inner_h) = pad_inner_rect(width, height);
    (
        ((point_x - inset_x) / inner_w).clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS),
        ((point_y - inset_y) / inner_h).clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS),
    )
}

/// Normalized title position to the point the puck is painted at. Positions
/// clamp at `MIN_MOTION_TEXT_POS`, which is what stops a title from touching
/// the card edge, so the puck's travel is visibly short of the pad border.
fn text_pos_to_point(pos_x: f64, pos_y: f64, width: f64, height: f64) -> (f64, f64) {
    let (inset_x, inset_y, inner_w, inner_h) = pad_inner_rect(width, height);
    (inset_x + pos_x * inner_w, inset_y + pos_y * inner_h)
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
    fn text_pad_mapping_round_trips_through_the_marker_positions() {
        let (width, height) = (260.0, 152.0);
        for value in [MIN_MOTION_TEXT_POS, 0.5, MAX_MOTION_TEXT_POS] {
            let (x, y) = text_pos_to_point(value, value, width, height);
            let (back_x, back_y) = point_to_text_pos(x, y, width, height);
            assert!((back_x - value).abs() < 1e-9, "x {value} round-trips");
            assert!((back_y - value).abs() < 1e-9, "y {value} round-trips");
        }
    }

    #[test]
    fn text_pad_clicks_clamp_to_the_model_band() {
        let (width, height) = (260.0, 152.0);
        // Far outside the pad in both directions.
        assert_eq!(
            point_to_text_pos(-500.0, -500.0, width, height),
            (MIN_MOTION_TEXT_POS, MIN_MOTION_TEXT_POS)
        );
        assert_eq!(
            point_to_text_pos(9_999.0, 9_999.0, width, height),
            (MAX_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS)
        );
    }

    #[test]
    fn text_pad_puck_stays_fully_inside_at_every_extreme() {
        let (width, height) = (260.0, 152.0);
        for value in [MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS] {
            let (x, y) = text_pos_to_point(value, value, width, height);
            assert!(
                x >= MARKER_RADIUS && x <= width - MARKER_RADIUS,
                "puck x {x} stays inside the pad"
            );
            assert!(
                y >= MARKER_RADIUS && y <= height - MARKER_RADIUS,
                "puck y {y} stays inside the pad"
            );
        }
    }

    /// The default the pad opens on is the model's, so a title that has never
    /// been dragged sits where the old 50 % / 78 % sliders showed it.
    #[test]
    fn text_pad_default_matches_the_model_band() {
        for value in [DEFAULT_MOTION_TEXT_POS_X, DEFAULT_MOTION_TEXT_POS_Y] {
            assert!((MIN_MOTION_TEXT_POS..=MAX_MOTION_TEXT_POS).contains(&value));
        }
    }
}
