//! Canvas click press/release, canvas Escape, and text/number/eyedropper placement (PR 10.17).
//!
//! Owns drawing-area Escape (cancel text edit), primary-button click begin/end,
//! text create/re-edit/caret placement, eyedropper completion, number placement,
//! and text-handle release. Motion and window keyboard stay in sibling modules.

use gtk4::{
    gdk, glib, prelude::*, ApplicationWindow, Box as GtkBox, Button, DrawingArea,
    EventControllerKey, GestureClick, Label,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    color::palette_index_for_color,
    render::cursor_position_for_text_point,
    state::EditorState,
    types::{
        ArrowStyle, DrawColor, FontSettings, FontStyle, Point, TextAlignment, TextDecoration, Tool,
        ViewTransform,
    },
};

use super::super::{
    canvas::{sample_editor_color_at_point, sample_rendered_color_at_point},
    color_picker,
    cursor::set_window_cursor_name,
};
use super::{MOVE_HANDLE_DRAG_RADIUS, RESIZE_HANDLE_DRAG_SIZE};

const TEXT_SIZE_OPTIONS: [i32; 12] = [12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72];
const TEXT_FONT_FAMILIES: [&str; 5] = ["Sans", "Serif", "Monospace", "Fantasy", "Cursive"];

fn sync_text_option_selection(list: &GtkBox, selected_index: Option<usize>) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        let is_active = selected_index == Some(index);
        if is_active {
            button.add_css_class("editor-text-inspector-option-active");
        } else {
            button.remove_css_class("editor-text-inspector-option-active");
        }

        if let Some(content) = button.child() {
            if let Ok(row) = content.downcast::<GtkBox>() {
                if let Some(check_icon) = row.last_child() {
                    if let Ok(widget) = check_icon.downcast::<gtk4::Widget>() {
                        widget.set_visible(is_active);
                    }
                }
            }
        }

        index += 1;
    }
}

/// Wire canvas Escape + primary click press/release controllers on the drawing area.
pub(super) fn wire_canvas_click(
    window: &ApplicationWindow,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    drawing_area: &DrawingArea,
    color_buttons: &[Button],
    color_picker_dot: &GtkBox,
    color_class_names: &[&'static str],
    space_pan_active: &Rc<Cell<bool>>,
    eyedropper_mode: &Rc<Cell<bool>>,
    eyedropper_from_sidebar: &Rc<Cell<bool>>,
    eyedropper_point: &Rc<RefCell<Option<Point>>>,
    eyedropper_rendered: &Rc<RefCell<Option<RgbaImage>>>,
    canvas_eyedropper_ring: &DrawingArea,
    set_picker_panel_visibility: &Rc<dyn Fn(bool)>,
    apply_picker_color_to_editor: &Rc<dyn Fn(DrawColor)>,
    sync_picker_from_color: &Rc<dyn Fn(DrawColor)>,
    add_color_to_custom_slots: &Rc<dyn Fn(DrawColor)>,
    sync_size_control: &Rc<dyn Fn()>,
    text_size_label: &Label,
    font_family_label: &Label,
    text_size_list: &gtk4::Box,
    font_family_list: &gtk4::Box,
    sync_select_inspector: &Rc<dyn Fn()>,
) {
    // Clone collections once for closures that need owned Vecs.
    let color_buttons = color_buttons.to_vec();
    let color_class_names = color_class_names.to_vec();
    let key_controller = EventControllerKey::new();
    let state_key = state.clone();
    let drawing_area_key = drawing_area.downgrade();

    key_controller.connect_key_pressed(move |_, key, _, _| {
        let keyval = key;

        if keyval == gdk::Key::Escape {
            let has_active_edit = state_key.lock().unwrap().active_text_bounds.is_some();
            if has_active_edit {
                state_key.lock().unwrap().cancel_text_edit();
                if let Some(area) = drawing_area_key.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }

            // Escape drops the selected number marker so the floating bar returns to
            // its "next marker" state instead of editing that marker.
            let deselected = {
                let mut st = state_key.lock().unwrap();
                if st.selected_tool == Tool::Number && st.selected_action_index.is_some() {
                    st.clear_selection();
                    true
                } else {
                    false
                }
            };
            if deselected {
                if let Some(area) = drawing_area_key.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }
        }

        glib::Propagation::Proceed
    });

    drawing_area.add_controller(key_controller);

    let click = GestureClick::new();
    click.set_button(1);
    let window_click = window.clone();
    let state_click = state.clone();
    let transform_click = transform.clone();
    let drawing_area_click = drawing_area.downgrade();
    let color_buttons_click = color_buttons.clone();
    let color_picker_dot_click = color_picker_dot.clone();
    let color_class_names_click = color_class_names.clone();
    let eyedropper_mode_click = eyedropper_mode.clone();
    let eyedropper_from_sidebar_click = eyedropper_from_sidebar.clone();
    let eyedropper_point_click = eyedropper_point.clone();
    let eyedropper_rendered_click = eyedropper_rendered.clone();
    let space_pan_active_click = space_pan_active.clone();
    let set_picker_panel_visibility_canvas_click = set_picker_panel_visibility.clone();
    let canvas_eyedropper_ring_click = canvas_eyedropper_ring.clone();
    let apply_picker_color_to_editor_canvas_click = apply_picker_color_to_editor.clone();
    let sync_picker_from_color_canvas_click = sync_picker_from_color.clone();
    let add_color_to_custom_slots_click = add_color_to_custom_slots.clone();
    let sync_size_control_canvas_click = sync_size_control.clone();
    let text_size_label_click = text_size_label.clone();
    let font_family_label_click = font_family_label.clone();
    let text_size_list_click = text_size_list.clone();
    let font_family_list_click = font_family_list.clone();
    let sync_select_inspector_canvas_click = sync_select_inspector.clone();
    click.connect_pressed(move |gesture, n_press, x, y| {
        if space_pan_active_click.get() {
            gesture.set_state(gtk4::EventSequenceState::Denied);
            return;
        }

        let t = *transform_click.lock().unwrap();
        let view_point = Point { x, y };

        let text_hit = {
            let st = state_click.lock().unwrap();
            st.active_text_bounds.as_ref().map(|bounds| {
                let click_image = t.view_to_image_clamped(view_point);
                let inside_bounds = click_image.x >= bounds.rect.x as f64
                    && click_image.x <= (bounds.rect.x + bounds.rect.width) as f64
                    && click_image.y >= bounds.rect.y as f64
                    && click_image.y <= (bounds.rect.y + bounds.rect.height) as f64;

                let handle_hit = bounds.move_handles.iter().find_map(|(handle, center)| {
                    let center_view = Point {
                        x: center.x * t.scale + t.offset_x,
                        y: center.y * t.scale + t.offset_y,
                    };
                    let dx = x - center_view.x;
                    let dy = y - center_view.y;
                    if (dx * dx + dy * dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                        Some(handle.clone())
                    } else {
                        None
                    }
                });

                let resize_hit = bounds.resize_handle.as_ref().is_some_and(|(_, resize_pos)| {
                    let resize_view = Point {
                        x: resize_pos.x * t.scale + t.offset_x,
                        y: resize_pos.y * t.scale + t.offset_y,
                    };
                    let dx = x - resize_view.x;
                    let dy = y - resize_view.y;
                    dx.abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5 && dy.abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                });

                (click_image, inside_bounds, handle_hit, resize_hit)
            })
        };

        if let Some((click_image, inside_bounds, handle_hit, resize_hit)) = text_hit {
            if let Some(handle) = handle_hit {
                let mut st = state_click.lock().unwrap();
                st.active_text_is_dragging = true;
                st.active_text_drag_handle = Some(handle);
                st.active_text_drag_start = Some(click_image);
                st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
                st.active_text_is_resizing = false;
                st.reset_text_cursor_blink();
                return;
            }

            if resize_hit {
                let mut st = state_click.lock().unwrap();
                st.active_text_is_dragging = true;
                st.active_text_drag_handle = None;
                st.active_text_drag_start = Some(click_image);
                st.active_text_drag_start_bounds = st.active_text_bounds.as_ref().map(|b| b.rect);
                st.active_text_is_resizing = true;
                st.reset_text_cursor_blink();
                return;
            }

            if inside_bounds {
                let mut st = state_click.lock().unwrap();
                if let Some(input) = st.active_text_input.as_ref() {
                    let surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1)
                        .expect("create caret hit-test surface");
                    let context = gtk4::cairo::Context::new(&surface)
                        .expect("create caret hit-test context");
                    let font = FontSettings {
                        family: st.text_font_family.clone(),
                        size: st.text_size,
                        style: FontStyle::Normal,
                        decoration: TextDecoration::None,
                        alignment: TextAlignment::Left,
                    };
                    let cursor_position = cursor_position_for_text_point(
                        &context,
                        st.active_text_bounds.as_ref().unwrap(),
                        &input.text,
                        &font,
                        click_image,
                    );
                    st.set_text_cursor_position(cursor_position);
                } else {
                    st.reset_text_cursor_blink();
                }
                if let Some(area) = drawing_area_click.upgrade() {
                    area.grab_focus();
                    area.queue_draw();
                }
                return;
            }

            {
                let mut st = state_click.lock().unwrap();
                if let Some(action) = st.commit_text_input() {
                    st.push_action(action);
                }
            }
            if let Some(area) = drawing_area_click.upgrade() {
                area.queue_draw();
            }
        }

        if eyedropper_mode_click.get() {
            if !t.contains_view(view_point) {
                return;
            }

            let image_point = t.view_to_image_clamped(view_point);
            let picked_color = {
                let rendered = eyedropper_rendered_click.borrow();
                if let Some(rendered) = rendered.as_ref() {
                    sample_rendered_color_at_point(rendered, image_point)
                } else {
                    let st = state_click.lock().unwrap();
                    sample_editor_color_at_point(&st, image_point)
                }
            };

            let mut reopen_picker = false;
            let from_sidebar = eyedropper_from_sidebar_click.get();
            if let Some(color) = picked_color {
                // Only add to custom colors when picked from sidebar
                add_color_to_custom_slots_click(color);
                if !from_sidebar {
                    // Only apply to editor and sync picker if not from sidebar
                    apply_picker_color_to_editor_canvas_click(color);
                    sync_picker_from_color_canvas_click(color);
                    reopen_picker = true;
                }
            }

            eyedropper_mode_click.set(false);
            eyedropper_from_sidebar_click.set(false);
            *eyedropper_point_click.borrow_mut() = None;
            *eyedropper_rendered_click.borrow_mut() = None;
            canvas_eyedropper_ring_click.set_visible(false);
            set_window_cursor_name(&window_click, None);

            if reopen_picker {
                set_picker_panel_visibility_canvas_click(true);
            }

            if let Some(area) = drawing_area_click.upgrade() {
                area.queue_draw();
            }
            return;
        }

        if !t.contains_view(view_point) {
            return;
        }

        let image_point = t.view_to_image_clamped(view_point);
        let selected_tool = state_click.lock().unwrap().selected_tool;

        match selected_tool {
            Tool::Select => {
                let (selected_color_index, selected_text_size, selected_font_family, began_reedit) = {
                    let mut st = state_click.lock().unwrap();
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }
                    st.select_action_at_point_with_scale(image_point, t.scale);

                    // Ensure control_points are initialised for selected arrows.
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
                            st.arrow_editing_controls = true;
                        } else {
                            st.arrow_editing_controls = false;
                        }
                    }

                    let mut began_reedit = false;
                    if n_press >= 2 {
                        began_reedit = st.begin_editing_selected_text();
                    }
                    let selected_color = if began_reedit {
                        st.get_text_input().map(|input| input.color)
                    } else {
                        st.selected_action_color()
                    };
                    if let Some(color) = selected_color {
                        st.selected_color = color;
                    }
                    if let Some(text_size) = st.selected_text_action_size() {
                        st.text_size = text_size;
                    }
                    if let Some(stroke_size) = st.selected_action_stroke_size() {
                        st.stroke_size = stroke_size;
                    }
                    if let Some(font_family) = st.selected_text_font_family() {
                        st.text_font_family = font_family;
                    }
                    if let Some(crate::capture::editor::types::AnnotationAction::Obfuscate {
                        method,
                        amount,
                        ..
                    }) = st.selected_action()
                    {
                        let (method, amount) = (*method, *amount);
                        st.set_obfuscate_method(method);
                        st.set_current_obfuscate_amount(amount);
                    }

                    let selected_color_index = selected_color.map(palette_index_for_color);
                    let selected_text_size = Some(st.text_size);
                    let selected_font_family = Some(st.text_font_family.clone());
                    (selected_color_index, selected_text_size, selected_font_family, began_reedit)
                };

                sync_size_control_canvas_click();
                sync_select_inspector_canvas_click();
                if let Some(size) = selected_text_size {
                    text_size_label_click.set_label(&format!("{}pt", size as i32));
                    sync_text_option_selection(
                        &text_size_list_click,
                        TEXT_SIZE_OPTIONS
                            .iter()
                            .position(|candidate| *candidate == size as i32),
                    );
                }
                if let Some(family) = selected_font_family {
                    font_family_label_click.set_label(&family);
                    sync_text_option_selection(
                        &font_family_list_click,
                        TEXT_FONT_FAMILIES
                            .iter()
                            .position(|candidate| *candidate == family.as_str()),
                    );
                }

                if let Some(index) = selected_color_index {
                    color_picker::clear_active_color_picker_palette_state(&color_buttons_click);
                    color_picker::set_color_picker_trigger_dot_state(
                        &color_picker_dot_click,
                        &color_class_names_click,
                        index,
                    );
                }

                if let Some(area) = drawing_area_click.upgrade() {
                    if began_reedit {
                        area.grab_focus();
                    }
                    area.queue_draw();
                }
            }
            Tool::Text => {
                let (text_size, font_family) = {
                    let mut st = state_click.lock().unwrap();

                    // Commit any active text input first.
                    if st.active_text_input.is_some() {
                        st.commit_active_text_input();
                    }

                    // Check if the click lands on an existing text action.
                    let hit_index = st.actions.iter().enumerate().rev().find_map(|(index, action)| {
                        if matches!(action, crate::capture::editor::types::AnnotationAction::Text { .. })
                            && crate::capture::editor::selection::action_contains_point_with_padding(action, image_point, 0.0)
                        {
                            Some(index)
                        } else {
                            None
                        }
                    });

                    if let Some(index) = hit_index {
                        // Select the action and sync color/size state.
                        st.selected_action_index = Some(index);
                        if let Some(color) = st.selected_action_color() {
                            st.selected_color = color;
                        }
                        if let Some(sz) = st.selected_text_action_size() {
                            st.text_size = sz;
                        }
                        if let Some(fam) = st.selected_text_font_family() {
                            st.text_font_family = fam;
                        }

                        if n_press >= 2 {
                            // Double-click: begin re-editing.
                            st.begin_editing_selected_text();
                        } else {
                            // Single-click: first check if the click is on a
                            // TextEditBounds handle (circles / resize box).
                            // If yes → active_text_is_dragging path (motion handler).
                            // If no  → select_drag_anchor path (GestureDrag move).
                            let bounds_opt = if let Some(
                                crate::capture::editor::types::AnnotationAction::Text {
                                    position, text, font, max_width, ..
                                }
                            ) = st.actions.get(index) {
                                let surface = gtk4::cairo::ImageSurface::create(
                                    gtk4::cairo::Format::ARgb32, 1, 1,
                                ).ok();
                                surface.as_ref()
                                    .and_then(|s| gtk4::cairo::Context::new(s).ok())
                                    .map(|c| {
                                        let aw = max_width.unwrap_or_else(|| {
                                            (st.base_image.width() as f64 - position.x)
                                                .max(font.size * 1.8)
                                        });
                                        crate::capture::editor::render::text_action_bounds(
                                            &c, *position, text, font, Some(aw),
                                        )
                                    })
                            } else { None };

                            let mut handle_drag_started = false;
                            if let Some(bounds) = bounds_opt {
                                let handle_hit = bounds.move_handles.iter().find_map(|(h, center)| {
                                    let cv = Point {
                                        x: center.x * t.scale + t.offset_x,
                                        y: center.y * t.scale + t.offset_y,
                                    };
                                    let dx = x - cv.x;
                                    let dy = y - cv.y;
                                    if (dx*dx + dy*dy).sqrt() < MOVE_HANDLE_DRAG_RADIUS * 1.5 {
                                        Some(h.clone())
                                    } else { None }
                                });
                                let resize_hit = bounds.resize_handle.as_ref().is_some_and(
                                    |(_, rp)| {
                                        let rv = Point {
                                            x: rp.x * t.scale + t.offset_x,
                                            y: rp.y * t.scale + t.offset_y,
                                        };
                                        (x - rv.x).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                            && (y - rv.y).abs() < RESIZE_HANDLE_DRAG_SIZE * 1.5
                                    }
                                );

                                if handle_hit.is_some() || resize_hit {
                                    // Set up exactly like the active-edit handle path.
                                    // The motion handler and click_released handle the rest.
                                    st.active_text_bounds = Some(bounds);
                                    st.active_text_is_dragging = true;
                                    st.active_text_drag_handle = handle_hit;
                                    st.active_text_drag_start = Some(image_point);
                                    st.active_text_drag_start_bounds =
                                        st.active_text_bounds.as_ref().map(|b| b.rect);
                                    st.active_text_is_resizing = resize_hit;
                                    handle_drag_started = true;
                                }
                            }

                            if !handle_drag_started {
                                // No handle hit — set anchor for GestureDrag move.
                                st.select_drag_anchor = Some(image_point);
                                st.select_resize_handle = None;
                            }
                        }
                    } else {
                        // Click on empty area: deselect and start a new text box.
                        // Wide initial box so short words ("god") never wrap on
                        // the 3rd char and read as phantom whitespace/lag.
                        st.selected_action_index = None;
                        let initial_width = (st.text_size * 3.2).max(220.0);
                        let initial_height = (st.text_size * 1.45 + 16.0).max(44.0);
                        st.begin_text_input(image_point, initial_width, initial_height);
                    }

                    (st.text_size, st.text_font_family.clone())
                };

                text_size_label_click.set_label(&format!("{}pt", text_size as i32));
                font_family_label_click.set_label(&font_family);
                sync_text_option_selection(
                    &text_size_list_click,
                    TEXT_SIZE_OPTIONS
                        .iter()
                        .position(|candidate| *candidate == text_size as i32),
                );
                sync_text_option_selection(
                    &font_family_list_click,
                    TEXT_FONT_FAMILIES
                        .iter()
                        .position(|candidate| *candidate == font_family.as_str()),
                );

                if let Some(area) = drawing_area_click.upgrade() {
                    area.grab_focus();
                    area.queue_draw();
                }
            }
            Tool::Number => {
                {
                    let mut st = state_click.lock().unwrap();
                    // Clicking an existing marker re-opens its bar instead of stacking a
                    // second marker on top of it (mirrors the Box/Circle/Obfuscate
                    // reselect path). Empty canvas still places a new marker.
                    if !st.select_number_action_at_point_with_scale(image_point, t.scale) {
                        st.add_number_marker(image_point);
                    }
                }
                sync_size_control_canvas_click();
                if let Some(area) = drawing_area_click.upgrade() {
                    area.queue_draw();
                }
            }
            _ => {}
        }
    });

    let state_release = state.clone();
    let drawing_area_release = drawing_area.downgrade();
    click.connect_released(move |_gesture, _n_press, _x, _y| {
        let should_refocus = {
            let mut st = state_release.lock().unwrap();
            if st.active_text_is_dragging {
                let was_resizing = st.active_text_is_resizing;
                st.active_text_is_dragging = false;
                st.active_text_drag_handle = None;
                st.active_text_drag_start = None;
                st.active_text_drag_start_bounds = None;
                st.active_text_is_resizing = false;

                if st.active_text_input.is_some() {
                    // Active edit session: reflow text to fit new bounds.
                    if was_resizing {
                        st.fit_active_text_to_layout_preserving_box();
                    } else {
                        st.fit_active_text_to_layout_preserving_font_size();
                    }
                    true // refocus for typing
                } else if let (Some(bounds), Some(index)) =
                    (st.active_text_bounds.take(), st.selected_action_index)
                {
                    // Committed action handle resize: write new bounds back.
                    let mut changed = false;
                    if let Some(crate::capture::editor::types::AnnotationAction::Text {
                        position,
                        font,
                        max_width,
                        ..
                    }) = st.actions.get_mut(index)
                    {
                        let padding_y = 8.0;
                        let next_x = bounds.rect.x as f64;
                        let next_y = bounds.rect.y as f64 + font.size + padding_y;
                        let next_width = Some(bounds.rect.width as f64);
                        changed = position.x != next_x
                            || position.y != next_y
                            || *max_width != next_width;
                        position.x = next_x;
                        position.y = next_y;
                        *max_width = next_width;
                    }
                    if changed {
                        st.clear_redo_history();
                    }
                    st.finish_history_interaction();
                    false
                } else {
                    false
                }
            } else {
                false
            }
        };
        if let Some(area) = drawing_area_release.upgrade() {
            if should_refocus {
                area.grab_focus();
            }
            area.queue_draw();
        }
    });

    drawing_area.add_controller(click);
}

#[cfg(test)]
mod tests {
    #[test]
    fn canvas_click_owns_escape_press_release_and_text_sync() {
        let source = include_str!("click.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("pub(super) fn wire_canvas_click(")
                && production.contains("let key_controller = EventControllerKey::new();")
                && production.contains("if keyval == gdk::Key::Escape {")
                && production.contains("cancel_text_edit()")
                && production.contains("drawing_area.add_controller(key_controller);")
                && production.contains("let click = GestureClick::new();")
                && production.contains("click.connect_pressed(move |gesture, n_press, x, y| {")
                && production.contains("click.connect_released(move |_gesture, _n_press, _x, _y| {")
                && production.contains("drawing_area.add_controller(click);")
                && production.contains("eyedropper_mode_click.get()")
                && production.contains("add_number_marker")
                && production.contains("fn sync_text_option_selection")
                && production.contains("TEXT_SIZE_OPTIONS")
                && production.contains("TEXT_FONT_FAMILIES"),
            "click.rs must own canvas Escape, click press/release, eyedropper/number/text paths, and text option sync"
        );
    }
}
