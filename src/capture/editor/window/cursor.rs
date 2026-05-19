use gtk4::gdk;
use gtk4::gdk::Cursor;
use gtk4::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk4::{prelude::*, ApplicationWindow};

use super::super::color::{selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale};
use super::super::pen_weight::HighlighterMode;
use super::super::state::EditorState;
use super::super::text_detect::{clamp_cursor_size, DetectionStatus};
use super::super::types::{
    cursor_name_for_select_handle, AnnotationAction, Point, Tool, ViewTransform,
};

/// Default highlighter cursor size (when no text detected)
pub const DEFAULT_HIGHLIGHTER_CURSOR_SIZE: f64 = 16.0;

/// Default pen cursor size
pub const DEFAULT_PEN_CURSOR_SIZE: f64 = 8.0;

/// Cursor width ratio relative to height
const CURSOR_WIDTH_RATIO: f64 = 1.0;

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
            shadow: false,
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
        Tool::Highlighter => {
            // Note: Highlighter uses custom cursor set via update_cursor_for_position
            // This returns crosshair as fallback
            "crosshair"
        }
        Tool::Box | Tool::Circle => {
            if state.select_drag_anchor.is_some() {
                if let Some(handle) = state.select_resize_handle {
                    cursor_name_for_select_handle(handle)
                } else {
                    "grabbing"
                }
            } else {
                // Check resize handles on the selected action first.
                if let Some(index) = state.selected_action_index {
                    if let Some(selected) = state.actions.get(index) {
                        let is_matching_type = match selected {
                            AnnotationAction::Box { .. } => state.selected_tool == Tool::Box,
                            AnnotationAction::Circle { .. } => state.selected_tool == Tool::Circle,
                            _ => false,
                        };
                        if is_matching_type {
                            let handle_hit_radius =
                                selection_handle_hit_radius_for_scale(transform.scale);
                            if let Some(handle) =
                                super::super::selection::action_resize_handle_at_point_with_radius(
                                    selected,
                                    image_point,
                                    handle_hit_radius,
                                )
                            {
                                return cursor_name_for_select_handle(handle);
                            }
                        }
                    }
                }
                let hit_padding = selection_hit_padding_for_scale(transform.scale);
                let is_over_action = state.actions.iter().rev().any(|action| {
                    let matches_tool = match action {
                        AnnotationAction::Box { .. } => state.selected_tool == Tool::Box,
                        AnnotationAction::Circle { .. } => state.selected_tool == Tool::Circle,
                        _ => false,
                    };
                    matches_tool
                        && super::super::selection::action_contains_point_with_padding(
                            action,
                            image_point,
                            hit_padding,
                        )
                });
                if is_over_action {
                    "grab"
                } else {
                    "crosshair"
                }
            }
        }
        Tool::Pen | Tool::Arrow | Tool::Line | Tool::Number | Tool::Obfuscate | Tool::Focus => {
            "crosshair"
        }
    }
}

/// Create a rounded rectangle highlighter cursor surface
pub fn create_highlighter_cursor_surface(
    height: f64,
    _color: (f64, f64, f64, f64),
) -> Option<gtk4::cairo::ImageSurface> {
    let clamped_height = clamp_cursor_size(height);
    let height = clamped_height;
    let width = clamped_height * CURSOR_WIDTH_RATIO;

    let pad = 6.0;
    let surface_width = (width + pad * 2.0).ceil() as i32;
    let surface_height = (height + pad * 2.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;

    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let x = pad;
    let y = pad;
    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);

    fn rounded_rect(ctx: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
        ctx.new_sub_path();
        ctx.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        ctx.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        ctx.arc(
            x + r,
            y + h - r,
            r,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        );
        ctx.arc(
            x + r,
            y + r,
            r,
            std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
        );
        ctx.close_path();
    }

    // White outline (wider stroke behind)
    rounded_rect(&context, x, y, width, height, radius);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(3.5);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();

    // Black stripe on top
    rounded_rect(&context, x, y, width, height, radius);
    context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    context.set_line_width(1.5);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();

    Some(surface)
}

/// Create a cursor that indicates text detection is in progress.
/// Shows a dashed-outline rounded rectangle at the default size.
pub fn create_highlighter_detecting_cursor() -> Option<gtk4::cairo::ImageSurface> {
    let height = DEFAULT_HIGHLIGHTER_CURSOR_SIZE;
    let width = DEFAULT_HIGHLIGHTER_CURSOR_SIZE * CURSOR_WIDTH_RATIO;

    let pad = 6.0;
    let surface_width = (width + pad * 2.0).ceil() as i32;
    let surface_height = (height + pad * 2.0).ceil() as i32;

    let surface = gtk4::cairo::ImageSurface::create(
        gtk4::cairo::Format::ARgb32,
        surface_width,
        surface_height,
    )
    .ok()?;

    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let x = pad;
    let y = pad;
    let radius = CURSOR_CORNER_RADIUS.min(width / 2.0).min(height / 2.0);

    fn rounded_rect(ctx: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
        ctx.new_sub_path();
        ctx.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        ctx.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        ctx.arc(
            x + r,
            y + h - r,
            r,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        );
        ctx.arc(
            x + r,
            y + r,
            r,
            std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
        );
        ctx.close_path();
    }

    // Dashed gray outline to indicate "loading" state
    rounded_rect(&context, x, y, width, height, radius);
    context.set_source_rgba(0.5, 0.5, 0.5, 0.8);
    context.set_line_width(2.0);
    context.set_dash(&[4.0, 4.0], 0.0);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    let _ = context.stroke();

    Some(surface)
}

/// Create a circular pen cursor surface
pub fn create_pen_cursor_surface(
    size: f64,
    _color: (f64, f64, f64, f64),
) -> Option<gtk4::cairo::ImageSurface> {
    let size = clamp_cursor_size(size);

    let pad = 6.0;
    let surface_size = (size + pad * 2.0).ceil() as i32;

    let surface =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, surface_size, surface_size)
            .ok()?;

    let context = gtk4::cairo::Context::new(&surface).ok()?;

    let center = pad + size / 2.0;
    let radius = size / 2.0;

    // White outline (wider stroke behind)
    context.arc(center, center, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    context.set_line_width(3.5);
    context.stroke().ok()?;

    // Black circle on top
    context.arc(center, center, radius, 0.0, std::f64::consts::TAU);
    context.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    context.set_line_width(1.5);
    context.stroke().ok()?;

    Some(surface)
}

/// Set custom pen cursor on window
pub fn set_pen_cursor(window: &gtk4::ApplicationWindow, size: f64, color: (f64, f64, f64, f64)) {
    if let Some(surface) = create_pen_cursor_surface(size, color) {
        if let Some(texture) = surface_to_texture(surface) {
            let pad = 6.0;
            let hotspot = (pad + size / 2.0) as i32;
            let cursor = Cursor::from_texture(&texture, hotspot, hotspot, None);
            if let Some(surface) = window.surface() {
                surface.set_cursor(Some(&cursor));
            }
        }
    }
}

/// Update cursor for pen tool based on position and state.
pub fn update_pen_cursor(window: &gtk4::ApplicationWindow, state: &EditorState) {
    let color = (
        state.selected_color.r,
        state.selected_color.g,
        state.selected_color.b,
        1.0,
    );

    set_pen_cursor(window, state.pen_weight.stroke_width(), color)
}

/// Set custom highlighter cursor on window
pub fn set_highlighter_cursor(
    window: &gtk4::ApplicationWindow,
    height: f64,
    color: (f64, f64, f64, f64),
) {
    if let Some(surface) = create_highlighter_cursor_surface(height, color) {
        if let Some(texture) = surface_to_texture(surface) {
            let hotspot_x = (texture.width() as f64 * 0.35).ceil() as i32;
            let hotspot_y = texture.height() / 2;
            let cursor = Cursor::from_texture(&texture, hotspot_x, hotspot_y, None);
            if let Some(surface) = window.surface() {
                surface.set_cursor(Some(&cursor));
            }
        }
    }
}

/// Update cursor based on current state and position.
///
/// In text-aware mode, the cursor only adopts a detected text height when
/// the pointer is directly over a detected text region.
pub fn update_cursor_for_position(
    window: &gtk4::ApplicationWindow,
    state: &EditorState,
    image_point: Point,
    _view_scale: f64,
) {
    if state.selected_tool != Tool::Highlighter {
        return;
    }

    let color = (
        state.selected_color.r,
        state.selected_color.g,
        state.selected_color.b,
        0.4,
    );

    if let Some(locked_height) = state.locked_highlighter_stroke_size {
        set_highlighter_cursor(window, locked_height, color);
        return;
    }

    match state.highlighter_mode {
        HighlighterMode::TextAware => {
            if let Ok(detector) = state.text_detector.lock() {
                match detector.status() {
                    DetectionStatus::Pending => {
                        // Detection still running — show loading cursor
                        if let Some(surface) = create_highlighter_detecting_cursor() {
                            if let Some(texture) = surface_to_texture(surface) {
                                let hotspot_x =
                                    ((DEFAULT_HIGHLIGHTER_CURSOR_SIZE * CURSOR_WIDTH_RATIO) / 2.0
                                        + 6.0) as i32;
                                let hotspot_y =
                                    (DEFAULT_HIGHLIGHTER_CURSOR_SIZE / 2.0 + 6.0) as i32;
                                let cursor =
                                    Cursor::from_texture(&texture, hotspot_x, hotspot_y, None);
                                if let Some(w_surface) = window.surface() {
                                    w_surface.set_cursor(Some(&cursor));
                                }
                                return;
                            }
                        }
                    }
                    DetectionStatus::Failed(_) => {
                        // Detection failed — show default cursor
                        set_highlighter_cursor(window, DEFAULT_HIGHLIGHTER_CURSOR_SIZE, color);
                        return;
                    }
                    DetectionStatus::Ready => {
                        if let Some(height) = detector.best_text_height_at_point(image_point) {
                            set_highlighter_cursor(window, height, color);
                            return;
                        }
                    }
                }
            }
            // Fallback to default cursor size
            set_highlighter_cursor(window, DEFAULT_HIGHLIGHTER_CURSOR_SIZE, color);
        }
        HighlighterMode::Freehand => {
            set_highlighter_cursor(window, state.pen_weight.stroke_width(), color);
        }
    }
}

/// Convert cairo surface to gdk Texture
fn surface_to_texture(mut surface: gtk4::cairo::ImageSurface) -> Option<gdk::Texture> {
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride();
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
