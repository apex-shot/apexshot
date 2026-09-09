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
