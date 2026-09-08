//! Shared Motion preview/export drawing.

use gtk4::cairo::{Context, Filter, Format, ImageSurface, LinearGradient, Matrix};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recording::editor::model::{
    affine_from_three_points as affine_components, card_depth, project_card_corners, project_point,
    MotionAppearance, MotionBackgroundFillType, MotionBlurBudgetMode, MotionState,
    MotionTextSegment, MotionTransform, MOTION_EXPORT_FPS,
};

pub fn draw_motion_frame(
    context: &Context,
    width: i32,
    height: i32,
    surface: &ImageSurface,
    motion: &MotionState,
    background_surface: Option<&ImageSurface>,
    time: f64,
    checkerboard: bool,
    prefers_dark: bool,
    live_preview: bool,
) {
    paint_backdrop(
        context,
        width,
        height,
        motion,
        background_surface,
        checkerboard,
        prefers_dark,
    );
    // Padding, zoom, and titles all lay out against the background's
    // rectangle so the card can never sit outside the scene it belongs to.
    let stage = if checkerboard {
        MotionStage::preview(f64::from(width), f64::from(height))
    } else {
        MotionStage::frame(f64::from(width), f64::from(height))
    };
    let current_transform = motion.sample(time);
    let current_anchor = motion.zoom_anchor_at(time);
    // The card is drawn as a triangle mesh that approximates the perspective
    // warp. Zooming in magnifies each affine cell until the tessellation
    // reads as a wavy warp, so live previews subdivide more when zoomed.
    let mesh_div = if live_preview {
        if current_transform.scale > 1.75 {
            7
        } else {
            3
        }
    } else {
        8
    };
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
                stage,
                transform,
                anchor,
                &motion.appearance,
                sample.opacity,
                mesh_div,
            );
        }
    }
    draw_transformed_card(
        context,
        surface,
        stage,
        current_transform,
        current_anchor,
        &motion.appearance,
        1.0,
        mesh_div,
    );
    paint_motion_text(context, surface, stage, motion, time);
}

/// The rectangle the Motion card is laid out inside: the background fill's
/// area. Exports lay out against the full frame; the editor lays out against
/// the bounded scene panel so padding can never push the card outside the
/// background. Both rectangles share the viewport's center.
#[derive(Clone, Copy)]
pub(super) struct MotionStage {
    pub bounds_w: f64,
    pub bounds_h: f64,
    pub center_x: f64,
    pub center_y: f64,
}

impl MotionStage {
    pub(super) fn frame(width: f64, height: f64) -> Self {
        Self {
            bounds_w: width,
            bounds_h: height,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }

    pub(super) fn preview(width: f64, height: f64) -> Self {
        let (_, _, bounds_w, bounds_h) = motion_scene_bounds(width, height);
        Self {
            bounds_w,
            bounds_h,
            center_x: width / 2.0,
            center_y: height / 2.0,
        }
    }
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
    stage: MotionStage,
    motion: &MotionState,
    time: f64,
) {
    let transform = motion.sample(time);
    let layout = CardLayout::with_padding(
        surface,
        stage,
        transform,
        motion.zoom_anchor_at(time),
        motion.appearance.background_padding,
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
    stage: MotionStage,
    padding: f64,
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

    let layout = CardLayout::with_padding(surface, stage, transform, zoom_anchor, padding);
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
        stage,
        padding,
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
    fn with_padding(
        surface: &ImageSurface,
        stage: MotionStage,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
        padding: f64,
    ) -> Self {
        let img_w = surface.width().max(1) as f64;
        let img_h = surface.height().max(1) as f64;
        let pad = padding.clamp(0.0, (stage.bounds_w.min(stage.bounds_h) - 2.0).max(0.0));
        let fit = ((stage.bounds_w - pad) / img_w)
            .min((stage.bounds_h - pad) / img_h)
            .clamp(0.05, 1.0);
        let (cx, cy) = motion_card_center(img_w, img_h, fit, stage, transform, zoom_anchor);
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
    stage: MotionStage,
    padding: f64,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    view_x: f64,
    view_y: f64,
) -> (f64, f64) {
    let layout = CardLayout::with_padding(surface, stage, transform, zoom_anchor, padding);
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
    motion: &MotionState,
    background_surface: Option<&ImageSurface>,
    checkerboard: bool,
    prefers_dark: bool,
) {
    let appearance = &motion.appearance;
    // The editor canvas keeps its checkerboard; a chosen fill paints inside a
    // bounded scene panel so the fill reads as a layer with visible
    // boundaries. Exports have no canvas: fills cover the full frame.
    let scene = if checkerboard {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
        Some(motion_scene_bounds(f64::from(width), f64::from(height)))
    } else {
        None
    };
    // The preview shows the plain checkerboard until a fill is chosen;
    // Shotbase's black scene for an unset fill only applies to exports.
    if checkerboard
        && matches!(
            appearance.background_fill_type,
            MotionBackgroundFillType::None
        )
    {
        return;
    }
    let _ = context.save();
    if let Some((x, y, scene_w, scene_h)) = scene {
        let radius = 16.0_f64.min(scene_w.min(scene_h) * 0.5);
        rounded_rectangle(context, x, y, scene_w, scene_h, radius);
        context.clip();
    }
    match appearance.background_fill_type {
        // Shotbase's explicit Motion-mode rule: None is a black scene, not
        // the editor's transparent checkerboard.
        MotionBackgroundFillType::None => {
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().ok();
        }
        MotionBackgroundFillType::Color => {
            let [r, g, b, a] = appearance.background_color;
            context.set_source_rgba(r, g, b, a);
            context.paint().ok();
        }
        MotionBackgroundFillType::Gradient => {
            let [r1, g1, b1, a1] = appearance.gradient_color_1;
            let [r2, g2, b2, a2] = appearance.gradient_color_2;
            let (x, y, w, h) = scene.unwrap_or((0.0, 0.0, f64::from(width), f64::from(height)));
            let gradient = LinearGradient::new(x, y, x + w, y + h);
            gradient.add_color_stop_rgba(0.0, r1, g1, b1, a1);
            gradient.add_color_stop_rgba(1.0, r2, g2, b2, a2);
            context.set_source(&gradient).ok();
            context.paint().ok();
        }
        MotionBackgroundFillType::Wallpaper | MotionBackgroundFillType::Image => {
            let path = match appearance.background_fill_type {
                MotionBackgroundFillType::Wallpaper => appearance.wallpaper_image_name.as_deref(),
                MotionBackgroundFillType::Image => appearance.custom_background_image.as_deref(),
                _ => None,
            };
            let (x, y, scene_w, scene_h) =
                scene.unwrap_or((0.0, 0.0, f64::from(width), f64::from(height)));
            if let Some(surface) = background_surface {
                paint_image_background(
                    context,
                    surface,
                    x,
                    y,
                    scene_w,
                    scene_h,
                    appearance.background_blur,
                );
            } else if let Some(surface) = path.and_then(load_motion_background_surface) {
                paint_image_background(
                    context,
                    &surface,
                    x,
                    y,
                    scene_w,
                    scene_h,
                    appearance.background_blur,
                );
            } else {
                context.set_source_rgb(0.0, 0.0, 0.0);
                context.paint().ok();
            }
        }
    }
    paint_background_noise(context, width, height, appearance.background_noise);
    context.restore().ok();
}

/// Preview-only scene panel: an inset rounded rectangle that keeps the
/// editor's checkerboard visible around the Motion fill as its boundary.
fn motion_scene_bounds(width: f64, height: f64) -> (f64, f64, f64, f64) {
    const MARGIN: f64 = 24.0;
    let w = width.max(0.0);
    let h = height.max(0.0);
    let inset = MARGIN.min(w.min(h) * 0.25);
    (
        inset,
        inset,
        (w - inset * 2.0).max(1.0),
        (h - inset * 2.0).max(1.0),
    )
}

pub(super) fn load_motion_background_surface(path: &str) -> Option<ImageSurface> {
    let image = image::open(path).ok()?.into_rgba8();
    crate::capture::editor::render::rgba_image_to_surface(&image)
}

fn paint_image_background(
    context: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    blur: f64,
) {
    let blur = blur.clamp(0.0, 1.0);
    if blur <= 0.001 {
        paint_cover_fit_at(context, surface, x, y, width, height, Filter::Good);
        return;
    }

    // Render the cover-fitted image once at a bounded resolution, then blur
    // its pixels. Downsampling alone only softened resampling artifacts and
    // did not produce a reliable background blur.
    let render_scale = (720.0 / width.max(height)).min(1.0);
    let render_w = (width * render_scale).ceil().max(1.0) as i32;
    let render_h = (height * render_scale).ceil().max(1.0) as i32;
    let Ok(mut rendered) = ImageSurface::create(Format::ARgb32, render_w, render_h) else {
        paint_cover_fit_at(context, surface, x, y, width, height, Filter::Good);
        return;
    };
    if let Ok(rendered_context) = Context::new(&rendered) {
        paint_cover_fit(&rendered_context, surface, render_w, render_h, Filter::Good);
    }
    rendered.flush();
    let stride = rendered.stride() as usize;
    let mut rgba = {
        let Ok(data) = rendered.data() else {
            return;
        };
        crate::capture::editor::render::cairo_argb_to_rgba_image(
            render_w as u32,
            render_h as u32,
            stride,
            data.as_ref(),
        )
    };
    let blur_rect = crate::capture::editor::types::Rect {
        x: 0,
        y: 0,
        width: render_w,
        height: render_h,
    };
    let radius = (blur * 32.0 * render_scale).max(1.0);
    for _ in 0..3 {
        crate::capture::editor::render::apply_blur_rect(&mut rgba, blur_rect, radius, true);
    }
    let Some(blurred) = crate::capture::editor::render::rgba_image_to_surface(&rgba) else {
        return;
    };
    let _ = context.save();
    context.translate(x, y);
    context.scale(width / f64::from(render_w), height / f64::from(render_h));
    context.set_source_surface(&blurred, 0.0, 0.0).ok();
    context.source().set_filter(Filter::Bilinear);
    context.paint().ok();
    context.restore().ok();
}

fn paint_cover_fit_at(
    context: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    filter: Filter,
) {
    let _ = context.save();
    context.translate(x, y);
    paint_cover_fit(
        context,
        surface,
        width.ceil() as i32,
        height.ceil() as i32,
        filter,
    );
    context.restore().ok();
}

/// Draw a surface cover-fitted (scaled to fill, center-cropped) into the
/// given rectangle.
fn paint_cover_fit(
    context: &Context,
    surface: &ImageSurface,
    width: i32,
    height: i32,
    filter: Filter,
) {
    let source_w = surface.width().max(1) as f64;
    let source_h = surface.height().max(1) as f64;
    let scale = (f64::from(width) / source_w).max(f64::from(height) / source_h);
    let drawn_w = source_w * scale;
    let drawn_h = source_h * scale;
    let _ = context.save();
    context.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
    context.clip();
    context.translate(
        (f64::from(width) - drawn_w) * 0.5,
        (f64::from(height) - drawn_h) * 0.5,
    );
    context.scale(scale, scale);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(filter);
    context.paint().ok();
    context.restore().ok();
}

fn paint_background_noise(context: &Context, width: i32, height: i32, amount: f64) {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.001 {
        return;
    }
    // Fixed pseudo-noise keeps every frame stable (and therefore exportable)
    // rather than shimmering as the Motion playhead advances.
    let step = 4;
    for y in (0..height.max(0)).step_by(step) {
        for x in (0..width.max(0)).step_by(step) {
            let hash =
                ((x as u32).wrapping_mul(73_856_093)) ^ ((y as u32).wrapping_mul(19_349_663));
            let light = if hash & 1 == 0 { 1.0 } else { 0.0 };
            context.set_source_rgba(light, light, light, amount * 0.045);
            context.rectangle(f64::from(x), f64::from(y), step as f64, step as f64);
            context.fill().ok();
        }
    }
}

fn draw_transformed_card(
    context: &Context,
    surface: &ImageSurface,
    stage: MotionStage,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
    appearance: &MotionAppearance,
    alpha: f64,
    mesh_div: usize,
) {
    // The radius rounds the captured image's own corners; the background
    // scene behind it stays a full rectangle.
    let rounded = rounded_motion_surface(surface, appearance.border_radius);
    let surface = rounded.as_ref().unwrap_or(surface);
    let img_w = surface.width() as f64;
    let img_h = surface.height() as f64;
    if img_w < 1.0 || img_h < 1.0 {
        return;
    }
    let pad = appearance
        .background_padding
        .clamp(0.0, (stage.bounds_w.min(stage.bounds_h) - 2.0).max(0.0));
    let fit = ((stage.bounds_w - pad) / img_w)
        .min((stage.bounds_h - pad) / img_h)
        .clamp(0.05, 1.0);
    let (cx, cy) = motion_card_center(img_w, img_h, fit, stage, transform, zoom_anchor);
    let corners = project_card_corners(img_w, img_h, fit, transform, cx, cy);
    if alpha >= 0.99 {
        paint_card_shadow(context, stage, corners, appearance, transform.perspective);
    }

    paint_perspective_card(
        context, surface, img_w, img_h, fit, transform, cx, cy, alpha, mesh_div,
    );
    if alpha >= 0.99 && appearance.border_thickness > 0.0 {
        let [r, g, b, a] = appearance.border_fill_color;
        context.set_source_rgba(r, g, b, a);
        context.set_line_width(appearance.border_thickness.max(0.0));
        context.move_to(corners[0].0, corners[0].1);
        for corner in &corners[1..] {
            context.line_to(corner.0, corner.1);
        }
        context.close_path();
        context.stroke().ok();
    }
}

/// Render the card into a scratch surface clipped to a rounded rectangle so
/// the captured image's corners appear rounded wherever the card is drawn.
fn rounded_motion_surface(surface: &ImageSurface, radius: f64) -> Option<ImageSurface> {
    let radius = radius.max(0.0);
    if radius < 0.5 {
        return None;
    }
    let width = surface.width().max(1);
    let height = surface.height().max(1);
    let rounded = ImageSurface::create(Format::ARgb32, width, height).ok()?;
    let context = Context::new(&rounded).ok()?;
    rounded_rectangle(
        &context,
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
        radius.min(f64::from(width.min(height)) * 0.5),
    );
    context.clip();
    context.set_source_surface(surface, 0.0, 0.0).ok()?;
    context.paint().ok()?;
    Some(rounded)
}

fn rounded_rectangle(context: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width.min(height) * 0.5).max(0.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2 * 3.0,
    );
    context.close_path();
}

fn paint_card_shadow(
    context: &Context,
    stage: MotionStage,
    corners: [(f64, f64); 4],
    appearance: &MotionAppearance,
    perspective: f64,
) {
    let blur = appearance.shadow_blur.max(0.0);
    let opacity = appearance.shadow_opacity.clamp(0.0, 1.0);
    if opacity <= 0.001 {
        return;
    }

    let base_x = appearance.shadow_position.0;
    let base_y = appearance.shadow_position.1 + perspective * 10.0;

    let scene_x = stage.center_x - stage.bounds_w * 0.5;
    let scene_y = stage.center_y - stage.bounds_h * 0.5;
    let _ = context.save();
    context.rectangle(scene_x, scene_y, stage.bounds_w, stage.bounds_h);
    context.clip();

    if blur < 0.5 {
        context.set_source_rgba(0.0, 0.0, 0.0, opacity);
        context.move_to(corners[0].0 + base_x, corners[0].1 + base_y);
        for corner in &corners[1..] {
            context.line_to(corner.0 + base_x, corner.1 + base_y);
        }
        context.close_path();
        context.fill().ok();
        context.restore().ok();
        return;
    }

    // Rasterize one shadow silhouette, then blur its alpha mask. The previous
    // implementation painted concentric polygon copies, leaving staircase
    // edges and making blur look like a larger solid shadow.
    let render_scale = (720.0 / stage.bounds_w.max(stage.bounds_h)).min(1.0);
    let mask_w = (stage.bounds_w * render_scale).ceil().max(1.0) as i32;
    let mask_h = (stage.bounds_h * render_scale).ceil().max(1.0) as i32;
    let blurred_surface = (|| {
        let mut mask = ImageSurface::create(Format::ARgb32, mask_w, mask_h).ok()?;
        {
            let mask_context = Context::new(&mask).ok()?;
            mask_context.set_source_rgba(0.0, 0.0, 0.0, opacity);
            mask_context.move_to(
                (corners[0].0 + base_x - scene_x) * render_scale,
                (corners[0].1 + base_y - scene_y) * render_scale,
            );
            for corner in &corners[1..] {
                mask_context.line_to(
                    (corner.0 + base_x - scene_x) * render_scale,
                    (corner.1 + base_y - scene_y) * render_scale,
                );
            }
            mask_context.close_path();
            mask_context.fill().ok()?;
        }
        mask.flush();
        let stride = mask.stride() as usize;
        let mut rgba = {
            let data = mask.data().ok()?;
            crate::capture::editor::render::cairo_argb_to_rgba_image(
                mask_w as u32,
                mask_h as u32,
                stride,
                data.as_ref(),
            )
        };
        // CSS-style blur radii are approximately twice Gaussian sigma. A
        // slightly tighter factor keeps the shadow soft without inflating it.
        let sigma = (blur * render_scale * 0.4).max(0.1);
        let mask_rect = crate::capture::editor::types::Rect {
            x: 0,
            y: 0,
            width: mask_w,
            height: mask_h,
        };
        // Three separable box passes closely approximate a Gaussian while
        // keeping animated preview work linear in the mask dimensions.
        for _ in 0..3 {
            crate::capture::editor::render::apply_blur_rect(&mut rgba, mask_rect, sigma, true);
        }
        crate::capture::editor::render::rgba_image_to_surface(&rgba)
    })();

    if let Some(surface) = blurred_surface {
        context.translate(scene_x, scene_y);
        context.scale(
            stage.bounds_w / f64::from(mask_w),
            stage.bounds_h / f64::from(mask_h),
        );
        context.set_source_surface(&surface, 0.0, 0.0).ok();
        context.source().set_filter(Filter::Bilinear);
        context.paint().ok();
    }
    context.restore().ok();
}

/// Keep the selected source point stationary while a camera scales in. The
/// transform's ordinary X/Y position is applied first; the anchor offset then
/// compensates only for zoom. Preview, export, title painting, and inverse
/// title placement all use this same center calculation.
fn motion_card_center(
    img_w: f64,
    img_h: f64,
    fit: f64,
    stage: MotionStage,
    transform: MotionTransform,
    zoom_anchor: (f64, f64),
) -> (f64, f64) {
    let cx = stage.center_x + transform.pos_x * stage.bounds_w * 0.12;
    let cy = stage.center_y + transform.pos_y * stage.bounds_h * 0.12;
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
    let background_surface = match motion.appearance.background_fill_type {
        MotionBackgroundFillType::Wallpaper => motion
            .appearance
            .wallpaper_image_name
            .as_deref()
            .and_then(load_motion_background_surface),
        MotionBackgroundFillType::Image => motion
            .appearance
            .custom_background_image
            .as_deref()
            .and_then(load_motion_background_surface),
        _ => None,
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
                background_surface.as_ref(),
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
        draw_motion_frame, motion_pose_differs, motion_text_contains_view_point, paint_card_shadow,
        paint_image_background, view_point_to_motion_text_position, CardLayout, MotionStage,
    };
    use crate::recording::editor::model::{
        project_card_corners, MotionBackgroundFillType, MotionState, MotionTransform,
        DEFAULT_MOTION_ZOOM,
    };
    use gtk4::cairo::{Context, Format, ImageSurface};

    /// Shotbase starts Motion with an empty track; tests add their own clip.
    fn motion_with_first_clip() -> MotionState {
        let mut motion = MotionState::default();
        motion
            .add_segment_at(0.0)
            .expect("a fresh track accepts a first move");
        motion
    }

    /// Export-style layout: the full frame with Shotbase's default padding.
    fn frame_layout(
        surface: &ImageSurface,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> CardLayout {
        CardLayout::with_padding(
            surface,
            MotionStage::frame(1440.0, 900.0),
            transform,
            zoom_anchor,
            96.0,
        )
    }

    fn render_appearance_frame(
        card: &ImageSurface,
        motion: &MotionState,
        preview: bool,
    ) -> ImageSurface {
        let frame = ImageSurface::create(Format::ARgb32, 128, 96).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_motion_frame(
                &context, 128, 96, card, motion, None, 0.0, preview, true, false,
            );
        }
        frame.flush();
        frame
    }

    #[test]
    fn scene_fill_is_bounded_in_preview_and_full_frame_on_export() {
        let card = ImageSurface::create(Format::ARgb32, 8, 8).unwrap();
        let mut motion = MotionState::default();
        let center = 48 * 128 * 4 + 64 * 4;

        // The preview keeps the checkerboard canvas until a fill is chosen.
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        assert_ne!(&data[..4], &[0, 0, 0, 255]);

        // A chosen fill paints inside the bounded scene panel: the panel
        // center takes the color while the canvas corner stays checkerboard.
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [0.2, 0.4, 0.6, 1.0];
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        // Cairo ARgb32 is BGRA on the Linux targets we support.
        assert_eq!(&data[center..center + 4], &[153, 102, 51, 255]);
        assert_ne!(&data[..4], &[153, 102, 51, 255]);

        // Exports have no editor canvas: the fill covers the whole frame and
        // an unset fill is Shotbase's black scene. The radius belongs to the
        // card, so the background corners stay filled regardless of it.
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.border_radius = 40.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[..4], &[0, 0, 0, 255]);
        assert_eq!(&data[center..center + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn border_radius_rounds_the_captured_card_not_the_background() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().ok();
        }
        card.flush();
        let mut motion = MotionState::default();
        motion.appearance.background_padding = 0.0;
        // Black export scene behind an opaque white card: the card fills the
        // middle of the frame, so its corner pixels are directly observable.
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.background_color = [0.0, 0.0, 0.0, 1.0];
        let card_center = 48 * 128 * 4 + 64 * 4;
        let card_corner = 17 * 128 * 4 + 33 * 4;

        // Square card: the image reaches into its own corners.
        motion.appearance.border_radius = 0.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[card_corner..card_corner + 4], &[255, 255, 255, 255]);

        // A radius of half the card side rounds the corners away entirely:
        // the image corner is cut and the background shows through, while the
        // card center stays image.
        motion.appearance.border_radius = 32.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[card_corner..card_corner + 4], &[0, 0, 0, 255]);
        assert_eq!(&data[card_center..card_center + 4], &[255, 255, 255, 255]);
    }

    #[test]
    fn card_shadow_has_a_smooth_falloff_and_stays_inside_the_scene() {
        let mut frame = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        let stage = MotionStage {
            bounds_w: 120.0,
            bounds_h: 80.0,
            center_x: 80.0,
            center_y: 60.0,
        };
        let corners = [(50.0, 40.0), (110.0, 40.0), (110.0, 80.0), (50.0, 80.0)];
        let mut appearance = MotionState::default().appearance;
        appearance.shadow_opacity = 1.0;
        appearance.shadow_blur = 20.0;
        appearance.shadow_position = (0.0, 0.0);
        {
            let context = Context::new(&frame).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            paint_card_shadow(&context, stage, corners, &appearance, 0.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize, y: usize| data[(y * 160 + x) * 4];

        assert_eq!(channel(10, 60), 255, "shadow escaped the scene bounds");
        assert!(channel(80, 60) < channel(46, 60));
        assert!(channel(46, 60) < channel(30, 60));

        let mut falloff = (25..50).map(|x| channel(x, 60)).collect::<Vec<_>>();
        falloff.sort_unstable();
        falloff.dedup();
        assert!(falloff.len() > 12, "shadow edge is visibly stepped");
    }

    #[test]
    fn zero_blur_keeps_a_hard_offset_shadow() {
        let mut frame = ImageSurface::create(Format::ARgb32, 100, 80).unwrap();
        let stage = MotionStage::frame(100.0, 80.0);
        let corners = [(30.0, 20.0), (70.0, 20.0), (70.0, 60.0), (30.0, 60.0)];
        let mut appearance = MotionState::default().appearance;
        appearance.shadow_opacity = 1.0;
        appearance.shadow_blur = 0.0;
        appearance.shadow_position = (8.0, 8.0);
        {
            let context = Context::new(&frame).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            paint_card_shadow(&context, stage, corners, &appearance, 0.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize, y: usize| data[(y * 100 + x) * 4];
        assert_eq!(channel(75, 40), 0);
        assert_eq!(channel(25, 40), 255);
    }

    #[test]
    fn background_blur_softens_image_edges() {
        let source = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&source).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.rectangle(0.0, 0.0, 32.0, 64.0);
            context.fill().unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.rectangle(32.0, 0.0, 32.0, 64.0);
            context.fill().unwrap();
        }
        source.flush();

        let mut frame = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            paint_image_background(&context, &source, 0.0, 0.0, 64.0, 64.0, 1.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize| data[(32 * 64 + x) * 4];
        assert!(channel(28) > 0);
        assert!(channel(28) < channel(36));
        assert!(channel(36) < 255);
    }

    #[test]
    fn new_segment_starts_identity_and_holds_shotbase_default_zoom() {
        let motion = motion_with_first_clip();
        let start = motion.sample(0.0);
        let end = motion.sample(motion.duration);
        assert!((start.scale - 1.0).abs() < 1e-6);
        assert!(start.rotation_y.abs() < 1e-6);
        assert!((end.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        assert!(end.rotation_y.abs() < 1e-6);
        assert_eq!(motion.segments.len(), 1);
        assert_eq!(motion.selected, Some(0));
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
    fn stretching_duration_holds_end_pose_after_the_move() {
        let mut motion = motion_with_first_clip();
        let move_end = motion.segments[0].end;
        motion.set_duration(6.0);
        assert!((motion.segments[0].end - move_end).abs() < 1e-6);
        let end = motion.sample(6.0);
        assert!((end.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn effect_segments_inherit_the_camera_pose_before_them() {
        let mut motion = motion_with_first_clip();
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
        let mut motion = motion_with_first_clip();
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
        let layout = frame_layout(&surface, transform, zoom_anchor);
        let expected = (0.27, 0.71);
        let point = layout.project(expected.0 * layout.img_w, expected.1 * layout.img_h);
        let actual = view_point_to_motion_text_position(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
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
        let layout = frame_layout(&surface, transform, zoom_anchor);
        let title_center =
            layout.project(segment.pos_x * layout.img_w, segment.pos_y * layout.img_h);

        assert!(motion_text_contains_view_point(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            title_center.0,
            title_center.1,
        ));
        assert!(!motion_text_contains_view_point(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            4.0,
            4.0,
        ));
    }

    #[test]
    fn shotbase_transform_timing_defaults_drive_glide_motion() {
        let mut motion = motion_with_first_clip();
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
        // linear progress at t=0.6 of a 1.2s transition is 0.5: scale 1 + (2-1)*0.5
        assert!((linear_progress - 1.5).abs() < 0.002);
    }

    #[test]
    fn transition_ms_slider_drives_the_global_timing() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_transition_ms(300);
        assert!((motion.transform_timing.transition_duration - 0.3).abs() < 1e-9);
        // Inside the shortened window the move has already finished.
        let held = motion.sample(0.35);
        assert!((held.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        // A zero-duration transition jumps straight to the target pose.
        motion.set_selected_transition_ms(0);
        assert!((motion.sample(0.0).scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn blur_and_position_setters_stick() {
        let mut motion = motion_with_first_clip();
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
        let mut motion = motion_with_first_clip();
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
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.42);
        motion.add_segment_at(3.0).expect("second effect clip");

        assert_eq!(motion.segments.len(), 2);
        assert!((motion.sample(0.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(3.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(motion.duration).perspective - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn recovered_segment_intensity_blends_the_camera_target() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_intensity(0.0);

        assert_eq!(motion.sample(motion.duration), MotionTransform::default());

        motion.set_selected_intensity(0.5);
        let half = motion.sample(motion.duration);
        assert!((half.scale - 1.5).abs() < 1e-6);
        assert!(half.rotation_y.abs() < 1e-6);
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
        let before = frame_layout(&surface, unzoomed, (0.5, 0.5));
        let after = frame_layout(&surface, transform, zoom_anchor);
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
        let mut motion = motion_with_first_clip();
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
        let mut motion = motion_with_first_clip();
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
