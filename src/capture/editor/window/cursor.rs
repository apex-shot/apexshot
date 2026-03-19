use gtk4::gdk;
use gtk4::gdk::Cursor;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::{prelude::*, ApplicationWindow};

use super::super::color::{selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale};
use super::super::state::EditorState;
use super::super::text_detect::clamp_cursor_size;
use super::super::types::{
    cursor_name_for_select_handle, AnnotationAction, Point, Tool, ViewTransform,
};

/// Default highlighter cursor size (when no text detected)
pub const DEFAULT_HIGHLIGHTER_CURSOR_SIZE: f64 = 16.0;

/// Cursor width ratio relative to height
const CURSOR_WIDTH_RATIO: f64 = 1.5;

/// Corner radius for highlighter cursor
const CURSOR_CORNER_RADIUS: f64 = 4.0;

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
        "grab"
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
        | Tool::Obfuscate
        | Tool::Focus => "crosshair",
    }
}

/// Create a rounded rectangle highlighter cursor surface
pub fn create_highlighter_cursor_surface(
    height: f64,
    color: (f64, f64, f64, f64),
) -> Option<gtk4::cairo::ImageSurface> {
    let height = clamp_cursor_size(height);
    let width = height * CURSOR_WIDTH_RATIO;

    // Add padding for the rounded corners
    let surface_width = (width + 4.0).ceil() as i32;
    let surface_height = (height + 4.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;

    let context = gtk4::cairo::Context::new(&surface).ok()?;

    // Draw rounded rectangle
    let x = 2.0;
    let y = 2.0;
    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);

    context.new_sub_path();
    context.arc(x + width - radius, y + radius, radius, -std::f64::consts::FRAC_PI_2, 0.0);
    context.arc(x + width - radius, y + height - radius, radius, 0.0, std::f64::consts::FRAC_PI_2);
    context.arc(x + radius, y + height - radius, radius, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    context.arc(x + radius, y + radius, radius, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2);
    context.close_path();

    context.set_source_rgba(color.0, color.1, color.2, color.3);
    context.fill();

    Some(surface)
}

/// Set custom highlighter cursor on window
pub fn set_highlighter_cursor(
    window: &gtk4::ApplicationWindow,
    height: f64,
    color: (f64, f64, f64, f64),
) {
    if let Some(surface) = create_highlighter_cursor_surface(height, color) {
        if let Some(texture) = surface_to_texture(surface) {
            let cursor = Cursor::from_texture(&texture, 0, (height / 2.0) as i32, None);
            if let Some(surface) = window.surface() {
                surface.set_cursor(Some(&cursor));
            }
        }
    }
}

/// Convert cairo surface to gdk Texture
fn surface_to_texture(mut surface: gtk4::cairo::ImageSurface) -> Option<gdk::Texture> {
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride() as i32;
    let data = surface.data().ok()?.to_vec();

    let pixbuf = Pixbuf::from_bytes(
        &gtk4::glib::Bytes::from(&data),
        Colorspace::Rgb,
        true,
        8,
        width,
        height,
        stride,
    );

    Some(gdk::Texture::for_pixbuf(&pixbuf))
}
