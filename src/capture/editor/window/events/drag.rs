//! Canvas drag begin/update/end gesture family (PR 10.16).
//!
//! Owns the complete `GestureDrag` lifecycle on the drawing area: Space-pan,
//! select/arrow/text/box/circle interaction, freehand draw finalize,
//! redraw throttling, and effect-rebuild flags. Click/motion/keyboard stay in
//! sibling modules.

use gtk4::{gdk, glib, prelude::*, ApplicationWindow, DrawingArea, GestureDrag, ScrolledWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    color::DRAG_REDRAW_INTERVAL_US,
    state::EditorState,
    types::{ArrowStyle, Point, Tool, ViewTransform},
};

use super::super::cursor::set_window_cursor_name;
use super::{MOVE_HANDLE_DRAG_RADIUS, RESIZE_HANDLE_DRAG_SIZE};

const ARROW_CLICK_NOOP_DISTANCE: f64 = 3.0;

fn stable_pan_offset(
    surface_origin: Option<(f64, f64)>,
    surface_position: Option<(f64, f64)>,
    gesture_offset: (f64, f64),
) -> (f64, f64) {
    match (surface_origin, surface_position) {
        (Some((start_x, start_y)), Some((current_x, current_y))) => {
            (current_x - start_x, current_y - start_y)
        }
        _ => gesture_offset,
    }
}

/// Wire canvas `GestureDrag` begin/update/end and attach the controller.
pub(super) fn wire_canvas_drag(
    window: &ApplicationWindow,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    drawing_area: &DrawingArea,
    canvas_scroller: &ScrolledWindow,
    space_pan_active: &Rc<Cell<bool>>,
    space_pan_dragging: &Rc<Cell<bool>>,
    space_pan_origin: &Rc<Cell<(f64, f64)>>,
    eyedropper_mode: &Rc<Cell<bool>>,
    rebuild_effects_async: &Rc<dyn Fn()>,
    sync_size_control: &Rc<dyn Fn()>,
    sync_select_inspector: &Rc<dyn Fn()>,
) {
    let drag_start_transform = Rc::new(RefCell::new(None::<ViewTransform>));
    let drag = GestureDrag::new();
    let drag_last_redraw = Rc::new(Cell::new(0_i64));
    let space_pan_surface_origin = Rc::new(Cell::new(None::<(f64, f64)>));
    let eyedropper_mode_drag_begin = eyedropper_mode.clone();
    let state_drag_begin = state.clone();
    let transform_drag_begin = transform.clone();
    let drawing_area_begin = drawing_area.downgrade();
    let drag_last_redraw_begin = drag_last_redraw.clone();
    let space_pan_active_drag_begin = space_pan_active.clone();
    let space_pan_dragging_begin = space_pan_dragging.clone();
    let space_pan_origin_begin = space_pan_origin.clone();
    let space_pan_surface_origin_begin = space_pan_surface_origin.clone();
    let canvas_scroller_space_pan_begin = canvas_scroller.clone();
    let window_space_pan_begin = window.downgrade();
    let drag_start_transform_begin = drag_start_transform.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        if space_pan_active_drag_begin.get() {
            let hadj = canvas_scroller_space_pan_begin.hadjustment();
            let vadj = canvas_scroller_space_pan_begin.vadjustment();
            space_pan_origin_begin.set((hadj.value(), vadj.value()));
            space_pan_surface_origin_begin
                .set(gesture.current_event().and_then(|event| event.position()));
            space_pan_dragging_begin.set(true);
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if let Some(window) = window_space_pan_begin.upgrade() {
                set_window_cursor_name(&window, Some("grabbing"));
            }
            return;
        }

        if eyedropper_mode_drag_begin.get() {
            return;
        }

        let t = *transform_drag_begin.lock().unwrap();
        drag_start_transform_begin.borrow_mut().replace(t);
        let view_point = Point { x, y };

        if !t.contains_view(view_point) {
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        let mut st = state_drag_begin.lock().unwrap();

        if st.selected_tool == Tool::Crop {
            st.begin_crop_drag(t.view_to_image_clamped(view_point), t.scale);
            st.drag_start_view = Some(view_point);
            drop(st);
            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            return;
        }

        if st.selected_tool == Tool::Select {
            let image_point = t.view_to_image_clamped(view_point);

            // Check if selected action is an arrow — allow control handle editing.
            let selected_is_arrow = st
                .selected_action_index
                .and_then(|i| st.actions.get(i))
                .map(|a| matches!(a, crate::capture::editor::types::AnnotationAction::Arrow { .. }))
                .unwrap_or(false);

            if selected_is_arrow {
                // Ensure control_points are initialised for curved/double arrows.
                if let Some(idx) = st.selected_action_index {
                    if let Some(crate::capture::editor::types::AnnotationAction::Arrow {
                        style,
                        control_points,
                        start,
                        end,
                        ..
                    }) = st.actions.get_mut(idx)
                    {
                        if control_points.is_none() {
                            match style {
                                ArrowStyle::Curved | ArrowStyle::Double => {
                                    let mid = Point {
                                        x: (start.x + end.x) / 2.0,
                                        y: (start.y + end.y) / 2.0,
                                    };
                                    *control_points = Some(vec![*start, mid, *end]);
                                }
                                _ => {
                                    *control_points = Some(vec![*start, *end]);
                                }
                            }
                        }
                    }
                }

                // 1a. Handle hit — check control handles first.
                if let Some(handle_idx) = st.arrow_control_handle_at(image_point) {
                    st.arrow_control_dragging = Some(handle_idx);
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    return;
                }

                // 1b. Body hit — drag the whole arrow; keep handles visible.
                let idx = st.selected_action_index.unwrap();
                let hit_body = crate::capture::editor::selection::action_contains_point_with_padding(
                    &st.actions[idx],
                    image_point,
                    8.0,
                );
                if hit_body {
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }

            // Generic select drag (non-arrow or click outside arrow).
            st.drag_start_view = Some(view_point);
            st.begin_select_drag_with_scale(t.view_to_image_clamped(view_point), t.scale);
            drop(st);

            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
        }

        // Arrow tool: unified interaction — handle drag, body drag, or new draw.
        if st.selected_tool == Tool::Arrow {
            let image_point = t.view_to_image_clamped(view_point);

            // --- Case 1: an arrow is already selected ---
            let selected_is_arrow = st
                .selected_action_index
                .and_then(|i| st.actions.get(i))
                .map(|a| matches!(a, crate::capture::editor::types::AnnotationAction::Arrow { .. }))
                .unwrap_or(false);

            if selected_is_arrow {
                // 1a. Handle hit — always check this first regardless of arrow_editing_controls.
                if let Some(handle_idx) = st.arrow_control_handle_at(image_point) {
                    st.arrow_control_dragging = Some(handle_idx);
                    st.arrow_editing_controls = true;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    return;
                }

                // 1b. Body hit — drag the whole arrow; keep handles visible.
                let idx = st.selected_action_index.unwrap();
                let hit_body = crate::capture::editor::selection::action_contains_point_with_padding(
                    &st.actions[idx],
                    image_point,
                    8.0,
                );
                if hit_body {
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.arrow_editing_controls = true; // keep handles visible during move
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }

                // 1c. Clicked outside the selected arrow — deselect, fall through to new draw.
                st.selected_action_index = None;
                st.select_drag_anchor = None;
                st.arrow_editing_controls = false;
            }

            // --- Case 2: no arrow selected — check if click lands on an existing arrow ---
            if st.selected_action_index.is_none()
                && st.select_action_at_point_with_scale(image_point, t.scale)
            {
                let is_arrow = st
                    .selected_action()
                    .map(|a| matches!(a, crate::capture::editor::types::AnnotationAction::Arrow { .. }))
                    .unwrap_or(false);
                if is_arrow {
                    // Ensure control_points are initialised
                    if let Some(idx) = st.selected_action_index {
                        if let Some(crate::capture::editor::types::AnnotationAction::Arrow {
                            style,
                            control_points,
                            start,
                            end,
                            ..
                        }) = st.actions.get_mut(idx)
                        {
                            if control_points.is_none() {
                                match style {
                                    ArrowStyle::Curved | ArrowStyle::Double => {
                                        let mid = Point {
                                            x: (start.x + end.x) / 2.0,
                                            y: (start.y + end.y) / 2.0,
                                        };
                                        *control_points = Some(vec![*start, mid, *end]);
                                    }
                                    _ => {
                                        *control_points = Some(vec![*start, *end]);
                                    }
                                }
                            }
                        }
                    }
                    st.arrow_editing_controls = true;
                    st.select_drag_anchor = Some(image_point);
                    st.select_resize_handle = None;
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                } else {
                    // Hit something that isn't an arrow — deselect, fall through to new draw.
                    st.selected_action_index = None;
                    st.select_drag_anchor = None;
                }
            }
        }

        // Text tool with a selected action: check handles first, then fall back to move.
        if st.selected_tool == Tool::Text
            && st.selected_action_index.is_some()
            && st.active_text_input.is_none()
        {
            let image_point = t.view_to_image_clamped(view_point);

            // Compute the committed action's TextEditBounds for handle hit-testing.
            let bounds_opt = if let Some(index) = st.selected_action_index {
                if let Some(crate::capture::editor::types::AnnotationAction::Text {
                    position,
                    text,
                    font,
                    max_width,
                    ..
                }) = st.actions.get(index)
                {
                    let surface =
                        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok();
                    surface
                        .as_ref()
                        .and_then(|s| gtk4::cairo::Context::new(s).ok())
                        .map(|c| {
                            let aw = max_width.unwrap_or_else(|| {
                                (st.base_image.width() as f64 - position.x).max(font.size * 1.8)
                            });
                            crate::capture::editor::render::text_action_bounds(
                                &c,
                                *position,
                                text,
                                font,
                                Some(aw),
                            )
                        })
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(bounds) = bounds_opt {
                // Hit-test left/right circles.
                let handle_hit = bounds.move_handles.iter().find_map(|(h, center)| {
                    let cv = Point {
                        x: center.x * t.scale + t.offset_x,
                        y: center.y * t.scale + t.offset_y,
                    };
                    let dx = x - cv.x;
                    let dy = y - cv.y;
                    if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                        Some(h.clone())
                    } else {
                        None
                    }
                });
                // Hit-test bottom-right resize box.
                let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, rp)| {
                    let rv = Point {
                        x: rp.x * t.scale + t.offset_x,
                        y: rp.y * t.scale + t.offset_y,
                    };
                    (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                        && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                });

                if handle_hit.is_some() || resize_hit {
                    // Handle drag: set up active_text_is_dragging so the motion
                    // handler takes over — same as the active-edit handle path.
                    st.begin_history_interaction();
                    st.active_text_bounds = Some(bounds);
                    st.active_text_is_dragging = true;
                    st.active_text_drag_handle = handle_hit;
                    st.active_text_drag_start = Some(image_point);
                    st.active_text_drag_start_bounds =
                        st.active_text_bounds.as_ref().map(|b| b.rect);
                    st.active_text_is_resizing = resize_hit;
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }

            // No handle hit — move the whole action.
            st.drag_start_view = Some(view_point);
            st.select_drag_anchor = Some(image_point);
            st.select_resize_handle = None;
            drop(st);
            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
        }

        if matches!(st.selected_tool, Tool::Text | Tool::Number) {
            return;
        }

        // Box/Circle/Obfuscate tool: unified interaction — resize, move, or draw new.
        if matches!(
            st.selected_tool,
            Tool::Box | Tool::Circle | Tool::Obfuscate
        ) {
            let image_point = t.view_to_image_clamped(view_point);

            // If an action is already selected and we're dragging it, continue.
            if st.selected_action_index.is_some() && st.select_drag_anchor.is_some() {
                drop(st);
                if let Some(area) = drawing_area_begin.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_begin.set(glib::monotonic_time());
                return;
            }

            // If an action is already selected, check resize handles first, then body hit.
            if st.selected_action_index.is_some() {
                if let Some(index) = st.selected_action_index {
                    if let Some(selected) = st.actions.get(index) {
                        let is_matching_type = match selected {
                            crate::capture::editor::types::AnnotationAction::Box { .. } => {
                                st.selected_tool == Tool::Box
                            }
                            crate::capture::editor::types::AnnotationAction::Circle { .. } => {
                                st.selected_tool == Tool::Circle
                            }
                            crate::capture::editor::types::AnnotationAction::Obfuscate { .. } => {
                                st.selected_tool == Tool::Obfuscate
                            }
                            _ => false,
                        };
                        if is_matching_type {
                            // Check resize handles first.
                            let handle_hit_radius =
                                crate::capture::editor::color::selection_handle_hit_radius_for_scale(t.scale);
                            if let Some(handle) =
                                crate::capture::editor::selection::action_resize_handle_at_point_with_radius(
                                    selected,
                                    image_point,
                                    handle_hit_radius,
                                )
                            {
                                st.select_resize_handle = Some(handle);
                                st.select_drag_anchor = Some(image_point);
                                st.drag_start_view = Some(view_point);
                                drop(st);
                                if let Some(area) = drawing_area_begin.upgrade() {
                                    area.queue_draw();
                                }
                                drag_last_redraw_begin.set(glib::monotonic_time());
                                return;
                            }

                            // Body hit — move the whole action.
                            let hit_padding =
                                crate::capture::editor::color::selection_hit_padding_for_scale(t.scale);
                            if crate::capture::editor::selection::action_contains_point_with_padding(
                                selected,
                                image_point,
                                hit_padding,
                            ) {
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                                st.drag_start_view = Some(view_point);
                                drop(st);
                                if let Some(area) = drawing_area_begin.upgrade() {
                                    area.queue_draw();
                                }
                                drag_last_redraw_begin.set(glib::monotonic_time());
                                return;
                            }
                        }
                    }
                }
                // Clicked outside the selected action — deselect, fall through to new draw.
                st.selected_action_index = None;
                st.select_drag_anchor = None;
            }

            // No action selected — check if click lands on an existing matching action.
            if st.selected_action_index.is_none() {
                let hit_padding = crate::capture::editor::color::selection_hit_padding_for_scale(t.scale);
                let hit_index = st
                    .actions
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, action)| {
                        let is_matching_type = match action {
                            crate::capture::editor::types::AnnotationAction::Box { .. } => {
                                st.selected_tool == Tool::Box
                            }
                            crate::capture::editor::types::AnnotationAction::Circle { .. } => {
                                st.selected_tool == Tool::Circle
                            }
                            crate::capture::editor::types::AnnotationAction::Obfuscate { .. } => {
                                st.selected_tool == Tool::Obfuscate
                            }
                            _ => false,
                        };
                        is_matching_type
                            && crate::capture::editor::selection::action_contains_point_with_padding(
                                action,
                                image_point,
                                hit_padding,
                            )
                    })
                    .map(|(index, _)| index);

                if let Some(index) = hit_index {
                    st.selected_action_index = Some(index);
                    // Check resize handles on the newly selected action.
                    let handle_hit_radius =
                        crate::capture::editor::color::selection_handle_hit_radius_for_scale(t.scale);
                    if let Some(handle) =
                        crate::capture::editor::selection::action_resize_handle_at_point_with_radius(
                            &st.actions[index],
                            image_point,
                            handle_hit_radius,
                        )
                    {
                        st.select_resize_handle = Some(handle);
                    } else {
                        st.select_resize_handle = None;
                    }
                    st.select_drag_anchor = Some(image_point);
                    st.drag_start_view = Some(view_point);
                    drop(st);
                    if let Some(area) = drawing_area_begin.upgrade() {
                        area.queue_draw();
                    }
                    drag_last_redraw_begin.set(glib::monotonic_time());
                    return;
                }
            }
            // No hit — fall through to normal draw.
        }

        st.drag_shift_active = shift_pressed;
        st.begin_drag(t.view_to_image_clamped(view_point));
        st.drag_start_view = Some(view_point);
        drop(st);

        if let Some(area) = drawing_area_begin.upgrade() {
            area.queue_draw();
        }
        drag_last_redraw_begin.set(glib::monotonic_time());
    });

    let eyedropper_mode_drag_update = eyedropper_mode.clone();
    let state_drag_update = state.clone();
    let transform_drag_update = transform.clone();
    let drawing_area_update = drawing_area.downgrade();
    let drag_last_redraw_update = drag_last_redraw.clone();
    let space_pan_dragging_update = space_pan_dragging.clone();
    let space_pan_origin_update = space_pan_origin.clone();
    let space_pan_surface_origin_update = space_pan_surface_origin.clone();
    let canvas_scroller_space_pan_update = canvas_scroller.clone();
    let rebuild_effects_async_drag_update = rebuild_effects_async.clone();
    let drag_start_transform_update = drag_start_transform.clone();
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if space_pan_dragging_update.get() {
            let hadj = canvas_scroller_space_pan_update.hadjustment();
            let vadj = canvas_scroller_space_pan_update.vadjustment();
            let (start_x, start_y) = space_pan_origin_update.get();
            // GestureDrag offsets use drawing-area coordinates. Scrolling moves
            // that widget beneath the pointer, feeding the adjustment back into
            // the next offset and causing visible oscillation. GDK event positions
            // are surface-relative, so their delta remains stable while panning.
            let (pan_x, pan_y) = stable_pan_offset(
                space_pan_surface_origin_update.get(),
                gesture.current_event().and_then(|event| event.position()),
                (offset_x, offset_y),
            );
            hadj.set_value((start_x - pan_x).clamp(hadj.lower(), hadj.upper() - hadj.page_size()));
            vadj.set_value((start_y - pan_y).clamp(vadj.lower(), vadj.upper() - vadj.page_size()));
            return;
        }

        if eyedropper_mode_drag_update.get() {
            return;
        }

        let t = drag_start_transform_update
            .borrow()
            .unwrap_or_else(|| *transform_drag_update.lock().unwrap());
        let mut st = state_drag_update.lock().unwrap();

        if st.selected_tool == Tool::Crop {
            if let Some(start_view) = st.drag_start_view {
                let current_view = Point {
                    x: start_view.x + offset_x,
                    y: start_view.y + offset_y,
                };
                let shift_pressed = gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                st.update_crop_drag(t.view_to_image_clamped(current_view), shift_pressed);
            }
            drop(st);
            if let Some(area) = drawing_area_update.upgrade() {
                area.queue_draw();
            }
            return;
        }

        // Arrow control point dragging
        if let Some(handle_idx) = st.arrow_control_dragging {
            let start_view = st.drag_start_view.unwrap_or(Point { x: 0.0, y: 0.0 });
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };
            let image_point = if handle_idx == 1 {
                t.view_to_image(current_view)
            } else {
                t.view_to_image_clamped(current_view)
            };
            st.move_arrow_control_handle(handle_idx, image_point);
            drop(st);
            if let Some(area) = drawing_area_update.upgrade() {
                area.queue_draw();
            }
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        // Text tool handle drag: the motion handler handles updates via raw motion events.
        // Just skip drag_update for handle drags — don't interfere.
        if st.selected_tool == Tool::Text
            && st.active_text_input.is_none()
            && st.active_text_is_dragging
        {
            return;
        }

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.selected_action_index.is_some()
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(st.selected_tool, Tool::Box | Tool::Circle | Tool::Obfuscate)
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some())
            {
                let now = glib::monotonic_time();
                if now - drag_last_redraw_update.get() < DRAG_REDRAW_INTERVAL_US {
                    return;
                }

                let moved = st.update_select_drag(t.view_to_image_clamped(current_view));
                // Check if we moved/resized an effect action (obfuscate/focus).
                // If so, trigger a real-time async rebuild so the effect updates
                // during the drag rather than only on release.
                // Clear the dirty flag here so we don't re-schedule on every
                // drag tick — the coalescing in rebuild_effects_async handles
                // the case where a rebuild is already in-flight.
                let needs_effect_rebuild = st.select_drag_effect_dirty;
                if needs_effect_rebuild {
                    st.select_drag_effect_dirty = false;
                }
                drag_last_redraw_update.set(now);
                drop(st);
                if moved {
                    if needs_effect_rebuild {
                        rebuild_effects_async_drag_update();
                    }
                    if let Some(area) = drawing_area_update.upgrade() {
                        area.queue_draw();
                    }
                }
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number)
                && !(st.selected_tool == Tool::Text
                    && st.selected_action_index.is_some()
                    && st.active_text_input.is_none())
            {
                return;
            }

            if !t.contains_view(current_view) {
                return;
            }

            st.drag_shift_active = shift_pressed;
            st.update_drag(t.view_to_image(current_view));
            drop(st);
            let now = glib::monotonic_time();
            if now - drag_last_redraw_update.get() >= DRAG_REDRAW_INTERVAL_US {
                drag_last_redraw_update.set(now);
                if let Some(area) = drawing_area_update.upgrade() {
                    area.queue_draw();
                }
            }
        }
    });

    let eyedropper_mode_drag_end = eyedropper_mode.clone();
    let state_drag_end = state.clone();
    let transform_drag_end = transform.clone();
    let drawing_area_end = drawing_area.downgrade();
    let drag_last_redraw_end = drag_last_redraw.clone();
    let space_pan_active_end = space_pan_active.clone();
    let space_pan_dragging_end = space_pan_dragging.clone();
    let space_pan_surface_origin_end = space_pan_surface_origin.clone();
    let window_space_pan_end = window.downgrade();
    let sync_size_control_drag_end = sync_size_control.clone();
    let sync_select_inspector_drag_end = sync_select_inspector.clone();
    let rebuild_effects_async_drag_end = rebuild_effects_async.clone();
    let drag_start_transform_end = drag_start_transform.clone();
    drag.connect_drag_end(move |gesture, offset_x, offset_y| {
        let _ = gesture.current_event_state();
        if space_pan_dragging_end.replace(false) {
            space_pan_surface_origin_end.set(None);
            if let Some(window) = window_space_pan_end.upgrade() {
                set_window_cursor_name(
                    &window,
                    if space_pan_active_end.get() {
                        Some("grab")
                    } else {
                        None
                    },
                );
            }
            return;
        }

        if eyedropper_mode_drag_end.get() {
            return;
        }

        let t = drag_start_transform_end
            .borrow()
            .unwrap_or_else(|| *transform_drag_end.lock().unwrap());
        let mut st = state_drag_end.lock().unwrap();

        if st.selected_tool == Tool::Crop {
            if let Some(start_view) = st.drag_start_view {
                let current_view = Point {
                    x: start_view.x + offset_x,
                    y: start_view.y + offset_y,
                };
                let shift_pressed = gesture
                    .current_event_state()
                    .contains(gdk::ModifierType::SHIFT_MASK);
                st.update_crop_drag(t.view_to_image_clamped(current_view), shift_pressed);
            }
            st.end_crop_drag();
            st.drag_start_view = None;
            drop(st);
            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            return;
        }

        // Arrow control point dragging: clear and return
        if st.arrow_control_dragging.is_some() {
            st.finalize_arrow_interaction_cleanup();
            drop(st);
            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            return;
        }

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select
                || (st.selected_tool == Tool::Arrow
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some()
                    && st.arrow_control_dragging.is_none())
                || (st.selected_tool == Tool::Text
                    && st.active_text_input.is_none()
                    && !st.active_text_is_dragging)
                || (matches!(st.selected_tool, Tool::Box | Tool::Circle | Tool::Obfuscate)
                    && st.selected_action_index.is_some()
                    && st.select_drag_anchor.is_some())
            {
                st.update_select_drag(t.view_to_image_clamped(current_view));
                if st.end_select_drag_without_rebuild_and_check_effect() {
                    rebuild_effects_async_drag_end.clone()();
                }
                drop(st);

                sync_size_control_drag_end();
                sync_select_inspector_drag_end();
                if let Some(area) = drawing_area_end.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_end.set(glib::monotonic_time());
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }

            if st.selected_tool == Tool::Arrow
                && st.selected_action_index.is_none()
                && offset_x.hypot(offset_y) < ARROW_CLICK_NOOP_DISTANCE
            {
                st.finalize_arrow_interaction_cleanup();
                drop(st);
                if let Some(area) = drawing_area_end.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_end.set(glib::monotonic_time());
                return;
            }

            if let Some(action) = st.finalize_drag_action() {
                // Check if this action requires async effect rebuild
                let needs_async_rebuild = EditorState::action_requires_effect_rebuild(&action);
                st.push_action(action);
                drop(st);
                if needs_async_rebuild {
                    rebuild_effects_async_drag_end.clone()();
                }
            } else {
                st.clear_drag();
                drop(st); // MUST drop before calling sync_size_control which also locks state
            }

            sync_size_control_drag_end();

            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_end.set(glib::monotonic_time());
        }
    });
    drawing_area.add_controller(drag);
}

#[cfg(test)]
mod tests {
    use super::stable_pan_offset;

    #[test]
    fn space_pan_uses_surface_delta_instead_of_moving_widget_offset() {
        assert_eq!(
            stable_pan_offset(Some((500.0, 300.0)), Some((536.0, 284.0)), (11.0, 9.0)),
            (36.0, -16.0)
        );
    }

    #[test]
    fn space_pan_falls_back_to_gesture_offset_without_pointer_position() {
        assert_eq!(
            stable_pan_offset(Some((500.0, 300.0)), None, (11.0, 9.0)),
            (11.0, 9.0)
        );
    }

    #[test]
    fn canvas_drag_owns_begin_update_end_and_space_pan() {
        let source = include_str!("drag.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("pub(super) fn wire_canvas_drag(")
                && production.contains("drag.connect_drag_begin(move |gesture, x, y| {")
                && production.contains("drag.connect_drag_update(move |gesture, offset_x, offset_y| {")
                && production.contains("drag.connect_drag_end(move |gesture, offset_x, offset_y| {")
                && production.contains("drawing_area.add_controller(drag);")
                && production.contains("if space_pan_active_drag_begin.get() {")
                && production.contains("space_pan_dragging_begin.set(true);")
                && production.contains("DRAG_REDRAW_INTERVAL_US")
                && production.contains("st.end_select_drag_without_rebuild_and_check_effect()")
                && production.contains("st.finalize_drag_action()")
                && production.contains("ARROW_CLICK_NOOP_DISTANCE"),
            "drag.rs must own the full GestureDrag begin/update/end family including Space-pan and finalize"
        );
    }
}
