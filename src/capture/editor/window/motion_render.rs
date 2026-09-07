//! Shared Motion preview/export drawing.

use gtk4::cairo::{Context, Format, ImageSurface, Matrix};
use image::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recording::editor::model::{
    affine_from_three_points as affine_components, card_depth, project_card_corners, project_point,
    MotionState, MotionTransform, MOTION_EXPORT_FPS,
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
    let live = live_preview;
    let mesh_div = if live { 3 } else { 8 };
    let blur = if live {
        0.0
    } else {
        motion.motion_blur.clamp(0.0, 1.0)
    };
    if blur > 0.02 {
        let samples = 3 + (blur * 6.0).round() as i32;
        let lookback = 0.03 + blur * 0.10;
        for i in (1..samples).rev() {
            let sample_t = (time - lookback * f64::from(i) / f64::from(samples - 1)).max(0.0);
            let alpha = 0.22 * (1.0 - f64::from(i) / f64::from(samples));
            draw_transformed_card(
                context,
                surface,
                width as f64,
                height as f64,
                motion.sample(sample_t),
                alpha,
                mesh_div,
            );
        }
    }
    draw_transformed_card(
        context,
        surface,
        width as f64,
        height as f64,
        motion.sample(time),
        1.0,
        mesh_div,
    );
    paint_motion_text(context, width as f64, height as f64, motion, time);
}

fn paint_motion_text(context: &Context, width: f64, height: f64, motion: &MotionState, time: f64) {
    context.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
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
        let size = (height * 0.048 * segment.size.clamp(0.5, 2.2)).clamp(14.0, 72.0);
        context.set_font_size(size);
        let Ok(ext) = context.text_extents(line) else {
            continue;
        };
        let cx = segment.pos_x.clamp(0.05, 0.95) * width;
        let cy = segment.pos_y.clamp(0.05, 0.95) * height + style.offset_y;
        let x = cx - ext.width() / 2.0 - ext.x_bearing();
        let y = cy - ext.height() / 2.0 - ext.y_bearing();
        context.set_source_rgba(0.0, 0.0, 0.0, 0.42 * style.alpha);
        context.move_to(x, y + 2.0);
        let _ = context.show_text(line);
        context.set_source_rgba(1.0, 1.0, 1.0, style.alpha);
        context.move_to(x, y);
        let _ = context.show_text(line);
    }
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
        .min(1.0)
        .max(0.05);
    let cx = viewport_w / 2.0 + transform.pos_x * viewport_w * 0.12;
    let cy = viewport_h / 2.0 + transform.pos_y * viewport_h * 0.12;
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
    use crate::recording::editor::model::{
        project_card_corners, MotionSegment, MotionState, MotionTransform,
        DEFAULT_MOTION_END_ROTATION_Y, DEFAULT_MOTION_END_SCALE,
    };

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
    fn blur_and_position_setters_stick() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        motion.set_motion_blur(0.4);
        motion.set_selected_end_pos_x(0.5);
        motion.set_selected_end_pos_y(-0.25);
        assert!((motion.motion_blur - 0.4).abs() < 1e-6);
        let end = motion.sample(motion.duration);
        assert!((end.pos_x - 0.5).abs() < 1e-6);
        assert!((end.pos_y + 0.25).abs() < 1e-6);
        let start = motion.sample(0.0);
        assert!(start.pos_x.abs() < 1e-6);
        assert!(start.pos_y.abs() < 1e-6);
    }

    #[test]
    fn add_text_fills_a_gap_and_fade_differs_from_slide() {
        let mut motion = MotionState::default();
        motion.seed_default_cinematic();
        let first = motion.add_text_at(0.05).expect("text clip");
        assert_eq!(motion.text_segments.len(), 1);
        assert_eq!(motion.selected_text, Some(first));
        assert!(motion.selected.is_none());
        assert!(motion.add_text_at(0.1).is_none());
        let fade = motion.text_segments[0].sample(0.12).expect("fade sample");
        motion.set_selected_text_animation(
            crate::recording::editor::model::MotionTextAnimation::Slide,
        );
        let slide = motion.text_segments[0].sample(0.12).expect("slide sample");
        assert!(slide.offset_y > fade.offset_y + 4.0);
        assert!(fade.alpha > 0.0 && fade.alpha < 1.0);
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
