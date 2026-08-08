use gtk4::{
    gdk, glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, DrawingArea,
    EventControllerFocus, EventControllerScroll, EventControllerScrollFlags, GestureClick,
    GestureDrag, Label, Popover, ScrolledWindow,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::super::super::state::EditorState;
use super::super::super::types::ViewTransform;

pub(super) const MIN_ZOOM_LEVEL: f64 = 0.25;
pub(super) const MAX_ZOOM_LEVEL: f64 = 6.0;
pub(super) const ZOOM_STEP: f64 = 1.1;

pub(super) fn clamp_zoom_level(value: f64) -> f64 {
    value.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL)
}

/// Wire zoom UI controls, ctrl+scroll zoom, and right-button pan.
/// Returns a shared `apply_zoom_change` callback for keyboard shortcuts.
pub(super) fn wire_zoom_controls(
    window: &ApplicationWindow,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    drawing_area: &DrawingArea,
    canvas_scroller: &ScrolledWindow,
    zoom_button: &Button,
    zoom_header_label: &Label,
    zoom_popup: &GtkBox,
    zoom_minus_btn: &Button,
    zoom_plus_btn: &Button,
    zoom_in_btn: &Button,
    zoom_out_btn: &Button,
    fit_to_screen_btn: &Button,
    zoom_to_selection_btn: &Button,
    zoom_level: &Rc<Cell<f64>>,
    update_canvas_content_size: &Rc<dyn Fn()>,
) -> Rc<dyn Fn(f64)> {
    let apply_zoom_change: Rc<dyn Fn(f64)> = Rc::new({
        let zoom_level = zoom_level.clone();
        let update_canvas_content_size = update_canvas_content_size.clone();
        let drawing_area = drawing_area.clone();
        move |next_zoom| {
            zoom_level.set(clamp_zoom_level(next_zoom));
            update_canvas_content_size();
            drawing_area.queue_draw();
        }
    });

    // Make popup focusable and close on focus loss (click outside)
    zoom_popup.set_can_focus(true);
    let zoom_popup_focus = zoom_popup.clone();
    let focus_controller = EventControllerFocus::new();
    focus_controller.connect_leave(move |_| {
        zoom_popup_focus.set_visible(false);
    });
    zoom_popup.add_controller(focus_controller);

    let zoom_popup_btn = zoom_popup.clone();
    zoom_button.connect_clicked(move |_| {
        let becoming_visible = !zoom_popup_btn.is_visible();
        zoom_popup_btn.set_visible(becoming_visible);
        if becoming_visible {
            zoom_popup_btn.grab_focus();
        }
    });

    // Close popup when clicking outside of it (on the window)
    let zoom_popup_window_click = zoom_popup.clone();
    let zoom_button_for_click = zoom_button.clone();
    let window_for_click = window.clone();
    let window_click = GestureClick::new();
    window_click.set_button(0); // Listen for all buttons
    window_click.connect_pressed(move |_, _, click_x, click_y| {
        if !zoom_popup_window_click.is_visible() {
            return;
        }

        // Get popup position relative to window
        let (popup_win_x, popup_win_y) = zoom_popup_window_click
            .translate_coordinates(&window_for_click, 0.0, 0.0)
            .unwrap_or((0.0, 0.0));
        let popup_alloc = zoom_popup_window_click.allocation();

        let in_popup = click_x >= popup_win_x
            && click_x <= popup_win_x + popup_alloc.width() as f64
            && click_y >= popup_win_y
            && click_y <= popup_win_y + popup_alloc.height() as f64;

        // Check if click is on the zoom button (toggle button)
        let (btn_x, btn_y) = zoom_button_for_click
            .translate_coordinates(&window_for_click, 0.0, 0.0)
            .unwrap_or((0.0, 0.0));
        let btn_alloc = zoom_button_for_click.allocation();
        let in_button = click_x >= btn_x
            && click_x <= btn_x + btn_alloc.width() as f64
            && click_y >= btn_y
            && click_y <= btn_y + btn_alloc.height() as f64;

        if !in_popup && !in_button {
            zoom_popup_window_click.set_visible(false);
        }
    });
    window.add_controller(window_click);

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_in = zoom_level.clone();
    let zoom_popup_in = zoom_popup.clone();
    zoom_in_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(zoom_level_in.get() * ZOOM_STEP);
        let _ = b;
        zoom_popup_in.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_out = zoom_level.clone();
    let zoom_popup_out = zoom_popup.clone();
    zoom_out_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(zoom_level_out.get() / ZOOM_STEP);
        let _ = b;
        zoom_popup_out.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_popup_fit = zoom_popup.clone();
    fit_to_screen_btn.connect_clicked(move |b| {
        apply_zoom_change_btn(1.0);
        let _ = b;
        zoom_popup_fit.set_visible(false);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_minus = zoom_level.clone();
    zoom_minus_btn.connect_clicked(move |_| {
        apply_zoom_change_btn(zoom_level_minus.get() / ZOOM_STEP);
    });

    let apply_zoom_change_btn = apply_zoom_change.clone();
    let zoom_level_plus = zoom_level.clone();
    zoom_plus_btn.connect_clicked(move |_| {
        apply_zoom_change_btn(zoom_level_plus.get() * ZOOM_STEP);
    });

    // Make the header label clickable to reset zoom to 100%
    let apply_zoom_change_label = apply_zoom_change.clone();
    let label_click = GestureClick::new();
    label_click.connect_pressed(move |_, _, _, _| {
        apply_zoom_change_label(1.0);
    });
    zoom_header_label.add_controller(label_click);

    let zoom_popup_sel = zoom_popup.clone();
    let state_zoom_sel = state.clone();
    let transform_zoom_sel = transform.clone();
    let drawing_area_zoom_sel = drawing_area.clone();
    let zoom_level_zoom_sel = zoom_level.clone();
    let canvas_scroller_zoom_sel = canvas_scroller.clone();
    zoom_to_selection_btn.connect_clicked(move |b| {
        let selection_rect = {
            let st = state_zoom_sel.lock().unwrap();
            if let Some(crop_rect) = st.draft_crop_rect().or(st.crop_selection) {
                Some(crop_rect)
            } else if let Some(action_idx) = st.selected_action_index {
                if let Some(action) = st.actions.get(action_idx) {
                    super::super::super::selection::action_bounds_with_padding(action, 0.0)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(rect) = selection_rect {
            let scroller_w = canvas_scroller_zoom_sel.allocated_width() as f64;
            let scroller_h = canvas_scroller_zoom_sel.allocated_height() as f64;
            let padding = super::super::canvas::CANVAS_PADDING as f64 * 2.0 + 40.0;
            let available_w = (scroller_w - padding).max(100.0);
            let available_h = (scroller_h - padding).max(100.0);

            let scale_x = available_w / rect.width.max(1) as f64;
            let scale_y = available_h / rect.height.max(1) as f64;
            let new_scale = scale_x.min(scale_y).clamp(0.25, 6.0);

            // Update zoom level and transform
            zoom_level_zoom_sel.set(new_scale);
            {
                let mut t = transform_zoom_sel.lock().unwrap();
                t.scale = new_scale;
                // Center the rect in the view
                t.offset_x =
                    (scroller_w - rect.width as f64 * new_scale) / 2.0 - rect.x as f64 * new_scale;
                t.offset_y =
                    (scroller_h - rect.height as f64 * new_scale) / 2.0 - rect.y as f64 * new_scale;
            }

            drawing_area_zoom_sel.queue_draw();
        }

        if let Some(popover) = b.ancestor(Popover::static_type()) {
            popover.downcast::<Popover>().unwrap().popdown();
        }
        zoom_popup_sel.set_visible(false);
    });

    let scroll_controller = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL
            | EventControllerScrollFlags::HORIZONTAL
            | EventControllerScrollFlags::DISCRETE,
    );
    let apply_zoom_change_scroll = apply_zoom_change.clone();
    let zoom_level_scroll = zoom_level.clone();
    scroll_controller.connect_scroll(move |controller, dx, dy| {
        if !controller
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK)
        {
            return glib::Propagation::Proceed;
        }

        // Prefer vertical; fall back to horizontal for devices that only emit dx.
        let delta = if dy != 0.0 { dy } else { dx };
        if delta < 0.0 {
            apply_zoom_change_scroll(zoom_level_scroll.get() * ZOOM_STEP);
        } else if delta > 0.0 {
            apply_zoom_change_scroll(zoom_level_scroll.get() / ZOOM_STEP);
        }
        glib::Propagation::Stop
    });
    drawing_area.add_controller(scroll_controller);

    let pan_origin = Rc::new(Cell::new((0.0, 0.0)));
    let pan_drag = GestureDrag::new();
    pan_drag.set_button(3);
    let pan_origin_begin = pan_origin.clone();
    let canvas_scroller_begin = canvas_scroller.clone();
    pan_drag.connect_drag_begin(move |_, _x, _y| {
        let hadj = canvas_scroller_begin.hadjustment();
        let vadj = canvas_scroller_begin.vadjustment();
        pan_origin_begin.set((hadj.value(), vadj.value()));
    });
    let pan_origin_update = pan_origin.clone();
    let canvas_scroller_update = canvas_scroller.clone();
    pan_drag.connect_drag_update(move |_, offset_x, offset_y| {
        let hadj = canvas_scroller_update.hadjustment();
        let vadj = canvas_scroller_update.vadjustment();
        let (start_x, start_y) = pan_origin_update.get();
        hadj.set_value((start_x - offset_x).clamp(hadj.lower(), hadj.upper() - hadj.page_size()));
        vadj.set_value((start_y - offset_y).clamp(vadj.lower(), vadj.upper() - vadj.page_size()));
    });
    drawing_area.add_controller(pan_drag);

    apply_zoom_change
}
