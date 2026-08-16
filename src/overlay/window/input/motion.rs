//! Overlay motion/leave: hover priority, cursors, popup ownership, slider drags.

use super::super::super::geometry::{
    clamp_point_to_bounds, current_selection_rect, cursor_name_for_handle, detect_resize_handle,
    is_inside_selection,
};
use super::super::super::hit_testing::{capture_crop_menu_hit_item, toolbar_hit_at};
use super::super::super::layout::{
    compute_scroll_popup_layout, compute_volume_popup_layout, compute_window_picker_layout,
    volume_from_pill_y, ToolbarHit,
};
use super::super::super::recording::hit_testing::{
    recording_crop_menu_hit_item, recording_tile_at, settings_dropdown_hit_item,
    settings_menu_hit_item,
};
use super::super::super::state::{OverlayMode, SelectorState};
use super::super::audio::{set_mic_volume, set_speaker_volume};
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea, EventControllerMotion};
use std::sync::{Arc, Mutex};

/// Install motion + leave controllers on the drawing area.
///
/// Preserves hover priority (menus → popups → tiles/toolbar → selection),
/// cursor selection, crosshair updates, GIF/volume slider ownership while
/// dragging, and leave/reset of hover state.
pub(in crate::overlay::window) fn wire_selection_motion(
    window: &ApplicationWindow,
    state: Arc<Mutex<SelectorState>>,
    drawing_area: &DrawingArea,
    screen_width: i32,
    screen_height: i32,
) {
    let motion_controller = EventControllerMotion::new();
    let state_motion = state.clone();
    let drawing_area_weak_motion = drawing_area.downgrade();
    let window_weak_motion = window.downgrade();
    motion_controller.connect_motion(move |_, x, y| {
        let (cursor_name, hover_changed, _done) = {
            let mut st = state_motion.lock().unwrap();
            if st.overlay_mode == OverlayMode::CrosshairCapture {
                let (x, y) = clamp_point_to_bounds(x, y, screen_width as f64, screen_height as f64);
                st.current_x = x;
                st.current_y = y;
                drop(st);
                if let Some(da) = drawing_area_weak_motion.upgrade() {
                    da.queue_draw();
                }
                ("crosshair".to_string(), false, true)
            } else {
                let rect = current_selection_rect(&st);

                // GIF slider dragging — update value from X position
                if let Some(slider) = st.recording.gif_slider_dragging {
                    if st.recording.settings_menu_open {
                        let menu_x = (rect.left + (rect.width() - 440.0) / 2.0)
                            .clamp(10.0, screen_width as f64 - 450.0);
                        if slider == 0 {
                            let slider_x = menu_x + 200.0;
                            let slider_w = 208.0;
                            let click_x = x.clamp(slider_x, slider_x + slider_w);
                            st.recording.gif_fps = 5.0 + (click_x - slider_x) / slider_w * 55.0;
                        } else {
                            let slider_x = menu_x + 160.0;
                            let q_slider_w = 248.0;
                            let click_x = x.clamp(slider_x, slider_x + q_slider_w);
                            st.recording.gif_quality =
                                ((click_x - slider_x) / q_slider_w).clamp(0.0, 1.0);
                        }
                    }
                    st.recording.hovered_settings_item = -1;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    drop(st);
                    if let Some(da) = drawing_area_weak_motion.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }

                // Volume slider dragging
                if st.recording.volume_slider_dragging
                    && (st.recording.mic_volume_popup_open
                        || st.recording.speaker_volume_popup_open)
                {
                    let vol = compute_volume_popup_layout(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                    );
                    let volume = volume_from_pill_y(vol.panel, y);
                    let should_write = st
                        .recording
                        .last_volume_system_write
                        .is_none_or(|last| last.elapsed() >= std::time::Duration::from_millis(75));
                    if should_write {
                        st.recording.last_volume_system_write = Some(std::time::Instant::now());
                    }
                    if st.recording.mic_volume_popup_open {
                        st.recording.mic_volume = volume;
                        if should_write {
                            set_mic_volume(volume);
                        }
                    } else {
                        st.recording.speaker_volume = volume;
                        if should_write {
                            set_speaker_volume(volume);
                        }
                    }
                    drop(st);
                    if let Some(da) = drawing_area_weak_motion.upgrade() {
                        da.queue_draw();
                    }
                    return;
                }

                // Capture crop menu hover check
                if st.capture_crop_menu_open {
                    let item = capture_crop_menu_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.map(|i| i as i32).unwrap_or(-1);
                    let changed = next != st.hovered_capture_crop_menu_item;
                    if changed {
                        st.hovered_capture_crop_menu_item = next;
                    }
                    st.recording.hovered_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;
                    // Clear other hovers
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.recording.crop_menu_open {
                    let item = recording_crop_menu_hit_item(
                        rect.left,
                        rect.top,
                        rect.width(),
                        rect.height(),
                        screen_width as f64,
                        screen_height as f64,
                        x,
                        y,
                    );
                    let next = item.map(|i| i as i32).unwrap_or(-1);
                    let changed = next != st.recording.hovered_crop_menu_item;
                    if changed {
                        st.recording.hovered_crop_menu_item = next;
                    }
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.recording.settings_menu_open {
                    if let Some(drop_idx) = st.recording.settings_dropdown_open {
                        let next = settings_dropdown_hit_item(
                            rect.left,
                            rect.top,
                            rect.width(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                            st.recording.settings_tab,
                            drop_idx,
                        )
                        .map(|index| index as i32)
                        .unwrap_or(-1);
                        let changed = next != st.recording.hovered_settings_dropdown_item;
                        st.recording.hovered_settings_dropdown_item = next;
                        st.recording.hovered_settings_item = -1;
                        st.hovered_capture_crop_menu_item = -1;
                        st.recording.hovered_crop_menu_item = -1;
                        st.hover_tool_index = None;
                        st.hover_size_panel = false;
                        st.hover_crop_panel = false;
                        st.recording.hover_record_tile = None;
                        ("pointer".to_string(), changed, true)
                    } else {
                        let item = settings_menu_hit_item(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                            st.recording.settings_tab,
                        );
                        let next = item.unwrap_or(-1);
                        let changed = next != st.recording.hovered_settings_item;
                        if changed {
                            st.recording.hovered_settings_item = next;
                        }
                        st.recording.hovered_settings_dropdown_item = -1;
                        st.hovered_capture_crop_menu_item = -1;
                        st.recording.hovered_crop_menu_item = -1;
                        st.hover_tool_index = None;
                        st.hover_size_panel = false;
                        st.hover_crop_panel = false;
                        st.recording.hover_record_tile = None;
                        ("pointer".to_string(), changed, true)
                    }
                } else if st.window_picker_open {
                    let n = st.windows.len();
                    let (center_x, center_y) = if st.completed || st.is_dragging {
                        let r = current_selection_rect(&st);
                        (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
                    } else {
                        (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
                    };
                    let picker = compute_window_picker_layout(
                        center_x,
                        center_y,
                        screen_width as f64,
                        screen_height as f64,
                        n,
                    );

                    let mut next_entry = -1;
                    if picker.panel.contains(x, y) && y >= picker.list_y {
                        let idx = ((y - picker.list_y) / picker.item_h) as i32;
                        if idx >= 0 && (idx as usize) < st.windows.len() {
                            next_entry = idx;
                        }
                    }

                    let changed = st.hovered_window_picker_entry != next_entry;
                    st.hovered_window_picker_entry = next_entry;
                    st.hovered_scroll_popup_close = false;
                    st.hovered_scroll_download = false;
                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), changed, true)
                } else if st.scroll_popup_open {
                    // Scroll popup hover handling
                    let (cx, cy) = if st.completed || st.is_dragging {
                        let r = current_selection_rect(&st);
                        (r.left + r.width() / 2.0, r.top + r.height() / 2.0)
                    } else {
                        (screen_width as f64 / 2.0, screen_height as f64 / 2.0)
                    };
                    let scroll = compute_scroll_popup_layout(cx, cy);

                    let next_hover_close = scroll.close.contains(x, y);
                    let changed_close = st.hovered_scroll_popup_close != next_hover_close;
                    if changed_close {
                        st.hovered_scroll_popup_close = next_hover_close;
                    }

                    // Download button hover
                    let next_hover_download = scroll.download.contains(x, y);
                    let changed_download = st.hovered_scroll_download != next_hover_download;
                    if changed_download {
                        st.hovered_scroll_download = next_hover_download;
                    }

                    st.hover_tool_index = None;
                    st.hover_size_panel = false;
                    st.hover_crop_panel = false;
                    st.recording.hover_record_tile = None;
                    ("pointer".to_string(), true, true)
                } else {
                    let record_hit = if st.recording.panel_open {
                        recording_tile_at(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                        )
                    } else {
                        None
                    };
                    let hit = if st.recording.panel_open {
                        None
                    } else {
                        toolbar_hit_at(
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            screen_width as f64,
                            screen_height as f64,
                            x,
                            y,
                        )
                    };

                    let mut next_hovered_window = None;
                    if !st.completed && !st.is_dragging && hit.is_none() && record_hit.is_none() {
                        for (i, win) in st.windows.iter().enumerate() {
                            if x >= win.x as f64
                                && x <= (win.x + win.width) as f64
                                && y >= win.y as f64
                                && y <= (win.y + win.height) as f64
                            {
                                next_hovered_window = Some(i);
                                break;
                            }
                        }
                    }

                    let (
                        next_hover_tool_index,
                        next_hover_size_panel,
                        next_hover_crop_panel,
                        next_hover_record_tile,
                        cursor_name,
                    ) = match hit {
                        Some(ToolbarHit::Tool(index)) if !st.recording.panel_open => {
                            (Some(index), false, false, None, "pointer")
                        }
                        Some(ToolbarHit::SizePanel) if !st.recording.panel_open => {
                            (None, true, false, None, "default")
                        }
                        Some(ToolbarHit::CropPanel) if !st.recording.panel_open => {
                            (None, false, true, None, "pointer")
                        }
                        None => {
                            if let Some(tile) = record_hit {
                                (None, false, false, Some(tile), "pointer")
                            } else {
                                let c = if st.completed || st.is_dragging {
                                    if st.recording.panel_open {
                                        "fleur"
                                    } else {
                                        detect_resize_handle(x, y, rect)
                                            .map(cursor_name_for_handle)
                                            .unwrap_or_else(|| {
                                                if is_inside_selection(x, y, rect) {
                                                    "fleur"
                                                } else {
                                                    "crosshair"
                                                }
                                            })
                                    }
                                } else if next_hovered_window.is_some() {
                                    "pointer"
                                } else {
                                    "crosshair"
                                };
                                (None, false, false, None, c)
                            }
                        }
                        _ => (None, false, false, None, "crosshair"),
                    };

                    let hover_changed = st.hover_tool_index != next_hover_tool_index
                        || st.hover_size_panel != next_hover_size_panel
                        || st.hover_crop_panel != next_hover_crop_panel
                        || st.recording.hover_record_tile != next_hover_record_tile
                        || st.hovered_window != next_hovered_window;

                    st.hover_tool_index = next_hover_tool_index;
                    st.hover_size_panel = next_hover_size_panel;
                    st.hover_crop_panel = next_hover_crop_panel;
                    st.recording.hover_record_tile = next_hover_record_tile;
                    st.hovered_window = next_hovered_window;
                    st.hovered_capture_crop_menu_item = -1;
                    st.recording.hovered_crop_menu_item = -1;
                    st.recording.hovered_settings_item = -1;
                    st.recording.hovered_settings_dropdown_item = -1;

                    (cursor_name.to_string(), hover_changed, false)
                }
            }
        };

        if let Some(win) = window_weak_motion.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name(&cursor_name, None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if hover_changed {
            if let Some(drawing_area) = drawing_area_weak_motion.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    let state_motion_leave = state;
    let drawing_area_weak_leave = drawing_area.downgrade();
    let window_weak_leave = window.downgrade();
    motion_controller.connect_leave(move |_| {
        let mut st = state_motion_leave.lock().unwrap();
        let was_hovering = st.hover_tool_index.is_some()
            || st.hover_size_panel
            || st.hover_crop_panel
            || st.recording.hover_record_tile.is_some()
            || st.hovered_capture_crop_menu_item != -1
            || st.recording.hovered_crop_menu_item != -1
            || st.recording.hovered_settings_item != -1
            || st.recording.hovered_settings_dropdown_item != -1;
        st.hover_tool_index = None;
        st.hover_size_panel = false;
        st.hover_crop_panel = false;
        st.recording.hover_record_tile = None;
        st.hovered_capture_crop_menu_item = -1;
        st.recording.hovered_crop_menu_item = -1;
        st.recording.hovered_settings_item = -1;
        st.recording.hovered_settings_dropdown_item = -1;
        drop(st);

        // Reset cursor
        if let Some(win) = window_weak_leave.upgrade() {
            if let Some(surf) = win.surface() {
                let cursor = gdk::Cursor::from_name("crosshair", None);
                surf.set_cursor(cursor.as_ref());
            }
        }
        if was_hovering {
            if let Some(drawing_area) = drawing_area_weak_leave.upgrade() {
                drawing_area.queue_draw();
            }
        }
    });

    drawing_area.add_controller(motion_controller);
}

#[cfg(test)]
mod tests {
    #[test]
    fn motion_owner_covers_hover_priority_cursors_and_leave() {
        let source = include_str!("motion.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production motion source");
        assert!(
            production.contains("fn wire_selection_motion"),
            "motion must own wire_selection_motion"
        );
        assert!(
            production.contains("connect_motion") && production.contains("connect_leave"),
            "motion must own motion + leave together"
        );
        assert!(
            production.contains("gif_slider_dragging")
                && production.contains("volume_slider_dragging"),
            "motion must own active slider drags"
        );
        assert!(
            production.contains("capture_crop_menu_open")
                && production.contains("window_picker_open")
                && production.contains("scroll_popup_open")
                && production.contains("settings_menu_open"),
            "motion must keep popup hover priority"
        );
        assert!(
            production.contains("cursor_name_for_handle")
                && production.contains("crosshair")
                && production.contains("fleur"),
            "motion must keep cursor selection"
        );
        assert!(
            production.contains("hover_tool_index = None")
                && production.contains("from_name(\"crosshair\""),
            "leave must reset hover state and crosshair cursor"
        );
    }
}
