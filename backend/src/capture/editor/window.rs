use gtk4::gdk;
use gtk4::{
    glib, prelude::*, Application, ApplicationWindow, Box as GtkBox, Button, CenterBox,
    CheckButton, CssProvider, DragSource, DrawingArea, DropTarget, Entry, EventControllerKey,
    EventControllerMotion, FileChooserAction, FileChooserNative, FileFilter, GestureClick,
    GestureDrag, Image, Label, MenuButton, Orientation, Overlay, Popover, ResponseType, Scale,
    ScrolledWindow, Stack,
};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use super::color::{
    custom_color_slots_css, load_persisted_custom_slot_colors, move_custom_color_between_slots,
    parse_alpha_percent, parse_channel_u8, parse_hex_rgb, picker_dynamic_css,
    save_persisted_custom_slot_colors, selection_handle_hit_radius_for_scale,
    selection_hit_padding_for_scale, DEFAULT_COLOR_INDEX, DRAG_REDRAW_INTERVAL_US, DRAW_COLORS,
    MAX_STROKE_SIZE, MAX_TEXT_SIZE, MIN_STROKE_SIZE, MIN_TEXT_SIZE,
};
use super::io_ops::{copy_uri_to_clipboard, open_target, save_edited_image};
use super::render::{
    draw_annotation_action, draw_canvas_checkerboard_background, draw_crop_overlay,
    draw_draft_action, draw_focus_overlay, draw_rgba_to_context, draw_selection_handles,
    draw_selection_outline, rgba_image_to_surface,
};
use super::selection::{action_bounds_with_padding, action_resize_handles};
use super::state::EditorState;
use super::types::{
    cursor_name_for_select_handle, AnnotationAction, CropAspectRatio, EditorError,
    PickerColorState, Point, Rect, Tool, ViewTransform,
};
use super::ui_support::{
    color_swatch_button, footer_icon_button, icon_tool_button, install_editor_css,
    prefers_dark_glass_theme, prefers_reduced_transparency, recommended_window_size,
    recommended_window_size_with_extra_width, set_active_color_button, set_active_tool_button,
    set_crop_apply_button_state, show_text_dialog, show_text_edit_dialog, traffic_light_button,
};

mod icon_names {
    pub use shipped::*;
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}

const EYEDROPPER_LOUPE_SIZE: i32 = 132;
const EYEDROPPER_LOUPE_GRID_SIZE: i32 = 15;
const EYEDROPPER_LOUPE_PIXEL_SIZE: f64 = 8.0;
const PICKER_PANEL_WIDTH: i32 = 252;
const PICKER_SLIDER_WIDTH: i32 = 220;
const PICKER_HEX_ENTRY_WIDTH: i32 = 214;
const BACKGROUND_SIDEBAR_WIDTH: i32 = 228;
const BACKGROUND_GRADIENT_PREVIEW_SIZE: u32 = 96;
const BACKGROUND_GRADIENT_PREVIEW_FILES: [&str; 10] = [
    "codioful-formerly-gradienta-n2XqPm7Bqhk-unsplash.jpg",
    "codioful-formerly-gradienta-O10vBIDRkZw-unsplash.jpg",
    "kunal-patil-2hB-jhXLd3c-unsplash.jpg",
    "kunal-patil-8ZKlgI_G-mw-unsplash.jpg",
    "luke-chesser-CxBx_J3yp9g-unsplash.jpg",
    "luke-chesser-pJadQetzTkI-unsplash.jpg",
    "magicpattern-87PP9Zd7MNo-unsplash.jpg",
    "magicpattern-bevXKKL7E9g-unsplash.jpg",
    "magicpattern-oPH_5xuMgQw-unsplash.jpg",
    "milad-fakurian-nY14Fs8pxT8-unsplash.jpg",
];
const BACKGROUND_GRADIENT_PREVIEW_CLASSES: [&str; 10] = [
    "editor-background-gradient-preview-1",
    "editor-background-gradient-preview-2",
    "editor-background-gradient-preview-3",
    "editor-background-gradient-preview-4",
    "editor-background-gradient-preview-5",
    "editor-background-gradient-preview-6",
    "editor-background-gradient-preview-7",
    "editor-background-gradient-preview-8",
    "editor-background-gradient-preview-9",
    "editor-background-gradient-preview-10",
];
const BACKGROUND_PLAIN_COLOR_CLASSES: [&str; 18] = [
    "editor-background-plain-color-1",
    "editor-background-plain-color-2",
    "editor-background-plain-color-3",
    "editor-background-plain-color-4",
    "editor-background-plain-color-5",
    "editor-background-plain-color-6",
    "editor-background-plain-color-7",
    "editor-background-plain-color-8",
    "editor-background-plain-color-9",
    "editor-background-plain-color-10",
    "editor-background-plain-color-11",
    "editor-background-plain-color-12",
    "editor-background-plain-color-13",
    "editor-background-plain-color-14",
    "editor-background-plain-color-15",
    "editor-background-plain-color-16",
    "editor-background-plain-color-17",
    "editor-background-plain-color-18",
];

fn background_gradient_asset_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/capture/editor/background-images")
        .join(file_name)
}

fn load_background_preview_image(path: &Path, preview_size: u32) -> Option<RgbaImage> {
    let image = image::open(path).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let square_size = width.min(height);
    let crop_x = (width - square_size) / 2;
    let crop_y = (height - square_size) / 2;
    let cropped =
        image::imageops::crop_imm(&image, crop_x, crop_y, square_size, square_size).to_image();

    Some(image::imageops::resize(
        &cropped,
        preview_size,
        preview_size,
        image::imageops::FilterType::Triangle,
    ))
}

fn load_background_gradient_preview_image(file_name: &str, preview_size: u32) -> Option<RgbaImage> {
    load_background_preview_image(&background_gradient_asset_path(file_name), preview_size)
}

fn parse_wallpaper_setting(raw_value: &str) -> Option<PathBuf> {
    let trimmed = raw_value.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }

    if let Ok(uri) = url::Url::parse(trimmed) {
        if uri.scheme() == "file" {
            return uri.to_file_path().ok().filter(|path| path.is_file());
        }
    }

    let path = PathBuf::from(trimmed);
    path.is_file().then_some(path)
}

fn detect_system_wallpaper_path() -> Option<PathBuf> {
    let setting_queries = [
        ("org.gnome.desktop.background", "picture-uri-dark"),
        ("org.gnome.desktop.background", "picture-uri"),
        ("org.cinnamon.desktop.background", "picture-uri"),
        ("org.mate.background", "picture-filename"),
    ];

    for (schema, key) in setting_queries {
        let output = match Command::new("gsettings")
            .arg("get")
            .arg(schema)
            .arg(key)
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = parse_wallpaper_setting(&stdout) {
            return Some(path);
        }
    }

    None
}

fn draw_preview_tile_surface(
    context: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
    corner_radius: f64,
) {
    let width = width.max(1) as f64;
    let height = height.max(1) as f64;
    let radius = corner_radius.min(width * 0.5).min(height * 0.5);

    context.new_path();
    context.arc(
        width - radius,
        radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        width - radius,
        height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        radius,
        height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        radius,
        radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
    context.clip();

    let surface_width = surface.width().max(1) as f64;
    let surface_height = surface.height().max(1) as f64;
    let scale = (width / surface_width).max(height / surface_height);
    let draw_width = surface_width * scale;
    let draw_height = surface_height * scale;
    let offset_x = (width - draw_width) * 0.5;
    let offset_y = (height - draw_height) * 0.5;

    let _ = context.save();
    context.translate(offset_x, offset_y);
    context.scale(scale, scale);
    if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
        let _ = context.paint();
    }
    let _ = context.restore();
}

fn build_background_gradient_preview_button(
    index: usize,
) -> (
    Button,
    DrawingArea,
    Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>,
) {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class(BACKGROUND_GRADIENT_PREVIEW_CLASSES[index]);
    button.set_tooltip_text(Some("Gradient"));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    let preview_surface = Rc::new(RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let preview_surface_draw = preview_surface.clone();
    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = preview_surface_draw.borrow().as_ref() {
            draw_preview_tile_surface(context, surface, width, height, 12.0);
        }
    });

    button.set_child(Some(&preview_area));

    (button, preview_area, preview_surface)
}

fn build_background_wallpaper_preview_button(
    surface: Option<gtk4::cairo::ImageSurface>,
    tooltip: &str,
) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.set_tooltip_text(Some(tooltip));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    preview_area.set_draw_func(move |_area, context, width, height| {
        if let Some(surface) = surface.as_ref() {
            draw_preview_tile_surface(context, surface, width, height, 12.0);
        }
    });

    button.set_child(Some(&preview_area));
    button
}

fn build_background_add_wallpaper_button() -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-add-button");
    button.set_tooltip_text(Some("Add wallpaper"));

    let plus_label = Label::new(Some("+"));
    plus_label.add_css_class("editor-background-add-label");
    button.set_child(Some(&plus_label));

    button
}

fn build_background_blurred_preview_button(index: usize) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_size_request(56, 56);
    button.add_css_class("editor-background-gradient-button");
    button.add_css_class("editor-background-blurred-button");
    button.set_tooltip_text(Some(&format!("Blurred {}", index + 1)));

    let preview_area = DrawingArea::new();
    preview_area.add_css_class("editor-background-gradient-preview-area");
    preview_area.set_content_width(56);
    preview_area.set_content_height(56);
    preview_area.set_hexpand(false);
    preview_area.set_vexpand(false);

    button.set_child(Some(&preview_area));
    button
}

fn build_background_plain_color_button(index: usize) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.set_hexpand(false);
    button.set_vexpand(false);
    button.set_halign(gtk4::Align::Center);
    button.set_valign(gtk4::Align::Center);
    button.set_size_request(18, 18);
    button.add_css_class("editor-background-plain-color-button");
    button.add_css_class(BACKGROUND_PLAIN_COLOR_CLASSES[index]);
    button.set_tooltip_text(Some(&format!("Plain color {}", index + 1)));
    button
}

fn build_background_plain_color_cell(index: usize) -> GtkBox {
    let cell = GtkBox::new(Orientation::Vertical, 0);
    cell.add_css_class("editor-background-plain-color-cell");
    cell.set_hexpand(true);
    cell.set_halign(gtk4::Align::Fill);
    cell.set_valign(gtk4::Align::Center);
    cell.append(&build_background_plain_color_button(index));
    cell
}


fn rebuild_wallpaper_preview_grid(
    wallpaper_grid: &GtkBox,
    wallpaper_previews: &[(String, Option<gtk4::cairo::ImageSurface>)],
    add_button: &Button,
) {
    while let Some(child) = wallpaper_grid.first_child() {
        wallpaper_grid.remove(&child);
    }

    let new_row = || {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.add_css_class("editor-background-wallpaper-row");
        row
    };

    let mut row = new_row();
    let mut items_in_row = 0usize;

    for (tooltip, surface) in wallpaper_previews {
        if items_in_row == 5 {
            wallpaper_grid.append(&row);
            row = new_row();
            items_in_row = 0;
        }

        let preview_button = build_background_wallpaper_preview_button(surface.clone(), tooltip);
        row.append(&preview_button);
        items_in_row += 1;
    }

    if items_in_row == 5 {
        wallpaper_grid.append(&row);
        row = new_row();
    }

    row.append(add_button);
    wallpaper_grid.append(&row);
}

pub fn open_image_editor(path: PathBuf) -> Result<(), EditorError> {
    if !path.exists() {
        return Err(EditorError::MissingFile(path));
    }

    let app = Application::builder()
        .application_id("com.apexshot.capture.editor")
        .build();

    app.connect_activate(move |application| {
        setup_editor_window(application, path.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn set_window_cursor_name(window: &ApplicationWindow, cursor_name: Option<&str>) {
    if let Some(surface) = window.surface() {
        let cursor = cursor_name.and_then(|name| gdk::Cursor::from_name(name, None));
        surface.set_cursor(cursor.as_ref());
    }
}

fn transparent_drag_icon_texture() -> Option<gdk::Texture> {
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::new(gtk4::gdk_pixbuf::Colorspace::Rgb, true, 8, 1, 1)?;
    pixbuf.fill(0x0000_0000);
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

fn sample_editor_color_at_point(
    state: &EditorState,
    image_point: Point,
) -> Option<super::types::DrawColor> {
    let rendered = state.to_rendered_image().ok()?;
    sample_rendered_color_at_point(&rendered, image_point)
}

fn crop_canvas_overflow(
    crop_rect: Option<Rect>,
    image_width: f64,
    image_height: f64,
    scale: f64,
    crop_mode_active: bool,
) -> (f64, f64, f64, f64) {
    let (left, top, right, bottom) = if let Some(rect) = crop_rect {
        (
            (-rect.x).max(0) as f64 * scale,
            (-rect.y).max(0) as f64 * scale,
            ((rect.x + rect.width) as f64 - image_width).max(0.0) * scale,
            ((rect.y + rect.height) as f64 - image_height).max(0.0) * scale,
        )
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    if !crop_mode_active {
        return (left.ceil(), top.ceil(), right.ceil(), bottom.ceil());
    }

    let reserve = 180.0;
    (
        left.max(reserve).ceil(),
        top.max(reserve).ceil(),
        right.max(reserve).ceil(),
        bottom.max(reserve).ceil(),
    )
}

fn sample_rendered_color_at_point(
    rendered: &RgbaImage,
    image_point: Point,
) -> Option<super::types::DrawColor> {
    let width = rendered.width();
    let height = rendered.height();
    if width == 0 || height == 0 {
        return None;
    }

    let sample_x = image_point
        .x
        .floor()
        .clamp(0.0, width.saturating_sub(1) as f64) as u32;
    let sample_y = image_point
        .y
        .floor()
        .clamp(0.0, height.saturating_sub(1) as f64) as u32;

    let rgba = rendered.get_pixel(sample_x, sample_y).0;
    Some(super::types::DrawColor::new(
        rgba[0] as f64 / 255.0,
        rgba[1] as f64 / 255.0,
        rgba[2] as f64 / 255.0,
        rgba[3] as f64 / 255.0,
    ))
}

fn eyedropper_loupe_position(cursor_x: f64, cursor_y: f64) -> (i32, i32) {
    let half_size = EYEDROPPER_LOUPE_SIZE as f64 / 2.0;
    (
        (cursor_x - half_size).round() as i32,
        (cursor_y - half_size).round() as i32,
    )
}

fn draw_eyedropper_loupe(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    rendered: &RgbaImage,
    image_point: Point,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    let image_width = rendered.width();
    let image_height = rendered.height();
    if image_width == 0 || image_height == 0 {
        return;
    }

    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let radius = width.min(height) as f64 / 2.0 - 2.0;
    if radius <= 0.0 {
        return;
    }

    let center_px_x = image_point
        .x
        .floor()
        .clamp(0.0, image_width.saturating_sub(1) as f64) as i32;
    let center_px_y = image_point
        .y
        .floor()
        .clamp(0.0, image_height.saturating_sub(1) as f64) as i32;

    let grid_size = EYEDROPPER_LOUPE_GRID_SIZE.max(1);
    let half_grid = grid_size / 2;
    let grid_extent = grid_size as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE;
    let grid_start_x = center_x - grid_extent / 2.0;
    let grid_start_y = center_y - grid_extent / 2.0;

    let _ = context.save();
    context.arc(center_x, center_y, radius, 0.0, TAU);
    let _ = context.clip();

    context.set_source_rgba(0.06, 0.07, 0.09, 0.94);
    let _ = context.paint();

    let max_source_x = image_width.saturating_sub(1) as i32;
    let max_source_y = image_height.saturating_sub(1) as i32;

    for row in 0..grid_size {
        for col in 0..grid_size {
            let source_x = (center_px_x + col - half_grid).clamp(0, max_source_x) as u32;
            let source_y = (center_px_y + row - half_grid).clamp(0, max_source_y) as u32;
            let rgba = rendered.get_pixel(source_x, source_y).0;

            context.set_source_rgba(
                rgba[0] as f64 / 255.0,
                rgba[1] as f64 / 255.0,
                rgba[2] as f64 / 255.0,
                rgba[3] as f64 / 255.0,
            );

            let dest_x = grid_start_x + col as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE;
            let dest_y = grid_start_y + row as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE;
            context.rectangle(
                dest_x.floor(),
                dest_y.floor(),
                EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5,
                EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5,
            );
            let _ = context.fill();
        }
    }

    context.set_source_rgba(0.0, 0.0, 0.0, 0.24);
    context.set_line_width(1.0);
    for line in 0..=grid_size {
        let x = grid_start_x + line as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5;
        context.move_to(x, grid_start_y);
        context.line_to(x, grid_start_y + grid_extent);

        let y = grid_start_y + line as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5;
        context.move_to(grid_start_x, y);
        context.line_to(grid_start_x + grid_extent, y);
    }
    let _ = context.stroke();

    let _ = context.restore();

    context.arc(center_x, center_y, radius - 0.5, 0.0, TAU);
    context.set_source_rgba(1.0, 1.0, 1.0, 0.98);
    context.set_line_width(2.6);
    let _ = context.stroke_preserve();
    context.set_source_rgba(0.0, 0.0, 0.0, 0.74);
    context.set_line_width(1.2);
    let _ = context.stroke();

    let target_x = grid_start_x + half_grid as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5;
    let target_y = grid_start_y + half_grid as f64 * EYEDROPPER_LOUPE_PIXEL_SIZE + 0.5;
    let target_size = EYEDROPPER_LOUPE_PIXEL_SIZE - 1.0;

    context.rectangle(target_x, target_y, target_size, target_size);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.96);
    context.set_line_width(2.0);
    let _ = context.stroke_preserve();
    context.set_source_rgba(1.0, 1.0, 1.0, 0.97);
    context.set_line_width(1.0);
    let _ = context.stroke();
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
            if let Some(handle) = super::selection::action_resize_handle_at_point_with_radius(
                selected,
                point,
                handle_hit_radius,
            ) {
                return cursor_name_for_select_handle(handle);
            }

            let hit_padding = selection_hit_padding_for_scale(view_scale);
            if super::selection::action_contains_point_with_padding(selected, point, hit_padding) {
                return "grab";
            }
        }
    }

    let hit_padding = selection_hit_padding_for_scale(view_scale);
    if state.actions.iter().any(|action| {
        super::selection::action_contains_point_with_padding(action, point, hit_padding)
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
        if let Some(handle) = super::selection::action_resize_handle_at_point_with_radius(
            &crop_action,
            point,
            handle_hit_radius,
        ) {
            return cursor_name_for_select_handle(handle);
        }

        let hit_padding = selection_hit_padding_for_scale(view_scale);
        if super::selection::action_contains_point_with_padding(&crop_action, point, hit_padding) {
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

fn apply_size_control_ui_state(
    state: &EditorState,
    size_group: &GtkBox,
    size_down_btn: &Button,
    size_up_btn: &Button,
) {
    size_group.set_visible(true);
    size_down_btn.set_label("-");
    size_up_btn.set_label("+");

    let Some(mode) = state.active_size_control_mode() else {
        size_group.add_css_class("size-group-inactive");
        size_down_btn.set_tooltip_text(Some("Current tool does not support size changes"));
        size_up_btn.set_tooltip_text(Some("Current tool does not support size changes"));
        size_down_btn.set_sensitive(false);
        size_up_btn.set_sensitive(false);
        return;
    };

    size_group.remove_css_class("size-group-inactive");
    let value = state.active_size_value().unwrap_or_default();

    match mode {
        super::types::SizeControlMode::Stroke => {
            size_down_btn.set_tooltip_text(Some("Decrease stroke size"));
            size_up_btn.set_tooltip_text(Some("Increase stroke size"));
            size_down_btn.set_sensitive(value > MIN_STROKE_SIZE + f64::EPSILON);
            size_up_btn.set_sensitive(value < MAX_STROKE_SIZE - f64::EPSILON);
        }
        super::types::SizeControlMode::Text => {
            size_down_btn.set_tooltip_text(Some("Decrease text size"));
            size_up_btn.set_tooltip_text(Some("Increase text size"));
            size_down_btn.set_sensitive(value > MIN_TEXT_SIZE + f64::EPSILON);
            size_up_btn.set_sensitive(value < MAX_TEXT_SIZE - f64::EPSILON);
        }
    }
}

fn set_active_color_picker_state(
    color_buttons: &[Button],
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    set_active_color_button(color_buttons, active_index);

    set_color_picker_trigger_dot_state(trigger_dot, color_classes, active_index);
}

fn clear_active_color_picker_palette_state(color_buttons: &[Button]) {
    for button in color_buttons {
        button.remove_css_class("active-color");
    }
}

fn clear_color_picker_trigger_dot_state(trigger_dot: &GtkBox, color_classes: &[&str]) {
    for class_name in color_classes {
        trigger_dot.remove_css_class(class_name);
    }
}

fn set_color_picker_trigger_dot_state(
    trigger_dot: &GtkBox,
    color_classes: &[&str],
    active_index: usize,
) {
    clear_color_picker_trigger_dot_state(trigger_dot, color_classes);

    if let Some(class_name) = color_classes.get(active_index) {
        trigger_dot.add_css_class(class_name);
    }
}

pub fn setup_editor_window(app: &Application, path: PathBuf) {
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    install_editor_css();

    let image = match image::open(&path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("Failed to load image for editing: {e}");
            app.quit();
            return;
        }
    };

    let (img_width, img_height) = image.dimensions();
    let state = Arc::new(Mutex::new(EditorState::new(image)));
    let transform = Arc::new(Mutex::new(ViewTransform::for_image(
        img_width as f64,
        img_height as f64,
    )));

    let (default_width, default_height) =
        recommended_window_size(img_width as i32, img_height as i32);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Screenshot Editor")
        .default_width(default_width)
        .default_height(default_height)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");

    let _dark_glass = prefers_dark_glass_theme();
    let reduced_transparency = prefers_reduced_transparency();
    root.add_css_class("editor-theme-dark");
    if reduced_transparency {
        root.add_css_class("editor-reduced-transparency");
    }

    // Toolbar
    let toolbar = CenterBox::new();
    toolbar.add_css_class("editor-toolbar");

    let traffic_close = traffic_light_button("traffic-light-red", "Close");
    let traffic_minimize = traffic_light_button("traffic-light-yellow", "Minimize");
    let traffic_zoom = traffic_light_button("traffic-light-green", "Zoom");

    let traffic_lights = GtkBox::new(Orientation::Horizontal, 6);
    traffic_lights.add_css_class("editor-traffic-lights");
    traffic_lights.append(&traffic_close);
    traffic_lights.append(&traffic_minimize);
    traffic_lights.append(&traffic_zoom);

    let select_btn = icon_tool_button("pointer-primary-click-symbolic", "Select");
    let crop_btn = icon_tool_button(icon_names::CROP, "Crop");
    crop_btn.add_css_class("standalone-tool");
    let background_btn = icon_tool_button("image-x-generic-symbolic", "Background");
    background_btn.add_css_class("standalone-tool");
    let draw_btn = icon_tool_button(icon_names::DOCUMENT_EDIT_REGULAR, "Pen");

    let left_group = GtkBox::new(Orientation::Horizontal, 16);
    left_group.add_css_class("editor-toolbar-left");
    left_group.append(&traffic_lights);
    toolbar.set_start_widget(Some(&left_group));

    let arrow_btn = icon_tool_button(icon_names::GO_NEXT, "Arrow");
    let line_btn = icon_tool_button(icon_names::DRAW_LINE, "Line");
    let box_btn = icon_tool_button(icon_names::DRAW_RECTANGLE, "Box");
    let circle_btn = icon_tool_button(icon_names::CIRCLE_LINE_REGULAR, "Circle");
    let text_btn = icon_tool_button(icon_names::INSERT_TEXT, "Text");
    let number_btn = icon_tool_button(icon_names::PIN, "Number");
    let highlighter_btn = icon_tool_button(icon_names::HIGHLIGHT_REGULAR, "Highlighter");
    let blur_btn = icon_tool_button(icon_names::FOG, "Blur");
    let focus_btn = icon_tool_button(icon_names::SMALL_RECTANGLE_IN_FOCUS, "Focus");
    let censor_btn = icon_tool_button(icon_names::EYE_OFF_REGULAR, "Censor");

    let sep_1 = GtkBox::new(Orientation::Vertical, 0);
    sep_1.add_css_class("editor-tools-divider");
    sep_1.set_vexpand(true);

    let sep_2 = GtkBox::new(Orientation::Vertical, 0);
    sep_2.add_css_class("editor-tools-divider");
    sep_2.set_vexpand(true);

    // Color specs
    let color_specs = [
        ("Black", "editor-color-black"),
        ("Blue", "editor-color-blue"),
        ("Dark Green", "editor-color-dark-green"),
        ("Red", "editor-color-red"),
        ("Orange", "editor-color-orange"),
        ("Yellow", "editor-color-yellow"),
        ("Green", "editor-color-green"),
        ("Cyan", "editor-color-cyan"),
        ("Blue Bright", "editor-color-blue-bright"),
        ("Purple", "editor-color-purple"),
        ("Pink", "editor-color-pink"),
        ("White", "editor-color-white"),
    ];
    let visible_color_specs = &color_specs[..10];
    let color_class_names: Vec<&str> = color_specs
        .iter()
        .map(|(_, class_name)| *class_name)
        .collect();
    let color_buttons: Vec<Button> = visible_color_specs
        .iter()
        .map(|(tooltip, class_name)| color_swatch_button(class_name, tooltip))
        .collect();

    // Color picker trigger
    let color_picker_trigger = MenuButton::new();
    color_picker_trigger.set_has_frame(false);
    color_picker_trigger.set_focusable(false);
    color_picker_trigger.set_can_target(false);
    color_picker_trigger.set_tooltip_text(Some("Colors"));
    color_picker_trigger.set_icon_name("");
    color_picker_trigger.set_hexpand(true);
    color_picker_trigger.set_vexpand(true);
    color_picker_trigger.set_halign(gtk4::Align::Fill);
    color_picker_trigger.set_valign(gtk4::Align::Fill);
    color_picker_trigger.add_css_class("editor-color-trigger-menu-button");
    color_picker_trigger.add_css_class("flat");

    let color_picker_dot = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_dot.set_size_request(20, 20);
    color_picker_dot.set_halign(gtk4::Align::Center);
    color_picker_dot.set_valign(gtk4::Align::Center);
    color_picker_dot.add_css_class("editor-color-trigger-dot");
    color_picker_dot.add_css_class(color_specs[DEFAULT_COLOR_INDEX].1);

    let trigger_divider = GtkBox::new(Orientation::Vertical, 0);
    trigger_divider.add_css_class("editor-color-trigger-divider");

    let color_picker_arrow_box = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_arrow_box.add_css_class("editor-color-trigger-arrow-box");
    color_picker_arrow_box.set_halign(gtk4::Align::Center);
    color_picker_arrow_box.set_valign(gtk4::Align::Center);
    let color_picker_arrow = Image::from_icon_name("pan-down-symbolic");
    color_picker_arrow.set_pixel_size(10);
    color_picker_arrow.add_css_class("editor-color-trigger-arrow");
    color_picker_arrow_box.append(&color_picker_arrow);

    let color_picker_trigger_shell = GtkBox::new(Orientation::Horizontal, 0);
    color_picker_trigger_shell.add_css_class("editor-color-trigger-shell");
    color_picker_trigger_shell.set_valign(gtk4::Align::Center);
    color_picker_trigger_shell.append(&color_picker_dot);
    color_picker_trigger_shell.append(&trigger_divider);
    color_picker_trigger_shell.append(&color_picker_arrow_box);

    let color_picker_trigger_host = Overlay::new();
    color_picker_trigger_host.set_child(Some(&color_picker_trigger_shell));
    color_picker_trigger_host.add_overlay(&color_picker_trigger);

    let color_picker_shell_click = GestureClick::new();
    let color_picker_trigger_popup = color_picker_trigger.clone();
    color_picker_shell_click.connect_pressed(move |_, _, _, _| {
        color_picker_trigger_popup.popup();
    });
    color_picker_trigger_shell.add_controller(color_picker_shell_click);

    // Popover
    let color_popover = Popover::new();
    color_popover.set_has_arrow(false);
    color_popover.set_autohide(true);
    color_popover.set_position(gtk4::PositionType::Bottom);
    color_popover.set_offset(0, 4);
    color_popover.add_css_class("editor-color-popover");

    // Popover content
    let popover_root = GtkBox::new(Orientation::Horizontal, 0);
    popover_root.add_css_class("editor-color-popover-body");
    popover_root.set_halign(gtk4::Align::Start);
    popover_root.set_hexpand(false);

    // Swatches side
    let swatches_side = GtkBox::new(Orientation::Vertical, 0);
    swatches_side.add_css_class("editor-color-swatches-side");
    swatches_side.set_hexpand(false);

    let color_columns = GtkBox::new(Orientation::Horizontal, 6);
    color_columns.add_css_class("editor-color-dropdown-columns");
    color_columns.set_halign(gtk4::Align::Center);
    color_columns.set_homogeneous(true);

    // Column 1: default colors
    let color_column_primary = GtkBox::new(Orientation::Vertical, 1);
    color_column_primary.add_css_class("editor-color-dropdown-column");
    color_column_primary.set_halign(gtk4::Align::Center);
    for button in &color_buttons {
        color_column_primary.append(button);
    }

    // Column 2: custom slots
    let color_column_secondary = GtkBox::new(Orientation::Vertical, 1);
    color_column_secondary.add_css_class("editor-color-dropdown-column");
    color_column_secondary.set_halign(gtk4::Align::Center);

    let custom_slot_colors = Rc::new(RefCell::new(load_persisted_custom_slot_colors(
        color_buttons.len(),
    )));
    let custom_slot_css_provider = CssProvider::new();
    let mut custom_slot_buttons: Vec<Button> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_overlays: Vec<Overlay> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_placeholders: Vec<GtkBox> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_dots: Vec<GtkBox> = Vec::with_capacity(color_buttons.len());
    let mut custom_slot_remove_buttons: Vec<Button> = Vec::with_capacity(color_buttons.len());

    for index in 0..color_buttons.len() {
        let placeholder_btn = Button::new();
        placeholder_btn.set_has_frame(false);
        placeholder_btn.set_focusable(false);
        placeholder_btn.add_css_class("editor-color-button");
        placeholder_btn.add_css_class("editor-custom-color-slot");

        let placeholder = GtkBox::new(Orientation::Horizontal, 0);
        placeholder.set_size_request(18, 18);
        placeholder.set_halign(gtk4::Align::Center);
        placeholder.set_valign(gtk4::Align::Center);
        placeholder.add_css_class("editor-color-placeholder-dot");

        let custom_dot = GtkBox::new(Orientation::Horizontal, 0);
        custom_dot.set_size_request(18, 18);
        custom_dot.set_halign(gtk4::Align::Center);
        custom_dot.set_valign(gtk4::Align::Center);
        custom_dot.add_css_class("editor-color-dot");
        custom_dot.set_widget_name(&format!("editor-custom-color-dot-{index}"));

        let remove_btn = Button::new();
        remove_btn.set_has_frame(false);
        remove_btn.set_focusable(false);
        remove_btn.set_visible(false);
        remove_btn.set_tooltip_text(Some("Remove custom color"));
        remove_btn.set_halign(gtk4::Align::End);
        remove_btn.set_valign(gtk4::Align::Start);
        remove_btn.set_margin_top(-3);
        remove_btn.set_margin_end(-3);
        remove_btn.add_css_class("editor-custom-color-remove-button");
        let remove_icon = Image::from_icon_name("window-close-symbolic");
        remove_icon.set_pixel_size(7);
        remove_icon.add_css_class("editor-custom-color-remove-icon");
        remove_btn.set_child(Some(&remove_icon));

        placeholder_btn.set_child(Some(&placeholder));

        let slot_overlay = Overlay::new();
        slot_overlay.add_css_class("editor-custom-color-slot-overlay");
        slot_overlay.set_child(Some(&placeholder_btn));
        slot_overlay.add_overlay(&remove_btn);

        let hover_controller = EventControllerMotion::new();
        let remove_btn_enter = remove_btn.clone();
        let custom_slot_colors_enter = custom_slot_colors.clone();
        hover_controller.connect_enter(move |_, _, _| {
            if custom_slot_colors_enter.borrow()[index].is_some() {
                remove_btn_enter.set_visible(true);
            }
        });
        let remove_btn_leave = remove_btn.clone();
        hover_controller.connect_leave(move |_| {
            remove_btn_leave.set_visible(false);
        });
        slot_overlay.add_controller(hover_controller);

        color_column_secondary.append(&slot_overlay);
        custom_slot_overlays.push(slot_overlay.clone());
        custom_slot_buttons.push(placeholder_btn);
        custom_slot_placeholders.push(placeholder);
        custom_slot_dots.push(custom_dot);
        custom_slot_remove_buttons.push(remove_btn);
    }

    color_columns.append(&color_column_primary);
    color_columns.append(&color_column_secondary);

    // Universal color row
    let color_universal_row = GtkBox::new(Orientation::Horizontal, 4);
    color_universal_row.add_css_class("editor-color-dropdown-footer");
    color_universal_row.set_halign(gtk4::Align::Center);

    let universal_color_btn = Button::new();
    universal_color_btn.set_has_frame(false);
    universal_color_btn.set_focusable(false);
    universal_color_btn.set_tooltip_text(Some("Color picker"));
    universal_color_btn.add_css_class("editor-universal-color-button");
    let universal_color_wheel = GtkBox::new(Orientation::Horizontal, 0);
    universal_color_wheel.set_size_request(22, 22);
    universal_color_wheel.add_css_class("editor-universal-color-wheel");
    universal_color_btn.set_child(Some(&universal_color_wheel));

    let universal_arrow_btn = Button::new();
    universal_arrow_btn.set_has_frame(false);
    universal_arrow_btn.set_focusable(false);
    universal_arrow_btn.set_tooltip_text(Some("Open color picker"));
    universal_arrow_btn.add_css_class("editor-universal-arrow-button");
    let universal_arrow_icon = Image::from_icon_name(icon_names::GO_NEXT);
    universal_arrow_icon.set_pixel_size(12);
    universal_arrow_btn.set_child(Some(&universal_arrow_icon));

    color_universal_row.append(&universal_color_btn);
    color_universal_row.append(&universal_arrow_btn);

    swatches_side.append(&color_columns);
    swatches_side.append(&color_universal_row);

    // Picker panel
    let picker_panel = GtkBox::new(Orientation::Vertical, 10);
    picker_panel.add_css_class("editor-color-picker-panel");
    picker_panel.set_halign(gtk4::Align::Start);
    picker_panel.set_hexpand(false);
    picker_panel.set_width_request(PICKER_PANEL_WIDTH);
    picker_panel.set_visible(false);

    let picker_state = Rc::new(RefCell::new(PickerColorState::from_color(
        DRAW_COLORS[DEFAULT_COLOR_INDEX],
    )));
    let picker_update_in_progress = Rc::new(Cell::new(false));

    // Gradient area
    let gradient_area = DrawingArea::new();
    gradient_area.set_content_width(PICKER_PANEL_WIDTH);
    gradient_area.set_content_height(150);
    gradient_area.set_size_request(PICKER_PANEL_WIDTH, 150);
    gradient_area.set_halign(gtk4::Align::Start);
    gradient_area.set_hexpand(false);
    gradient_area.add_css_class("editor-gradient-area");
    let picker_state_draw = picker_state.clone();
    gradient_area.set_draw_func(move |_area, cr, width, height| {
        let picker = *picker_state_draw.borrow();
        let w = width as f64;
        let h = height as f64;
        let (hue_r, hue_g, hue_b) = super::types::hsv_to_rgb(picker.hue, 1.0, 1.0);
        cr.set_source_rgb(hue_r, hue_g, hue_b);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let white_grad = gtk4::cairo::LinearGradient::new(0.0, 0.0, w, 0.0);
        white_grad.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 1.0);
        white_grad.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        let _ = cr.set_source(&white_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let black_grad = gtk4::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        black_grad.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        black_grad.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
        let _ = cr.set_source(&black_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let cx = picker.saturation * w;
        let cy = (1.0 - picker.value) * h;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.set_line_width(2.0);
        cr.arc(cx, cy, 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.stroke();
    });

    // Hue slider
    let hue_slider = Scale::with_range(Orientation::Horizontal, 0.0, 360.0, 1.0);
    hue_slider.set_draw_value(false);
    hue_slider.set_hexpand(false);
    hue_slider.set_halign(gtk4::Align::Start);
    hue_slider.set_width_request(PICKER_SLIDER_WIDTH);
    hue_slider.add_css_class("editor-hue-slider");

    let hue_row = GtkBox::new(Orientation::Horizontal, 8);
    hue_row.set_halign(gtk4::Align::Start);
    hue_row.set_hexpand(false);
    hue_row.set_width_request(PICKER_PANEL_WIDTH);

    let hue_preview = GtkBox::new(Orientation::Horizontal, 0);
    hue_preview.set_size_request(24, 24);
    hue_preview.set_halign(gtk4::Align::Start);
    hue_preview.add_css_class("editor-color-preview");
    hue_preview.set_widget_name("editor-picker-preview-hue");

    hue_row.append(&hue_slider);
    hue_row.append(&hue_preview);

    // Opacity slider
    let opacity_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    opacity_slider.set_draw_value(false);
    opacity_slider.set_hexpand(false);
    opacity_slider.set_halign(gtk4::Align::Start);
    opacity_slider.set_width_request(PICKER_SLIDER_WIDTH);
    opacity_slider.add_css_class("editor-opacity-slider");
    opacity_slider.set_widget_name("editor-picker-opacity-slider");

    let opacity_row = GtkBox::new(Orientation::Horizontal, 8);
    opacity_row.set_halign(gtk4::Align::Start);
    opacity_row.set_hexpand(false);
    opacity_row.set_width_request(PICKER_PANEL_WIDTH);
    let opacity_row_spacer = GtkBox::new(Orientation::Horizontal, 0);
    opacity_row_spacer.set_size_request(24, 24);
    opacity_row.append(&opacity_slider);
    opacity_row.append(&opacity_row_spacer);

    // Hex entry
    let hex_entry = Entry::new();
    hex_entry.set_max_length(6);
    hex_entry.set_width_chars(6);
    hex_entry.set_max_width_chars(6);
    hex_entry.set_width_request(PICKER_HEX_ENTRY_WIDTH);
    hex_entry.set_halign(gtk4::Align::Start);
    hex_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&hex_entry, 0.5);
    hex_entry.add_css_class("editor-hex-entry");

    let hex_label = Label::new(Some("HEX"));
    hex_label.add_css_class("editor-color-field-label");
    hex_label.set_halign(gtk4::Align::Center);
    hex_label.set_xalign(0.5);

    let eyedropper_btn = Button::new();
    eyedropper_btn.set_has_frame(false);
    eyedropper_btn.set_valign(gtk4::Align::Center);
    eyedropper_btn.add_css_class("editor-eyedropper-button");
    let eyedropper_icon = Image::from_icon_name("color-select-symbolic");
    eyedropper_icon.set_pixel_size(16);
    eyedropper_btn.set_child(Some(&eyedropper_icon));

    let hex_input_row = GtkBox::new(Orientation::Horizontal, 8);
    hex_input_row.set_halign(gtk4::Align::Start);
    hex_input_row.set_hexpand(false);
    hex_input_row.set_width_request(PICKER_PANEL_WIDTH);
    hex_input_row.append(&hex_entry);
    hex_input_row.append(&eyedropper_btn);

    let hex_row = GtkBox::new(Orientation::Vertical, 2);
    hex_row.set_halign(gtk4::Align::Start);
    hex_row.set_hexpand(false);
    hex_row.set_width_request(PICKER_PANEL_WIDTH);
    hex_row.append(&hex_input_row);
    hex_row.append(&hex_label);

    // RGBA inputs
    let rgba_row = GtkBox::new(Orientation::Horizontal, 6);
    rgba_row.set_halign(gtk4::Align::Start);
    rgba_row.set_hexpand(false);
    rgba_row.set_width_request(PICKER_PANEL_WIDTH);
    rgba_row.set_homogeneous(true);

    let r_entry = Entry::new();
    r_entry.set_max_length(3);
    r_entry.set_width_chars(3);
    r_entry.set_max_width_chars(3);
    r_entry.set_width_request(50);
    r_entry.set_halign(gtk4::Align::Start);
    r_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&r_entry, 0.5);
    r_entry.add_css_class("editor-rgba-entry");
    let r_label = Label::new(Some("R"));
    r_label.add_css_class("editor-color-field-label");
    r_label.set_halign(gtk4::Align::Center);
    r_label.set_xalign(0.5);
    let r_col = GtkBox::new(Orientation::Vertical, 2);
    r_col.set_halign(gtk4::Align::Fill);
    r_col.set_hexpand(true);
    r_col.append(&r_entry);
    r_col.append(&r_label);
    rgba_row.append(&r_col);

    let g_entry = Entry::new();
    g_entry.set_max_length(3);
    g_entry.set_width_chars(3);
    g_entry.set_max_width_chars(3);
    g_entry.set_width_request(50);
    g_entry.set_halign(gtk4::Align::Start);
    g_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&g_entry, 0.5);
    g_entry.add_css_class("editor-rgba-entry");
    let g_label = Label::new(Some("G"));
    g_label.add_css_class("editor-color-field-label");
    g_label.set_halign(gtk4::Align::Center);
    g_label.set_xalign(0.5);
    let g_col = GtkBox::new(Orientation::Vertical, 2);
    g_col.set_halign(gtk4::Align::Fill);
    g_col.set_hexpand(true);
    g_col.append(&g_entry);
    g_col.append(&g_label);
    rgba_row.append(&g_col);

    let b_entry = Entry::new();
    b_entry.set_max_length(3);
    b_entry.set_width_chars(3);
    b_entry.set_max_width_chars(3);
    b_entry.set_width_request(50);
    b_entry.set_halign(gtk4::Align::Start);
    b_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&b_entry, 0.5);
    b_entry.add_css_class("editor-rgba-entry");
    let b_label = Label::new(Some("B"));
    b_label.add_css_class("editor-color-field-label");
    b_label.set_halign(gtk4::Align::Center);
    b_label.set_xalign(0.5);
    let b_col = GtkBox::new(Orientation::Vertical, 2);
    b_col.set_halign(gtk4::Align::Fill);
    b_col.set_hexpand(true);
    b_col.append(&b_entry);
    b_col.append(&b_label);
    rgba_row.append(&b_col);

    let a_entry = Entry::new();
    a_entry.set_max_length(3);
    a_entry.set_width_chars(3);
    a_entry.set_max_width_chars(3);
    a_entry.set_width_request(50);
    a_entry.set_halign(gtk4::Align::Start);
    a_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&a_entry, 0.5);
    a_entry.add_css_class("editor-rgba-entry");
    let a_label = Label::new(Some("A"));
    a_label.add_css_class("editor-color-field-label");
    a_label.set_halign(gtk4::Align::Center);
    a_label.set_xalign(0.5);
    let a_col = GtkBox::new(Orientation::Vertical, 2);
    a_col.set_halign(gtk4::Align::Fill);
    a_col.set_hexpand(true);
    a_col.append(&a_entry);
    a_col.append(&a_label);
    rgba_row.append(&a_col);

    // Add to colors button
    let add_to_colors_btn = Button::with_label("\u{FF0B} Add to My Colors");
    add_to_colors_btn.set_has_frame(false);
    add_to_colors_btn.set_hexpand(false);
    add_to_colors_btn.set_halign(gtk4::Align::Start);
    add_to_colors_btn.set_width_request(PICKER_PANEL_WIDTH - 32);
    add_to_colors_btn.add_css_class("editor-add-to-colors-button");

    universal_color_wheel.set_widget_name("editor-picker-universal-wheel");

    let picker_css_provider = CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &picker_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &custom_slot_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let refresh_custom_color_slots: Rc<dyn Fn()> = Rc::new({
        let custom_slot_colors = custom_slot_colors.clone();
        let custom_slot_buttons = custom_slot_buttons.clone();
        let custom_slot_placeholders = custom_slot_placeholders.clone();
        let custom_slot_dots = custom_slot_dots.clone();
        let custom_slot_remove_buttons = custom_slot_remove_buttons.clone();
        let custom_slot_css_provider = custom_slot_css_provider.clone();
        move || {
            let custom_colors = custom_slot_colors.borrow();
            for (index, slot_button) in custom_slot_buttons.iter().enumerate() {
                if custom_colors[index].is_some() {
                    slot_button.add_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_dots[index]));
                } else {
                    slot_button.remove_css_class("has-custom-color");
                    slot_button.set_child(Some(&custom_slot_placeholders[index]));
                }

                custom_slot_remove_buttons[index].set_visible(false);
            }

            let css = custom_color_slots_css(custom_colors.as_slice());
            custom_slot_css_provider.load_from_data(&css);
        }
    });
    refresh_custom_color_slots();

    let picker_content = GtkBox::new(Orientation::Vertical, 12);
    picker_content.set_halign(gtk4::Align::Start);
    picker_content.set_hexpand(false);
    picker_content.set_width_request(PICKER_PANEL_WIDTH);
    picker_content.set_vexpand(true);
    picker_content.append(&gradient_area);
    picker_content.append(&hue_row);
    picker_content.append(&opacity_row);
    picker_content.append(&hex_row);
    picker_content.append(&rgba_row);

    picker_panel.append(&picker_content);
    picker_panel.append(&add_to_colors_btn);

    popover_root.append(&swatches_side);
    popover_root.append(&picker_panel);

    // Wire universal buttons
    let set_picker_panel_visibility: Rc<dyn Fn(bool)> = Rc::new({
        let picker_panel = picker_panel.clone();
        let universal_arrow_icon = universal_arrow_icon.clone();
        move |visible| {
            picker_panel.set_visible(visible);
            if visible {
                universal_arrow_icon.add_css_class("editor-picker-back-arrow");
            } else {
                universal_arrow_icon.remove_css_class("editor-picker-back-arrow");
            }
        }
    });

    let picker_panel_toggle_arrow = picker_panel.clone();
    let set_picker_panel_visibility_arrow = set_picker_panel_visibility.clone();
    universal_arrow_btn.connect_clicked(move |_| {
        set_picker_panel_visibility_arrow(!picker_panel_toggle_arrow.is_visible());
    });

    let picker_panel_toggle_wheel = picker_panel.clone();
    let set_picker_panel_visibility_wheel = set_picker_panel_visibility.clone();
    universal_color_btn.connect_clicked(move |_| {
        set_picker_panel_visibility_wheel(!picker_panel_toggle_wheel.is_visible());
    });

    // Reset picker panel when popover closes
    let set_picker_panel_visibility_closed = set_picker_panel_visibility.clone();
    color_popover.connect_closed(move |_| {
        set_picker_panel_visibility_closed(false);
    });

    set_picker_panel_visibility(false);

    color_popover.set_child(Some(&popover_root));
    color_picker_trigger.set_popover(Some(&color_popover));

    let color_group = GtkBox::new(Orientation::Horizontal, 0);
    color_group.add_css_class("editor-color-group");
    color_group.append(&color_picker_trigger_host);

    let size_down_btn = Button::with_label("-");
    size_down_btn.set_has_frame(false);
    size_down_btn.set_tooltip_text(Some("Decrease stroke size"));
    size_down_btn.add_css_class("editor-tool-button");

    let size_up_btn = Button::with_label("+");
    size_up_btn.set_has_frame(false);
    size_up_btn.set_tooltip_text(Some("Increase stroke size"));
    size_up_btn.add_css_class("editor-tool-button");

    let size_group = GtkBox::new(Orientation::Horizontal, 2);
    size_group.add_css_class("editor-tools-group");
    size_group.add_css_class("editor-size-group");
    size_group.append(&size_down_btn);
    size_group.append(&size_up_btn);

    let crop_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    crop_tools_group.add_css_class("editor-tools-group");
    crop_tools_group.append(&crop_btn);

    let background_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    background_tools_group.add_css_class("editor-tools-group");
    background_tools_group.append(&background_btn);

    let crop_type_button = MenuButton::new();
    crop_type_button.set_has_frame(false);
    crop_type_button.set_focusable(false);
    crop_type_button.set_icon_name("");
    crop_type_button.add_css_class("editor-crop-type-button");
    crop_type_button.add_css_class("editor-tool-button");
    crop_type_button.add_css_class("flat");
    crop_type_button.set_tooltip_text(Some("Crop type"));

    let crop_type_label = Label::new(Some("Freeform"));
    crop_type_label.add_css_class("editor-crop-type-label");
    crop_type_label.set_xalign(0.0);

    let crop_type_arrow_box = GtkBox::new(Orientation::Horizontal, 0);
    crop_type_arrow_box.add_css_class("editor-crop-type-arrow-box");
    crop_type_arrow_box.set_halign(gtk4::Align::Center);
    crop_type_arrow_box.set_valign(gtk4::Align::Center);
    let crop_type_arrow = Image::from_icon_name("pan-down-symbolic");
    crop_type_arrow.set_pixel_size(10);
    crop_type_arrow.add_css_class("editor-crop-type-arrow");
    crop_type_arrow_box.append(&crop_type_arrow);

    let crop_type_shell = GtkBox::new(Orientation::Horizontal, 8);
    crop_type_shell.add_css_class("editor-crop-type-shell");
    crop_type_shell.set_valign(gtk4::Align::Fill);
    crop_type_shell.append(&crop_type_label);
    crop_type_shell.append(&crop_type_arrow_box);

    let crop_type_host = Overlay::new();
    crop_type_host.set_size_request(68, 30);
    crop_type_host.set_valign(gtk4::Align::Center);
    crop_type_host.set_child(Some(&crop_type_shell));
    crop_type_host.add_overlay(&crop_type_button);
    crop_type_button.set_valign(gtk4::Align::Fill);
    crop_type_button.set_halign(gtk4::Align::Fill);

    let crop_type_popover = Popover::new();
    crop_type_popover.set_has_arrow(false);
    crop_type_popover.set_autohide(true);
    crop_type_popover.set_position(gtk4::PositionType::Bottom);
    crop_type_popover.set_offset(0, 4);
    crop_type_popover.add_css_class("editor-crop-type-popover");

    let crop_type_list = GtkBox::new(Orientation::Vertical, 4);
    crop_type_list.add_css_class("editor-crop-type-popover-body");

    crop_type_popover.set_child(Some(&crop_type_list));
    crop_type_button.set_popover(Some(&crop_type_popover));

    let crop_type_group = GtkBox::new(Orientation::Horizontal, 0);
    crop_type_group.add_css_class("editor-tools-group");
    crop_type_group.add_css_class("editor-crop-type-group");
    crop_type_group.append(&crop_type_host);

    let crop_type_shell_click = GestureClick::new();
    let crop_type_button_popup = crop_type_button.clone();
    crop_type_shell_click.connect_pressed(move |_, _, _, _| {
        crop_type_button_popup.popup();
    });
    crop_type_shell.add_controller(crop_type_shell_click);

    let crop_width_entry = Entry::new();
    crop_width_entry.set_editable(false);
    crop_width_entry.set_focusable(false);
    crop_width_entry.set_width_chars(5);
    crop_width_entry.set_max_width_chars(6);
    crop_width_entry.set_width_request(68);
    crop_width_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&crop_width_entry, 0.5);
    crop_width_entry.add_css_class("editor-crop-size-entry");

    let crop_size_separator = Label::new(Some("×"));
    crop_size_separator.add_css_class("editor-crop-size-separator");

    let crop_height_entry = Entry::new();
    crop_height_entry.set_editable(false);
    crop_height_entry.set_focusable(false);
    crop_height_entry.set_width_chars(5);
    crop_height_entry.set_max_width_chars(6);
    crop_height_entry.set_width_request(68);
    crop_height_entry.set_hexpand(false);
    gtk4::prelude::EditableExt::set_alignment(&crop_height_entry, 0.5);
    crop_height_entry.add_css_class("editor-crop-size-entry");

    let crop_size_group = GtkBox::new(Orientation::Horizontal, 4);
    crop_size_group.add_css_class("editor-tools-group");
    crop_size_group.add_css_class("editor-crop-size-group");
    crop_size_group.append(&crop_width_entry);
    crop_size_group.append(&crop_size_separator);
    crop_size_group.append(&crop_height_entry);

    let crop_mode_group = GtkBox::new(Orientation::Horizontal, 8);
    crop_mode_group.add_css_class("editor-crop-mode-group");
    crop_mode_group.append(&crop_type_group);
    crop_mode_group.append(&crop_size_group);

    let primary_tools_group = GtkBox::new(Orientation::Horizontal, 2);
    primary_tools_group.add_css_class("editor-tools-group");
    primary_tools_group.add_css_class("editor-primary-tools-group");
    primary_tools_group.append(&select_btn);
    primary_tools_group.append(&draw_btn);
    primary_tools_group.append(&sep_1);
    primary_tools_group.append(&box_btn);
    primary_tools_group.append(&circle_btn);
    primary_tools_group.append(&arrow_btn);
    primary_tools_group.append(&line_btn);
    primary_tools_group.append(&text_btn);
    primary_tools_group.append(&blur_btn);
    primary_tools_group.append(&focus_btn);
    primary_tools_group.append(&censor_btn);
    primary_tools_group.append(&number_btn);
    primary_tools_group.append(&highlighter_btn);
    primary_tools_group.append(&sep_2);

    let standard_mode_group = GtkBox::new(Orientation::Horizontal, 10);
    standard_mode_group.add_css_class("editor-toolbar-mode-group");
    standard_mode_group.append(&primary_tools_group);
    standard_mode_group.append(&size_group);

    let toolbar_mode_stack = Stack::new();
    toolbar_mode_stack.add_css_class("editor-toolbar-mode-stack");
    toolbar_mode_stack.set_hhomogeneous(false);
    toolbar_mode_stack.set_vhomogeneous(false);
    toolbar_mode_stack.add_named(&standard_mode_group, Some("standard"));
    toolbar_mode_stack.add_named(&crop_mode_group, Some("crop"));
    toolbar_mode_stack.set_visible_child_name("standard");

    let center_group = GtkBox::new(Orientation::Horizontal, 10);
    center_group.add_css_class("editor-toolbar-center");
    center_group.append(&crop_tools_group);
    center_group.append(&background_tools_group);
    center_group.append(&toolbar_mode_stack);
    center_group.append(&color_group);
    left_group.append(&center_group);

    let background_sidebar = GtkBox::new(Orientation::Vertical, 10);
    background_sidebar.add_css_class("editor-background-sidebar");
    background_sidebar.set_width_request(BACKGROUND_SIDEBAR_WIDTH);
    background_sidebar.set_hexpand(false);
    background_sidebar.set_visible(false);
    background_sidebar.set_vexpand(true);

    let background_sidebar_options = GtkBox::new(Orientation::Vertical, 8);
    background_sidebar_options.add_css_class("editor-background-sidebar-options");

    let background_none_btn = Button::with_label("None");
    background_none_btn.set_has_frame(false);
    background_none_btn.set_halign(gtk4::Align::Start);
    background_none_btn.set_hexpand(false);
    background_none_btn.set_size_request(312, -1);
    background_none_btn.add_css_class("editor-background-option-button");
    background_none_btn.add_css_class("active-background-option");

    let gradients_section = GtkBox::new(Orientation::Vertical, 10);
    gradients_section.add_css_class("editor-background-gradients-section");

    let gradients_title = Label::new(Some("Gradients"));
    gradients_title.add_css_class("editor-background-section-title");
    gradients_title.set_xalign(0.0);

    let gradients_grid = GtkBox::new(Orientation::Vertical, 8);
    gradients_grid.add_css_class("editor-background-gradients-grid");

    let mut background_preview_areas: Vec<DrawingArea> =
        Vec::with_capacity(BACKGROUND_GRADIENT_PREVIEW_FILES.len());
    let mut background_preview_surfaces: Vec<Rc<RefCell<Option<gtk4::cairo::ImageSurface>>>> =
        Vec::with_capacity(BACKGROUND_GRADIENT_PREVIEW_FILES.len());

    for (row_index, chunk) in BACKGROUND_GRADIENT_PREVIEW_FILES.chunks(5).enumerate() {
        let gradient_row = GtkBox::new(Orientation::Horizontal, 8);
        gradient_row.add_css_class("editor-background-gradients-row");

        for (column_index, _) in chunk.iter().enumerate() {
            let preview_index = row_index * 5 + column_index;
            let (preview_button, preview_area, preview_surface) =
                build_background_gradient_preview_button(preview_index);
            background_preview_areas.push(preview_area);
            background_preview_surfaces.push(preview_surface);
            gradient_row.append(&preview_button);
        }

        gradients_grid.append(&gradient_row);
    }

    let background_preview_areas = Rc::new(background_preview_areas);
    let background_preview_surfaces = Rc::new(background_preview_surfaces);
    let background_gradient_previews_started = Rc::new(Cell::new(false));
    let start_background_gradient_preview_loading: Rc<dyn Fn()> = Rc::new({
        let background_preview_areas = background_preview_areas.clone();
        let background_preview_surfaces = background_preview_surfaces.clone();
        let background_gradient_previews_started = background_gradient_previews_started.clone();
        move || {
            if background_gradient_previews_started.replace(true) {
                return;
            }

            let (sender, receiver) = std::sync::mpsc::channel::<(usize, RgbaImage)>();
            std::thread::spawn(move || {
                for (index, file_name) in BACKGROUND_GRADIENT_PREVIEW_FILES.iter().enumerate() {
                    if let Some(preview_image) = load_background_gradient_preview_image(
                        file_name,
                        BACKGROUND_GRADIENT_PREVIEW_SIZE,
                    ) {
                        if sender.send((index, preview_image)).is_err() {
                            break;
                        }
                    }
                }
            });

            let background_preview_areas = background_preview_areas.clone();
            let background_preview_surfaces = background_preview_surfaces.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(16), move || loop {
                match receiver.try_recv() {
                    Ok((index, preview_image)) => {
                        if let Some(surface) = rgba_image_to_surface(&preview_image) {
                            *background_preview_surfaces[index].borrow_mut() = Some(surface);
                            background_preview_areas[index].queue_draw();
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        return glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return glib::ControlFlow::Break;
                    }
                }
            });
        }
    });

    gradients_section.append(&gradients_title);
    gradients_section.append(&gradients_grid);

    let wallpaper_section = GtkBox::new(Orientation::Vertical, 10);
    wallpaper_section.add_css_class("editor-background-wallpaper-section");

    let wallpaper_title = Label::new(Some("Wallpaper"));
    wallpaper_title.add_css_class("editor-background-section-title");
    wallpaper_title.set_xalign(0.0);

    let wallpaper_grid = GtkBox::new(Orientation::Vertical, 8);
    wallpaper_grid.add_css_class("editor-background-wallpaper-grid");

    let wallpaper_preview_path = detect_system_wallpaper_path()
        .unwrap_or_else(|| background_gradient_asset_path(BACKGROUND_GRADIENT_PREVIEW_FILES[0]));
    let wallpaper_preview_surface =
        load_background_preview_image(&wallpaper_preview_path, BACKGROUND_GRADIENT_PREVIEW_SIZE)
            .and_then(|preview_image| rgba_image_to_surface(&preview_image));
    let wallpaper_preview_label = wallpaper_preview_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(|file_name| format!("Wallpaper: {file_name}"))
        .unwrap_or_else(|| "Wallpaper".to_string());
    let wallpaper_previews = Rc::new(RefCell::new(vec![(
        wallpaper_preview_label,
        wallpaper_preview_surface,
    )]));
    let add_wallpaper_btn = build_background_add_wallpaper_button();

    {
        let previews = wallpaper_previews.borrow();
        rebuild_wallpaper_preview_grid(&wallpaper_grid, previews.as_slice(), &add_wallpaper_btn);
    }

    add_wallpaper_btn.connect_clicked({
        let wallpaper_previews = wallpaper_previews.clone();
        let wallpaper_grid = wallpaper_grid.clone();
        let add_wallpaper_btn = add_wallpaper_btn.clone();
        let window_weak = window.downgrade();
        move |_| {
            let chooser = FileChooserNative::new(
                Some("Choose wallpaper image"),
                window_weak.upgrade().as_ref(),
                FileChooserAction::Open,
                Some("Add"),
                Some("Cancel"),
            );

            let filter = FileFilter::new();
            filter.set_name(Some("Images"));
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/jpeg");
            filter.add_mime_type("image/webp");
            filter.add_mime_type("image/gif");
            filter.add_pattern("*.png");
            filter.add_pattern("*.jpg");
            filter.add_pattern("*.jpeg");
            filter.add_pattern("*.webp");
            filter.add_pattern("*.gif");
            chooser.add_filter(&filter);

            chooser.connect_response({
                let wallpaper_previews = wallpaper_previews.clone();
                let wallpaper_grid = wallpaper_grid.clone();
                let add_wallpaper_btn = add_wallpaper_btn.clone();
                move |dialog, response| {
                    if response == ResponseType::Accept {
                        if let Some(file) = dialog.file() {
                            if let Some(path) = file.path() {
                                if let Some(surface) = load_background_preview_image(
                                    &path,
                                    BACKGROUND_GRADIENT_PREVIEW_SIZE,
                                )
                                .and_then(|preview_image| rgba_image_to_surface(&preview_image))
                                {
                                    let label = path
                                        .file_name()
                                        .and_then(|file_name| file_name.to_str())
                                        .map(|file_name| format!("Wallpaper: {file_name}"))
                                        .unwrap_or_else(|| "Wallpaper".to_string());
                                    wallpaper_previews.borrow_mut().push((label, Some(surface)));
                                    let previews = wallpaper_previews.borrow();
                                    rebuild_wallpaper_preview_grid(
                                        &wallpaper_grid,
                                        previews.as_slice(),
                                        &add_wallpaper_btn,
                                    );
                                }
                            }
                        }
                    }

                    dialog.hide();
                }
            });

            chooser.show();
        }
    });

    wallpaper_section.append(&wallpaper_title);
    wallpaper_section.append(&wallpaper_grid);

    let blurred_section = GtkBox::new(Orientation::Vertical, 10);
    blurred_section.add_css_class("editor-background-blurred-section");

    let blurred_title = Label::new(Some("Blurred"));
    blurred_title.add_css_class("editor-background-section-title");
    blurred_title.set_xalign(0.0);

    let blurred_row = GtkBox::new(Orientation::Horizontal, 8);
    blurred_row.add_css_class("editor-background-blurred-row");

    for index in 0..3 {
        let blurred_button = build_background_blurred_preview_button(index);
        blurred_row.append(&blurred_button);
    }

    blurred_section.append(&blurred_title);
    blurred_section.append(&blurred_row);

    let plain_color_section = GtkBox::new(Orientation::Vertical, 10);
    plain_color_section.add_css_class("editor-background-plain-color-section");

    let plain_color_title = Label::new(Some("Plain color"));
    plain_color_title.add_css_class("editor-background-section-title");
    plain_color_title.set_xalign(0.0);

    let plain_color_grid = GtkBox::new(Orientation::Vertical, 8);
    plain_color_grid.add_css_class("editor-background-plain-color-grid");

    for row_index in 0..2 {
        let plain_color_row = GtkBox::new(Orientation::Horizontal, 8);
        plain_color_row.add_css_class("editor-background-plain-color-row");
        plain_color_row.set_hexpand(true);
        plain_color_row.set_homogeneous(true);

        for column_index in 0..9 {
            let color_index = row_index * 9 + column_index;
            let color_cell = build_background_plain_color_cell(color_index);
            plain_color_row.append(&color_cell);
        }

        let row_end_spacer = GtkBox::new(Orientation::Horizontal, 0);
        row_end_spacer.add_css_class("editor-background-plain-color-end-spacer");
        plain_color_row.append(&row_end_spacer);

        plain_color_grid.append(&plain_color_row);
    }

    plain_color_section.append(&plain_color_title);
    plain_color_section.append(&plain_color_grid);

    let background_padding_divider_row = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider_row.add_css_class("editor-background-divider-row");
    let background_padding_divider = GtkBox::new(Orientation::Horizontal, 0);
    background_padding_divider.add_css_class("editor-background-divider");
    background_padding_divider.set_size_request(252, -1);
    background_padding_divider_row.append(&background_padding_divider);

    let padding_section = GtkBox::new(Orientation::Vertical, 10);
    padding_section.add_css_class("editor-background-padding-section");

    let padding_title = Label::new(Some("Padding"));
    padding_title.add_css_class("editor-background-section-title");
    padding_title.set_xalign(0.0);

    let padding_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    padding_slider_row.add_css_class("editor-background-padding-slider-row");
    let padding_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    padding_slider.add_css_class("editor-opacity-slider");
    padding_slider.add_css_class("editor-background-padding-slider");
    padding_slider.set_draw_value(false);
    padding_slider.set_value(24.0);
    padding_slider.set_size_request(252, -1);
    padding_slider_row.append(&padding_slider);

    padding_section.append(&padding_title);
    padding_section.append(&padding_slider_row);

    let compact_controls = GtkBox::new(Orientation::Vertical, 8);
    compact_controls.add_css_class("editor-background-compact-controls");

    let insert_shadow_row = GtkBox::new(Orientation::Horizontal, 16);
    insert_shadow_row.add_css_class("editor-background-compact-controls-row");
    insert_shadow_row.set_homogeneous(true);

    let insert_section = GtkBox::new(Orientation::Vertical, 10);
    insert_section.add_css_class("editor-background-compact-slider-section");

    let insert_title = Label::new(Some("Insert"));
    insert_title.add_css_class("editor-background-section-title");
    insert_title.set_xalign(0.0);

    let insert_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    insert_slider_row.add_css_class("editor-background-compact-slider-row");
    let insert_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    insert_slider.add_css_class("editor-opacity-slider");
    insert_slider.add_css_class("editor-background-compact-slider");
    insert_slider.set_draw_value(false);
    insert_slider.set_value(20.0);
    insert_slider.set_size_request(136, -1);
    insert_slider_row.append(&insert_slider);

    insert_section.append(&insert_title);
    insert_section.append(&insert_slider_row);

    let auto_balance_section = GtkBox::new(Orientation::Vertical, 2);
    auto_balance_section.add_css_class("editor-background-compact-slider-section");

    let auto_balance_title = Label::new(Some("Auto-balance"));
    auto_balance_title.add_css_class("editor-background-section-title");
    auto_balance_title.set_xalign(0.0);

    let auto_balance_check_row = GtkBox::new(Orientation::Horizontal, 0);
    auto_balance_check_row.add_css_class("editor-background-checkbox-row");
    let auto_balance_check = CheckButton::with_label("");
    auto_balance_check.add_css_class("editor-background-checkbox");
    auto_balance_check.set_halign(gtk4::Align::Start);
    auto_balance_check_row.append(&auto_balance_check);

    auto_balance_section.append(&auto_balance_title);
    auto_balance_section.append(&auto_balance_check_row);

    insert_shadow_row.append(&insert_section);
    insert_shadow_row.append(&auto_balance_section);

    let shadow_corners_row = GtkBox::new(Orientation::Horizontal, 16);
    shadow_corners_row.add_css_class("editor-background-compact-controls-row");
    shadow_corners_row.set_homogeneous(true);

    let shadow_section = GtkBox::new(Orientation::Vertical, 10);
    shadow_section.add_css_class("editor-background-compact-slider-section");

    let shadow_title = Label::new(Some("Shadow"));
    shadow_title.add_css_class("editor-background-section-title");
    shadow_title.set_xalign(0.0);

    let shadow_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    shadow_slider_row.add_css_class("editor-background-compact-slider-row");
    let shadow_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    shadow_slider.add_css_class("editor-opacity-slider");
    shadow_slider.add_css_class("editor-background-compact-slider");
    shadow_slider.set_draw_value(false);
    shadow_slider.set_value(28.0);
    shadow_slider.set_size_request(136, -1);
    shadow_slider_row.append(&shadow_slider);

    shadow_section.append(&shadow_title);
    shadow_section.append(&shadow_slider_row);

    let alignment_section = GtkBox::new(Orientation::Vertical, 10);
    alignment_section.add_css_class("editor-background-compact-slider-section");

    let alignment_title = Label::new(Some("Alignment"));
    alignment_title.add_css_class("editor-background-section-title");
    alignment_title.set_xalign(0.0);

    let alignment_grid = GtkBox::new(Orientation::Vertical, 6);
    alignment_grid.add_css_class("editor-background-alignment-grid");
    alignment_grid.set_halign(gtk4::Align::Start);

    let alignment_positions = [
        [
            ("top-left", "Top left"),
            ("top-center", "Top middle"),
            ("top-right", "Top right"),
        ],
        [
            ("center-left", "Left"),
            ("center", "Middle"),
            ("center-right", "Right"),
        ],
        [
            ("bottom-left", "Bottom left"),
            ("bottom-center", "Bottom middle"),
            ("bottom-right", "Bottom right"),
        ],
    ];

    for row_items in alignment_positions {
        let alignment_row = GtkBox::new(Orientation::Horizontal, 6);
        alignment_row.add_css_class("editor-background-alignment-row");
        alignment_row.set_homogeneous(true);

        for (position_class, tooltip) in row_items {
            let alignment_frame = GtkBox::new(Orientation::Horizontal, 0);
            alignment_frame.add_css_class("editor-background-alignment-icon-frame");
            alignment_frame.add_css_class(position_class);
            alignment_frame.set_size_request(6, 4);

            match position_class {
                "top-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "top-center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "top-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::Start);
                }
                "center-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "center-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::Center);
                }
                "bottom-left" => {
                    alignment_frame.set_halign(gtk4::Align::Start);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                "bottom-center" => {
                    alignment_frame.set_halign(gtk4::Align::Center);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                "bottom-right" => {
                    alignment_frame.set_halign(gtk4::Align::End);
                    alignment_frame.set_valign(gtk4::Align::End);
                }
                _ => {}
            }

            let alignment_icon = Overlay::new();
            alignment_icon.add_css_class("editor-background-alignment-icon");
            alignment_icon.set_size_request(26, 17);
            alignment_icon.set_child(Some(&alignment_frame));

            let alignment_button = Button::new();
            alignment_button.set_child(Some(&alignment_icon));
            alignment_button.set_has_frame(false);
            alignment_button.set_focusable(false);
            alignment_button.set_hexpand(true);
            alignment_button.set_tooltip_text(Some(tooltip));
            alignment_button.add_css_class("editor-background-alignment-button");
            alignment_row.append(&alignment_button);
        }

        alignment_grid.append(&alignment_row);
    }

    alignment_section.append(&alignment_title);
    alignment_section.append(&alignment_grid);
    shadow_section.append(&alignment_section);

    let corners_section = GtkBox::new(Orientation::Vertical, 10);
    corners_section.add_css_class("editor-background-compact-slider-section");

    let ratio_section = GtkBox::new(Orientation::Vertical, 10);
    ratio_section.add_css_class("editor-background-compact-slider-section");

    let ratio_title = Label::new(Some("Ratio"));
    ratio_title.add_css_class("editor-background-section-title");
    ratio_title.set_xalign(0.0);

    let ratio_dropdown_row = GtkBox::new(Orientation::Horizontal, 0);
    ratio_dropdown_row.add_css_class("editor-background-ratio-dropdown-row");
    let ratio_dropdown = gtk4::DropDown::from_strings(&[
        "Original",
        "1:1",
        "4:3",
        "16:9",
        "21:9",
    ]);
    ratio_dropdown.add_css_class("editor-background-ratio-dropdown");
    ratio_dropdown.set_size_request(130, -1);
    ratio_dropdown.set_halign(gtk4::Align::Start);
    ratio_dropdown.set_selected(0);
    ratio_dropdown_row.append(&ratio_dropdown);

    ratio_section.append(&ratio_title);
    ratio_section.append(&ratio_dropdown_row);

    let corners_title = Label::new(Some("Corners"));
    corners_title.add_css_class("editor-background-section-title");
    corners_title.set_xalign(0.0);

    let corners_slider_row = GtkBox::new(Orientation::Horizontal, 0);
    corners_slider_row.add_css_class("editor-background-compact-slider-row");
    let corners_slider = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    corners_slider.add_css_class("editor-opacity-slider");
    corners_slider.add_css_class("editor-background-compact-slider");
    corners_slider.set_draw_value(false);
    corners_slider.set_value(18.0);
    corners_slider.set_size_request(136, -1);
    corners_slider_row.append(&corners_slider);

    corners_section.append(&corners_title);
    corners_section.append(&corners_slider_row);
    corners_section.append(&ratio_section);

    shadow_corners_row.append(&shadow_section);
    shadow_corners_row.append(&corners_section);

    compact_controls.append(&insert_shadow_row);
    compact_controls.append(&shadow_corners_row);

    background_sidebar_options.append(&background_none_btn);
    background_sidebar_options.append(&gradients_section);
    background_sidebar_options.append(&wallpaper_section);
    background_sidebar_options.append(&blurred_section);
    background_sidebar_options.append(&plain_color_section);
    background_sidebar_options.append(&background_padding_divider_row);
    background_sidebar_options.append(&padding_section);
    background_sidebar_options.append(&compact_controls);

    background_sidebar.append(&background_sidebar_options);

    let update_toolbar_for_tool: Rc<dyn Fn(Tool)> = Rc::new({
        let toolbar_mode_stack = toolbar_mode_stack.clone();
        let background_sidebar = background_sidebar.clone();
        let start_background_gradient_preview_loading =
            start_background_gradient_preview_loading.clone();
        let window = window.downgrade();
        let image_width = img_width as i32;
        let image_height = img_height as i32;
        move |tool| {
            toolbar_mode_stack.set_visible_child_name(if matches!(tool, Tool::Crop) {
                "crop"
            } else {
                "standard"
            });

            let background_mode = matches!(tool, Tool::Background);
            background_sidebar.set_visible(background_mode);

            if background_mode {
                start_background_gradient_preview_loading();
                let (target_width, target_height) = recommended_window_size_with_extra_width(
                    image_width,
                    image_height,
                    BACKGROUND_SIDEBAR_WIDTH,
                );
                if let Some(window) = window.upgrade() {
                    window.set_default_size(
                        window.allocated_width().max(target_width),
                        window.allocated_height().max(target_height),
                    );
                }
            }
        }
    });

    let undo_btn = icon_tool_button(icon_names::ARROW_UNDO_REGULAR, "Undo");
    let redo_btn = icon_tool_button(icon_names::ARROW_REDO_REGULAR, "Redo");
    let delete_selected_btn = icon_tool_button("edit-delete-symbolic", "Delete selected");
    undo_btn.set_sensitive(false);
    redo_btn.set_sensitive(false);
    delete_selected_btn.set_sensitive(false);

    let history_group = GtkBox::new(Orientation::Horizontal, 2);
    history_group.add_css_class("editor-tools-group");
    history_group.append(&undo_btn);
    history_group.append(&redo_btn);
    history_group.append(&delete_selected_btn);

    let right_tools = GtkBox::new(Orientation::Horizontal, 12);
    right_tools.add_css_class("editor-toolbar-right-tools");
    right_tools.append(&history_group);

    let save_btn = Button::with_label("Done");
    save_btn.set_has_frame(false);
    save_btn.add_css_class("editor-done-button");
    save_btn.add_css_class("body");
    save_btn.set_valign(gtk4::Align::Center);

    let apply_crop_btn = Button::with_label("Apply");
    apply_crop_btn.set_has_frame(false);
    apply_crop_btn.add_css_class("editor-done-button");
    apply_crop_btn.add_css_class("body");
    apply_crop_btn.set_valign(gtk4::Align::Center);
    apply_crop_btn.set_visible(false);
    apply_crop_btn.set_sensitive(false);

    let apply_crop_slot = GtkBox::new(Orientation::Horizontal, 0);
    apply_crop_slot.add_css_class("crop-apply-slot");
    apply_crop_slot.append(&apply_crop_btn);
    apply_crop_slot.set_visible(false);

    let right_group = GtkBox::new(Orientation::Horizontal, 16);
    right_group.add_css_class("editor-toolbar-right");
    right_group.append(&right_tools);
    right_group.append(&apply_crop_slot);
    right_group.append(&save_btn);
    toolbar.set_end_widget(Some(&right_group));

    // Footer
    let (pin_btn, pin_icon) = footer_icon_button(icon_names::VIEW_PIN, "Pin window");
    let drag_btn = Button::with_label("Drag me");
    drag_btn.set_has_frame(false);
    drag_btn.set_tooltip_text(Some("Drag to move editor window"));
    drag_btn.add_css_class("editor-footer-drag-button");
    drag_btn.add_css_class("body");
    let (copy_btn, _) = footer_icon_button(icon_names::COPY_REGULAR, "Copy file URI");
    let (upload_btn, _) = footer_icon_button(icon_names::CLOUD_ARROW_UP_REGULAR, "Upload");

    let footer = GtkBox::new(Orientation::Horizontal, 0);
    footer.add_css_class("editor-footer");

    let footer_left = GtkBox::new(Orientation::Horizontal, 0);
    footer_left.set_hexpand(true);
    footer_left.set_halign(gtk4::Align::Start);
    footer_left.append(&pin_btn);

    let footer_center = GtkBox::new(Orientation::Horizontal, 0);
    footer_center.set_hexpand(true);
    footer_center.set_halign(gtk4::Align::Center);
    footer_center.append(&drag_btn);

    let footer_right = GtkBox::new(Orientation::Horizontal, 6);
    footer_right.set_hexpand(true);
    footer_right.set_halign(gtk4::Align::End);
    footer_right.append(&copy_btn);
    footer_right.append(&upload_btn);

    footer.append(&footer_left);
    footer.append(&footer_center);
    footer.append(&footer_right);

    let tool_buttons = vec![
        crop_btn.clone(),
        background_btn.clone(),
        select_btn.clone(),
        draw_btn.clone(),
        box_btn.clone(),
        circle_btn.clone(),
        arrow_btn.clone(),
        line_btn.clone(),
        text_btn.clone(),
        blur_btn.clone(),
        censor_btn.clone(),
        number_btn.clone(),
        highlighter_btn.clone(),
        focus_btn.clone(),
    ];
    set_active_tool_button(&tool_buttons, 6);
    set_active_color_picker_state(
        &color_buttons,
        &color_picker_dot,
        &color_class_names,
        DEFAULT_COLOR_INDEX,
    );
    {
        let st = state.lock().unwrap();
        apply_size_control_ui_state(&st, &size_group, &size_down_btn, &size_up_btn);
    }
    update_toolbar_for_tool(Tool::Arrow);

    // Canvas
    let drawing_area = DrawingArea::new();
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(false);
    drawing_area.set_content_width(img_width as i32);
    drawing_area.set_content_height(img_height as i32);
    drawing_area.set_size_request(img_width as i32, img_height as i32);
    drawing_area.add_css_class("editor-canvas");

    let update_crop_size_fields: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let crop_width_entry = crop_width_entry.clone();
        let crop_height_entry = crop_height_entry.clone();
        move || {
            let st = state.lock().unwrap();
            if let Some(rect) = st.draft_crop_rect().or(st.crop_selection) {
                crop_width_entry.set_text(&rect.width.max(0).to_string());
                crop_height_entry.set_text(&rect.height.max(0).to_string());
            } else {
                crop_width_entry.set_text("");
                crop_height_entry.set_text("");
            }
        }
    });

    for crop_type in CropAspectRatio::ALL {
        let option_button = Button::with_label(crop_type.label());
        option_button.set_has_frame(false);
        option_button.add_css_class("editor-crop-type-option");
        let crop_type_label_option = crop_type_label.clone();
        let crop_type_popover_option = crop_type_popover.clone();
        let state_crop_type_option = state.clone();
        let drawing_area_crop_type_option = drawing_area.downgrade();
        let update_crop_size_fields_option = update_crop_size_fields.clone();
        option_button.connect_clicked(move |_| {
            crop_type_label_option.set_label(crop_type.label());
            {
                let mut st = state_crop_type_option.lock().unwrap();
                st.set_crop_aspect_ratio(crop_type);
                if st.selected_tool == Tool::Crop {
                    st.ensure_crop_selection_initialized();
                }
            }
            update_crop_size_fields_option();
            crop_type_popover_option.popdown();
            if let Some(area) = drawing_area_crop_type_option.upgrade() {
                area.queue_draw();
            }
        });
        crop_type_list.append(&option_button);
    }

    let canvas_overlay = Overlay::new();
    canvas_overlay.set_hexpand(true);
    canvas_overlay.set_vexpand(false);
    canvas_overlay.set_size_request(img_width as i32, img_height as i32);
    canvas_overlay.set_child(Some(&drawing_area));

    let canvas_scroller = ScrolledWindow::new();
    canvas_scroller.set_hexpand(true);
    canvas_scroller.set_vexpand(true);
    canvas_scroller.set_has_frame(false);
    canvas_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    canvas_scroller.set_child(Some(&canvas_overlay));

    let canvas_padding = 24_i32;

    let canvas_eyedropper_ring = DrawingArea::new();
    canvas_eyedropper_ring.add_css_class("editor-screen-eyedropper-ring");
    canvas_eyedropper_ring.set_halign(gtk4::Align::Start);
    canvas_eyedropper_ring.set_valign(gtk4::Align::Start);
    canvas_eyedropper_ring.set_size_request(EYEDROPPER_LOUPE_SIZE, EYEDROPPER_LOUPE_SIZE);
    canvas_eyedropper_ring.set_visible(false);
    canvas_eyedropper_ring.set_can_target(false);
    canvas_overlay.add_overlay(&canvas_eyedropper_ring);

    let eyedropper_mode = Rc::new(Cell::new(false));
    let eyedropper_point = Rc::new(RefCell::new(None::<Point>));
    let eyedropper_rendered = Rc::new(RefCell::new(None::<RgbaImage>));

    canvas_eyedropper_ring.set_draw_func({
        let eyedropper_point_draw = eyedropper_point.clone();
        let eyedropper_rendered_draw = eyedropper_rendered.clone();
        move |_, context, width, height| {
            let Some(point) = *eyedropper_point_draw.borrow() else {
                return;
            };

            let rendered = eyedropper_rendered_draw.borrow();
            let Some(rendered) = rendered.as_ref() else {
                return;
            };

            draw_eyedropper_loupe(context, width, height, rendered, point);
        }
    });

    let canvas_workspace = GtkBox::new(Orientation::Horizontal, 0);
    canvas_workspace.set_hexpand(true);
    canvas_workspace.set_vexpand(true);
    canvas_workspace.add_css_class("editor-canvas-workspace");
    canvas_scroller.set_hexpand(true);
    background_sidebar.set_halign(gtk4::Align::Start);
    canvas_workspace.append(&background_sidebar);
    canvas_workspace.append(&canvas_scroller);

    let canvas = GtkBox::new(Orientation::Vertical, 0);
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    canvas.add_css_class("editor-canvas-frame");
    canvas.append(&canvas_workspace);

    root.append(&toolbar);
    root.append(&canvas);
    root.append(&footer);
    window.set_child(Some(&root));

    let update_canvas_content_size: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let drawing_area = drawing_area.clone();
        let canvas_overlay = canvas_overlay.clone();
        let canvas_scroller = canvas_scroller.clone();
        let canvas_padding = canvas_padding;
        move || {
            let (image_w, image_h, crop_rect, crop_mode_active) = {
                let st = state.lock().unwrap();
                (
                    st.working_image.width().max(1) as i32,
                    st.working_image.height().max(1) as i32,
                    st.draft_crop_rect().or(st.crop_selection),
                    st.selected_tool == Tool::Crop,
                )
            };

            let scroller_width = canvas_scroller.allocated_width().max(1) as f64;
            let available_width = (scroller_width - (canvas_padding * 2 + 2) as f64).max(1.0);
            let scale = (available_width / image_w as f64).min(1.0);
            let fitted_w = ((image_w as f64) * scale).round().max(1.0) as i32;
            let fitted_h = ((image_h as f64) * scale).round().max(1.0) as i32;
            let (overflow_left, overflow_top, overflow_right, overflow_bottom) =
                crop_canvas_overflow(
                    crop_rect,
                    image_w as f64,
                    image_h as f64,
                    scale,
                    crop_mode_active,
                );
            let canvas_w = fitted_w
                + canvas_padding * 2
                + overflow_left.round() as i32
                + overflow_right.round() as i32;
            let canvas_h = fitted_h
                + canvas_padding * 2
                + overflow_top.round() as i32
                + overflow_bottom.round() as i32;

            drawing_area.set_content_width(canvas_w);
            drawing_area.set_content_height(canvas_h);
            drawing_area.set_size_request(canvas_w, canvas_h);
            canvas_overlay.set_size_request(canvas_w, canvas_h);
        }
    });
    update_canvas_content_size();

    {
        let update_canvas_content_size_tick = update_canvas_content_size.clone();
        let state_canvas_tick = state.clone();
        let last_canvas_signature = Rc::new(Cell::new((
            0_i32, 0_i32, 0_i32, 0_i32, 0_i32, 0_i32, 0_i32, false,
        )));
        let last_canvas_signature_tick = last_canvas_signature.clone();
        canvas_scroller.add_tick_callback(move |scroller, _| {
            let width = scroller.allocated_width();
            let signature = {
                let st = state_canvas_tick.lock().unwrap();
                let crop_rect = st.draft_crop_rect().or(st.crop_selection);
                let (crop_x, crop_y, crop_w, crop_h) = crop_rect
                    .map(|rect| (rect.x, rect.y, rect.width, rect.height))
                    .unwrap_or((0, 0, 0, 0));
                (
                    width,
                    st.working_image.width().max(1) as i32,
                    st.working_image.height().max(1) as i32,
                    crop_x,
                    crop_y,
                    crop_w,
                    crop_h,
                    st.selected_tool == Tool::Crop,
                )
            };
            if width > 0 && signature != last_canvas_signature_tick.get() {
                last_canvas_signature_tick.set(signature);
                update_canvas_content_size_tick();
            }
            glib::ControlFlow::Continue
        });
    }

    // Picker UI update functions
    let update_picker_ui: Rc<dyn Fn(PickerColorState)> = Rc::new({
        let hue_slider = hue_slider.clone();
        let opacity_slider = opacity_slider.clone();
        let hex_entry = hex_entry.clone();
        let r_entry = r_entry.clone();
        let g_entry = g_entry.clone();
        let b_entry = b_entry.clone();
        let a_entry = a_entry.clone();
        let gradient_area = gradient_area.clone();
        let picker_css_provider = picker_css_provider.clone();
        let picker_update_in_progress = picker_update_in_progress.clone();
        move |picker| {
            picker_update_in_progress.set(true);

            hue_slider.set_value(picker.hue);
            opacity_slider.set_value(picker.alpha * 100.0);

            let color = picker.to_color();
            let (r, g, b, _) = super::color::draw_color_to_rgba_u8(color);
            hex_entry.set_text(&super::color::draw_color_to_hex(color));
            r_entry.set_text(&r.to_string());
            g_entry.set_text(&g.to_string());
            b_entry.set_text(&b.to_string());
            a_entry.set_text(&(picker.alpha * 100.0).round().to_string());

            picker_css_provider.load_from_data(&picker_dynamic_css(color));
            gradient_area.queue_draw();

            picker_update_in_progress.set(false);
        }
    });

    let apply_picker_color_to_editor: Rc<dyn Fn(super::types::DrawColor)> = Rc::new({
        let state_picker_apply = state.clone();
        let drawing_area_picker_apply = drawing_area.downgrade();
        let color_buttons_picker = color_buttons.clone();
        let color_picker_dot_picker = color_picker_dot.clone();
        let color_class_names_picker = color_class_names.clone();
        move |color| {
            {
                let mut st = state_picker_apply.lock().unwrap();
                if st.selected_tool == Tool::Crop {
                    st.set_crop_background_color(color);
                } else {
                    st.selected_color = color;
                    let _ = st.set_selected_action_color(color);
                }
            }

            let nearest_index = super::color::palette_index_for_color(color);
            clear_active_color_picker_palette_state(&color_buttons_picker);
            set_color_picker_trigger_dot_state(
                &color_picker_dot_picker,
                &color_class_names_picker,
                nearest_index,
            );

            if let Some(area) = drawing_area_picker_apply.upgrade() {
                area.queue_draw();
            }
        }
    });

    let sync_picker_from_color: Rc<dyn Fn(super::types::DrawColor)> = Rc::new({
        let picker_state = picker_state.clone();
        let update_picker_ui = update_picker_ui.clone();
        move |color| {
            let picker = PickerColorState::from_color(color);
            *picker_state.borrow_mut() = picker;
            update_picker_ui(picker);
        }
    });

    let commit_picker_state: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let update_picker_ui = update_picker_ui.clone();
        let apply_picker_color_to_editor = apply_picker_color_to_editor.clone();
        move || {
            let picker = *picker_state.borrow();
            update_picker_ui(picker);
            apply_picker_color_to_editor(picker.to_color());
        }
    });

    let sync_picker_for_active_tool: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let color_buttons = color_buttons.clone();
        let color_picker_dot = color_picker_dot.clone();
        let color_class_names = color_class_names.clone();
        let sync_picker_from_color = sync_picker_from_color.clone();
        move || {
            let (active_color, show_palette_state) = {
                let st = state.lock().unwrap();
                if st.selected_tool == Tool::Crop {
                    (st.crop_background_color, st.crop_background_color_explicit)
                } else {
                    (st.selected_color, true)
                }
            };
            sync_picker_from_color(active_color);
            clear_active_color_picker_palette_state(&color_buttons);
            if show_palette_state {
                set_color_picker_trigger_dot_state(
                    &color_picker_dot,
                    &color_class_names,
                    super::color::palette_index_for_color(active_color),
                );
            } else {
                clear_color_picker_trigger_dot_state(&color_picker_dot, &color_class_names);
            }
        }
    });

    sync_picker_for_active_tool();

    // Hue slider
    let picker_state_hue = picker_state.clone();
    let picker_update_in_progress_hue = picker_update_in_progress.clone();
    let commit_picker_state_hue = commit_picker_state.clone();
    hue_slider.connect_value_changed(move |slider| {
        if picker_update_in_progress_hue.get() {
            return;
        }

        picker_state_hue.borrow_mut().hue = super::types::normalize_hue(slider.value());
        commit_picker_state_hue();
    });

    // Opacity slider
    let picker_state_opacity = picker_state.clone();
    let picker_update_in_progress_opacity = picker_update_in_progress.clone();
    let commit_picker_state_opacity = commit_picker_state.clone();
    opacity_slider.connect_value_changed(move |slider| {
        if picker_update_in_progress_opacity.get() {
            return;
        }

        picker_state_opacity.borrow_mut().alpha = (slider.value() / 100.0).clamp(0.0, 1.0);
        commit_picker_state_opacity();
    });

    // Gradient area interactions
    let update_sv_from_position: Rc<dyn Fn(f64, f64)> = Rc::new({
        let gradient_area = gradient_area.clone();
        let picker_state = picker_state.clone();
        let commit_picker_state = commit_picker_state.clone();
        move |x, y| {
            let width = gradient_area.allocated_width().max(1) as f64;
            let height = gradient_area.allocated_height().max(1) as f64;
            let saturation = (x / width).clamp(0.0, 1.0);
            let value = (1.0 - (y / height)).clamp(0.0, 1.0);

            {
                let mut picker = picker_state.borrow_mut();
                picker.saturation = saturation;
                picker.value = value;
            }

            commit_picker_state();
        }
    });

    let gradient_dragging = Rc::new(Cell::new(false));

    let gradient_click = GestureClick::new();
    let gradient_dragging_press = gradient_dragging.clone();
    let update_sv_click = update_sv_from_position.clone();
    gradient_click.connect_pressed(move |_, _, x, y| {
        gradient_dragging_press.set(true);
        update_sv_click(x, y);
    });

    let gradient_dragging_release = gradient_dragging.clone();
    gradient_click.connect_released(move |_, _, _, _| {
        gradient_dragging_release.set(false);
    });
    gradient_area.add_controller(gradient_click);

    let gradient_motion = EventControllerMotion::new();
    let gradient_dragging_motion = gradient_dragging.clone();
    let update_sv_motion = update_sv_from_position.clone();
    gradient_motion.connect_motion(move |_, x, y| {
        if gradient_dragging_motion.get() {
            update_sv_motion(x, y);
        }
    });
    gradient_area.add_controller(gradient_motion);

    // Hex entry
    let picker_state_hex = picker_state.clone();
    let picker_update_in_progress_hex = picker_update_in_progress.clone();
    let commit_picker_state_hex = commit_picker_state.clone();
    hex_entry.connect_changed(move |entry| {
        if picker_update_in_progress_hex.get() {
            return;
        }

        let text = entry.text();
        let Some((r, g, b)) = parse_hex_rgb(text.as_str()) else {
            return;
        };

        let (hue, saturation, value) =
            super::types::rgb_to_hsv(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        {
            let mut picker = picker_state_hex.borrow_mut();
            picker.hue = hue;
            picker.saturation = saturation;
            picker.value = value;
        }

        commit_picker_state_hex();
    });

    // RGBA entries
    let update_picker_from_rgba_entries: Rc<dyn Fn()> = Rc::new({
        let picker_state = picker_state.clone();
        let r_entry = r_entry.clone();
        let g_entry = g_entry.clone();
        let b_entry = b_entry.clone();
        let a_entry = a_entry.clone();
        let commit_picker_state = commit_picker_state.clone();
        move || {
            let Some(r) = parse_channel_u8(r_entry.text().as_str()) else {
                return;
            };
            let Some(g) = parse_channel_u8(g_entry.text().as_str()) else {
                return;
            };
            let Some(b) = parse_channel_u8(b_entry.text().as_str()) else {
                return;
            };
            let Some(alpha) = parse_alpha_percent(a_entry.text().as_str()) else {
                return;
            };

            let (hue, saturation, value) =
                super::types::rgb_to_hsv(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
            {
                let mut picker = picker_state.borrow_mut();
                picker.hue = hue;
                picker.saturation = saturation;
                picker.value = value;
                picker.alpha = alpha;
            }

            commit_picker_state();
        }
    });

    let picker_update_in_progress_r = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_r = update_picker_from_rgba_entries.clone();
    r_entry.connect_changed(move |_| {
        if picker_update_in_progress_r.get() {
            return;
        }
        update_picker_from_rgba_entries_r();
    });

    let picker_update_in_progress_g = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_g = update_picker_from_rgba_entries.clone();
    g_entry.connect_changed(move |_| {
        if picker_update_in_progress_g.get() {
            return;
        }
        update_picker_from_rgba_entries_g();
    });

    let picker_update_in_progress_b = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_b = update_picker_from_rgba_entries.clone();
    b_entry.connect_changed(move |_| {
        if picker_update_in_progress_b.get() {
            return;
        }
        update_picker_from_rgba_entries_b();
    });

    let picker_update_in_progress_a = picker_update_in_progress.clone();
    let update_picker_from_rgba_entries_a = update_picker_from_rgba_entries.clone();
    a_entry.connect_changed(move |_| {
        if picker_update_in_progress_a.get() {
            return;
        }
        update_picker_from_rgba_entries_a();
    });

    // Universal button sync
    let sync_picker_for_active_tool_arrow = sync_picker_for_active_tool.clone();
    let picker_panel_arrow = picker_panel.clone();
    universal_arrow_btn.connect_clicked(move |_| {
        if picker_panel_arrow.is_visible() {
            sync_picker_for_active_tool_arrow();
        }
    });

    let sync_picker_for_active_tool_wheel = sync_picker_for_active_tool.clone();
    let picker_panel_wheel = picker_panel.clone();
    universal_color_btn.connect_clicked(move |_| {
        if picker_panel_wheel.is_visible() {
            sync_picker_for_active_tool_wheel();
        }
    });

    // Add color to custom slots
    let add_color_to_custom_slots: Rc<dyn Fn(super::types::DrawColor)> = Rc::new({
        let custom_slot_colors = custom_slot_colors.clone();
        let refresh_custom_color_slots = refresh_custom_color_slots.clone();
        move |color_to_add| {
            let mut custom_colors = custom_slot_colors.borrow_mut();
            let Some(slot_index) = custom_colors.iter().position(Option::is_none) else {
                return;
            };

            custom_colors[slot_index] = Some(color_to_add);
            save_persisted_custom_slot_colors(custom_colors.as_slice());
            drop(custom_colors);
            refresh_custom_color_slots();
        }
    });

    let picker_state_add_to_colors = picker_state.clone();
    let add_color_to_custom_slots_add = add_color_to_custom_slots.clone();
    add_to_colors_btn.connect_clicked(move |_| {
        let color_to_add = picker_state_add_to_colors.borrow().to_color();
        add_color_to_custom_slots_add(color_to_add);
    });

    // Eyedropper
    let color_popover_eyedropper = color_popover.clone();
    let state_eyedropper_activate = state.clone();
    let eyedropper_mode_activate = eyedropper_mode.clone();
    let eyedropper_point_activate = eyedropper_point.clone();
    let eyedropper_rendered_activate = eyedropper_rendered.clone();
    let canvas_eyedropper_ring_activate = canvas_eyedropper_ring.clone();
    let drawing_area_eyedropper = drawing_area.downgrade();
    let window_eyedropper = window.downgrade();
    eyedropper_btn.connect_clicked(move |_| {
        color_popover_eyedropper.popdown();
        eyedropper_mode_activate.set(true);
        *eyedropper_point_activate.borrow_mut() = None;
        *eyedropper_rendered_activate.borrow_mut() = state_eyedropper_activate
            .lock()
            .unwrap()
            .to_rendered_image()
            .ok();
        canvas_eyedropper_ring_activate.set_visible(false);
        canvas_eyedropper_ring_activate.queue_draw();

        if let Some(window) = window_eyedropper.upgrade() {
            set_window_cursor_name(&window, Some("crosshair"));
        }

        if let Some(area) = drawing_area_eyedropper.upgrade() {
            area.queue_draw();
        }
    });

    // Drawing area draw function
    let cached_surface = Rc::new(std::cell::RefCell::new(None::<gtk4::cairo::ImageSurface>));
    let cached_surface_revision = Rc::new(Cell::new(0_u64));

    let state_draw = state.clone();
    let transform_draw = transform.clone();
    let undo_btn_draw = undo_btn.clone();
    let redo_btn_draw = redo_btn.clone();
    let delete_selected_btn_draw = delete_selected_btn.clone();
    let size_group_draw = size_group.clone();
    let size_down_btn_draw = size_down_btn.clone();
    let size_up_btn_draw = size_up_btn.clone();
    let cached_surface_draw = cached_surface.clone();
    let cached_surface_revision_draw = cached_surface_revision.clone();
    let canvas_padding_draw = canvas_padding as f64;
    drawing_area.set_draw_func(move |_, context, width, height| {
        let st = state_draw.lock().unwrap();
        let (can_undo, can_redo) = st.history_availability();
        undo_btn_draw.set_sensitive(can_undo);
        redo_btn_draw.set_sensitive(can_redo);
        delete_selected_btn_draw.set_sensitive(st.can_remove_selected_action());
        apply_size_control_ui_state(
            &st,
            &size_group_draw,
            &size_down_btn_draw,
            &size_up_btn_draw,
        );
        let image_width = st.working_image.width() as f64;
        let image_height = st.working_image.height() as f64;
        let crop_rect = st.draft_crop_rect().or(st.crop_selection);
        let crop_mode_active = st.selected_tool == Tool::Crop;

        let base_view_width = (width as f64 - canvas_padding_draw * 2.0).max(1.0);
        let base_scale = (base_view_width / image_width).min(1.0);
        let (overflow_left, overflow_top, overflow_right, overflow_bottom) = crop_canvas_overflow(
            crop_rect,
            image_width,
            image_height,
            base_scale,
            crop_mode_active,
        );
        let view_width =
            (width as f64 - canvas_padding_draw * 2.0 - overflow_left - overflow_right).max(1.0);
        let view_height =
            (height as f64 - canvas_padding_draw * 2.0 - overflow_top - overflow_bottom).max(1.0);

        let mut t = ViewTransform::fit(image_width, image_height, view_width, view_height);
        t.offset_x += canvas_padding_draw + overflow_left;
        t.offset_y += canvas_padding_draw + overflow_top;

        *transform_draw.lock().unwrap() = t;

        context.set_operator(gtk4::cairo::Operator::Source);
        draw_canvas_checkerboard_background(
            context,
            width,
            height,
            if crop_mode_active && st.crop_background_color_explicit {
                Some(st.crop_background_color)
            } else {
                None
            },
        );
        context.set_operator(gtk4::cairo::Operator::Over);

        let _ = context.save();
        context.translate(t.offset_x, t.offset_y);
        context.scale(t.scale, t.scale);

        if crop_mode_active && st.crop_background_color_explicit {
            if let Some(crop_rect) = crop_rect {
                context.set_source_rgba(
                    st.crop_background_color.r,
                    st.crop_background_color.g,
                    st.crop_background_color.b,
                    st.crop_background_color.a,
                );
                context.rectangle(
                    crop_rect.x as f64,
                    crop_rect.y as f64,
                    crop_rect.width as f64,
                    crop_rect.height as f64,
                );
                let _ = context.fill();
            }
        }

        if cached_surface_revision_draw.get() != st.working_image_revision
            || cached_surface_draw.borrow().is_none()
        {
            *cached_surface_draw.borrow_mut() = rgba_image_to_surface(&st.working_image);
            cached_surface_revision_draw.set(st.working_image_revision);
        }

        if let Some(surface) = cached_surface_draw.borrow().as_ref() {
            if context.set_source_surface(surface, 0.0, 0.0).is_ok() {
                let _ = context.paint();
            }
        } else {
            draw_rgba_to_context(context, &st.working_image);
        }

        for action in &st.actions {
            if let AnnotationAction::Focus { rect } = action {
                draw_focus_overlay(
                    context,
                    st.working_image.width() as f64,
                    st.working_image.height() as f64,
                    *rect,
                    false,
                );
            }
        }

        for action in &st.actions {
            if matches!(
                action,
                AnnotationAction::Blur { .. }
                    | AnnotationAction::Focus { .. }
                    | AnnotationAction::Censor { .. }
            ) {
                continue;
            }
            draw_annotation_action(context, action);
        }

        if let Some(draft) = st.draft_action() {
            if let AnnotationAction::Focus { rect } = &draft {
                draw_focus_overlay(
                    context,
                    st.working_image.width() as f64,
                    st.working_image.height() as f64,
                    *rect,
                    true,
                );
            } else {
                draw_draft_action(context, &draft);
            }
        }

        if crop_mode_active {
            if let Some(crop_rect) = crop_rect {
                let canvas_left = -t.offset_x / t.scale;
                let canvas_top = -t.offset_y / t.scale;
                let canvas_width = width as f64 / t.scale;
                let canvas_height = height as f64 / t.scale;
                let _ = context.save();
                context.rectangle(canvas_left, canvas_top, canvas_width, canvas_height);
                context.rectangle(
                    crop_rect.x as f64,
                    crop_rect.y as f64,
                    crop_rect.width as f64,
                    crop_rect.height as f64,
                );
                context.set_fill_rule(gtk4::cairo::FillRule::EvenOdd);
                context.set_source_rgba(0.0, 0.0, 0.0, 140.0 / 255.0);
                let _ = context.fill();
                let _ = context.restore();
            }
        }

        if let Some(crop_rect) = st.draft_crop_rect().or(st.crop_selection) {
            draw_crop_overlay(
                context,
                st.working_image.width() as f64,
                st.working_image.height() as f64,
                crop_rect,
                st.selected_tool == Tool::Crop,
            );
        }

        if let Some(selected_action) = st.selected_action() {
            let selection_padding = selection_hit_padding_for_scale(t.scale);
            if let Some(bounds) = action_bounds_with_padding(selected_action, selection_padding) {
                draw_selection_outline(context, bounds, t.scale);
            }

            let handles = action_resize_handles(selected_action);
            if !handles.is_empty() {
                draw_selection_handles(context, &handles, st.select_resize_handle, t.scale);
            }
        }
        let _ = context.restore();
    });

    // Tool button connections
    let state_select = state.clone();
    let drawing_area_select = drawing_area.downgrade();
    let buttons_select = tool_buttons.clone();
    let apply_crop_btn_select = apply_crop_btn.clone();
    let update_toolbar_for_tool_select = update_toolbar_for_tool.clone();
    select_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_select, 2);
        state_select.lock().unwrap().set_tool(Tool::Select);
        update_toolbar_for_tool_select(Tool::Select);
        set_crop_apply_button_state(&apply_crop_btn_select, false, false);
        if let Some(area) = drawing_area_select.upgrade() {
            area.queue_draw();
        }
    });

    let window_minimize = window.downgrade();
    traffic_minimize.connect_clicked(move |_| {
        if let Some(window) = window_minimize.upgrade() {
            window.minimize();
        }
    });

    let zoomed_state = Rc::new(Cell::new(false));
    let zoomed_state_btn = zoomed_state.clone();
    let window_zoom = window.downgrade();
    traffic_zoom.connect_clicked(move |_| {
        if let Some(window) = window_zoom.upgrade() {
            let next_zoomed = !zoomed_state_btn.get();
            zoomed_state_btn.set(next_zoomed);
            window.set_fullscreened(next_zoomed);
        }
    });

    let state_crop = state.clone();
    let drawing_area_crop = drawing_area.downgrade();
    let buttons_crop = tool_buttons.clone();
    let apply_crop_btn_crop = apply_crop_btn.clone();
    let update_toolbar_for_tool_crop = update_toolbar_for_tool.clone();
    let update_crop_size_fields_crop = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_crop = sync_picker_for_active_tool.clone();
    crop_btn.connect_clicked(move |_| {
        let (next_tool, has_selection) = {
            let mut st = state_crop.lock().unwrap();
            if st.selected_tool == Tool::Crop {
                st.set_tool(Tool::Arrow);
                (Tool::Arrow, false)
            } else {
                st.set_tool(Tool::Crop);
                st.ensure_crop_selection_initialized();
                (Tool::Crop, st.crop_selection.is_some())
            }
        };

        if matches!(next_tool, Tool::Crop) {
            set_active_tool_button(&buttons_crop, 0);
        } else {
            set_active_tool_button(&buttons_crop, 6);
        }
        update_toolbar_for_tool_crop(next_tool);
        sync_picker_for_active_tool_crop();
        set_crop_apply_button_state(
            &apply_crop_btn_crop,
            matches!(next_tool, Tool::Crop),
            has_selection,
        );
        update_crop_size_fields_crop();
        if let Some(area) = drawing_area_crop.upgrade() {
            area.queue_draw();
        }
    });

    let state_background = state.clone();
    let drawing_area_background = drawing_area.downgrade();
    let buttons_background = tool_buttons.clone();
    let apply_crop_btn_background = apply_crop_btn.clone();
    let update_toolbar_for_tool_background = update_toolbar_for_tool.clone();
    background_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_background, 1);
        state_background.lock().unwrap().set_tool(Tool::Background);
        update_toolbar_for_tool_background(Tool::Background);
        set_crop_apply_button_state(&apply_crop_btn_background, false, false);
        if let Some(area) = drawing_area_background.upgrade() {
            area.queue_draw();
        }
    });

    let state_draw_mode = state.clone();
    let drawing_area_draw_mode = drawing_area.downgrade();
    let buttons_draw_mode = tool_buttons.clone();
    let apply_crop_btn_draw_mode = apply_crop_btn.clone();
    let update_toolbar_for_tool_draw_mode = update_toolbar_for_tool.clone();
    draw_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_draw_mode, 3);
        state_draw_mode.lock().unwrap().set_tool(Tool::Pen);
        update_toolbar_for_tool_draw_mode(Tool::Pen);
        set_crop_apply_button_state(&apply_crop_btn_draw_mode, false, false);
        if let Some(area) = drawing_area_draw_mode.upgrade() {
            area.queue_draw();
        }
    });

    let state_arrow = state.clone();
    let drawing_area_arrow = drawing_area.downgrade();
    let buttons_arrow = tool_buttons.clone();
    let apply_crop_btn_arrow = apply_crop_btn.clone();
    let update_toolbar_for_tool_arrow = update_toolbar_for_tool.clone();
    arrow_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_arrow, 6);
        state_arrow.lock().unwrap().set_tool(Tool::Arrow);
        update_toolbar_for_tool_arrow(Tool::Arrow);
        set_crop_apply_button_state(&apply_crop_btn_arrow, false, false);
        if let Some(area) = drawing_area_arrow.upgrade() {
            area.queue_draw();
        }
    });

    let state_line = state.clone();
    let drawing_area_line = drawing_area.downgrade();
    let buttons_line = tool_buttons.clone();
    let apply_crop_btn_line = apply_crop_btn.clone();
    let update_toolbar_for_tool_line = update_toolbar_for_tool.clone();
    line_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_line, 7);
        state_line.lock().unwrap().set_tool(Tool::Line);
        update_toolbar_for_tool_line(Tool::Line);
        set_crop_apply_button_state(&apply_crop_btn_line, false, false);
        if let Some(area) = drawing_area_line.upgrade() {
            area.queue_draw();
        }
    });

    // Drag window gesture
    let drag_window_gesture = GestureClick::new();
    drag_window_gesture.set_button(1);
    let window_drag = window.downgrade();
    drag_window_gesture.connect_pressed(move |gesture, _, x, y| {
        let Some(window) = window_drag.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };

        let Some(surface) = window.surface() else {
            return;
        };

        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    drag_btn.add_controller(drag_window_gesture);

    // Pin button
    let pin_state = Arc::new(AtomicBool::new(false));
    let pin_state_btn = pin_state.clone();
    let pin_icon_btn = pin_icon.clone();
    pin_btn.connect_clicked(move |_| {
        let now_pinned = !pin_state_btn.load(Ordering::Relaxed);
        pin_state_btn.store(now_pinned, Ordering::Relaxed);
        pin_icon_btn.set_icon_name(Some(if now_pinned {
            icon_names::PIN
        } else {
            icon_names::VIEW_PIN
        }));
    });

    // Copy button
    let path_copy = path.clone();
    copy_btn.connect_clicked(move |_| {
        if let Err(e) = copy_uri_to_clipboard(&path_copy) {
            eprintln!("Copy failed: {e}");
        }
    });

    // Upload button
    let path_upload = path.clone();
    upload_btn.connect_clicked(move |_| {
        let target = path_upload
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| path_upload.clone());

        if let Err(e) = open_target(&target) {
            eprintln!("Upload action failed: {e}");
        }
    });

    // More tool buttons
    let state_box = state.clone();
    let drawing_area_box = drawing_area.downgrade();
    let buttons_box = tool_buttons.clone();
    let apply_crop_btn_box = apply_crop_btn.clone();
    let update_toolbar_for_tool_box = update_toolbar_for_tool.clone();
    box_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_box, 4);
        state_box.lock().unwrap().set_tool(Tool::Box);
        update_toolbar_for_tool_box(Tool::Box);
        set_crop_apply_button_state(&apply_crop_btn_box, false, false);
        if let Some(area) = drawing_area_box.upgrade() {
            area.queue_draw();
        }
    });

    let state_circle = state.clone();
    let drawing_area_circle = drawing_area.downgrade();
    let buttons_circle = tool_buttons.clone();
    let apply_crop_btn_circle = apply_crop_btn.clone();
    let update_toolbar_for_tool_circle = update_toolbar_for_tool.clone();
    circle_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_circle, 5);
        state_circle.lock().unwrap().set_tool(Tool::Circle);
        update_toolbar_for_tool_circle(Tool::Circle);
        set_crop_apply_button_state(&apply_crop_btn_circle, false, false);
        if let Some(area) = drawing_area_circle.upgrade() {
            area.queue_draw();
        }
    });

    let state_text = state.clone();
    let drawing_area_text = drawing_area.downgrade();
    let buttons_text = tool_buttons.clone();
    let apply_crop_btn_text = apply_crop_btn.clone();
    let update_toolbar_for_tool_text = update_toolbar_for_tool.clone();
    text_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_text, 8);
        state_text.lock().unwrap().set_tool(Tool::Text);
        update_toolbar_for_tool_text(Tool::Text);
        set_crop_apply_button_state(&apply_crop_btn_text, false, false);
        if let Some(area) = drawing_area_text.upgrade() {
            area.queue_draw();
        }
    });

    let state_blur = state.clone();
    let drawing_area_blur = drawing_area.downgrade();
    let buttons_blur = tool_buttons.clone();
    let apply_crop_btn_blur = apply_crop_btn.clone();
    let update_toolbar_for_tool_blur = update_toolbar_for_tool.clone();
    blur_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_blur, 9);
        state_blur.lock().unwrap().set_tool(Tool::Blur);
        update_toolbar_for_tool_blur(Tool::Blur);
        set_crop_apply_button_state(&apply_crop_btn_blur, false, false);
        if let Some(area) = drawing_area_blur.upgrade() {
            area.queue_draw();
        }
    });

    let state_censor = state.clone();
    let drawing_area_censor = drawing_area.downgrade();
    let buttons_censor = tool_buttons.clone();
    let apply_crop_btn_censor = apply_crop_btn.clone();
    let update_toolbar_for_tool_censor = update_toolbar_for_tool.clone();
    censor_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_censor, 10);
        state_censor.lock().unwrap().set_tool(Tool::Censor);
        update_toolbar_for_tool_censor(Tool::Censor);
        set_crop_apply_button_state(&apply_crop_btn_censor, false, false);
        if let Some(area) = drawing_area_censor.upgrade() {
            area.queue_draw();
        }
    });

    let state_focus = state.clone();
    let drawing_area_focus = drawing_area.downgrade();
    let buttons_focus = tool_buttons.clone();
    let apply_crop_btn_focus = apply_crop_btn.clone();
    let update_toolbar_for_tool_focus = update_toolbar_for_tool.clone();
    focus_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_focus, 13);
        state_focus.lock().unwrap().set_tool(Tool::Focus);
        update_toolbar_for_tool_focus(Tool::Focus);
        set_crop_apply_button_state(&apply_crop_btn_focus, false, false);
        if let Some(area) = drawing_area_focus.upgrade() {
            area.queue_draw();
        }
    });

    let state_number = state.clone();
    let drawing_area_number = drawing_area.downgrade();
    let buttons_number = tool_buttons.clone();
    let apply_crop_btn_number = apply_crop_btn.clone();
    let update_toolbar_for_tool_number = update_toolbar_for_tool.clone();
    number_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_number, 11);
        state_number.lock().unwrap().set_tool(Tool::Number);
        update_toolbar_for_tool_number(Tool::Number);
        set_crop_apply_button_state(&apply_crop_btn_number, false, false);
        if let Some(area) = drawing_area_number.upgrade() {
            area.queue_draw();
        }
    });

    let state_highlighter = state.clone();
    let drawing_area_highlighter = drawing_area.downgrade();
    let buttons_highlighter = tool_buttons.clone();
    let apply_crop_btn_highlighter = apply_crop_btn.clone();
    let update_toolbar_for_tool_highlighter = update_toolbar_for_tool.clone();
    highlighter_btn.connect_clicked(move |_| {
        set_active_tool_button(&buttons_highlighter, 12);
        state_highlighter
            .lock()
            .unwrap()
            .set_tool(Tool::Highlighter);
        update_toolbar_for_tool_highlighter(Tool::Highlighter);
        set_crop_apply_button_state(&apply_crop_btn_highlighter, false, false);
        if let Some(area) = drawing_area_highlighter.upgrade() {
            area.queue_draw();
        }
    });

    // Custom slot drag and drop
    let dragged_custom_slot_index = Rc::new(Cell::new(None::<usize>));
    let suppress_custom_slot_click_once = Rc::new(Cell::new(false));

    for (index, slot_button) in custom_slot_buttons.iter().enumerate() {
        let slot_overlay = custom_slot_overlays[index].clone();

        let drag_source = DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);
        let transparent_drag_icon = transparent_drag_icon_texture();
        let custom_slot_colors_drag = custom_slot_colors.clone();
        let dragged_custom_slot_index_prepare = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_prepare = suppress_custom_slot_click_once.clone();
        drag_source.connect_prepare(move |source, _, _| {
            if custom_slot_colors_drag.borrow()[index].is_none() {
                return None;
            }

            if let Some(icon) = transparent_drag_icon.as_ref() {
                source.set_icon(Some(icon), 0, 0);
            } else {
                source.set_icon(None::<&gdk::Paintable>, 0, 0);
            }

            dragged_custom_slot_index_prepare.set(Some(index));
            suppress_custom_slot_click_once_prepare.set(true);
            let value = glib::Value::from(index as u32);
            Some(gdk::ContentProvider::for_value(&value))
        });
        let dragged_custom_slot_index_end = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_end = suppress_custom_slot_click_once.clone();
        drag_source.connect_drag_end(move |_, _, _| {
            dragged_custom_slot_index_end.set(None);
            suppress_custom_slot_click_once_end.set(true);
        });
        slot_overlay.add_controller(drag_source);

        let drop_target = DropTarget::new(glib::Type::U32, gdk::DragAction::MOVE);
        let custom_slot_colors_drop = custom_slot_colors.clone();
        let refresh_custom_color_slots_drop = refresh_custom_color_slots.clone();
        let suppress_custom_slot_click_once_drop = suppress_custom_slot_click_once.clone();
        drop_target.connect_drop(move |_, value, _, _| {
            let Ok(from_index_raw) = value.get::<u32>() else {
                return false;
            };

            let moved = {
                let mut colors = custom_slot_colors_drop.borrow_mut();
                move_custom_color_between_slots(
                    colors.as_mut_slice(),
                    from_index_raw as usize,
                    index,
                )
            };

            if moved {
                refresh_custom_color_slots_drop();
                save_persisted_custom_slot_colors(custom_slot_colors_drop.borrow().as_slice());
                suppress_custom_slot_click_once_drop.set(true);
            }

            moved
        });
        slot_overlay.add_controller(drop_target);

        let custom_slot_colors_click = custom_slot_colors.clone();
        let apply_picker_color_to_editor_click = apply_picker_color_to_editor.clone();
        let sync_picker_from_color_click = sync_picker_from_color.clone();
        let color_popover_click = color_popover.clone();
        let dragged_custom_slot_index_click = dragged_custom_slot_index.clone();
        let suppress_custom_slot_click_once_click = suppress_custom_slot_click_once.clone();
        slot_button.connect_clicked(move |_| {
            if dragged_custom_slot_index_click.get().is_some() {
                return;
            }

            if suppress_custom_slot_click_once_click.replace(false) {
                return;
            }

            let Some(color) = custom_slot_colors_click.borrow()[index] else {
                return;
            };

            apply_picker_color_to_editor_click(color);
            sync_picker_from_color_click(color);
            color_popover_click.popdown();
        });
    }

    for (index, remove_button) in custom_slot_remove_buttons.iter().enumerate() {
        let custom_slot_colors_remove = custom_slot_colors.clone();
        let refresh_custom_color_slots_remove = refresh_custom_color_slots.clone();
        remove_button.connect_clicked(move |_| {
            let mut custom_colors = custom_slot_colors_remove.borrow_mut();
            if custom_colors[index].is_none() {
                return;
            }

            custom_colors[index] = None;
            save_persisted_custom_slot_colors(custom_colors.as_slice());
            drop(custom_colors);
            refresh_custom_color_slots_remove();
        });
    }

    // Color buttons
    for (index, button) in color_buttons.iter().enumerate() {
        let state_color = state.clone();
        let drawing_area_color = drawing_area.downgrade();
        let color_buttons_group = color_buttons.clone();
        let color_picker_dot_group = color_picker_dot.clone();
        let color_class_names_group = color_class_names.clone();
        let color_popover_group = color_popover.clone();
        let sync_picker_from_color_group = sync_picker_from_color.clone();
        button.connect_clicked(move |_| {
            let mut st = state_color.lock().unwrap();
            if st.selected_tool == Tool::Crop {
                st.set_crop_background_color(DRAW_COLORS[index]);
            } else {
                st.set_color_index(index);
                st.set_selected_action_color(DRAW_COLORS[index]);
            }
            drop(st);

            sync_picker_from_color_group(DRAW_COLORS[index]);

            set_active_color_picker_state(
                &color_buttons_group,
                &color_picker_dot_group,
                &color_class_names_group,
                index,
            );
            color_popover_group.popdown();
            if let Some(area) = drawing_area_color.upgrade() {
                area.queue_draw();
            }
        });
    }

    // Size buttons
    let state_size_down = state.clone();
    let drawing_area_size_down = drawing_area.downgrade();
    size_down_btn.connect_clicked(move |_| {
        if state_size_down.lock().unwrap().adjust_active_size(-1.0) {
            if let Some(area) = drawing_area_size_down.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_size_up = state.clone();
    let drawing_area_size_up = drawing_area.downgrade();
    size_up_btn.connect_clicked(move |_| {
        if state_size_up.lock().unwrap().adjust_active_size(1.0) {
            if let Some(area) = drawing_area_size_up.upgrade() {
                area.queue_draw();
            }
        }
    });

    // Apply crop button
    let state_apply_crop = state.clone();
    let drawing_area_apply_crop = drawing_area.downgrade();
    let buttons_apply_crop = tool_buttons.clone();
    let apply_crop_btn_click = apply_crop_btn.clone();
    let update_canvas_content_size_apply = update_canvas_content_size.clone();
    let update_toolbar_for_tool_apply_crop = update_toolbar_for_tool.clone();
    let update_crop_size_fields_apply_crop = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_apply_crop = sync_picker_for_active_tool.clone();
    apply_crop_btn.connect_clicked(move |_| {
        let apply_result = {
            let mut st = state_apply_crop.lock().unwrap();
            let result = st.apply_crop_selection();
            if result.as_ref().is_ok_and(|applied| *applied) {
                st.set_tool(Tool::Arrow);
            }
            result
        };

        match apply_result {
            Ok(true) => {
                update_canvas_content_size_apply();
                set_active_tool_button(&buttons_apply_crop, 6);
                update_toolbar_for_tool_apply_crop(Tool::Arrow);
                sync_picker_for_active_tool_apply_crop();
                set_crop_apply_button_state(&apply_crop_btn_click, false, false);
                update_crop_size_fields_apply_crop();
                if let Some(area) = drawing_area_apply_crop.upgrade() {
                    area.queue_draw();
                }
            }
            Ok(false) => {
                set_crop_apply_button_state(&apply_crop_btn_click, true, false);
                update_crop_size_fields_apply_crop();
            }
            Err(e) => {
                eprintln!("Failed to apply crop: {e}");
            }
        }
    });

    // Undo/Redo/Delete buttons
    let state_undo = state.clone();
    let drawing_area_undo = drawing_area.downgrade();
    undo_btn.connect_clicked(move |_| {
        if state_undo.lock().unwrap().undo() {
            if let Some(area) = drawing_area_undo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_redo = state.clone();
    let drawing_area_redo = drawing_area.downgrade();
    redo_btn.connect_clicked(move |_| {
        if state_redo.lock().unwrap().redo() {
            if let Some(area) = drawing_area_redo.upgrade() {
                area.queue_draw();
            }
        }
    });

    let state_delete_selected = state.clone();
    let drawing_area_delete_selected = drawing_area.downgrade();
    delete_selected_btn.connect_clicked(move |_| {
        if state_delete_selected
            .lock()
            .unwrap()
            .remove_selected_action()
        {
            if let Some(area) = drawing_area_delete_selected.upgrade() {
                area.queue_draw();
            }
        }
    });

    // Save button
    let state_save = state.clone();
    let path_save = path.clone();
    let window_save = window.downgrade();
    let app_save = app.downgrade();
    save_btn.connect_clicked(move |_| {
        let save_result = {
            let st = state_save.lock().unwrap();
            save_edited_image(&path_save, &st)
        };

        match save_result {
            Ok(()) => {
                if let Some(window) = window_save.upgrade() {
                    window.close();
                }
                if let Some(app) = app_save.upgrade() {
                    app.quit();
                }
            }
            Err(e) => {
                eprintln!("Failed to save edited image: {e}");
            }
        }
    });

    // Close button
    let window_close = window.downgrade();
    let app_close = app.downgrade();
    traffic_close.connect_clicked(move |_| {
        if let Some(window) = window_close.upgrade() {
            window.close();
        }
        if let Some(app) = app_close.upgrade() {
            app.quit();
        }
    });

    // Drag gesture for drawing
    let drag = GestureDrag::new();
    let drag_last_redraw = Rc::new(Cell::new(0_i64));
    let eyedropper_mode_drag_begin = eyedropper_mode.clone();
    let state_drag_begin = state.clone();
    let transform_drag_begin = transform.clone();
    let drawing_area_begin = drawing_area.downgrade();
    let drag_last_redraw_begin = drag_last_redraw.clone();
    let apply_crop_btn_drag_begin = apply_crop_btn.clone();
    let update_crop_size_fields_drag_begin = update_crop_size_fields.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        if eyedropper_mode_drag_begin.get() {
            return;
        }

        let t = *transform_drag_begin.lock().unwrap();
        let view_point = Point { x, y };

        let selected_tool = {
            let st = state_drag_begin.lock().unwrap();
            st.selected_tool
        };
        if !t.contains_view(view_point) && selected_tool != Tool::Crop {
            return;
        }

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        let mut st = state_drag_begin.lock().unwrap();

        if st.selected_tool == Tool::Select {
            st.drag_start_view = Some(view_point);
            st.begin_select_drag_with_scale(t.view_to_image_clamped(view_point), t.scale);
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

        if st.selected_tool == Tool::Crop {
            let image_point = t.view_to_image(view_point);
            st.drag_start_view = Some(view_point);
            if st.begin_crop_drag_with_scale(image_point, t.scale) {
                let has_selection = st.crop_selection.is_some();
                drop(st);
                set_crop_apply_button_state(&apply_crop_btn_drag_begin, true, has_selection);
                update_crop_size_fields_drag_begin();
                if let Some(area) = drawing_area_begin.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_begin.set(glib::monotonic_time());
                return;
            }

            st.drag_shift_active = shift_pressed;
            st.begin_drag(image_point);
            st.crop_selection = None;
            drop(st);
            set_crop_apply_button_state(&apply_crop_btn_drag_begin, true, false);
            update_crop_size_fields_drag_begin();
            if let Some(area) = drawing_area_begin.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_begin.set(glib::monotonic_time());
            return;
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
    let update_crop_size_fields_drag_update = update_crop_size_fields.clone();
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if eyedropper_mode_drag_update.get() {
            return;
        }

        let t = *transform_drag_update.lock().unwrap();
        let mut st = state_drag_update.lock().unwrap();

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select {
                let now = glib::monotonic_time();
                if now - drag_last_redraw_update.get() < DRAG_REDRAW_INTERVAL_US {
                    return;
                }

                let moved = st.update_select_drag(t.view_to_image_clamped(current_view));
                drag_last_redraw_update.set(now);
                drop(st);
                if moved {
                    if let Some(area) = drawing_area_update.upgrade() {
                        area.queue_draw();
                    }
                }
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }

            if st.selected_tool == Tool::Crop {
                let now = glib::monotonic_time();
                if now - drag_last_redraw_update.get() < DRAG_REDRAW_INTERVAL_US {
                    return;
                }

                let image_point = t.view_to_image(current_view);
                if st.select_drag_anchor.is_some() {
                    st.update_crop_drag(image_point);
                } else {
                    st.drag_shift_active = shift_pressed;
                    st.update_drag(image_point);
                }
                drag_last_redraw_update.set(now);
                drop(st);
                update_crop_size_fields_drag_update();
                if let Some(area) = drawing_area_update.upgrade() {
                    area.queue_draw();
                }
                return;
            }

            st.drag_shift_active = shift_pressed;
            st.update_drag(t.view_to_image_clamped(current_view));
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
    let apply_crop_btn_drag_end = apply_crop_btn.clone();
    let update_crop_size_fields_drag_end = update_crop_size_fields.clone();
    drag.connect_drag_end(move |gesture, offset_x, offset_y| {
        if eyedropper_mode_drag_end.get() {
            return;
        }

        let t = *transform_drag_end.lock().unwrap();
        let mut st = state_drag_end.lock().unwrap();

        let shift_pressed = gesture
            .current_event_state()
            .contains(gdk::ModifierType::SHIFT_MASK);

        if let Some(start_view) = st.drag_start_view {
            let current_view = Point {
                x: start_view.x + offset_x,
                y: start_view.y + offset_y,
            };

            if st.selected_tool == Tool::Select {
                st.update_select_drag(t.view_to_image_clamped(current_view));
                st.end_select_drag();
                drop(st);

                if let Some(area) = drawing_area_end.upgrade() {
                    area.queue_draw();
                }
                drag_last_redraw_end.set(glib::monotonic_time());
                return;
            }

            if matches!(st.selected_tool, Tool::Text | Tool::Number) {
                return;
            }

            let mut crop_selection_ready = None;
            if st.selected_tool == Tool::Crop {
                let image_point = t.view_to_image(current_view);
                if st.select_drag_anchor.is_some() {
                    st.update_crop_drag(image_point);
                    crop_selection_ready = Some(st.crop_selection.is_some());
                    st.end_crop_drag();
                } else {
                    st.drag_shift_active = shift_pressed;
                    st.update_drag(image_point);
                    st.crop_selection = st.draft_crop_rect();
                    crop_selection_ready = Some(st.crop_selection.is_some());
                    st.clear_drag();
                }
            } else if let Some(action) = st.finalize_drag_action() {
                st.push_action(action);
            } else {
                st.clear_drag();
            }
            drop(st);

            if let Some(has_selection) = crop_selection_ready {
                set_crop_apply_button_state(&apply_crop_btn_drag_end, true, has_selection);
            }
            update_crop_size_fields_drag_end();

            if let Some(area) = drawing_area_end.upgrade() {
                area.queue_draw();
            }
            drag_last_redraw_end.set(glib::monotonic_time());
        }
    });
    drawing_area.add_controller(drag);

    // Click gesture
    let click = GestureClick::new();
    let window_click = window.clone();
    let state_click = state.clone();
    let transform_click = transform.clone();
    let drawing_area_click = drawing_area.downgrade();
    let color_buttons_click = color_buttons.clone();
    let color_picker_dot_click = color_picker_dot.clone();
    let color_class_names_click = color_class_names.clone();
    let eyedropper_mode_click = eyedropper_mode.clone();
    let eyedropper_point_click = eyedropper_point.clone();
    let eyedropper_rendered_click = eyedropper_rendered.clone();
    let color_popover_canvas_click = color_popover.clone();
    let set_picker_panel_visibility_canvas_click = set_picker_panel_visibility.clone();
    let canvas_eyedropper_ring_click = canvas_eyedropper_ring.clone();
    let apply_picker_color_to_editor_canvas_click = apply_picker_color_to_editor.clone();
    let sync_picker_from_color_canvas_click = sync_picker_from_color.clone();
    click.connect_pressed(move |_, n_press, x, y| {
        let t = *transform_click.lock().unwrap();
        let view_point = Point { x, y };

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

            let mut reopen_color_popover = false;
            if let Some(color) = picked_color {
                apply_picker_color_to_editor_canvas_click(color);
                sync_picker_from_color_canvas_click(color);
                reopen_color_popover = true;
            }

            eyedropper_mode_click.set(false);
            *eyedropper_point_click.borrow_mut() = None;
            *eyedropper_rendered_click.borrow_mut() = None;
            canvas_eyedropper_ring_click.set_visible(false);
            set_window_cursor_name(&window_click, None);

            if reopen_color_popover {
                set_picker_panel_visibility_canvas_click(true);
                color_popover_canvas_click.popup();
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
                let (edit_target, selected_color_index) = {
                    let mut st = state_click.lock().unwrap();
                    st.select_action_at_point_with_scale(image_point, t.scale);
                    let selected_color = st.selected_action_color();
                    if let Some(color) = selected_color {
                        st.selected_color = color;
                    }
                    if let Some(text_size) = st.selected_text_action_size() {
                        st.text_size = text_size;
                    }
                    if let Some(stroke_size) = st.selected_action_stroke_size() {
                        st.stroke_size = stroke_size;
                    }

                    let selected_color_index =
                        selected_color.map(super::color::palette_index_for_color);
                    if n_press >= 2 {
                        (st.selected_text_action_data(), selected_color_index)
                    } else {
                        (None, selected_color_index)
                    }
                };

                if let Some(index) = selected_color_index {
                    clear_active_color_picker_palette_state(&color_buttons_click);
                    set_color_picker_trigger_dot_state(
                        &color_picker_dot_click,
                        &color_class_names_click,
                        index,
                    );
                }

                if let Some(area) = drawing_area_click.upgrade() {
                    area.queue_draw();
                }

                if let Some((action_index, current_text)) = edit_target {
                    show_text_edit_dialog(
                        &window_click,
                        state_click.clone(),
                        action_index,
                        &current_text,
                        drawing_area_click.clone(),
                    );
                }
            }
            Tool::Text => {
                let (selected_color, text_size) = {
                    let st = state_click.lock().unwrap();
                    (st.selected_color, st.text_size)
                };
                show_text_dialog(
                    &window_click,
                    state_click.clone(),
                    image_point,
                    selected_color,
                    text_size,
                    drawing_area_click.clone(),
                );
            }
            Tool::Number => {
                state_click.lock().unwrap().add_number_marker(image_point);
                if let Some(area) = drawing_area_click.upgrade() {
                    area.queue_draw();
                }
            }
            _ => {}
        }
    });
    drawing_area.add_controller(click);

    // Motion controller
    let motion = EventControllerMotion::new();
    let eyedropper_mode_motion = eyedropper_mode.clone();
    let eyedropper_point_motion = eyedropper_point.clone();
    let canvas_eyedropper_ring_motion = canvas_eyedropper_ring.clone();
    let state_motion = state.clone();
    let transform_motion = transform.clone();
    let window_motion = window.downgrade();
    motion.connect_motion(move |_, x, y| {
        let t = *transform_motion.lock().unwrap();
        let view_point = Point { x, y };

        if eyedropper_mode_motion.get() {
            if !t.contains_view(view_point) {
                *eyedropper_point_motion.borrow_mut() = None;
                canvas_eyedropper_ring_motion.set_visible(false);
                if let Some(window) = window_motion.upgrade() {
                    set_window_cursor_name(&window, Some("crosshair"));
                }
                return;
            }

            *eyedropper_point_motion.borrow_mut() = Some(t.view_to_image_clamped(view_point));
            canvas_eyedropper_ring_motion.set_visible(true);
            let (left, top) = eyedropper_loupe_position(x, y);
            canvas_eyedropper_ring_motion.set_margin_start(left);
            canvas_eyedropper_ring_motion.set_margin_top(top);
            canvas_eyedropper_ring_motion.queue_draw();

            if let Some(window) = window_motion.upgrade() {
                set_window_cursor_name(&window, Some("none"));
            }
            return;
        }

        let cursor_name = {
            let st = state_motion.lock().unwrap();
            cursor_name_for_view_point(&st, t, view_point)
        };

        if let Some(window) = window_motion.upgrade() {
            set_window_cursor_name(&window, Some(cursor_name));
        }
    });

    let eyedropper_mode_motion_leave = eyedropper_mode.clone();
    let eyedropper_point_motion_leave = eyedropper_point.clone();
    let canvas_eyedropper_ring_motion_leave = canvas_eyedropper_ring.clone();
    let window_motion_leave = window.downgrade();
    motion.connect_leave(move |_| {
        *eyedropper_point_motion_leave.borrow_mut() = None;
        canvas_eyedropper_ring_motion_leave.set_visible(false);

        if let Some(window) = window_motion_leave.upgrade() {
            if eyedropper_mode_motion_leave.get() {
                set_window_cursor_name(&window, Some("crosshair"));
            } else {
                set_window_cursor_name(&window, None);
            }
        }
    });

    drawing_area.add_controller(motion);

    // Key controller
    let key_controller = EventControllerKey::new();
    let state_keys = state.clone();
    let drawing_area_keys = drawing_area.downgrade();
    let tool_buttons_keys = tool_buttons.clone();
    let apply_crop_btn_keys = apply_crop_btn.clone();
    let update_toolbar_for_tool_keys = update_toolbar_for_tool.clone();
    let update_crop_size_fields_keys = update_crop_size_fields.clone();
    let sync_picker_for_active_tool_keys = sync_picker_for_active_tool.clone();
    let eyedropper_mode_keys = eyedropper_mode.clone();
    let eyedropper_point_keys = eyedropper_point.clone();
    let eyedropper_rendered_keys = eyedropper_rendered.clone();
    let canvas_eyedropper_ring_keys = canvas_eyedropper_ring.clone();
    let window_keys = window.downgrade();
    let app_keys = app.downgrade();
    key_controller.connect_key_pressed(move |_, key, _, modifiers| {
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

        let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        let pressed = key.to_unicode();

        if ctrl && (pressed == Some('z') || pressed == Some('Z')) {
            let changed = if shift {
                state_keys.lock().unwrap().redo()
            } else {
                state_keys.lock().unwrap().undo()
            };
            if changed {
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if ctrl && (pressed == Some('y') || pressed == Some('Y')) {
            if state_keys.lock().unwrap().redo() {
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
            }
            return glib::Propagation::Stop;
        }

        if !ctrl {
            if let Some((tool, active_button)) =
                pressed.and_then(super::types::tool_shortcut_target)
            {
                set_active_tool_button(&tool_buttons_keys, active_button);
                let has_crop_selection = {
                    let mut st = state_keys.lock().unwrap();
                    st.set_tool(tool);
                    if matches!(tool, Tool::Crop) {
                        st.ensure_crop_selection_initialized();
                    }
                    st.crop_selection.is_some()
                };
                update_toolbar_for_tool_keys(tool);
                sync_picker_for_active_tool_keys();
                set_crop_apply_button_state(
                    &apply_crop_btn_keys,
                    matches!(tool, Tool::Crop),
                    has_crop_selection,
                );
                update_crop_size_fields_keys();
                if let Some(area) = drawing_area_keys.upgrade() {
                    area.queue_draw();
                }
                return glib::Propagation::Stop;
            }
        }

        if (key == gdk::Key::Delete || key == gdk::Key::BackSpace)
            && state_keys.lock().unwrap().remove_selected_action()
        {
            if let Some(area) = drawing_area_keys.upgrade() {
                area.queue_draw();
            }
            return glib::Propagation::Stop;
        }

        if key == gdk::Key::Escape {
            if let Some(window) = window_keys.upgrade() {
                window.close();
            }
            if let Some(app) = app_keys.upgrade() {
                app.quit();
            }
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let app_weak = app.downgrade();
    window.connect_close_request(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
        glib::Propagation::Proceed
    });

    window.present();
}
