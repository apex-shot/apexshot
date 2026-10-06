//! Window keyboard: Space-pan capture, text input, undo/redo, zoom, tools, delete (PR 10.18).
//!
//! Owns capture-phase Space pan and the bubble-phase window key controller.
//! Canvas Escape for text-edit cancel lives in `click.rs`.

use gtk4::{
    gdk, glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, DrawingArea,
    EventControllerKey,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    state::EditorState,
    types::{tool_shortcut_target, Point, Tool},
    ui_support::{set_active_tool_button, text_input_has_focus},
};

use super::super::cursor::set_window_cursor_name;
use super::zoom::ZOOM_STEP;

/// Wire capture-phase Space pan and window keyboard shortcuts.
pub(super) fn wire_window_keyboard(
    window: &ApplicationWindow,
    in_motion: &Rc<Cell<bool>>,
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
    tool_buttons: &[Button],
    crop_apply_btn: &Button,
    crop_cancel_btn: &Button,
    sync_motion_crop_history: &Rc<dyn Fn()>,
    space_pan_active: &Rc<Cell<bool>>,
    space_pan_dragging: &Rc<Cell<bool>>,
    eyedropper_mode: &Rc<Cell<bool>>,
    eyedropper_point: &Rc<RefCell<Option<Point>>>,
    eyedropper_rendered: &Rc<RefCell<Option<RgbaImage>>>,
    canvas_eyedropper_ring: &DrawingArea,
    zoom_level: &Rc<Cell<f64>>,
    apply_zoom_change: &Rc<dyn Fn(f64)>,
    zoom_popup: &GtkBox,
    update_toolbar_for_tool: &Rc<dyn Fn(Tool)>,
    sync_picker_for_active_tool: &Rc<dyn Fn()>,
    sync_select_inspector: &Rc<dyn Fn()>,
) {
    let tool_buttons = tool_buttons.to_vec();
    let crop_apply_btn = crop_apply_btn.clone();
    let crop_cancel_btn = crop_cancel_btn.clone();
    let sync_motion_crop_history = sync_motion_crop_history.clone();
    // Capture-phase Space handler: tool/chrome buttons are often focusable and
    // would activate on Space in the bubble phase, which breaks hand-pan after
    // the first tool click. Capture runs before the focused widget.
    let space_pan_controller = EventControllerKey::new();
    space_pan_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let space_pan_active_capture = space_pan_active.clone();
    let space_pan_dragging_capture = space_pan_dragging.clone();
    let eyedropper_mode_space = eyedropper_mode.clone();
    let state_space = state.clone();
    let in_motion_space = in_motion.clone();
    let window_space = window.downgrade();
    let drawing_area_space = drawing_area.downgrade();
    space_pan_controller.connect_key_pressed(move |_, key, _, _| {
        if in_motion_space.get() {
            return glib::Propagation::Proceed;
        }
        if key != gdk::Key::space || eyedropper_mode_space.get() {
            return glib::Propagation::Proceed;
        }

        // Canvas text editing and GTK entries still need Space as a character.
        if state_space.lock().unwrap().active_text_input.is_some() {
            return glib::Propagation::Proceed;
        }
        if let Some(window) = window_space.upgrade() {
            if text_input_has_focus(&window) {
                return glib::Propagation::Proceed;
            }
        }

        space_pan_active_capture.set(true);
        if let Some(window) = window_space.upgrade() {
            set_window_cursor_name(
                &window,
                Some(if space_pan_dragging_capture.get() {
                    "grabbing"
                } else {
                    "grab"
                }),
            );
        }
        if let Some(area) = drawing_area_space.upgrade() {
            area.grab_focus();
        }
        glib::Propagation::Stop
    });
    let space_pan_active_released = space_pan_active.clone();
    let space_pan_dragging_released = space_pan_dragging.clone();
    let eyedropper_mode_released = eyedropper_mode.clone();
    let window_released = window.downgrade();
    space_pan_controller.connect_key_released(move |_, key, _, _| {
        if key != gdk::Key::space || !space_pan_active_released.get() {
            return;
        }

        space_pan_active_released.set(false);
        if !space_pan_dragging_released.get() {
            if let Some(window) = window_released.upgrade() {
                set_window_cursor_name(
                    &window,
                    if eyedropper_mode_released.get() {
                        Some("crosshair")
                    } else {
                        None
                    },
                );
            }
        }
    });
    window.add_controller(space_pan_controller);

    let key_controller = EventControllerKey::new();
    let state_keys = state.clone();
    let drawing_area_keys = drawing_area.downgrade();
    let tool_buttons_keys = tool_buttons.clone();
    let update_toolbar_for_tool_keys = update_toolbar_for_tool.clone();
    let sync_picker_for_active_tool_keys = sync_picker_for_active_tool.clone();
    let sync_select_inspector_keys = sync_select_inspector.clone();
    let eyedropper_mode_keys = eyedropper_mode.clone();
    let eyedropper_point_keys = eyedropper_point.clone();
    let eyedropper_rendered_keys = eyedropper_rendered.clone();
    let canvas_eyedropper_ring_keys = canvas_eyedropper_ring.clone();
    let window_keys = window.downgrade();

    let zoom_level_keys = zoom_level.clone();
    let apply_zoom_change_keys = apply_zoom_change.clone();
    let zoom_popup_keys = zoom_popup.clone();
    let in_motion_keys = in_motion.clone();

    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
        if in_motion_keys.get() {
            return glib::Propagation::Proceed;
        }
        if window_keys
            .upgrade()
            .is_some_and(|window| text_input_has_focus(&window))
        {
            return glib::Propagation::Proceed;
        }
        if key == gdk::Key::Escape && eyedropper_mode_keys.get() {
            eyedropper_mode_keys.set(false);
            *eyedropper_point_keys.borrow_mut() = None;
            *eyedropper_rendered_keys.borrow_mut() = None;
            canvas_eyedropper_ring_keys.set_visible(false);
            if let Some(window) = window_keys.upgrade() {
                set_window_cursor_name(&window, None);
            }
            return glib::Propagation::Stop;
        }

        if state_keys.lock().unwrap().selected_tool == Tool::Crop {
            if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
                crop_apply_btn.emit_clicked();
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape {
                crop_cancel_btn.emit_clicked();
                return glib::Propagation::Stop;
            }
        }

        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        let pressed = key.to_unicode();

        {
            let mut st = state_keys.lock().unwrap();
            if st.active_text_input.is_some() {
                let mut should_cancel = false;
                let mut handled = true;

                match key {
                    gdk::Key::Escape => should_cancel = true,
                    gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\n'),
                    gdk::Key::BackSpace => st.delete_text_input_char(),
                    gdk::Key::space => st.add_text_input_char(' '),
                    gdk::Key::Left => st.move_cursor_left(),
                    gdk::Key::Right => st.move_cursor_right(),
                    _ => {
                        if !ctrl {
                            if let Some(ch) = pressed {
                                if !ch.is_control() {
                                    st.add_text_input_char(ch);
                                } else {
                                    handled = false;
                                }
                            } else {
                                handled = false;
                            }
                        } else {
                            handled = false;
                        }
                    }
                }

                if should_cancel {
                    st.cancel_text_input();
                }

                if handled && st.active_text_input.is_some() {
                    st.fit_active_text_to_layout();
                    st.reset_text_cursor_blink();
                }

                if handled || should_cancel {
                    drop(st);
                    if let Some(area) = drawing_area_keys.upgrade() {
                        area.queue_draw();
                    }
                    return glib::Propagation::Stop;
                }
            }
        }

        if ctrl && (pressed == Some('z') || pressed == Some('Z')) {
            let changed = if shift {
                state_keys.lock().unwrap().redo()
            } else {
                state_keys.lock().unwrap().undo()
            };
            if changed {
                sync_motion_crop_history();
                sync_select_inspector_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if ctrl && (pressed == Some('y') || pressed == Some('Y')) {
            if state_keys.lock().unwrap().redo() {
                sync_motion_crop_history();
                sync_select_inspector_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if ctrl {
            let mut handled = false;
            match key {
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                    apply_zoom_change_keys(zoom_level_keys.get() * ZOOM_STEP);
                    handled = true;
                }
                gdk::Key::minus | gdk::Key::underscore | gdk::Key::KP_Subtract => {
                    apply_zoom_change_keys(zoom_level_keys.get() / ZOOM_STEP);
                    handled = true;
                }
                gdk::Key::_0 | gdk::Key::KP_0 => {
                    apply_zoom_change_keys(1.0);
                    handled = true;
                }
                gdk::Key::_2 | gdk::Key::KP_2 => {
                    // Ctrl+2 zooms to 150% (shared apply_zoom_change path).
                    apply_zoom_change_keys(1.5);
                    handled = true;
                }
                _ => {}
            }

            if handled {
                zoom_popup_keys.set_visible(false);
                return glib::Propagation::Stop;
            }
        }

        if !ctrl {
            if let Some((tool, active_button)) = pressed.and_then(tool_shortcut_target) {
                set_active_tool_button(&tool_buttons_keys, active_button);
                {
                    let mut st = state_keys.lock().unwrap();
                    st.set_tool(tool);
                };
                update_toolbar_for_tool_keys(tool);
                sync_select_inspector_keys();
                sync_picker_for_active_tool_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }
        }

        if (key == gdk::Key::Delete || key == gdk::Key::BackSpace)
            && state_keys.lock().unwrap().remove_selected_action()
        {
            sync_select_inspector_keys();
            if let Some(area) = drawing_area_keys.upgrade() {
                area.queue_draw();
            }
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_fields_are_exempt_from_capture_and_editor_shortcuts() {
        let source = include_str!("keyboard.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        let capture = production
            .split("space_pan_controller.connect_key_pressed")
            .nth(1)
            .unwrap();
        assert!(
            capture.find("text_input_has_focus(&window)").unwrap()
                < capture.find("space_pan_active_capture.set(true)").unwrap()
        );
        let bubble = production
            .split("key_controller.connect_key_pressed")
            .nth(1)
            .unwrap();
        assert!(
            bubble.find("text_input_has_focus(&window)").unwrap()
                < bubble.find("st.active_text_input.is_some()").unwrap()
        );
        assert!(production.contains("!space_pan_active_released.get()"));
    }

    #[test]
    fn window_keyboard_owns_space_pan_text_zoom_and_shortcuts() {
        let source = include_str!("keyboard.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("pub(super) fn wire_window_keyboard(")
                && production.contains("space_pan_controller.set_propagation_phase(gtk4::PropagationPhase::Capture)")
                && production.contains("if key != gdk::Key::space || eyedropper_mode_space.get()")
                && production.contains("window.add_controller(space_pan_controller);")
                && production.contains("let key_controller = EventControllerKey::new();")
                && production.contains("gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\\n'),")
                && production.contains("apply_zoom_change_keys(1.5);")
                && production.contains("gdk::Key::_2 | gdk::Key::KP_2 => {")
                && production.contains("tool_shortcut_target")
                && production.contains("window.add_controller(key_controller);"),
            "keyboard.rs must own Space capture, text input, zoom shortcuts including Ctrl+2, and tool shortcuts"
        );
    }
}
