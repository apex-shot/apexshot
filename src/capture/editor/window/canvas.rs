use gtk4::{prelude::*, Box as GtkBox, DrawingArea, Orientation, Overlay, ScrolledWindow};
use image::RgbaImage;
use std::f64::consts::TAU;

use super::super::{
    state::EditorState,
    types::{DrawColor, Point, Rect},
};

pub(super) const CANVAS_PADDING: i32 = 24;
pub(super) const EYEDROPPER_LOUPE_SIZE: i32 = 132;
const EYEDROPPER_LOUPE_GRID_SIZE: i32 = 15;
const EYEDROPPER_LOUPE_PIXEL_SIZE: f64 = 8.0;

pub(super) struct CanvasShellParts {
    pub root: GtkBox,
    pub drawing_area: DrawingArea,
    pub canvas_overlay: Overlay,
    pub canvas_scroller: ScrolledWindow,
    pub canvas_eyedropper_ring: DrawingArea,
}

pub(super) fn build_canvas_shell(
    img_width: i32,
    img_height: i32,
    background_sidebar: &GtkBox,
    eyedropper_loupe_size: i32,
) -> CanvasShellParts {
    let drawing_area = DrawingArea::new();
    drawing_area.set_hexpand(true);
    drawing_area.set_vexpand(false);
    drawing_area.set_content_width(img_width);
    drawing_area.set_content_height(img_height);
    drawing_area.set_size_request(img_width, img_height);
    drawing_area.add_css_class("editor-canvas");

    let canvas_overlay = Overlay::new();
    canvas_overlay.set_hexpand(true);
    canvas_overlay.set_vexpand(false);
    canvas_overlay.set_size_request(img_width, img_height);
    canvas_overlay.set_child(Some(&drawing_area));

    let canvas_scroller = ScrolledWindow::new();
    canvas_scroller.set_hexpand(true);
    canvas_scroller.set_vexpand(true);
    canvas_scroller.set_has_frame(false);
    canvas_scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    canvas_scroller.set_child(Some(&canvas_overlay));

    let canvas_eyedropper_ring = DrawingArea::new();
    canvas_eyedropper_ring.add_css_class("editor-screen-eyedropper-ring");
    canvas_eyedropper_ring.set_halign(gtk4::Align::Start);
    canvas_eyedropper_ring.set_valign(gtk4::Align::Start);
    canvas_eyedropper_ring.set_size_request(eyedropper_loupe_size, eyedropper_loupe_size);
    canvas_eyedropper_ring.set_visible(false);
    canvas_eyedropper_ring.set_can_target(false);
    canvas_overlay.add_overlay(&canvas_eyedropper_ring);

    let canvas_workspace = GtkBox::new(Orientation::Horizontal, 0);
    canvas_workspace.set_hexpand(true);
    canvas_workspace.set_vexpand(true);
    canvas_workspace.add_css_class("editor-canvas-workspace");
    canvas_scroller.set_hexpand(true);
    background_sidebar.set_halign(gtk4::Align::Start);
    canvas_workspace.append(background_sidebar);
    canvas_workspace.append(&canvas_scroller);

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("editor-canvas-frame");
    root.append(&canvas_workspace);

    CanvasShellParts {
        root,
        drawing_area,
        canvas_overlay,
        canvas_scroller,
        canvas_eyedropper_ring,
    }
}

pub(super) fn sample_editor_color_at_point(
    state: &EditorState,
    image_point: Point,
) -> Option<DrawColor> {
    let rendered = state.to_rendered_image().ok()?;
    sample_rendered_color_at_point(&rendered, image_point)
}

pub(super) fn crop_canvas_overflow(
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

pub(super) fn sample_rendered_color_at_point(
    rendered: &RgbaImage,
    image_point: Point,
) -> Option<DrawColor> {
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
    Some(DrawColor::new(
        rgba[0] as f64 / 255.0,
        rgba[1] as f64 / 255.0,
        rgba[2] as f64 / 255.0,
        rgba[3] as f64 / 255.0,
    ))
}

pub(super) fn eyedropper_loupe_position(cursor_x: f64, cursor_y: f64) -> (i32, i32) {
    let half_size = EYEDROPPER_LOUPE_SIZE as f64 / 2.0;
    (
        (cursor_x - half_size).round() as i32,
        (cursor_y - half_size).round() as i32,
    )
}

pub(super) fn draw_eyedropper_loupe(
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
