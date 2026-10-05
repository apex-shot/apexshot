//! Obfuscate controls: docked method picker and floating intensity slider.
//!
//! The method picker uses the shared second-toolbar dock while Obfuscate is
//! active or Select holds an obfuscation. Only the intensity slider follows
//! the draft or selected region; Blackout has no intensity slider.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, Scale,
};

use crate::capture::editor::{
    color::{MAX_OBFUSCATE_AMOUNT, MIN_OBFUSCATE_AMOUNT},
    state::EditorState,
    types::{AnnotationAction, ObfuscateMethod, Tool, ViewTransform},
    ui_support::DockedBarInset,
};
use crate::i18n::t;

use super::floating_bar::{
    build_option_row, build_pill, dock_position, dock_reserve, move_bar, popdown_for, queue_draw,
    set_bar_shown, set_dock_reserve, sync_option_selection, wire_option_rows, DockRefs,
};
use super::OBFUSCATE_METHOD_OPTIONS;

const METHOD_ROW_CLASS: &str = "editor-obfuscate-inspector-option";
const METHOD_ACTIVE_CLASS: &str = "editor-obfuscate-inspector-option-active";

/// Clearance between the rect and its floating slider.
const BAR_GAP: f64 = 12.0;

pub(super) struct ObfuscateBar {
    pub(super) method_bar: GtkBox,
    pub(super) slider_bar: GtkBox,
    method_icon: Image,
    method_label: Label,
    method_list: GtkBox,
    slider: Scale,
}

/// Icon and label for a method; labels stay `t()`-wrapped at the call site so
/// the i18n catalog checker keeps seeing static literals.
fn method_visuals(method: ObfuscateMethod) -> (&'static str, &'static str) {
    match method {
        ObfuscateMethod::Pixelate => (super::icon_names::VIEW_GRID, "Pixelate"),
        ObfuscateMethod::Blur => (super::icon_names::BLUR, "Blur"),
        ObfuscateMethod::Blackout => (super::icon_names::MEDIA_PLAYBACK_STOP, "Blackout"),
    }
}

fn prepend_method_icon(row: &Button, method: ObfuscateMethod) {
    let Some(content) = row.child() else {
        return;
    };
    let Ok(content) = content.downcast::<GtkBox>() else {
        return;
    };
    let (icon_name, _) = method_visuals(method);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.prepend(&icon);
}

pub(super) fn build_obfuscate_bar(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    toolbar_button: &Button,
    inspector_list: &GtkBox,
    toolbar_slider: &Scale,
    rebuild: &Rc<dyn Fn()>,
) -> ObfuscateBar {
    let method_pill = build_pill("Obfuscate method");
    for (method, label) in OBFUSCATE_METHOD_OPTIONS {
        let row = build_option_row(&t(label), "editor-obfuscate-inspector-check");
        row.add_css_class(METHOD_ROW_CLASS);
        prepend_method_icon(&row, method);
        method_pill.list.append(&row);
    }
    // The pill shell is text-only; the method keeps its icon next to the label.
    let (initial_icon, _) = method_visuals(state.lock().unwrap().obfuscate_method());
    let method_icon = Image::from_icon_name(initial_icon);
    method_icon.set_pixel_size(14);
    if let Some(row) = method_pill.button.child().and_downcast::<GtkBox>() {
        row.prepend(&method_icon);
    }
    {
        let (icon_name, label) = method_visuals(state.lock().unwrap().obfuscate_method());
        method_icon.set_icon_name(Some(icon_name));
        method_pill.label.set_label(&t(label));
    }
    {
        let state = state.clone();
        let area = drawing_area.downgrade();
        let method_icon = method_icon.clone();
        let method_label = method_pill.label.clone();
        let floating_list = method_pill.list.clone();
        let inspector_list = inspector_list.clone();
        let toolbar_button = toolbar_button.clone();
        let rebuild = rebuild.clone();
        wire_option_rows(&method_pill.list, METHOD_ROW_CLASS, move |index, button| {
            let Some((method, _)) = OBFUSCATE_METHOD_OPTIONS.get(index) else {
                return;
            };
            let method = *method;
            state.lock().unwrap().set_obfuscate_method(method);
            if let Some(child) = toolbar_button.child() {
                if let Ok(img) = child.downcast::<Image>() {
                    img.set_icon_name(Some(method_visuals(method).0));
                }
            }
            let (icon_name, label) = method_visuals(method);
            method_icon.set_icon_name(Some(icon_name));
            method_label.set_label(&t(label));
            sync_option_selection(&floating_list, index, METHOD_ACTIVE_CLASS);
            sync_option_selection(&inspector_list, index, METHOD_ACTIVE_CLASS);
            rebuild();
            popdown_for(button);
            queue_draw(&area);
        });
    }

    let method_bar = GtkBox::new(Orientation::Horizontal, 8);
    method_bar.add_css_class("editor-text-floating-bar");
    method_bar.set_halign(Align::Start);
    method_bar.set_valign(Align::Start);
    method_bar.append(&method_pill.button);
    set_bar_shown(&method_bar, false);

    let slider = Scale::with_range(
        Orientation::Horizontal,
        MIN_OBFUSCATE_AMOUNT,
        MAX_OBFUSCATE_AMOUNT,
        0.5,
    );
    slider.add_css_class("editor-toolbar-size-slider");
    slider.add_css_class("editor-obfuscate-intensity-slider");
    slider.set_draw_value(false);
    slider.set_size_request(200, -1);
    slider.set_halign(Align::Fill);
    slider.set_valign(Align::Center);
    slider.set_hexpand(true);
    slider.set_value(state.lock().unwrap().current_obfuscate_amount());
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

    let slider_bar = GtkBox::new(Orientation::Horizontal, 4);
    slider_bar.add_css_class("editor-text-floating-bar");
    slider_bar.add_css_class("editor-obfuscate-slider-bar");
    slider_bar.set_halign(Align::Start);
    slider_bar.set_valign(Align::Start);
    slider_bar.append(&slider);
    slider_bar.set_visible(false);

    ObfuscateBar {
        method_bar,
        slider_bar,
        method_icon: method_icon.clone(),
        method_label: method_pill.label,
        method_list: method_pill.list,
        slider: slider.clone(),
    }
}

/// Dock the method picker while the tool is active and float its intensity
/// slider beside a draft or selected region.
pub(super) fn install_obfuscate_bar_tick(
    bar: &ObfuscateBar,
    drawing_area: &DrawingArea,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    toolbar_slider: &Scale,
    dock_refs: &DockRefs,
    inset: &DockedBarInset,
) {
    let method_bar = bar.method_bar.clone();
    let slider_bar = bar.slider_bar.clone();
    let method_icon = bar.method_icon.clone();
    let method_label = bar.method_label.clone();
    let method_list = bar.method_list.clone();
    let slider = bar.slider.clone();
    let state = state.clone();
    let transform = transform.clone();
    let toolbar_slider = toolbar_slider.clone();
    let inset = inset.clone();
    let dock_refs = DockRefs {
        scroller: dock_refs.scroller.clone(),
        drawing_area: dock_refs.drawing_area.clone(),
    };
    // Sticky max heights (never underestimate, so a bar never covers the rect);
    // widths track the last measurement so a max never off-centers a bar.
    let known_slider = Rc::new(Cell::new((220.0f64, 48.0f64)));

    drawing_area.add_tick_callback(move |widget, _| {
        let (show_method, rect_opt, method, amount, view) = {
            let st = state.lock().unwrap();
            // Live draft first; otherwise the selected rect (reselect) when the
            // Obfuscate or Select tool is active. Prefer the rect's own
            // method/amount so re-editing shows the truth.
            let mut rect_opt = None;
            let mut method = st.obfuscate_method();
            let mut amount = st.current_obfuscate_amount();
            if st.selected_tool == Tool::Obfuscate {
                if let Some(AnnotationAction::Obfuscate { rect, .. }) = st.draft_action() {
                    rect_opt = Some(rect);
                }
            }
            if rect_opt.is_none() {
                if let Some(AnnotationAction::Obfuscate {
                    rect,
                    method: m,
                    amount: a,
                }) = st.selected_action()
                {
                    if matches!(st.selected_tool, Tool::Obfuscate | Tool::Select) {
                        rect_opt = Some(*rect);
                        method = *m;
                        amount = *a;
                    }
                }
            }
            let show = st.selected_tool == Tool::Obfuscate || rect_opt.is_some();
            drop(st);
            let view = *transform.lock().unwrap();
            (show, rect_opt, method, amount, view)
        };
        set_bar_shown(&method_bar, show_method);
        if !show_method {
            set_dock_reserve(&inset, "obfuscate", 0.0, widget);
            if slider_bar.is_visible() {
                slider_bar.set_visible(false);
            }
            return glib::ControlFlow::Continue;
        }
        // Sync method UI + intensity UI from state.
        let (icon_name, label, tooltip) = match method {
            ObfuscateMethod::Pixelate => {
                let (icon, label) = method_visuals(method);
                (icon, label, "Pixelate intensity")
            }
            ObfuscateMethod::Blur => {
                let (icon, label) = method_visuals(method);
                (icon, label, "Blur intensity")
            }
            ObfuscateMethod::Blackout => {
                let (icon, label) = method_visuals(method);
                (icon, label, "Blackout has no intensity control")
            }
        };
        method_icon.set_icon_name(Some(icon_name));
        method_label.set_label(&t(label));
        if let Some(pos) = OBFUSCATE_METHOD_OPTIONS
            .iter()
            .position(|(m, _)| *m == method)
        {
            sync_option_selection(&method_list, pos, METHOD_ACTIVE_CLASS);
        }
        set_dock_reserve(
            &inset,
            "obfuscate",
            dock_reserve(method_bar.height() as f64),
            widget,
        );
        let (left, top) = dock_position(&dock_refs, method_bar.width() as f64);
        move_bar(&method_bar, left, top);
        let Some(rect) = rect_opt else {
            slider_bar.set_visible(false);
            return glib::ControlFlow::Continue;
        };
        let has_slider = method.has_slider();
        if has_slider {
            slider.set_sensitive(true);
            slider.set_tooltip_text(Some(tooltip));
            if (slider.value() - amount).abs() > f64::EPSILON {
                slider.set_value(amount);
            }
            // Keep the toolbar slider in sync while the floating pills own the tool.
            if (toolbar_slider.value() - amount).abs() > f64::EPSILON {
                toolbar_slider.set_range(MIN_OBFUSCATE_AMOUNT, MAX_OBFUSCATE_AMOUNT);
                toolbar_slider.set_value(amount);
            }
            toolbar_slider.set_tooltip_text(Some(tooltip));
        }
        let area_w = widget.width() as f64;
        let area_h = widget.height() as f64;
        let x = rect.x as f64 * view.scale + view.offset_x;
        let y = rect.y as f64 * view.scale + view.offset_y;
        let rect_h = rect.height as f64 * view.scale;
        let rect_cx = x + rect.width as f64 * view.scale / 2.0;
        // Slider pill at the bottom; flip above only when there is no room.
        if slider_bar.is_visible() != has_slider {
            slider_bar.set_visible(has_slider);
        }
        if has_slider {
            let (mut slider_w, mut slider_h) = known_slider.get();
            let (sbw, sbh) = (slider_bar.width() as f64, slider_bar.height() as f64);
            if sbw > 1.0 {
                slider_w = sbw;
            }
            if sbh > 1.0 {
                slider_h = slider_h.max(sbh);
            }
            known_slider.set((slider_w, slider_h));
            let mut slider_top = y + rect_h + BAR_GAP;
            if slider_top + slider_h > area_h {
                slider_top = y - slider_h - BAR_GAP;
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
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    fn production_source() -> &'static str {
        let source = include_str!("obfuscate_bar.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn obfuscate_bar_reuses_the_shared_pill_shell() {
        let source = production_source();
        assert!(
            source.contains("pub(super) fn build_obfuscate_bar(")
                && source.contains("pub(super) fn install_obfuscate_bar_tick(")
                && source.contains("build_pill(\"Obfuscate method\")")
                && source.contains("wire_option_rows(")
                && source.contains("sync_option_selection(")
                && source.contains("use super::floating_bar::{"),
            "The obfuscate method picker should reuse the shared floating-bar pill shell"
        );
    }

    #[test]
    fn obfuscate_method_uses_the_second_toolbar_even_before_drawing() {
        let source = production_source();
        assert!(
            source.contains("st.selected_tool == Tool::Obfuscate")
                && source.contains("set_bar_shown(&method_bar, show_method)")
                && source.contains("dock_position(&dock_refs, method_bar.width() as f64)")
                && source.contains("set_dock_reserve(&inset, \"obfuscate\", 0.0, widget)")
                && !source.contains("method_bar.set_visible(")
                && !source.contains("known_method"),
            "The method picker stays docked while the tool is armed and releases its space when hidden"
        );
    }

    #[test]
    fn only_the_intensity_slider_follows_the_drawn_region() {
        let source = production_source();
        assert!(source.contains("st.draft_action()"));
        assert!(source.contains("let Some(rect) = rect_opt else"));
        assert!(source.contains("rect.x as f64 * view.scale + view.offset_x"));
        assert!(source.contains("move_bar(&slider_bar, slider_left, slider_top)"));
        assert!(source.contains("let has_slider = method.has_slider()"));
        assert!(!source.contains("dock_reserve(slider_bar"));
    }

    #[test]
    fn obfuscate_slider_shell_matches_the_floating_bar_shape() {
        let css = include_str!("../css/06-text-actions.css");
        let start = css
            .find(".editor-obfuscate-slider-bar {")
            .expect("slider bar shell rule");
        let end = css[start..]
            .find('}')
            .map(|offset| start + offset)
            .unwrap_or(css.len());
        let rule = &css[start..end];
        assert!(
            rule.contains("border-radius: 12px;") && !rule.contains("999px"),
            "The intensity slider bar should be a rounded rectangle like the other floating bars, not a full pill"
        );
    }
}
