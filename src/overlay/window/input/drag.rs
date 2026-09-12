//! Selection drag gesture: new/move/resize with aspect ratio and surface suppression.

use super::super::super::api::SelectionResult;
use super::super::super::background::BackgroundFrame;
use super::super::super::geometry::{
    active_aspect_ratio, apply_aspect_to_selection, clamp_point_to_bounds, current_selection_rect,
    detect_resize_handle, is_inside_selection, update_selection_for_drag,
};
use super::super::super::hit_testing::{toolbar_hit_at, toolbar_item_at};
use super::super::super::icons::TOOLBAR_AREA_INDEX;
use super::super::super::layout::ToolbarHit;
use super::super::super::recording::hit_testing::recording_tile_at;
use super::super::super::state::{DragMode, OverlayMode, SelectorState};
use super::super::audio::{set_mic_volume, set_speaker_volume};
use super::super::result::send_selection_result;
use gtk4::glib::clone;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea, GestureDrag};
use std::sync::{Arc, Mutex};

/// Install capture-phase `GestureDrag` for area selection on the drawing area.
///
/// Preserves offset semantics, fixed-aspect resize (non-move), toolbar/menu
/// surface suppression, slider-drag pass-through, lock release before result
/// delivery or window closure, and crosshair finalize → `send_selection_result`.
pub(in crate::overlay::window) fn wire_selection_drag(
    window: &ApplicationWindow,
    state: Arc<Mutex<SelectorState>>,
    result_tx: std::sync::mpsc::Sender<SelectionResult>,
    drawing_area: &DrawingArea,
    background: Option<BackgroundFrame>,
    screen_width: i32,
    screen_height: i32,
) {
    // Setup drag gesture for area selection
    let drag_gesture = GestureDrag::builder()
        .propagation_phase(gtk4::PropagationPhase::Capture)
        .build();

    let state_drag = state.clone();
    let drawing_area_weak = drawing_area.downgrade();
    let result_tx_drag = result_tx;
    let window_weak_drag = window.downgrade();
    let background_drag = background;

    // Note: connect_drag_begin takes 3 params (gesture, x, y)
    drag_gesture.connect_drag_begin(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            let (start_x, start_y) =
                clamp_point_to_bounds(x, y, screen_width as f64, screen_height as f64);

            if st.overlay_mode == OverlayMode::CrosshairCapture {
                st.drag_origin_x = start_x;
                st.drag_origin_y = start_y;
                st.start_x = start_x;
                st.start_y = start_y;
                st.current_x = start_x;
                st.current_y = start_y;
                st.drag_mode = Some(DragMode::NewSelection);
                st.initial_rect = None;
                st.is_dragging = true;
                st.completed = false;
                st.active_tool_index = TOOLBAR_AREA_INDEX;
                drop(st);

                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }

            let rect = current_selection_rect(&st);

            // Suppress drag when clicking toolbar tools, size/crop panels
            if !st.capture_menu_area_mode
                && toolbar_item_at(
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                    screen_width as f64,
                    screen_height as f64,
                    start_x,
                    start_y,
                )
                .is_some()
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            let hit = toolbar_hit_at(
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                screen_width as f64,
                screen_height as f64,
                start_x,
                start_y,
            );
            if matches!(
                hit,
                Some(ToolbarHit::CropPanel) | Some(ToolbarHit::SizePanel)
            ) {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Suppress drag when clicking recording panel tiles
            if st.recording.panel_open
                && recording_tile_at(
                    rect.left,
                    rect.top,
                    rect.width(),
                    rect.height(),
                    screen_width as f64,
                    screen_height as f64,
                    start_x,
                    start_y,
                )
                .is_some()
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            // Any open menu owns this pointer press. The click handler may
            // close the menu or update a slider, but area move/resize/new
            // selection must not also start underneath it.
            if st.capture_crop_menu_open
                || st.recording.crop_menu_open
                || st.recording.settings_menu_open
                || st.recording.mic_volume_popup_open
                || st.recording.speaker_volume_popup_open
            {
                st.is_dragging = false;
                st.drag_mode = None;
                st.initial_rect = None;
                drop(st);
                return;
            }

            st.drag_origin_x = start_x;
            st.drag_origin_y = start_y;
            st.initial_rect = Some(current_selection_rect(&st));

            let drag_mode = if st.completed {
                let rect = current_selection_rect(&st);
                if let Some(handle) = detect_resize_handle(start_x, start_y, rect) {
                    // Cursor is on a border/corner handle — resize.
                    DragMode::Resize(handle)
                } else if is_inside_selection(start_x, start_y, rect) {
                    // Cursor is inside the selection — move the whole rect.
                    DragMode::Move
                } else {
                    // Cursor is outside the selection — start a new one.
                    DragMode::NewSelection
                }
            } else {
                DragMode::NewSelection
            };

            st.drag_mode = Some(drag_mode);

            if matches!(drag_mode, DragMode::NewSelection) {
                if let Some(win_idx) = st.hovered_window {
                    let win = &st.windows[win_idx];
                    let (wx, wy, ww, wh) = (
                        win.x as f64,
                        win.y as f64,
                        win.width as f64,
                        win.height as f64,
                    );
                    st.start_x = wx;
                    st.start_y = wy;
                    st.current_x = wx + ww;
                    st.current_y = wy + wh;
                    st.completed = true;
                    st.is_dragging = false;
                    st.drag_mode = None;
                } else {
                    st.start_x = start_x;
                    st.start_y = start_y;
                    st.current_x = start_x;
                    st.current_y = start_y;
                    st.completed = false;
                    st.is_dragging = true;
                }
                st.fullscreen_mode = false;
                if !st.recording.panel_open {
                    st.active_tool_index = TOOLBAR_AREA_INDEX;
                }
            } else {
                st.is_dragging = true;
            }
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_update(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.recording.gif_slider_dragging.is_some() || st.recording.volume_slider_dragging {
                drop(st);
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            let ratio = active_aspect_ratio(&st);
            if ratio > 0.0 && !matches!(st.drag_mode, Some(DragMode::Move)) {
                apply_aspect_to_selection(
                    &mut st,
                    ratio,
                    screen_width as f64,
                    screen_height as f64,
                );
            }
            drop(st);

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drag_gesture.connect_drag_end(clone!(
        #[strong]
        state_drag,
        #[strong]
        drawing_area_weak,
        #[strong]
        result_tx_drag,
        #[strong]
        window_weak_drag,
        #[strong]
        background_drag,
        move |_gesture, x, y| {
            let mut st = state_drag.lock().unwrap();
            if st.recording.gif_slider_dragging.is_some() || st.recording.volume_slider_dragging {
                let final_volume = if st.recording.volume_slider_dragging {
                    if st.recording.mic_volume_popup_open {
                        Some((true, st.recording.mic_volume))
                    } else if st.recording.speaker_volume_popup_open {
                        Some((false, st.recording.speaker_volume))
                    } else {
                        None
                    }
                } else {
                    None
                };
                st.recording.gif_slider_dragging = None;
                st.recording.volume_slider_dragging = false;
                st.recording.last_volume_system_write = None;
                drop(st);
                if let Some((microphone, volume)) = final_volume {
                    if microphone {
                        set_mic_volume(volume);
                    } else {
                        set_speaker_volume(volume);
                    }
                }
                if let Some(drawing_area) = drawing_area_weak.upgrade() {
                    drawing_area.queue_draw();
                }
                return;
            }
            update_selection_for_drag(&mut st, x, y, screen_width as f64, screen_height as f64);
            let ratio = active_aspect_ratio(&st);
            if ratio > 0.0 && !matches!(st.drag_mode, Some(DragMode::Move)) {
                apply_aspect_to_selection(
                    &mut st,
                    ratio,
                    screen_width as f64,
                    screen_height as f64,
                );
            }
            st.is_dragging = false;
            st.completed = true;
            st.drag_mode = None;
            st.initial_rect = None;
            let is_crosshair = st.overlay_mode == OverlayMode::CrosshairCapture;
            drop(st);

            if is_crosshair {
                if let Some(window) = window_weak_drag.upgrade() {
                    send_selection_result(
                        &state_drag,
                        &result_tx_drag,
                        &window,
                        screen_width,
                        screen_height,
                        background_drag.as_ref(),
                    );
                }
                return;
            }

            if let Some(drawing_area) = drawing_area_weak.upgrade() {
                drawing_area.queue_draw();
            }
        }
    ));

    drawing_area.add_controller(drag_gesture);
}

#[cfg(test)]
mod tests {
    #[test]
    fn drag_owner_covers_begin_update_end_and_suppression() {
        let source = include_str!("drag.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production drag source");
        assert!(
            production.contains("fn wire_selection_drag"),
            "drag must own wire_selection_drag"
        );
        assert!(
            production.contains("connect_drag_begin")
                && production.contains("connect_drag_update")
                && production.contains("connect_drag_end"),
            "drag must own the full GestureDrag family"
        );
        assert!(
            production.contains("PropagationPhase::Capture"),
            "drag controller must stay capture-phase"
        );
        assert!(
            production.contains("toolbar_item_at")
                && production.contains("recording_tile_at")
                && production.contains("settings_menu_open"),
            "drag must suppress under toolbar/tiles/menus"
        );
        assert!(
            production.contains("apply_aspect_to_selection")
                && production.contains("DragMode::Move"),
            "drag must keep fixed-aspect resize (non-move)"
        );
        assert!(
            production.contains("send_selection_result") && production.contains("CrosshairCapture"),
            "crosshair drag end must deliver selection after releasing the lock"
        );
        assert!(
            production.contains("volume_slider_dragging")
                && production.contains("gif_slider_dragging"),
            "drag must pass through active slider drags"
        );
    }
}
