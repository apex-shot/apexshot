use gtk4::gdk;
use gtk4::{prelude::*, ApplicationWindow};

use super::super::color::{selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale};
use super::super::state::EditorState;
use super::super::types::{
    cursor_name_for_select_handle, AnnotationAction, Point, Tool, ViewTransform,
};

pub(super) fn set_window_cursor_name(window: &ApplicationWindow, cursor_name: Option<&str>) {
    if let Some(surface) = window.surface() {
        let cursor = cursor_name.and_then(|name| gdk::Cursor::from_name(name, None));
        surface.set_cursor(cursor.as_ref());
    }
}

pub fn select_hover_cursor_name(
    state: &EditorState,
    point: Point,
    view_scale: f64,
) -> &'static str {
    if state.select_drag_anchor.is_some() {
        if let Some(handle) = state.select_resize_handle {
            return cursor_name_for_select_handle(handle);
        }
        return "grabbing";
    }

    if let Some(index) = state.selected_action_index {
        if let Some(selected) = state.actions.get(index) {
            let handle_hit_radius = selection_handle_hit_radius_for_scale(view_scale);
            if let Some(handle) = super::super::selection::action_resize_handle_at_point_with_radius(
                selected,
                point,
                handle_hit_radius,
            ) {
                return cursor_name_for_select_handle(handle);
            }

            let hit_padding = selection_hit_padding_for_scale(view_scale);
            if super::super::selection::action_contains_point_with_padding(
                selected,
                point,
                hit_padding,
            ) {
                return "grab";
            }
        }
    }

    let hit_padding = selection_hit_padding_for_scale(view_scale);
    if state.actions.iter().any(|action| {
        super::super::selection::action_contains_point_with_padding(action, point, hit_padding)
    }) {
        "pointer"
    } else {
        "default"
    }
}

fn crop_hover_cursor_name(state: &EditorState, point: Point, view_scale: f64) -> &'static str {
    if state.select_drag_anchor.is_some() {
        if let Some(handle) = state.select_resize_handle {
            return cursor_name_for_select_handle(handle);
        }
        return "grabbing";
    }

    if let Some(rect) = state.crop_selection {
        let crop_action = AnnotationAction::Box {
            rect,
            color: state.selected_color,
            stroke_size: state.stroke_size,
        };
        let handle_hit_radius = selection_handle_hit_radius_for_scale(view_scale);
        if let Some(handle) = super::super::selection::action_resize_handle_at_point_with_radius(
            &crop_action,
            point,
            handle_hit_radius,
        ) {
            return cursor_name_for_select_handle(handle);
        }

        let hit_padding = selection_hit_padding_for_scale(view_scale);
        if super::super::selection::action_contains_point_with_padding(
            &crop_action,
            point,
            hit_padding,
        ) {
            return "grab";
        }
    }

    "crosshair"
}

pub fn cursor_name_for_view_point(
    state: &EditorState,
    transform: ViewTransform,
    view_point: Point,
) -> &'static str {
    if state.selected_tool == Tool::Crop {
        let image_point = transform.view_to_image(view_point);
        return crop_hover_cursor_name(state, image_point, transform.scale);
    }

    if !transform.contains_view(view_point) {
        return "default";
    }

    let image_point = transform.view_to_image_clamped(view_point);
    match state.selected_tool {
        Tool::Select => select_hover_cursor_name(state, image_point, transform.scale),
        Tool::Text => "text",
        Tool::Crop => crop_hover_cursor_name(state, image_point, transform.scale),
        Tool::Background => "default",
        Tool::Pen
        | Tool::Highlighter
        | Tool::Circle
        | Tool::Arrow
        | Tool::Line
        | Tool::Box
        | Tool::Number
        | Tool::Blur
        | Tool::Focus
        | Tool::Censor => "crosshair",
    }
}
