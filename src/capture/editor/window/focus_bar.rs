//! Floating focus bar: background intensity slider.
//!
//! Like the obfuscate slider this stays anchored below the active focus rect
//! (draft or selected) instead of docking, because it edits that specific
//! rect. There is no method pill — intensity is the only control — so this is
//! just the slider pill, reusing the obfuscate slider styling as-is.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{glib, prelude::*, Align, Box as GtkBox, DrawingArea, Label, Orientation, Scale};

use crate::capture::editor::{
    color::{MAX_FOCUS_INTENSITY, MIN_FOCUS_INTENSITY},
    state::EditorState,
    types::{AnnotationAction, Tool, ViewTransform},
    ui_support::DockedBarInset,
};
use crate::i18n::t;

use super::floating_bar::{
    dock_position, dock_reserve, move_bar, set_bar_shown, set_dock_reserve, DockRefs,
};
use super::toolbar_controls::sub_bar;

pub(super) struct FocusBar {
    pub(super) label_bar: GtkBox,
    pub(super) slider_bar: GtkBox,
    slider: Scale,
}

pub(super) fn build_focus_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    toolbar_slider: &Scale,
    rebuild: &Rc<dyn Fn()>,
) -> FocusBar {
    let slider = Scale::with_range(
        Orientation::Horizontal,
        MIN_FOCUS_INTENSITY,
        MAX_FOCUS_INTENSITY,
        0.5,
    );
    slider.add_css_class("editor-toolbar-size-slider");
    slider.add_css_class("editor-obfuscate-intensity-slider");
    slider.set_draw_value(false);
    slider.set_size_request(200, -1);
    slider.set_halign(Align::Fill);
    slider.set_valign(Align::Center);
    slider.set_hexpand(true);
    slider.set_value(state.lock().unwrap().current_focus_intensity());
    {
        let state = state.clone();
        let area = drawing_area.clone();
        let toolbar_slider = toolbar_slider.clone();
        let rebuild = rebuild.clone();
        slider.connect_value_changed(move |slider| {
            let value = slider.value();
            if state.lock().unwrap().set_active_size_without_rebuild(value) {
                if (toolbar_slider.value() - value).abs() > f64::EPSILON {
                    toolbar_slider.set_value(value);
                }
                rebuild();
                area.queue_draw();
            }
        });
    }

    let label_bar = sub_bar("editor-focus-label-bar");
    let label = Label::new(Some(&t("Focus")));
    label.add_css_class("editor-subbar-pill-label");
    label_bar.append(&label);
    set_bar_shown(&label_bar, false);

    let slider_bar = GtkBox::new(Orientation::Horizontal, 4);
    slider_bar.add_css_class("editor-text-floating-bar");
    slider_bar.add_css_class("editor-obfuscate-slider-bar");
    slider_bar.set_halign(Align::Start);
    slider_bar.set_valign(Align::Start);
    slider_bar.append(&slider);
    slider_bar.set_visible(false);

    FocusBar {
        label_bar,
        slider_bar,
        slider: slider.clone(),
    }
}

/// Own the bar's visibility and placement below the active focus rect (draft
/// or selected). Owns visibility: Focus/Select tool plus a rect shows it.
pub(super) fn install_focus_bar_tick(
    bar: &FocusBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    toolbar_slider: &Scale,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
) {
    let label_bar = bar.label_bar.clone();
    let slider_bar = bar.slider_bar.clone();
    let slider = bar.slider.clone();
    let state = state.clone();
    let transform = transform.clone();
    let toolbar_slider = toolbar_slider.clone();
    let inset = inset.clone();
    let dock_refs = DockRefs {
        scroller: dock_refs.scroller.clone(),
        drawing_area: dock_refs.drawing_area.clone(),
    };
    // Sticky max height (never underestimate, so the bar never covers the
    // rect); width tracks the last measurement so a max never off-centers it.
    let known = Rc::new(Cell::new((220.0f64, 48.0f64)));

    drawing_area.add_tick_callback(move |widget, _| {
        let (show_bar, rect_opt, intensity, view) = {
            let st = state.lock().unwrap();
            // Live draft first; otherwise the selected rect (reselect) when the
            // Focus or Select tool is active. Prefer the rect's own intensity
            // so re-editing shows the truth.
            let mut show = st.selected_tool == Tool::Focus;
            let mut rect_opt = None;
            let mut intensity = st.current_focus_intensity();
            if st.selected_tool == Tool::Focus {
                if let Some(AnnotationAction::Focus { rect, .. }) = st.draft_action() {
                    show = true;
                    rect_opt = Some(rect);
                }
            }
            if rect_opt.is_none() {
                if let Some(AnnotationAction::Focus { rect, intensity: i }) = st.selected_action() {
                    if matches!(st.selected_tool, Tool::Focus | Tool::Select) {
                        show = true;
                        rect_opt = Some(*rect);
                        intensity = *i;
                    }
                }
            }
            drop(st);
            let view = *transform.lock().unwrap();
            (show, rect_opt, intensity, view)
        };
        // The docked label bar marks the armed tool, like the other option
        // bars; the anchored slider still owns the intensity edit.
        set_bar_shown(&label_bar, show_bar);
        if !show_bar {
            set_dock_reserve(&inset, "focus", 0.0, widget);
            if slider_bar.is_visible() {
                slider_bar.set_visible(false);
            }
            return glib::ControlFlow::Continue;
        }
        set_dock_reserve(
            &inset,
            "focus",
            dock_reserve(label_bar.height() as f64),
            widget,
        );
        let (left, top) = dock_position(&dock_refs, label_bar.width() as f64);
        move_bar(&label_bar, left, top);
        let Some(rect) = rect_opt else {
            if slider_bar.is_visible() {
                slider_bar.set_visible(false);
            }
            return glib::ControlFlow::Continue;
        };
        slider.set_tooltip_text(Some(&t("Focus background intensity")));
        if (slider.value() - intensity).abs() > f64::EPSILON {
            slider.set_value(intensity);
        }
        // Keep the toolbar slider in sync while the floating pill owns the tool.
        if (toolbar_slider.value() - intensity).abs() > f64::EPSILON {
            toolbar_slider.set_range(MIN_FOCUS_INTENSITY, MAX_FOCUS_INTENSITY);
            toolbar_slider.set_value(intensity);
        }
        toolbar_slider.set_tooltip_text(Some(&t("Focus background intensity")));

        let area_w = widget.width() as f64;
        let area_h = widget.height() as f64;
        let y = rect.y as f64 * view.scale + view.offset_y;
        let rect_h = rect.height as f64 * view.scale;
        let rect_cx =
            rect.x as f64 * view.scale + view.offset_x + rect.width as f64 * view.scale / 2.0;
        let gap = 12.0;
        let (mut slider_w, mut slider_h) = known.get();
        let (sbw, sbh) = (slider_bar.width() as f64, slider_bar.height() as f64);
        if sbw > 1.0 {
            slider_w = sbw;
        }
        if sbh > 1.0 {
            slider_h = slider_h.max(sbh);
        }
        known.set((slider_w, slider_h));
        // Below the rect; flip above only when there is no room.
        let mut slider_top = y + rect_h + gap;
        if slider_top + slider_h > area_h {
            slider_top = y - slider_h - gap;
        }
        if slider_top < 0.0 {
            slider_top = (area_h - slider_h).max(0.0);
        }
        let slider_left = (rect_cx - slider_w / 2.0)
            .max(0.0)
            .min((area_w - slider_w).max(0.0));
        move_bar(&slider_bar, slider_left, slider_top);
        if !slider_bar.is_visible() {
            slider_bar.set_visible(true);
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    fn production_source() -> &'static str {
        let source = include_str!("focus_bar.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn focus_bar_pairs_a_docked_label_with_the_rect_slider() {
        let source = production_source();
        assert!(
            source.contains("pub(super) fn build_focus_bar(")
                && source.contains("pub(super) fn install_focus_bar_tick(")
                && source.contains("st.selected_tool == Tool::Focus")
                && source.contains("AnnotationAction::Focus {")
                && source.contains("st.draft_action()")
                && source.contains("current_focus_intensity()")
                && source.contains("t(\"Focus background intensity\")"),
            "The focus bar edits intensity for the draft or selected rect, nothing else"
        );
        assert!(
            source.contains("let label_bar = sub_bar(") && source.contains("t(\"Focus\")"),
            "The docked bar labels the armed tool like the other option bars"
        );
    }

    #[test]
    fn focus_bar_docks_its_label_and_anchors_the_slider_to_the_rect() {
        let source = production_source();
        assert!(
            source.contains("set_bar_shown(&label_bar, show_bar)")
                && source.contains("dock_position(&dock_refs, label_bar.width() as f64)")
                && source.contains("set_dock_reserve(&inset, \"focus\"")
                && source.contains("rect.x as f64 * view.scale + view.offset_x")
                && source.contains("move_bar(&slider_bar,"),
            "The label docks in the chrome band while the intensity slider follows its rect"
        );
    }
}
