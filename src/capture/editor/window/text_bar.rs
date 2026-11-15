//! Floating text bar: colour, font + size.
//!
//! Like the arrow/number bars this one stays docked in the chrome band above
//! the canvas (see [`super::floating_bar::dock_position`]) while the Text tool
//! is armed or a committed text action is selected, so it never parks over the
//! blue text outline it edits. Clicking an existing text with the Text tool
//! re-selects it, which is how the bar comes back for an older text — size and
//! font picks then route through the selected-action setters.

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
use super::toolbar_controls::{sub_bar, MenuTracker};

pub(super) struct TextBar {
    pub(super) root: GtkBox,
    bar: GtkBox,
    colour: Rc<ColorControl>,
}

pub(super) fn build_text_bar(
    state: &Arc<Mutex<EditorState>>,
    font_family_group: &GtkBox,
    text_size_group: &GtkBox,
    apply_color: Rc<dyn Fn(crate::capture::editor::types::DrawColor)>,
    placement: Option<Rc<PickerPlacementContext>>,
    menus: &MenuTracker,
) -> TextBar {
    let bar = sub_bar("editor-text-docked-bar");

    let colour = Rc::new(ColorControl::new(state, apply_color, menus, placement));
    bar.append(&colour.button);
    bar.append(font_family_group);
    bar.append(text_size_group);

    for group in [font_family_group, text_size_group] {
        let mut child = group.first_child();
        while let Some(widget) = child {
            if let Ok(button) = widget.clone().downcast::<gtk4::Button>() {
                let mut inner = button.first_child();
                while let Some(w) = inner {
                    if let Ok(popover) = w.clone().downcast::<gtk4::Popover>() {
                        menus.register(&popover);
                    }
                    inner = w.next_sibling();
                }
                let press = gtk4::GestureClick::new();
                press.set_propagation_phase(gtk4::PropagationPhase::Capture);
                press.connect_pressed({
                    let menus = menus.clone();
                    move |_, _, _, _| menus.close_all()
                });
                button.add_controller(press);
            }
            child = widget.next_sibling();
        }
    }

    let root = colour.wrap(&bar);
    set_bar_shown(&root, false);
    TextBar { root, bar, colour }
}

/// Own the bar's visibility and keep it docked above the canvas, whether the
/// Text tool is armed, a text action is selected, or an edit is in progress.
pub(super) fn install_text_bar_tick(
    bar: &TextBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
    close_popovers: &Rc<dyn Fn(&GtkBox)>,
) {
    let root = bar.root.clone();
    let inner = bar.bar.clone();
    let colour = bar.colour.clone();
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
            let text_tool = st.selected_tool == Tool::Text;
            let has_text_selection =
                matches!(st.selected_action(), Some(AnnotationAction::Text { .. }))
                    && (text_tool || st.selected_tool == Tool::Select);
            let editing = st.active_text_bounds.is_some() || st.active_text_input.is_some();
            text_tool || has_text_selection || editing
        };

        // Show or hide first: a tick that bails out before revealing would
        // leave the bar transparent for good.
        set_bar_shown(&root, show);
        if !show {
            if was_shown.replace(false) {
                close_popovers(&root);
            }
            last_selection.set(None);
            set_dock_reserve(&inset, "text", 0.0, widget);
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
        }

        // Claim the band above the canvas; the layout moves the image down under
        // it, so a press on the bar is never a press the image needed.
        let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);
        set_dock_reserve(&inset, "text", dock_reserve(bar_h), widget);
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
    fn production_source() -> &'static str {
        let source = include_str!("text_bar.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn text_bar_is_single_and_stays_docked_above_the_canvas() {
        let source = production_source();
        assert!(
            source.contains("pub(super) fn build_text_bar(")
                && source.contains("pub(super) fn install_text_bar_tick(")
                && source.contains("st.selected_tool == Tool::Text")
                && source.contains("Some(AnnotationAction::Text { .. })")
                && source.contains("Tool::Select")
                && source.contains("active_text_bounds.is_some()")
                && source.contains("dock_position(&dock_refs, bar_w)")
                && source.contains("dock_reserve(bar_h)"),
            "One bar should stay docked above the canvas (armed, with a text selected, or editing) instead of anchoring over the outline"
        );
        assert!(
            !source.contains("bounds.rect.x as f64 * t.scale"),
            "A docked bar must not anchor to the text outline, or it covers the drawing"
        );
    }

    #[test]
    fn text_bar_reuses_the_floating_bar_shell() {
        let source = production_source();
        assert!(
            source.contains("sub_bar(\"editor-text-docked-bar\")")
                && source.contains("use super::floating_bar::{")
                && source.contains("set_bar_shown(&root, show)")
                && source.contains("set_dock_reserve(&inset, \"text\","),
            "The text bar should reuse the shared floating-bar shell and dock helpers"
        );
    }

    #[test]
    fn text_bar_font_and_size_popovers_join_the_shared_menu_tracker() {
        let source = production_source();
        assert!(
            source.contains("menus.register(&popover);")
                && source.contains("PropagationPhase::Capture")
                && source.contains("menus.close_all()"),
            "Font/size popovers must fold competing menus and fold when a competitor opens"
        );
    }
}
