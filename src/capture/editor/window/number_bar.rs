//! Floating number-tool bar: colour, numbering style, marker number, marker size.
//!
//! One contextual bar instead of one bar per marker. It stays docked in the
//! chrome band above the canvas while the Number tool is armed (or while the
//! Select tool holds a number marker), like the pen/arrow/shape/highlighter
//! bars — never parked over the marker itself, so it cannot cover the drawing.
//! Clicking an existing marker with the Number tool re-selects it, which is how
//! the bar comes back for an older marker. Mirrors the docked bars owned by
//! `window/mod.rs`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{glib, prelude::*, Box as GtkBox, Button, DrawingArea, Entry, Orientation};

use crate::capture::editor::{
    state::EditorState,
    types::{AnnotationAction, Tool},
    ui_support::DockedBarInset,
};
use crate::i18n::t;

use super::color_control::ColorControl;
use super::floating_bar::{
    dock_position, dock_reserve, queue_draw, set_bar_shown, set_dock_reserve, DockRefs,
};
use super::picker_placement::PickerPlacementContext;
use super::toolbar_controls::{sub_bar, MenuTracker, NumberControl, NumberControlKind};

/// Matches the render path's numbering ceiling.
const MAX_NUMBER: i64 = 9999;

pub(super) struct NumberBar {
    pub(super) root: GtkBox,
    bar: GtkBox,
    colour: Rc<ColorControl>,
    style: Rc<NumberControl>,
    size: Rc<NumberControl>,
    start_entry: Entry,
    syncing: Rc<std::cell::Cell<bool>>,
}

pub(super) fn build_number_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    apply_color: Rc<dyn Fn(crate::capture::editor::types::DrawColor)>,
    placement: Option<Rc<PickerPlacementContext>>,
    menus: &MenuTracker,
) -> NumberBar {
    let bar = sub_bar("editor-number-floating-bar");

    let colour = Rc::new(ColorControl::new(state, apply_color, menus, placement));
    bar.append(&colour.button);

    let style = Rc::new(NumberControl::new(
        state,
        drawing_area,
        menus,
        NumberControlKind::Style,
    ));
    bar.append(&style.button);
    style.anchor_to(&bar);

    // The `- [n] +` start stepper; edits update the armed start or the
    // selected marker's own number.
    let syncing = Rc::new(std::cell::Cell::new(false));
    let stepper_row = GtkBox::new(Orientation::Horizontal, 0);
    stepper_row.add_css_class("editor-subbar-stepper");
    stepper_row.set_valign(gtk4::Align::Center);
    let start_entry = Entry::new();
    start_entry.set_text("1");
    start_entry.set_width_chars(3);
    start_entry.set_max_width_chars(4);
    gtk4::prelude::EditableExt::set_alignment(&start_entry, 0.5);
    start_entry.add_css_class("editor-subbar-entry");
    for (label, delta) in [("−", -1i64), ("+", 1)] {
        let step = Button::with_label(label);
        step.set_focusable(false);
        step.add_css_class("editor-subbar-step");
        step.connect_clicked({
            let state = state.clone();
            let area = drawing_area.downgrade();
            move |_| {
                let changed = {
                    let mut st = state.lock().unwrap();
                    let next = (i64::from(st.active_number_start()) + delta).clamp(1, MAX_NUMBER);
                    st.set_active_number_start(next as u32)
                };
                if changed {
                    queue_draw(&area);
                }
            }
        });
        if delta < 0 {
            stepper_row.append(&step);
            stepper_row.append(&start_entry);
        } else {
            stepper_row.append(&step);
        }
    }
    start_entry.connect_changed({
        let state = state.clone();
        let area = drawing_area.downgrade();
        let syncing = syncing.clone();
        move |entry| {
            if syncing.get() {
                return;
            }
            if let Ok(value) = entry.text().parse::<u32>() {
                let changed = {
                    let mut st = state.lock().unwrap();
                    st.set_active_number_start(value.clamp(1, MAX_NUMBER as u32))
                };
                if changed {
                    queue_draw(&area);
                }
            }
        }
    });
    bar.append(&stepper_row);

    let size = Rc::new(NumberControl::new(
        state,
        drawing_area,
        menus,
        NumberControlKind::Size,
    ));
    bar.append(&size.button);
    size.anchor_to(&bar);

    let root = colour.wrap(&bar);
    set_bar_shown(&root, false);
    NumberBar {
        root,
        bar,
        colour,
        style,
        size,
        start_entry,
        syncing,
    }
}

/// Own the bar's visibility and keep it docked in the chrome band above the
/// canvas, whether the Number tool is armed or a number marker is selected.
pub(super) fn install_number_bar_tick(
    bar: &NumberBar,
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
    let size = bar.size.clone();
    let start_entry = bar.start_entry.clone();
    let syncing = bar.syncing.clone();
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
        let (show, has_marker) = {
            let st = state.lock().unwrap();
            let number_tool = st.selected_tool == Tool::Number;
            let has_marker = matches!(st.selected_action(), Some(AnnotationAction::Number { .. }))
                && (number_tool || st.selected_tool == Tool::Select);
            (number_tool || has_marker, has_marker)
        };

        // Show or hide first: a tick that bails out before revealing would
        // leave the bar transparent for good.
        set_bar_shown(&root, show);
        if !show {
            if was_shown.replace(false) {
                close_popovers(&root);
            }
            last_selection.set(None);
            set_dock_reserve(&inset, "number", 0.0, widget);
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
            size.sync(&st);
            let display = st.active_number_start_display();
            syncing.set(true);
            if start_entry.text().as_str() != display.as_str() {
                start_entry.set_text(&display);
            }
            syncing.set(false);
            start_entry.set_tooltip_text(Some(&number_start_tooltip(has_marker)));
        }

        // Claim the band above the canvas; the layout moves the image down under
        // it, so a press on the bar is never a press the image needed. The claim
        // happens before the bar is measured so the image and the bar move in the
        // same step, and the bar is only placed once GTK knows its size — placing
        // it at 0 would park it half a bar off centre for a frame.
        // Live size, like every other docked bar: a remembered guess would reserve
        // more (or less) canvas than the bar actually needs.
        let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);
        set_dock_reserve(&inset, "number", dock_reserve(bar_h), widget);
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

/// Stepper tooltip: what the number in the field refers to in each mode.
fn number_start_tooltip(has_marker: bool) -> String {
    if has_marker {
        t("Number of the selected marker")
    } else {
        t("Number the next marker starts at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::numbering_style::NumberingStyle;
    use crate::capture::editor::types::{DrawColor, Point};
    use image::RgbaImage;

    fn test_state() -> Arc<Mutex<EditorState>> {
        Arc::new(Mutex::new(EditorState::new(RgbaImage::new(32, 32))))
    }

    fn noop_colour() -> Rc<dyn Fn(DrawColor)> {
        Rc::new(|_| {})
    }

    fn push_selected_marker(st: &mut EditorState, number: u32) {
        st.actions.push(AnnotationAction::Number {
            position: Point { x: 4.0, y: 4.0 },
            number,
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            style: NumberingStyle::Numeric,
            size: crate::capture::editor::numbering_style::NumberSize::Medium,
            shadow: false,
        });
        st.selected_action_index = Some(0);
    }

    #[test]
    fn entry_typing_updates_the_start_and_clamps_at_the_top() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Number;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let bar = build_number_bar(&state, &area, noop_colour(), None, &menus);
            bar.start_entry.set_text("7");
            assert_eq!(state.lock().unwrap().active_number_start(), 7);
            bar.start_entry.set_text("123456");
            assert_eq!(state.lock().unwrap().active_number_start(), 9999);
            // A non-numeric entry leaves the start alone.
            bar.start_entry.set_text("abc");
            assert_eq!(state.lock().unwrap().active_number_start(), 9999);
        }) else {
            return;
        };
    }

    #[test]
    fn entry_renumbers_a_selected_marker_with_undo() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            {
                let mut st = state.lock().unwrap();
                st.selected_tool = Tool::Select;
                push_selected_marker(&mut st, 3);
            }
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let bar = build_number_bar(&state, &area, noop_colour(), None, &menus);
            bar.start_entry.set_text("9");
            {
                let st = state.lock().unwrap();
                let Some(AnnotationAction::Number { number, .. }) = st.selected_action() else {
                    panic!("marker should stay selected after renumbering");
                };
                assert_eq!(*number, 9);
            }
            assert!(state.lock().unwrap().undo());
            let st = state.lock().unwrap();
            let Some(AnnotationAction::Number { number, .. }) = st.actions.first() else {
                panic!("undo should restore the marker number");
            };
            assert_eq!(*number, 3);
        }) else {
            return;
        };
    }

    #[test]
    fn guarded_entry_sync_never_adds_state_or_history() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let state = test_state();
            state.lock().unwrap().selected_tool = Tool::Number;
            let area = DrawingArea::new();
            let menus = MenuTracker::default();
            let bar = build_number_bar(&state, &area, noop_colour(), None, &menus);
            let before = state.lock().unwrap().active_number_start();
            bar.syncing.set(true);
            bar.start_entry.set_text("42");
            bar.syncing.set(false);
            let st = state.lock().unwrap();
            assert_eq!(st.active_number_start(), before);
            assert_eq!(st.history_availability(), (false, false));
        }) else {
            return;
        };
    }
}
