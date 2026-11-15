//! Floating highlighter bar: colour, mode, compact stroke presets.
//!
//! The highlighter paints, so unlike the text/number/obfuscate bars this one never
//! anchors to an element: it stays docked while the tool is armed (or while the
//! Select tool holds a stroke), which keeps it still while you draw stroke after
//! stroke instead of parking it over the last freehand bounding box. Docking is
//! image-aware — see [`super::floating_bar::dock_position`] — so the bar sits in
//! the empty chrome band above the canvas and never swallows a press meant for the
//! top of the image.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{glib, prelude::*, ApplicationWindow, Box as GtkBox, DrawingArea};

use crate::capture::editor::{
    pen_weight::HighlighterMode, state::EditorState, types::Tool, ui_support::DockedBarInset,
};
use crate::i18n::t;

use super::color_control::ColorControl;
use super::cursor::{set_highlighter_cursor, DEFAULT_HIGHLIGHTER_CURSOR_SIZE};
use super::floating_bar::{
    dock_position, dock_reserve, queue_draw, set_bar_shown, set_dock_reserve, DockRefs,
};
use super::picker_placement::PickerPlacementContext;
use super::toolbar_controls::{sub_bar, text_pill, MenuTracker, StrokeControl};

/// Alpha the highlighter cursor preview uses (matches the toolbar wiring).
const CURSOR_ALPHA: f64 = 0.4;

/// Mode rows in menu order; index order is the tick index the bar syncs to.
const HIGHLIGHTER_MODES: [(HighlighterMode, &str); 2] = [
    (HighlighterMode::TextAware, "Detect text"),
    (HighlighterMode::Freehand, "Freehand"),
];

/// The freehand brush cursor for a stroke size picked from the presets.
fn refresh_freehand_cursor(window: &ApplicationWindow, state: &Arc<Mutex<EditorState>>) {
    let (size, color) = {
        let st = state.lock().unwrap();
        (
            st.stroke_size,
            (
                st.selected_color.r,
                st.selected_color.g,
                st.selected_color.b,
                CURSOR_ALPHA,
            ),
        )
    };
    set_highlighter_cursor(window, size, color);
}

pub(super) struct HighlighterBar {
    pub(super) root: GtkBox,
    bar: GtkBox,
    colour: Rc<ColorControl>,
    set_mode: Rc<dyn Fn(usize)>,
    weight: Rc<StrokeControl>,
}

pub(super) fn build_highlighter_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    window: &ApplicationWindow,
    apply_color: Rc<dyn Fn(crate::capture::editor::types::DrawColor)>,
    placement: Option<Rc<PickerPlacementContext>>,
    menus: &MenuTracker,
) -> HighlighterBar {
    let bar = sub_bar("editor-highlighter-floating-bar");

    let colour = Rc::new(ColorControl::new(state, apply_color, menus, placement));
    bar.append(&colour.button);

    // Static `t()` literals so the i18n catalog checker registers both messages.
    let mode_options = [t("Detect text"), t("Freehand")];
    let mode_labels: Vec<&str> = mode_options.iter().map(String::as_str).collect();
    let mode = text_pill(menus, &mode_labels, {
        let state = state.clone();
        let area = drawing_area.downgrade();
        let window = window.clone();
        Rc::new(move |index| {
            let Some((mode, _)) = HIGHLIGHTER_MODES.get(index).copied() else {
                return;
            };
            let changed = {
                let mut st = state.lock().unwrap();
                let changed = st.set_highlighter_mode_and_check(mode);
                let size = match st.highlighter_mode {
                    HighlighterMode::TextAware => DEFAULT_HIGHLIGHTER_CURSOR_SIZE,
                    HighlighterMode::Freehand => st.stroke_size,
                };
                let color = (
                    st.selected_color.r,
                    st.selected_color.g,
                    st.selected_color.b,
                    CURSOR_ALPHA,
                );
                drop(st);
                set_highlighter_cursor(&window, size, color);
                changed
            };
            if changed {
                queue_draw(&area);
            }
        })
    });
    bar.append(&mode.button);

    let refresh = Rc::new({
        let window = window.clone();
        let state = state.clone();
        move || refresh_freehand_cursor(&window, &state)
    });
    let weight = Rc::new(StrokeControl::new(
        state,
        drawing_area,
        menus,
        Some(refresh),
    ));
    bar.append(&weight.button);
    weight.anchor_to(&bar);

    let root = colour.wrap(&bar);
    set_bar_shown(&root, false);
    HighlighterBar {
        root,
        bar,
        colour,
        set_mode: mode.set_selected,
        weight,
    }
}

/// Own the bar's visibility and keep its controls synced with the tool state.
pub(super) fn install_highlighter_bar_tick(
    bar: &HighlighterBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
    close_popovers: &Rc<dyn Fn(&GtkBox)>,
) {
    let root = bar.root.clone();
    let inner = bar.bar.clone();
    let colour = bar.colour.clone();
    let set_mode = bar.set_mode.clone();
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
            let highlighter_tool = st.selected_tool == Tool::Highlighter;
            let has_stroke =
                st.selected_tool == Tool::Select && st.selected_highlighter_stroke_size().is_some();
            highlighter_tool || has_stroke
        };

        // Show or hide first: a tick that bails out before revealing would
        // leave the bar transparent for good.
        set_bar_shown(&root, show);
        if !show {
            if was_shown.replace(false) {
                close_popovers(&root);
            }
            last_selection.set(None);
            set_dock_reserve(&inset, "highlighter", 0.0, widget);
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
            if let Some(index) = HIGHLIGHTER_MODES
                .iter()
                .position(|(mode, _)| *mode == st.highlighter_mode)
            {
                set_mode(index);
            }
            weight.sync(&st);
        }

        // Claim the band above the canvas; the layout moves the image down under
        // it, so a press on the bar is never a press the image needed. The claim
        // happens before the bar is measured so the image and the bar move in the
        // same step, and the bar is only placed once GTK knows its size — placing
        // it at 0 would park it half a bar off centre for a frame.
        let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);
        set_dock_reserve(&inset, "highlighter", dock_reserve(bar_h), widget);
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
        let source = include_str!("highlighter_bar.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn highlighter_bar_shows_mode_for_tool_and_selected_stroke() {
        let source = production_source();
        assert!(
            source.contains("pub(super) fn build_highlighter_bar(")
                && source.contains("pub(super) fn install_highlighter_bar_tick(")
                && source.contains("st.selected_tool == Tool::Highlighter")
                && source.contains("st.selected_tool == Tool::Select")
                && source.contains("st.selected_highlighter_stroke_size().is_some()")
                && source.contains("HighlighterMode::TextAware")
                && source.contains("HighlighterMode::Freehand"),
            "The highlighter bar shows the mode while the tool is armed, and for a selected stroke"
        );
    }

    #[test]
    fn highlighter_bar_owns_the_stroke_presets_now() {
        let source = production_source();
        assert!(
            source.contains("StrokeControl::new("),
            "Thickness lives in the docked bar's stroke presets, not the primary rail"
        );
    }

    #[test]
    fn highlighter_bar_controls_route_through_the_state_setter() {
        let source = production_source();
        assert!(
            source.contains("set_highlighter_mode_and_check(mode)")
                && source.contains("set_highlighter_cursor(&window, size, color)"),
            "Bar picks must go through the state setter and refresh the brush cursor"
        );
    }

    #[test]
    fn highlighter_bar_reserves_its_own_space_instead_of_anchoring_to_the_stroke() {
        let source = production_source();
        assert!(
            source.contains("let (left, top) = dock_position(&dock_refs, bar_w);")
                && source.contains("dock_reserve(bar_h)")
                && source.contains("0.0, widget);"),
            "The bar claims the reserved band while docked and hands it back when hidden"
        );
        // No per-element anchoring: a paint tool's bar does not follow a stroke.
        assert!(
            !source.contains("view.offset_x") && !source.contains("transform"),
            "a paint tool's bar should not anchor to a freehand bounding box"
        );
    }

    #[test]
    fn highlighter_bar_labels_are_registered_for_translation() {
        let source = production_source();
        assert!(
            source.contains("t(\"Detect text\")") && source.contains("t(\"Freehand\")"),
            "Mode labels need static t() literals for the i18n catalog checker"
        );
    }
}
