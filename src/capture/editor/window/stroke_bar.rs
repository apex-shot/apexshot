use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{glib, prelude::*, Box as GtkBox, DrawingArea};

use crate::capture::editor::{
    state::EditorState,
    types::{AnnotationAction, Tool},
    ui_support::DockedBarInset,
};

use super::color_control::ColorControl;
use super::floating_bar::{dock_position, dock_reserve, set_bar_shown, set_dock_reserve, DockRefs};
use super::picker_placement::PickerPlacementContext;
use super::toolbar_controls::{sub_bar, MenuTracker, StrokeControl};

fn is_stroke_action(action: &AnnotationAction) -> bool {
    matches!(
        action,
        AnnotationAction::Pen { .. }
            | AnnotationAction::Line { .. }
            | AnnotationAction::Box { .. }
            | AnnotationAction::Circle { .. }
    )
}

pub(super) struct StrokeBar {
    pub(super) root: GtkBox,
    bar: GtkBox,
    colour: Rc<ColorControl>,
    weight: Rc<StrokeControl>,
}

pub(super) fn build_stroke_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    apply_color: Rc<dyn Fn(crate::capture::editor::types::DrawColor)>,
    placement: Option<Rc<PickerPlacementContext>>,
    menus: &MenuTracker,
    refresh_cursor: Option<Rc<dyn Fn()>>,
) -> StrokeBar {
    let bar = sub_bar("editor-stroke-floating-bar");

    let colour = Rc::new(ColorControl::new(state, apply_color, menus, placement));
    bar.append(&colour.button);
    let weight = Rc::new(StrokeControl::new(
        state,
        drawing_area,
        menus,
        refresh_cursor,
    ));
    bar.append(&weight.button);
    weight.anchor_to(&bar);

    let root = colour.wrap(&bar);
    set_bar_shown(&root, false);
    StrokeBar {
        root,
        bar,
        colour,
        weight,
    }
}

pub(super) fn install_stroke_bar_tick(
    bar: &StrokeBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
    close_popovers: &Rc<dyn Fn(&GtkBox)>,
) {
    let root = bar.root.clone();
    let inner = bar.bar.clone();
    let colour = bar.colour.clone();
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
            let stroke_tool = matches!(
                st.selected_tool,
                Tool::Pen | Tool::Line | Tool::Box | Tool::Circle
            );
            let selected = st.selected_tool == Tool::Select
                && st.selected_action().map(is_stroke_action).unwrap_or(false);
            stroke_tool || selected
        };

        set_bar_shown(&root, show);
        if !show {
            if was_shown.replace(false) {
                close_popovers(&root);
            }
            last_selection.set(None);
            set_dock_reserve(&inset, "stroke", 0.0, widget);
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
            weight.sync(&st);
        }

        let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);
        set_dock_reserve(&inset, "stroke", dock_reserve(bar_h), widget);
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
