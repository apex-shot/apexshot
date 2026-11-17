//! Floating arrow bar: colour, visual arrow styles, reverse, stroke presets.
//!
//! Like the highlighter bar this one never anchors to an element — the arrow is
//! painted with a drag, and a diagonal bounding box is no place to hang
//! controls. It stays docked in the chrome band above the canvas (see
//! [`super::floating_bar::dock_position`]) while the tool is armed or the
//! Select tool holds an arrow, and it shows that arrow's own style so an
//! existing arrow can be restyled from the same place.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{glib, prelude::*, Box as GtkBox, Button, DrawingArea};

use crate::capture::editor::{
    state::EditorState, types::AnnotationAction, types::Tool, ui_support::DockedBarInset,
};
use crate::i18n::t;

use super::color_control::ColorControl;
use super::floating_bar::{dock_position, dock_reserve, set_bar_shown, set_dock_reserve, DockRefs};
use super::picker_placement::PickerPlacementContext;
use super::toolbar_controls::{sub_bar, ArrowControl, MenuTracker, StrokeControl};

/// The Reverse button's whole effect.
///
/// A selected arrow is reversed in place (and that edit is undoable); with
/// nothing selected the flip applies to the next arrow drawn, so the tool's
/// default direction toggles. Pure, so both behaviours can be checked without a
/// toolkit; the button's wiring to it is checked separately.
fn reverse_arrow(state: &mut EditorState) {
    if !state.reverse_selected_arrow_action() {
        state.inverse_arrow_direction = !state.inverse_arrow_direction;
    }
}

pub(super) struct ArrowBar {
    pub(super) root: GtkBox,
    bar: GtkBox,
    colour: Rc<ColorControl>,
    style: Rc<ArrowControl>,
    reverse: Button,
    weight: Rc<StrokeControl>,
}

pub(super) fn build_arrow_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    apply_color: Rc<dyn Fn(crate::capture::editor::types::DrawColor)>,
    placement: Option<Rc<PickerPlacementContext>>,
    menus: &MenuTracker,
) -> ArrowBar {
    let bar = sub_bar("editor-arrow-floating-bar");

    let colour = Rc::new(ColorControl::new(state, apply_color, menus, placement));
    bar.append(&colour.button);

    let style = Rc::new(ArrowControl::new(state, drawing_area, menus));
    bar.append(&style.button);
    style.anchor_to(&bar);

    let reverse = Button::with_label(&t("Reverse"));
    reverse.set_focusable(false);
    reverse.add_css_class("editor-subbar-button");
    reverse.add_css_class("editor-arrow-reverse");
    reverse.connect_clicked({
        let state = state.clone();
        let area = drawing_area.downgrade();
        move |_| {
            {
                let mut st = state.lock().unwrap();
                reverse_arrow(&mut st);
            }
            super::floating_bar::queue_draw(&area);
        }
    });
    bar.append(&reverse);

    let weight = Rc::new(StrokeControl::new(state, drawing_area, menus, None));
    bar.append(&weight.button);
    weight.anchor_to(&bar);

    let root = colour.wrap(&bar);
    set_bar_shown(&root, false);
    ArrowBar {
        root,
        bar,
        colour,
        style,
        reverse,
        weight,
    }
}

/// Own the bar's visibility and keep its controls synced with the tool state.
pub(super) fn install_arrow_bar_tick(
    bar: &ArrowBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
    close_popovers: &Rc<dyn Fn(&GtkBox)>,
) {
    let root = bar.root.clone();
    let inner = bar.bar.clone();
    let colour = bar.colour.clone();
    let style = bar.style.clone();
    let reverse = bar.reverse.clone();
    let weight = bar.weight.clone();
    let state = state.clone();
    let inset = inset.clone();
    let close_popovers = close_popovers.clone();
    let dock_refs = DockRefs {
        scroller: dock_refs.scroller.clone(),
        drawing_area: dock_refs.drawing_area.clone(),
    };

    let was_shown = Cell::new(false);
    let last_selection = Cell::new(None::<(Tool, Option<usize>)>);
    drawing_area.add_tick_callback(move |widget, _| {
        let show = {
            let st = state.lock().unwrap();
            let arrow_tool = st.selected_tool == Tool::Arrow;
            let selected = matches!(st.selected_action(), Some(AnnotationAction::Arrow { .. }))
                && st.selected_tool == Tool::Select;
            arrow_tool || selected
        };

        // Show or hide first: a tick that bails out before revealing would
        // leave the bar transparent for good.
        set_bar_shown(&root, show);
        if !show {
            if was_shown.replace(false) {
                close_popovers(&root);
            }
            last_selection.set(None);
            set_dock_reserve(&inset, "arrow", 0.0, widget);
            return glib::ControlFlow::Continue;
        }
        was_shown.set(true);

        let context = {
            let st = state.lock().unwrap();
            (st.selected_tool, st.selected_action_index)
        };
        let previous = last_selection.replace(Some(context));
        if previous.is_some() && previous != Some(context) {
            close_popovers(&root);
        }

        {
            let st = state.lock().unwrap();
            colour.sync(&st);
            style.sync(&st);
            weight.sync(&st);
            let reversed = st
                .selected_arrow_reversed()
                .unwrap_or(st.inverse_arrow_direction);
            if reversed {
                reverse.add_css_class("selected");
            } else {
                reverse.remove_css_class("selected");
            }
            reverse.update_state(&[gtk4::accessible::State::Pressed(if reversed {
                gtk4::AccessibleTristate::True
            } else {
                gtk4::AccessibleTristate::False
            })]);
        }

        // Claim the band above the canvas; the layout moves the image down under
        // it, so a press on the bar is never a press the image needed. The claim
        // happens before the bar is measured so the image and the bar move in the
        // same step, and the bar is only placed once GTK knows its size — placing
        // it at 0 would park it half a bar off centre for a frame.
        let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);
        set_dock_reserve(&inset, "arrow", dock_reserve(bar_h), widget);
        let (left, top) = dock_position(&dock_refs, bar_w);
        if (root.margin_start() as f64 - left).abs() >= 1.0
            || (root.margin_top() as f64 - top).abs() >= 1.0
        {
            root.set_margin_start(left as i32);
            root.set_margin_top(top as i32);
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::{ArrowStyle, DrawColor, Point};
    use image::RgbaImage;

    fn test_state() -> Arc<Mutex<EditorState>> {
        Arc::new(Mutex::new(EditorState::new(RgbaImage::new(32, 32))))
    }

    fn noop_colour() -> Rc<dyn Fn(DrawColor)> {
        Rc::new(|_| {})
    }

    fn push_arrow(st: &mut EditorState) {
        st.actions.push(AnnotationAction::Arrow {
            start: Point { x: 1.0, y: 2.0 },
            end: Point { x: 20.0, y: 22.0 },
            color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 4.0,
            style: ArrowStyle::Standard,
            control_points: None,
            shadow: false,
            reversed: false,
        });
        st.selected_action_index = Some(0);
    }

    #[test]
    fn reverse_toggles_the_default_direction_without_a_selection() {
        let mut st = EditorState::new(RgbaImage::new(32, 32));
        st.selected_tool = Tool::Arrow;
        reverse_arrow(&mut st);
        assert!(st.inverse_arrow_direction);
        reverse_arrow(&mut st);
        assert!(!st.inverse_arrow_direction);
    }

    #[test]
    fn reverse_swaps_a_selected_arrow_and_undo_restores_it() {
        let mut st = EditorState::new(RgbaImage::new(32, 32));
        st.selected_tool = Tool::Select;
        push_arrow(&mut st);
        reverse_arrow(&mut st);
        {
            let Some(AnnotationAction::Arrow {
                start,
                end,
                reversed,
                ..
            }) = st.selected_action()
            else {
                panic!("arrow selection should survive reversing");
            };
            assert!(*reversed);
            assert_eq!(*start, Point { x: 20.0, y: 22.0 });
            assert_eq!(*end, Point { x: 1.0, y: 2.0 });
        }
        // Reversing a selected arrow is an edit, so it is undoable and the
        // tool's own default direction is left alone.
        assert!(!st.inverse_arrow_direction);
        assert!(st.undo());
        let Some(AnnotationAction::Arrow {
            start, reversed, ..
        }) = st.actions.first()
        else {
            panic!("undo should restore the original arrow");
        };
        assert!(!*reversed);
        assert_eq!(*start, Point { x: 1.0, y: 2.0 });
    }

    /// The button's wiring to [`reverse_arrow`], which is the only part a live
    /// toolkit adds.
    #[test]
    fn the_reverse_button_flips_the_arrow_direction() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Arrow;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let bar = build_arrow_bar(&state, &area, noop_colour(), None, &menus);
            bar.reverse.emit_clicked();
            assert!(state.lock().unwrap().inverse_arrow_direction);
        }) else {
            return;
        };
    }
}
