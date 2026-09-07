//! Shared Motion preview/export drawing.

use gtk4::cairo::{Context, Format, ImageSurface, Matrix};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recording::editor::model::{
    affine_from_three_points as affine_components, card_depth, project_card_corners, project_point,
    MotionBlurBudgetMode, MotionState, MotionTextSegment, MotionTransform, MOTION_EXPORT_FPS,
};

pub fn draw_motion_frame(
    context: &Context,
    width: i32,
    height: i32,
    surface: &ImageSurface,
    motion: &MotionState,
    time: f64,
    checkerboard: bool,
    prefers_dark: bool,
    live_preview: bool,
) {
    paint_backdrop(context, width, height, checkerboard, prefers_dark);
    let mesh_div = if live_preview { 3 } else { 8 };
    let current_transform = motion.sample(time);
    let current_anchor = motion.zoom_anchor_at(time);
    // Cairo has no equivalent of Shotbase's full-quality CIMotionBlur and
    // CIZoomBlur filters, so ApexShot uses this bounded temporal fallback.
    // The recovered schema and bounds remain shared with the source app.
    for sample in motion.motion_blur_settings.transform_trail(
        f64::from(MOTION_EXPORT_FPS),
        if live_preview {
            MotionBlurBudgetMode::LivePreviewPlayback
        } else {
            MotionBlurBudgetMode::FullQuality
        },
    ) {
        let sample_t = (time + sample.offset_seconds).max(0.0);
        if sample.opacity > 0.001 {
            let transform = motion.sample(sample_t);
            let anchor = motion.zoom_anchor_at(sample_t);
            if !motion_pose_differs(transform, current_transform, anchor, current_anchor) {
                continue;
            }
            draw_transformed_card(
                context,
                surface,
                width as f64,
                height as f64,
                transform,
                anchor,
                sample.opacity,
                mesh_div,
            );
        }
    }
    draw_transformed_card(
        context,
        surface,
        width as f64,
        height as f64,
        current_transform,
        current_anchor,
        1.0,
        mesh_div,
    );
    paint_motion_text(context, surface, width as f64, height as f64, motion, time);
}

/// A held camera pose is already identical to the sharp overlay. Skipping it
/// preserves Shotbase's budgeted-preview behavior and avoids mesh work without
/// changing the exported pixels.
fn motion_pose_differs(
    a: MotionTransform,
    b: MotionTransform,
    anchor_a: (f64, f64),
    anchor_b: (f64, f64),
) -> bool {
    const EPSILON: f64 = 0.0001;
    (a.scale - b.scale).abs() > EPSILON
        || (a.rotation_x - b.rotation_x).abs() > EPSILON
        || (a.rotation_y - b.rotation_y).abs() > EPSILON
        || (a.rotation_z - b.rotation_z).abs() > EPSILON
        || (a.perspective - b.perspective).abs() > EPSILON
        || (a.pos_x - b.pos_x).abs() > EPSILON
        || (a.pos_y - b.pos_y).abs() > EPSILON
        || (anchor_a.0 - anchor_b.0).abs() > EPSILON
        || (anchor_a.1 - anchor_b.1).abs() > EPSILON
}

/// Draw titles in the captured image's coordinate space, then project that
/// space through the current camera. This is deliberately shared by preview
/// and export: a title placed on the card follows yaw, pitch, roll, scale, and
/// position instead of floating over the editor viewport.
fn paint_motion_text(
    context: &Context,
    surface: &ImageSurface,
    viewport_w: f64,
    viewport_h: f64,
    motion: &MotionState,
    time: f64,
) {
    let transform = motion.sample(time);
    let layout = CardLayout::new(
        surface,
        viewport_w,
        viewport_h,
        transform,
        motion.zoom_anchor_at(time),
    );
    for segment in &motion.text_segments {
        let Some(style) = segment.sample(time) else {
            continue;
        };
        if style.alpha < 0.02 {
            continue;
        }
        let raw = segment.text.trim();
        let line = if raw.is_empty() { "Title" } else { raw };
        let line = line.lines().next().unwrap_or("Title");
        let line = visible_motion_text(line, segment.scope, style.reveal);
        if line.is_empty() {
            continue;
        }
        let size = (layout.img_h * 0.060 * segment.size.clamp(0.5, 2.2)).clamp(14.0, 160.0);
        let anchor_x = segment.pos_x.clamp(0.05, 0.95) * layout.img_w;
        let anchor_y = segment.pos_y.clamp(0.05, 0.95) * layout.img_h;
        let Some(matrix) = layout.local_matrix(anchor_x, anchor_y) else {
            continue;
        };
        let _ = context.save();
        context.transform(matrix);
        context.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        context.set_font_size(size);
        let Ok(ext) = context.text_extents(&line) else {
            let _ = context.restore();
            continue;
        };
        let animated_offset_x = style.offset_x * layout.img_h / 1080.0;
        let x = anchor_x + animated_offset_x - ext.width() / 2.0 - ext.x_bearing();
        // `MotionTextStyle` uses a 1080pt artboard baseline. Scale it into
        // the source image so the entrance distance remains consistent after
        // the card is fitted into either preview or export.
        let animated_offset = style.offset_y * layout.img_h / 1080.0;
        let y = anchor_y + animated_offset - ext.height() / 2.0 - ext.y_bearing();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.42 * style.alpha);
        context.move_to(x, y + 4.0);
        let _ = context.show_text(&line);
        context.set_source_rgba(1.0, 1.0, 1.0, style.alpha);
        context.move_to(x, y);
        let _ = context.show_text(&line);
        let _ = context.restore();
    }
}

fn visible_motion_text(
    text: &str,
    scope: crate::recording::editor::model::MotionTextScope,
    reveal: f64,
) -> String {
    if reveal >= 0.999 {
        return text.to_string();
    }
    match scope {
        crate::recording::editor::model::MotionTextScope::Character => {
            let count = (text.chars().count() as f64 * reveal).ceil() as usize;
            text.chars().take(count).collect()
        }
        crate::recording::editor::model::MotionTextScope::Word => {
            let words: Vec<_> = text.split_whitespace().collect();
            let count = (words.len() as f64 * reveal).ceil() as usize;
            words.into_iter().take(count).collect::<Vec<_>>().join(" ")
        }
        crate::recording::editor::model::MotionTextScope::Line => {
            if reveal > 0.0 {
                text.to_string()
            } else {
                String::new()
            }
        }
    }
}

/// Whether a preview pointer is on the rendered text itself. Text placement
/// must begin on this hit region; a selected title alone is not permission to
/// rewrite its coordinates by dragging empty canvas space.
pub fn motion_text_contains_view_point(
    surface: &ImageSurface,
    viewport_w: f64,
    viewport_h: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    segment: &MotionTextSegment,
    time: f64,
    view_x: f64,
    view_y: f64,
) -> bool {
    let Some(style) = segment.sample(time) else {
        return false;
    };
    let raw = segment.text.trim();
    let line = if raw.is_empty() { "Title" } else { raw };
    let line = visible_motion_text(
        line.lines().next().unwrap_or("Title"),
        segment.scope,
        style.reveal,
    );
    if line.is_empty() || style.alpha < 0.02 {
        return false;
    }

    let layout = CardLayout::new(surface, viewport_w, viewport_h, transform, zoom_anchor);
    let size = (layout.img_h * 0.060 * segment.size.clamp(0.5, 2.2)).clamp(14.0, 160.0);
    let Ok(measure) = Context::new(surface) else {
        return false;
    };
    measure.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    measure.set_font_size(size);
    let Ok(ext) = measure.text_extents(&line) else {
        return false;
    };
    let anchor_x =
        segment.pos_x.clamp(0.05, 0.95) * layout.img_w + style.offset_x * layout.img_h / 1080.0;
    let anchor_y =
        segment.pos_y.clamp(0.05, 0.95) * layout.img_h + style.offset_y * layout.img_h / 1080.0;
    let pointer = view_point_to_motion_text_position(
        surface,
        viewport_w,
        viewport_h,
        transform,
        zoom_anchor,
        view_x,
        view_y,
    );
    let pointer_x = pointer.0 * layout.img_w;
    let pointer_y = pointer.1 * layout.img_h;
    // Keep a small source-space hit slop so a title is still easy to grab at
    // normal preview zoom, while never turning empty card space into a drag.
    let slop = (8.0 / layout.fit.max(0.05)).min(24.0);
    let left = anchor_x - ext.width() / 2.0;
    let top = anchor_y - ext.height() / 2.0;
    pointer_x >= left - slop
        && pointer_x <= left + ext.width() + slop
        && pointer_y >= top - slop
        && pointer_y <= top + ext.height() + slop
}

#[derive(Clone, Copy)]
struct CardLayout {
    img_w: f64,
    img_h: f64,
    fit: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
}

impl CardLayout {
    fn new(
        surface: &ImageSurface,
        viewport_w: f64,
        viewport_h: f64,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> Self {
        let img_w = surface.width().max(1) as f64;
        let img_h = surface.height().max(1) as f64;
        let pad = 96.0;
        let fit = ((viewport_w - pad) / img_w)
            .min((viewport_h - pad) / img_h)
            .clamp(0.05, 1.0);
        let (cx, cy) = motion_card_center(
            img_w,
            img_h,
            fit,
            viewport_w,
            viewport_h,
            transform,
            zoom_anchor,
        );
        Self {
            img_w,
            img_h,
            fit,
            transform,
            cx,
            cy,
        }
    }

    fn project(&self, image_x: f64, image_y: f64) -> (f64, f64) {
        let hw = self.img_w * self.fit * self.transform.scale / 2.0;
        let hh = self.img_h * self.fit * self.transform.scale / 2.0;
        let depth = card_depth(hw, hh, self.transform.perspective);
        let (x, y) = project_point(
            (image_x / self.img_w * 2.0 - 1.0) * hw,
            (image_y / self.img_h * 2.0 - 1.0) * hh,
            self.transform,
            depth,
        );
        (self.cx + x, self.cy + y)
    }

    fn local_matrix(&self, image_x: f64, image_y: f64) -> Option<Matrix> {
        let origin = self.project(image_x, image_y);
        let x = self.project((image_x + 1.0).min(self.img_w), image_y);
        let y = self.project(image_x, (image_y + 1.0).min(self.img_h));
        let xx = x.0 - origin.0;
        let yx = x.1 - origin.1;
        let xy = y.0 - origin.0;
        let yy = y.1 - origin.1;
        if (xx * yy - xy * yx).abs() < 1e-8 {
            return None;
        }
        Some(Matrix::new(
            xx,
            yx,
            xy,
            yy,
            origin.0 - xx * image_x - xy * image_y,
            origin.1 - yx * image_x - yy * image_y,
        ))
    }
}

/// Convert a pointer in the Motion preview back into the source artboard.
/// A short Newton refinement keeps placement accurate for the non-linear
/// perspective projection used by the card mesh.
pub fn view_point_to_motion_text_position(
    surface: &ImageSurface,
    viewport_w: f64,
    viewport_h: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    view_x: f64,
    view_y: f64,
) -> (f64, f64) {
    let layout = CardLayout::new(surface, viewport_w, viewport_h, transform, zoom_anchor);
    let mut best = (0.5, 0.5);
    let mut best_distance = f64::INFINITY;
    for row in 0..=12 {
        for column in 0..=12 {
            let u = column as f64 / 12.0;
            let v = row as f64 / 12.0;
            let point = layout.project(u * layout.img_w, v * layout.img_h);
            let distance = (point.0 - view_x).powi(2) + (point.1 - view_y).powi(2);
            if distance < best_distance {
                best_distance = distance;
                best = (u, v);
            }
        }
    }
    for _ in 0..6 {
        let point = layout.project(best.0 * layout.img_w, best.1 * layout.img_h);
        let du = layout.project(
            ((best.0 + 0.002).min(1.0)) * layout.img_w,
            best.1 * layout.img_h,
        );
        let dv = layout.project(
            best.0 * layout.img_w,
            ((best.1 + 0.002).min(1.0)) * layout.img_h,
        );
        let j00 = (du.0 - point.0) / 0.002;
        let j10 = (du.1 - point.1) / 0.002;
        let j01 = (dv.0 - point.0) / 0.002;
        let j11 = (dv.1 - point.1) / 0.002;
        let det = j00 * j11 - j01 * j10;
        if det.abs() < 1e-7 {
            break;
        }
        let dx = point.0 - view_x;
        let dy = point.1 - view_y;
        best.0 = (best.0 - (j11 * dx - j01 * dy) / det).clamp(0.0, 1.0);
        best.1 = (best.1 - (-j10 * dx + j00 * dy) / det).clamp(0.0, 1.0);
    }
    (best.0.clamp(0.05, 0.95), best.1.clamp(0.05, 0.95))
}

fn paint_backdrop(
    context: &Context,
    width: i32,
    height: i32,
    checkerboard: bool,
    prefers_dark: bool,
) {
    if checkerboard {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
    } else if prefers_dark {
        context.set_source_rgb(0.067, 0.067, 0.067);
        context.paint().ok();
    } else {
        context.set_source_rgb(0.93, 0.94, 0.96);
        context.paint().ok();
    }
}

fn draw_transformed_card(
    context: &Context,
    surface: &ImageSurface,
    viewport_w: f64,
    viewport_h: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    alpha: f64,
    mesh_div: usize,
) {
    let img_w = surface.width() as f64;
    let img_h = surface.height() as f64;
    if img_w < 1.0 || img_h < 1.0 {
        return;
    }
    let pad = 96.0;
    let fit = ((viewport_w - pad) / img_w)
        .min((viewport_h - pad) / img_h)
        .clamp(0.05, 1.0);
    let (cx, cy) = motion_card_center(
        img_w,
        img_h,
        fit,
        viewport_w,
        viewport_h,
        transform,
        zoom_anchor,
    );
    let corners = project_card_corners(img_w, img_h, fit, transform, cx, cy);
    if alpha >= 0.99 {
        let drop = 16.0 + transform.perspective * 10.0;
        context.set_source_rgba(0.0, 0.0, 0.0, 0.28);
        context.move_to(corners[0].0, corners[0].1 + drop);
        context.line_to(corners[1].0, corners[1].1 + drop);
        context.line_to(corners[2].0, corners[2].1 + drop);
        context.line_to(corners[3].0, corners[3].1 + drop);
        context.close_path();
        let _ = context.fill();
    }

    paint_perspective_card(
        context, surface, img_w, img_h, fit, transform, cx, cy, alpha, mesh_div,
    );
}

/// Keep the selected source point stationary while a camera scales in. The
/// transform's ordinary X/Y position is applied first; the anchor offset then
/// compensates only for zoom. Preview, export, title painting, and inverse
/// title placement all use this same center calculation.
fn motion_card_center(
    img_w: f64,
    img_h: f64,
    fit: f64,
    viewport_w: f64,
    viewport_h: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
) -> (f64, f64) {
    let cx = viewport_w / 2.0 + transform.pos_x * viewport_w * 0.12;
    let cy = viewport_h / 2.0 + transform.pos_y * viewport_h * 0.12;
    let (anchor_x, anchor_y) = (zoom_anchor.0.clamp(0.0, 1.0), zoom_anchor.1.clamp(0.0, 1.0));
    if (transform.scale - 1.0).abs() < f64::EPSILON
        || ((anchor_x - 0.5).abs() < f64::EPSILON && (anchor_y - 0.5).abs() < f64::EPSILON)
    {
        return (cx, cy);
    }
    let half_w = img_w * fit / 2.0;
    let half_h = img_h * fit / 2.0;
    let local_x = (anchor_x * 2.0 - 1.0) * half_w;
    let local_y = (anchor_y * 2.0 - 1.0) * half_h;
    let mut unzoomed = transform;
    unzoomed.scale = 1.0;
    let before = project_point(
        local_x,
        local_y,
        unzoomed,
        card_depth(half_w, half_h, transform.perspective),
    );
    let scaled_half_w = half_w * transform.scale;
    let scaled_half_h = half_h * transform.scale;
    let after = project_point(
        local_x * transform.scale,
        local_y * transform.scale,
        transform,
        card_depth(scaled_half_w, scaled_half_h, transform.perspective),
    );
    (cx + before.0 - after.0, cy + before.1 - after.1)
}

fn paint_perspective_card(
    context: &Context,
    surface: &ImageSurface,
    img_w: f64,
    img_h: f64,
    fit: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
    alpha: f64,
    mesh_div: usize,
) {
    let hw = img_w * fit * transform.scale / 2.0;
    let hh = img_h * fit * transform.scale / 2.0;
    let depth = card_depth(hw, hh, transform.perspective);
    let bent = transform.rotation_x.abs()
        + transform.rotation_y.abs()
        + transform.rotation_z.abs()
        + transform.perspective;
    if bent < 0.05 {
        let corners = [
            (cx - hw, cy - hh),
            (cx + hw, cy - hh),
            (cx + hw, cy + hh),
            (cx - hw, cy + hh),
        ];
        paint_textured_triangle(
            context,
            surface,
            [(0.0, 0.0), (img_w, 0.0), (0.0, img_h)],
            [corners[0], corners[1], corners[3]],
            alpha,
        );
        paint_textured_triangle(
            context,
            surface,
            [(img_w, 0.0), (img_w, img_h), (0.0, img_h)],
            [corners[1], corners[2], corners[3]],
            alpha,
        );
        return;
    }

    // Pitch/yaw quads are not parallelograms. A 3-point affine per cell
    // misses the fourth corner and leaves vertical gaps; two triangles
    // from 3D-projected vertices cover the trapezoid.
    let div = mesh_div.max(2);
    let cols = div + 1;
    let mut grid = vec![(0.0, 0.0); cols * cols];
    for j in 0..=div {
        for i in 0..=div {
            let u = i as f64 / div as f64;
            let v = j as f64 / div as f64;
            let (px, py) =
                project_point((u * 2.0 - 1.0) * hw, (v * 2.0 - 1.0) * hh, transform, depth);
            grid[j * cols + i] = (cx + px, cy + py);
        }
    }
    for j in 0..div {
        for i in 0..div {
            let u0 = i as f64 / div as f64;
            let u1 = (i + 1) as f64 / div as f64;
            let v0 = j as f64 / div as f64;
            let v1 = (j + 1) as f64 / div as f64;
            let s00 = (u0 * img_w, v0 * img_h);
            let s10 = (u1 * img_w, v0 * img_h);
            let s01 = (u0 * img_w, v1 * img_h);
            let s11 = (u1 * img_w, v1 * img_h);
            let d00 = grid[j * cols + i];
            let d10 = grid[j * cols + i + 1];
            let d01 = grid[(j + 1) * cols + i];
            let d11 = grid[(j + 1) * cols + i + 1];
            paint_textured_triangle(context, surface, [s00, s10, s01], [d00, d10, d01], alpha);
            paint_textured_triangle(context, surface, [s10, s11, s01], [d10, d11, d01], alpha);
        }
    }
}

fn paint_textured_triangle(
    context: &Context,
    surface: &ImageSurface,
    src: [(f64, f64); 3],
    dest: [(f64, f64); 3],
    alpha: f64,
) {
    let Some(matrix) = affine_from_three_points(src, dest) else {
        return;
    };
    let clip = expand_triangle(dest, 0.6);
    let _ = context.save();
    context.move_to(clip[0].0, clip[0].1);
    context.line_to(clip[1].0, clip[1].1);
    context.line_to(clip[2].0, clip[2].1);
    context.close_path();
    context.clip();
    context.transform(matrix);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    if alpha < 0.999 {
        let _ = context.paint_with_alpha(alpha);
    } else {
        let _ = context.paint();
    }
    let _ = context.restore();
}

fn expand_triangle(pts: [(f64, f64); 3], px: f64) -> [(f64, f64); 3] {
    let cx = (pts[0].0 + pts[1].0 + pts[2].0) / 3.0;
    let cy = (pts[0].1 + pts[1].1 + pts[2].1) / 3.0;
    pts.map(|(x, y)| {
        let dx = x - cx;
        let dy = y - cy;
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        (x + dx / len * px, y + dy / len * px)
    })
}

fn affine_from_three_points(src: [(f64, f64); 3], dest: [(f64, f64); 3]) -> Option<Matrix> {
    let (xx, yx, xy, yy, x0, y0) = affine_components(src, dest)?;
    Some(Matrix::new(xx, yx, xy, yy, x0, y0))
}

pub fn export_motion_mp4(
    snapshot: &RgbaImage,
    motion: &MotionState,
    prefers_dark: bool,
    source_image: &Path,
) -> Result<PathBuf, String> {
    crate::recording::editor::ffmpeg::ensure_tools_available()
        .map_err(|error| error.to_string())?;

    let out_w = 1920i32;
    let out_h = 1080i32;
    let Some(card) = crate::capture::editor::render::rgba_image_to_surface(snapshot) else {
        return Err("could not prepare the Motion still".into());
    };

    let config = crate::config::load_config().sanitized();
    let fallback = source_image
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = config.video_editor_export_dir(&fallback);
    let _ = fs::create_dir_all(&dir);
    let stem = source_image
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("ApexShot");
    let output = unique_motion_path(&dir, stem);

    let work = std::env::temp_dir().join(format!(
        "apexshot-motion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&work).map_err(|error| error.to_string())?;

    let frame_count = ((motion.duration * f64::from(MOTION_EXPORT_FPS)).round() as u32).max(1);
    for index in 0..frame_count {
        let time = if frame_count <= 1 {
            0.0
        } else {
            motion.duration * f64::from(index) / f64::from(frame_count - 1)
        };
        let mut surface = ImageSurface::create(Format::ARgb32, out_w, out_h)
            .map_err(|error| error.to_string())?;
        {
            let context = Context::new(&surface).map_err(|error| error.to_string())?;
            draw_motion_frame(
                &context,
                out_w,
                out_h,
                &card,
                motion,
                time,
                false,
                prefers_dark,
                false,
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let image = {
            let data = surface.data().map_err(|error| error.to_string())?;
            crate::capture::editor::render::cairo_argb_to_rgba_image(
                out_w as u32,
                out_h as u32,
                stride,
                data.as_ref(),
            )
        };
        let frame_path = work.join(format!("frame_{index:04}.png"));
        image.save(&frame_path).map_err(|error| error.to_string())?;
    }

    let status = Command::new("ffmpeg")
        .args(["-y", "-framerate", &MOTION_EXPORT_FPS.to_string(), "-i"])
        .arg(work.join("frame_%04d.png"))
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            "-movflags",
            "+faststart",
        ])
        .arg(&output)
        .status()
        .map_err(|error| error.to_string())?;
    let _ = fs::remove_dir_all(&work);
    if !status.success() {
        return Err("ffmpeg failed to encode the Motion video".into());
    }
    Ok(output)
}

fn unique_motion_path(dir: &Path, stem: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            format!("{stem} Motion.mp4")
        } else {
            format!("{stem} Motion {n}.mp4")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        motion_pose_differs, motion_text_contains_view_point, view_point_to_motion_text_position,
        CardLayout,
    };
    use crate::recording::editor::model::{
        project_card_corners, MotionSegment, MotionState, MotionTransform,
        DEFAULT_MOTION_END_ROTATION_Y, DEFAULT_MOTION_END_SCALE,
    };
    use gtk4::cairo::{Format, ImageSurface};

    #[test]
    fn cinematic_sample_starts_identity_and_ends_at_target() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let start = motion.sample(0.0);
        let end = motion.sample(motion.duration);
        assert!((start.scale - 1.0).abs() < 1e-6);
        assert!(start.rotation_y.abs() < 1e-6);
        assert!((end.scale - DEFAULT_MOTION_END_SCALE).abs() < 1e-6);
        assert!((end.rotation_y - DEFAULT_MOTION_END_ROTATION_Y).abs() < 1e-6);
        assert_eq!(motion.segments.len(), 1);
        assert_eq!(motion.selected, Some(0));
        assert_eq!(motion.segments[0], MotionSegment::default_cinematic(3.0));
    }

    #[test]
    fn held_pose_does_not_spend_a_motion_blur_sample() {
        let pose = MotionTransform::default();
        assert!(!motion_pose_differs(pose, pose, (0.5, 0.5), (0.5, 0.5)));
        assert!(motion_pose_differs(
            MotionTransform {
                scale: 1.001,
                ..pose
            },
            pose,
            (0.5, 0.5),
            (0.5, 0.5),
        ));
        assert!(motion_pose_differs(pose, pose, (0.51, 0.5), (0.5, 0.5)));
    }

    #[test]
    fn stretching_duration_holds_end_pose_after_the_seeded_move() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let seed_end = motion.segments[0].end;
        motion.set_duration(6.0);
        assert!((motion.segments[0].end - seed_end).abs() < 1e-6);
        let end = motion.sample(6.0);
        assert!((end.scale - DEFAULT_MOTION_END_SCALE).abs() < 1e-6);
    }

    #[test]
    fn effect_segments_inherit_the_camera_pose_before_them() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_duration(6.0);
        motion
            .add_segment_at(2.0)
            .expect("a second non-overlapping move");

        let first_end = motion.segments[0].to;
        assert_eq!(motion.segments[1].from, first_end);
        assert_eq!(
            motion.sample(2.0),
            MotionTransform {
                perspective: motion.perspective_intensity,
                ..first_end
            }
        );

        motion.selected = Some(0);
        motion.set_selected_end_scale(1.5);
        assert!((motion.segments[1].from.scale - 1.5).abs() < 1e-6);
    }

    #[test]
    fn timeline_snap_targets_include_the_playhead_and_track_boundaries() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_duration(6.0);
        motion.playhead = 2.5;
        motion
            .add_text_at(3.0)
            .expect("a text segment after the seed move");

        assert!((motion.snap_effect_time(2.47, 0.05, None) - 2.5).abs() < 1e-6);
        assert!((motion.snap_text_time(2.96, 0.05, None) - 3.0).abs() < 1e-6);
        assert!((motion.snap_effect_time(5.98, 0.05, None) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn card_placement_round_trips_through_a_tilted_projection() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let transform = MotionTransform {
            scale: 1.18,
            rotation_x: -9.0,
            rotation_y: 14.0,
            rotation_z: 3.0,
            perspective: 0.24,
            pos_x: 0.18,
            pos_y: -0.12,
        };
        let zoom_anchor = (0.22, 0.78);
        let layout = CardLayout::new(&surface, 1440.0, 900.0, transform, zoom_anchor);
        let expected = (0.27, 0.71);
        let point = layout.project(expected.0 * layout.img_w, expected.1 * layout.img_h);
        let actual = view_point_to_motion_text_position(
            &surface,
            1440.0,
            900.0,
            transform,
            zoom_anchor,
            point.0,
            point.1,
        );
        assert!((actual.0 - expected.0).abs() < 0.003, "x: {actual:?}");
        assert!((actual.1 - expected.1).abs() < 0.003, "y: {actual:?}");
    }

    #[test]
    fn title_hit_test_rejects_empty_preview_space() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        let segment = &motion.text_segments[index];
        let transform = MotionTransform {
            scale: 1.12,
            rotation_x: -7.0,
            rotation_y: 10.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let zoom_anchor = (0.32, 0.68);
        let layout = CardLayout::new(&surface, 1440.0, 900.0, transform, zoom_anchor);
        let title_center =
            layout.project(segment.pos_x * layout.img_w, segment.pos_y * layout.img_h);

        assert!(motion_text_contains_view_point(
            &surface,
            1440.0,
            900.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            title_center.0,
            title_center.1,
        ));
        assert!(!motion_text_contains_view_point(
            &surface,
            1440.0,
            900.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            4.0,
            4.0,
        ));
    }

    #[test]
    fn glide_and_snappy_diverge_during_the_ease_window() {
        let mut glide = MotionState::default();
        glide.seed_default_cinematic();
        glide.set_selected_easing(crate::recording::editor::model::ZoomEasing::Glide);
        let mut snappy = MotionState::default();
        snappy.seed_default_cinematic();
        snappy.selected = Some(0);
        snappy.set_selected_easing(crate::recording::editor::model::ZoomEasing::Snappy);
        let t = 0.2;
        let g = glide.sample(t).scale;
        let s = snappy.sample(t).scale;
        assert!(
            (g - s).abs() > 0.01,
            "Glide and Snappy should disagree mid-ease, got {g} vs {s}"
        );
    }

    #[test]
    fn shotbase_transform_timing_defaults_drive_glide_motion() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let timing = motion.transform_timing;
        assert!((timing.transition_duration - 1.2).abs() < f64::EPSILON);
        assert!((timing.easing_x1 - 0.25).abs() < f64::EPSILON);
        assert!((timing.easing_y1 - 1.0).abs() < f64::EPSILON);
        assert!((timing.easing_x2 - 0.50).abs() < f64::EPSILON);
        assert!((timing.easing_y2 - 1.0).abs() < f64::EPSILON);

        let default_progress = motion.sample(0.6).scale;
        motion.set_transform_timing(
            crate::recording::editor::model::MotionEffectTransformTiming {
                transition_duration: 1.2,
                easing_x1: 0.0,
                easing_y1: 0.0,
                easing_x2: 1.0,
                easing_y2: 1.0,
            },
        );
        let linear_progress = motion.sample(0.6).scale;
        assert!(
            default_progress > linear_progress + 0.01,
            "Shotbase's recovered Bézier should advance ahead of linear at mid-transition"
        );
        assert!((linear_progress - 1.06).abs() < 0.002);
    }

    #[test]
    fn blur_and_position_setters_stick() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_motion_blur(0.4);
        motion.set_selected_end_pos_x(0.5);
        motion.set_selected_end_pos_y(-0.25);
        assert!((motion.motion_blur - 0.4).abs() < 1e-6);
        assert!(motion.motion_blur_settings.enabled);
        assert!((motion.effective_motion_blur() - 0.4).abs() < 1e-6);
        let end = motion.sample(motion.duration);
        assert!((end.pos_x - 0.5).abs() < 1e-6);
        assert!((end.pos_y + 0.25).abs() < 1e-6);
        let start = motion.sample(0.0);
        assert!(start.pos_x.abs() < 1e-6);
        assert!(start.pos_y.abs() < 1e-6);
    }

    #[test]
    fn recovered_disabled_effect_and_text_fields_are_no_ops() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_selected_perspective(0.0);
        motion.set_selected_disabled(true);
        assert_eq!(motion.sample(0.4), MotionTransform::default());
        assert_eq!(motion.sample(motion.duration), MotionTransform::default());

        let text = motion.add_text_at(0.05).expect("text clip");
        motion.set_selected_text_scope(crate::recording::editor::model::MotionTextScope::Word);
        motion.set_selected_text_typewriter_time(0.25);
        motion.set_selected_text_disabled(true);
        let segment = &motion.text_segments[text];
        assert_eq!(
            segment.scope,
            crate::recording::editor::model::MotionTextScope::Word
        );
        assert!((segment.typewriter_time - 0.25).abs() < f64::EPSILON);
        assert!(segment.sample(0.1).is_none());
    }

    #[test]
    fn recovered_perspective_intensity_is_global_across_effect_segments() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_selected_perspective(0.42);
        motion.add_segment_at(3.0).expect("second effect clip");

        assert_eq!(motion.segments.len(), 2);
        assert!((motion.sample(0.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(3.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(motion.duration).perspective - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn recovered_segment_intensity_blends_the_camera_target() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_selected_perspective(0.0);
        motion.set_selected_intensity(0.0);

        assert_eq!(motion.sample(motion.duration), MotionTransform::default());

        motion.set_selected_intensity(0.5);
        let half = motion.sample(motion.duration);
        assert!((half.scale - 1.06).abs() < 1e-6);
        assert!((half.rotation_y - 4.0).abs() < 1e-6);
    }

    #[test]
    fn recovered_zoom_anchor_holds_its_source_point_during_scale() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let zoom_anchor = (0.2, 0.78);
        let transform = MotionTransform {
            scale: 1.32,
            rotation_x: -7.0,
            rotation_y: 12.0,
            rotation_z: 2.0,
            perspective: 0.24,
            pos_x: 0.1,
            pos_y: -0.08,
        };
        let mut unzoomed = transform;
        unzoomed.scale = 1.0;
        let before = CardLayout::new(&surface, 1440.0, 900.0, unzoomed, (0.5, 0.5));
        let after = CardLayout::new(&surface, 1440.0, 900.0, transform, zoom_anchor);
        let source_x = zoom_anchor.0 * before.img_w;
        let source_y = zoom_anchor.1 * before.img_h;
        let expected = before.project(source_x, source_y);
        let actual = after.project(source_x, source_y);
        assert!(
            (expected.0 - actual.0).abs() < 1e-6,
            "x: {expected:?} {actual:?}"
        );
        assert!(
            (expected.1 - actual.1).abs() < 1e-6,
            "y: {expected:?} {actual:?}"
        );
    }

    #[test]
    fn text_effect_presets_and_typewriter_scope_are_applied() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let first = motion.add_text_at(0.05).expect("text clip");
        assert_eq!(motion.text_segments.len(), 1);
        assert_eq!(motion.selected_text, Some(first));
        assert!(motion.selected.is_none());
        assert!(motion.add_text_at(0.1).is_none());
        let none = motion.text_segments[0].sample(0.12).expect("none sample");
        motion.set_selected_text_animation(
            crate::recording::editor::model::MotionTextAnimation::SlideBottom,
        );
        let slide = motion.text_segments[0].sample(0.12).expect("slide sample");
        assert!(slide.offset_y > none.offset_y + 4.0);
        assert!(slide.alpha > 0.0 && slide.alpha < 1.0);
        motion.set_selected_text_animation(
            crate::recording::editor::model::MotionTextAnimation::Typewriter,
        );
        let typewriter = motion.text_segments[0]
            .sample(0.12)
            .expect("typewriter sample");
        assert!(typewriter.reveal > 0.0 && typewriter.reveal < 1.0);
        assert!((motion.text_segments[0].pos_x - 0.5).abs() < 1e-6);
        motion.set_selected_text_pos(0.2, 0.3);
        motion.set_selected_text_size(1.6);
        assert!((motion.text_segments[0].pos_x - 0.2).abs() < 1e-6);
        assert!((motion.text_segments[0].pos_y - 0.3).abs() < 1e-6);
        assert!((motion.text_segments[0].size - 1.6).abs() < 1e-6);
        assert!(motion.remove_selected());
        assert!(motion.text_segments.is_empty());
        assert!(motion.has_segments());
    }

    #[test]
    fn add_segment_fills_a_gap_and_rejects_overlap() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let first_end = motion.segments[0].end;
        assert!(motion.add_segment_at(first_end + 0.05).is_some());
        assert_eq!(motion.segments.len(), 2);
        assert!(motion.add_segment_at(0.2).is_none());
        motion.selected = Some(1);
        assert!(motion.remove_selected());
        assert_eq!(motion.segments.len(), 1);
    }

    #[test]
    fn identity_projection_keeps_the_card_axis_aligned() {
        let transform = MotionTransform::default();
        let corners = project_card_corners(400.0, 200.0, 1.0, transform, 500.0, 300.0);
        let width = corners[1].0 - corners[0].0;
        let height = corners[3].1 - corners[0].1;
        assert!((width - 400.0).abs() < 1.0);
        assert!((height - 200.0).abs() < 1.0);
        assert!((corners[0].1 - corners[1].1).abs() < 0.5);
        assert!((corners[0].0 - corners[3].0).abs() < 0.5);
    }

    #[test]
    fn yaw_projection_stays_a_single_quad_not_a_fan() {
        let transform = MotionTransform {
            scale: 1.12,
            rotation_y: 8.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let corners = project_card_corners(800.0, 450.0, 1.0, transform, 960.0, 540.0);
        for (x, y) in corners {
            assert!(x.is_finite() && y.is_finite(), "corner exploded to {x},{y}");
            assert!(x > 200.0 && x < 1720.0, "corner x {x} left the viewport");
            assert!(y > 100.0 && y < 980.0, "corner y {y} left the viewport");
        }
        let top_w = corners[1].0 - corners[0].0;
        let bot_w = corners[2].0 - corners[3].0;
        assert!(top_w > 200.0 && bot_w > 200.0);
        assert!(
            (top_w - bot_w).abs() < top_w * 0.35,
            "quad crossed or shredded: top {top_w} bot {bot_w}"
        );
    }

    #[test]
    fn pitch_quad_is_not_a_parallelogram() {
        let transform = MotionTransform {
            rotation_x: 16.0,
            perspective: 0.22,
            ..MotionTransform::default()
        };
        let corners = project_card_corners(800.0, 450.0, 1.0, transform, 960.0, 540.0);
        for (x, y) in corners {
            assert!(x.is_finite() && y.is_finite());
        }
        let top_w = (corners[1].0 - corners[0].0).abs();
        let bot_w = (corners[2].0 - corners[3].0).abs();
        assert!(
            (top_w - bot_w).abs() > 8.0,
            "pitch should change top vs bottom width, got {top_w} vs {bot_w}"
        );
        let parallelogram = (
            corners[0].0 + (corners[1].0 - corners[0].0) + (corners[3].0 - corners[0].0),
            corners[0].1 + (corners[1].1 - corners[0].1) + (corners[3].1 - corners[0].1),
        );
        let gap = ((parallelogram.0 - corners[2].0).powi(2)
            + (parallelogram.1 - corners[2].1).powi(2))
        .sqrt();
        assert!(
            gap > 8.0,
            "a 3-point affine would miss the fourth pitch corner by only {gap}"
        );
    }

    #[test]
    fn affine_from_three_points_maps_source_to_dest() {
        let matrix = super::affine_from_three_points(
            [(0.0, 0.0), (10.0, 0.0), (0.0, 5.0)],
            [(100.0, 200.0), (120.0, 204.0), (98.0, 210.0)],
        )
        .expect("triangle");
        let map = |x: f64, y: f64| {
            (
                matrix.xx() * x + matrix.xy() * y + matrix.x0(),
                matrix.yx() * x + matrix.yy() * y + matrix.y0(),
            )
        };
        let p0 = map(0.0, 0.0);
        let p1 = map(10.0, 0.0);
        let p2 = map(0.0, 5.0);
        assert!((p0.0 - 100.0).abs() < 1e-6 && (p0.1 - 200.0).abs() < 1e-6);
        assert!((p1.0 - 120.0).abs() < 1e-6 && (p1.1 - 204.0).abs() < 1e-6);
        assert!((p2.0 - 98.0).abs() < 1e-6 && (p2.1 - 210.0).abs() < 1e-6);
    }
}
