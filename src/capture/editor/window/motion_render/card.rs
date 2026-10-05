fn draw_transformed_card(
    context: &Context,
    surface: &ImageSurface,
    layout: CardLayout,
    appearance: &MotionAppearance,
    alpha: f64,
    mesh_div: usize,
    filter: Filter,
) {
    // The radius rounds the captured image's own corners; the background
    // scene behind it stays a full rectangle. It is expressed in source-card
    // pixels, so a downscaled preview texture scales it to keep the corner
    // visually identical to the full-resolution export. Callers prepare the
    // rounded surface once per frame; the blur renderer draws it per subframe.
    let img_w = layout.img_w();
    let img_h = layout.img_h();
    if img_w < 1.0 || img_h < 1.0 {
        return;
    }
    let transform = layout.transform;
    let (cx, cy) = (layout.cx, layout.cy);
    let stage = layout.stage;
    let ref_scale = motion_reference_scale(img_w, img_h);
    let fit = layout.source_fit();
    let canvas_fit = layout.canvas_fit;
    // Stage-space corner radius of the rounded card image. Callers bake the
    // radius into the texture in source pixels and the mesh draws it at
    // `fit * scale`, so frame geometry must apply both factors to stay glued
    // to the card edge. Without `scale`, a zoomed card's corners outrun the
    // frame and a sliver of background shows between image and frame.
    let card_radius = card_corner_radius(appearance, ref_scale, fit, transform.scale);
    let corners = project_card_corners(img_w, img_h, fit, transform, cx, cy);
    if alpha >= 0.99 {
        paint_card_shadow(
            context,
            stage,
            corners,
            card_radius,
            appearance.shadow_opacity,
            appearance.shadow_blur,
            appearance.shadow_position,
        );
    }

    // Stack presets: flat backing sheets behind the card, offset up-left
    // like stacked prints. Farthest sheet first.
    {
        let spec = appearance.frame_style.spec();
        let backings: Vec<_> = [spec.backing1, spec.backing2]
            .into_iter()
            .flatten()
            .collect();
        if !backings.is_empty() {
            // Perspective alone never bends the card: with zero rotation the
            // projection is exactly 1:1 (z stays 0, w stays 1), so a
            // perspective-only pose must take the flat path to match Static.
            let flat = transform.rotation_x.abs()
                + transform.rotation_y.abs()
                + transform.rotation_z.abs()
                < 0.05;
            let hw = img_w * fit * transform.scale / 2.0;
            let hh = img_h * fit * transform.scale / 2.0;
            let radius = card_radius;
            for backing in backings {
                let c = backing.color;
                let a = (c.a * alpha).clamp(0.0, 1.0);
                context.set_source_rgba(c.r, c.g, c.b, a);
                if flat {
                    let _ = context.save();
                    if backing.center_pivot {
                        context.translate(cx + backing.offset_x * canvas_fit, cy + backing.offset_y * canvas_fit);
                        context.rotate(backing.rotation_deg.to_radians());
                        context.translate(-hw, -hh);
                    } else {
                        context.translate(
                            cx - hw + backing.offset_x * canvas_fit + hw * 2.0,
                            cy - hh + backing.offset_y * canvas_fit + hh * 2.0,
                        );
                        context.rotate(backing.rotation_deg.to_radians());
                        context.translate(-hw * 2.0, -hh * 2.0);
                    }
                    rounded_rectangle(context, 0.0, 0.0, hw * 2.0, hh * 2.0, radius);
                    context.fill().ok();
                    let _ = context.restore();
                } else {
                    // Center-pivot sheets fan about the quad centroid, the
                    // rest about the hidden bottom-right corner.
                    let (px, py) = if backing.center_pivot {
                        (
                            (corners[0].0 + corners[1].0 + corners[2].0 + corners[3].0) / 4.0,
                            (corners[0].1 + corners[1].1 + corners[2].1 + corners[3].1) / 4.0,
                        )
                    } else {
                        corners[2]
                    };
                    let theta = backing.rotation_deg.to_radians();
                    let (sin, cos) = theta.sin_cos();
                    let mut first = true;
                    for corner in &corners {
                        let rx = corner.0 - px;
                        let ry = corner.1 - py;
                        let qx = px + backing.offset_x * canvas_fit + rx * cos - ry * sin;
                        let qy = py + backing.offset_y * canvas_fit + rx * sin + ry * cos;
                        if first {
                            context.move_to(qx, qy);
                            first = false;
                        } else {
                            context.line_to(qx, qy);
                        }
                    }
                    context.close_path();
                    context.fill().ok();
                }
            }
        }
    }

    let hw = img_w * fit * transform.scale / 2.0;
    let hh = img_h * fit * transform.scale / 2.0;
    paint_perspective_card(
        context, surface, hw, hh, transform, cx, cy, alpha, mesh_div, filter,
    );
    if alpha >= 0.99 {
        let spec = appearance.frame_style.spec();
        let mut expand = 0.0;
        // Same flat rule as backings above: perspective without rotation is
        // still a 1:1 projection, so borders must use the rounded-rect path
        // to match the Static canvas instead of the projected quad.
        let flat =
            transform.rotation_x.abs() + transform.rotation_y.abs() + transform.rotation_z.abs()
                < 0.05;
        let hw_flat = img_w * fit * transform.scale / 2.0;
        let hh_flat = img_h * fit * transform.scale / 2.0;
        // Projection depth for the frame outlines below, from the unexpanded
        // card so the rim tracks the same surface as the image mesh.
        let quad_depth = card_depth(hw_flat, hh_flat, transform.perspective);
        // Liquid Glass paints gradients along the card outline instead of the
        // flat strokes below; the outline is a rounded rect when the card is
        // square to the camera and the projected quad when it is tilted.
        if let Some(liquid) = crate::capture::editor::render::LiquidFrame::resolve(
            &spec,
            appearance.border_thickness * ref_scale * fit,
            1.0,
        ) {
            let path = |path_context: &Context, expand: f64| {
                if flat {
                    crate::capture::editor::render::rounded_rect_path(
                        path_context,
                        cx - hw_flat - expand,
                        cy - hh_flat - expand,
                        hw_flat * 2.0 + expand * 2.0,
                        hh_flat * 2.0 + expand * 2.0,
                        if card_radius <= 0.0 {
                            0.0
                        } else {
                            card_radius + expand
                        },
                    );
                } else {
                    // Tilted: the rim follows the rounded card edge through
                    // the projection. Tracing the sharp quad here drops the
                    // radius for the whole clip (the image keeps it via
                    // texture alpha) and snaps back when playback stops.
                    let outline = projected_rounded_rect_points(
                        hw_flat + expand,
                        hh_flat + expand,
                        if card_radius <= 0.0 {
                            0.0
                        } else {
                            card_radius + expand
                        },
                        transform,
                        quad_depth,
                        cx,
                        cy,
                    );
                    path_through_points(path_context, &outline);
                }
            };
            let (top, bottom) = if flat {
                (cy - hh_flat, cy + hh_flat)
            } else {
                let mut top = f64::INFINITY;
                let mut bottom = f64::NEG_INFINITY;
                for corner in &corners {
                    top = top.min(corner.1);
                    bottom = bottom.max(corner.1);
                }
                (top, bottom)
            };
            liquid.paint(context, top, bottom, path);
        }
        let stroke_outside = |context: &Context, thickness: f64, extra: f64| {
            let e = extra + thickness / 2.0;
            context.set_line_width(thickness.max(0.0));
            if flat {
                // A zero radius stays a sharp mitered frame: the expanded
                // path must not inherit half the line width as rounding.
                let path_radius = if card_radius <= 0.0 {
                    0.0
                } else {
                    card_radius + e
                };
                rounded_rectangle(
                    context,
                    cx - hw_flat - e,
                    cy - hh_flat - e,
                    hw_flat * 2.0 + e * 2.0,
                    hh_flat * 2.0 + e * 2.0,
                    path_radius,
                );
                context.stroke().ok();
            } else {
                // Tilted: same rounded projection as the Liquid rim so the
                // border never goes sharp mid-clip.
                let outline = projected_rounded_rect_points(
                    hw_flat + e,
                    hh_flat + e,
                    if card_radius <= 0.0 {
                        0.0
                    } else {
                        card_radius + e
                    },
                    transform,
                    quad_depth,
                    cx,
                    cy,
                );
                path_through_points(context, &outline);
                context.stroke().ok();
            }
            extra + thickness
        };
        if !spec.liquid && appearance.border_thickness > 0.0 && !spec.inset_border {
            let [r, g, b, a] = appearance.border_fill_color;
            context.set_source_rgba(r, g, b, a);
            expand = stroke_outside(
                context,
                (appearance.border_thickness * ref_scale * fit).max(0.0),
                expand,
            );
        }
        if !spec.liquid && spec.inset_border && appearance.border_thickness > 0.0 {
            // Inside placement: band sits fully within the card edge.
            let thickness = (appearance.border_thickness * ref_scale * fit).max(0.0);
            let [r, g, b, a] = appearance.border_fill_color;
            context.set_source_rgba(r, g, b, a);
            context.set_line_width(thickness);
            let e = thickness / 2.0;
            if flat {
                rounded_rectangle(
                    context,
                    cx - hw_flat + e,
                    cy - hh_flat + e,
                    (hw_flat * 2.0 - e * 2.0).max(1.0),
                    (hh_flat * 2.0 - e * 2.0).max(1.0),
                    (card_radius - e).max(0.0),
                );
                context.stroke().ok();
            } else {
                // Tilted: shrink in card space, then project — a uniform
                // band that matches the flat path under weak perspective.
                let outline = projected_rounded_rect_points(
                    (hw_flat * 2.0 - e * 2.0).max(1.0) / 2.0,
                    (hh_flat * 2.0 - e * 2.0).max(1.0) / 2.0,
                    (card_radius - e).max(0.0),
                    transform,
                    quad_depth,
                    cx,
                    cy,
                );
                path_through_points(context, &outline);
                context.stroke().ok();
            }
        }
        // Liquid Glass already painted its specular rim from the preset.
        for outer in [spec.outer1, spec.outer2]
            .into_iter()
            .flatten()
            .filter(|_| !spec.liquid)
        {
            let main_matches =
                (outer.thickness - spec.border_thickness).abs() < f64::EPSILON && outer.gap == 0.0;
            if main_matches {
                continue;
            }
            expand += outer.gap.max(0.0) * ref_scale * fit;
            context.set_source_rgba(outer.color.r, outer.color.g, outer.color.b, outer.color.a);
            expand = stroke_outside(
                context,
                (outer.thickness * ref_scale * fit).max(0.0),
                expand,
            );
        }
    }
}

/// Stage-space corner radius of the rounded card image under a camera pose.
/// Mirrors the caller's texture rounding (`border_radius * long_edge / 400`
/// in source pixels) plus the mesh's `fit * scale` draw factor, so every
/// frame path hugs the same corner arc the texture alpha cuts.
fn card_corner_radius(appearance: &MotionAppearance, ref_scale: f64, fit: f64, scale: f64) -> f64 {
    (appearance.border_radius * ref_scale * fit * scale).max(0.0)
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

/// Paint the card's drop shadow: the offset silhouette blurred across the
/// stage. Shared by the Motion preview/export and the Static preview so the
/// Appearance Shadow controls (opacity, blur, position) render identically in
/// both. `opacity`, `blur` and `position` are passed through unchanged, so each
/// caller draws them in its own drawing space: Motion in stage pixels, the
/// static canvas in view pixels. The two previews therefore show the same
/// number of on-screen pixels of blur and offset at any window size.
/// `radius` is the card's own corner radius in that same space, so the
/// silhouette follows the rounded card instead of showing square corners.
pub(crate) fn paint_card_shadow(
    context: &Context,
    stage: MotionStage,
    corners: [(f64, f64); 4],
    radius: f64,
    opacity: f64,
    blur: f64,
    position: (f64, f64),
) {
    let blur = blur.max(0.0);
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.001 {
        return;
    }

    // No perspective lift: the corners are already projected, and any extra
    // offset here would shift the shadow away from the Static canvas position
    // even when the card is flat. Shadow matches Static; tilt shows through
    // the projected quad itself.
    let (base_x, base_y) = position;

    let scene_x = stage.center_x - stage.bounds_w * 0.5;
    let scene_y = stage.center_y - stage.bounds_h * 0.5;
    let _ = context.save();
    context.rectangle(scene_x, scene_y, stage.bounds_w, stage.bounds_h);
    context.clip();

    if blur < 0.5 {
        context.set_source_rgba(0.0, 0.0, 0.0, opacity);
        trace_rounded_quad(
            context,
            corners,
            radius,
            (base_x, base_y),
            1.0,
            (0.0, 0.0),
        );
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
            trace_rounded_quad(
                &mask_context,
                corners,
                radius,
                (base_x, base_y),
                render_scale,
                (scene_x, scene_y),
            );
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

/// Trace the card silhouette as a rounded quad so the shadow follows the
/// card's border radius (`rounded_rect_path` uses a superellipse; a cubic per
/// corner is visually identical once the shadow is blurred). Works for the
/// flat static rect and the projected Motion quad: each corner is cut back by
/// `radius` along its two edges and bridged with a curve through the vertex.
/// Points are offset, scaled into mask space, and shifted by the scene origin.
fn trace_rounded_quad(
    context: &Context,
    corners: [(f64, f64); 4],
    radius: f64,
    offset: (f64, f64),
    scale: f64,
    origin: (f64, f64),
) {
    let points: [(f64, f64); 4] = corners.map(|(x, y)| {
        (
            (x + offset.0 - origin.0) * scale,
            (y + offset.1 - origin.1) * scale,
        )
    });
    // Each corner consumes `radius` on both of its edges, so the shortest edge
    // bounds it: half an edge keeps neighbouring corner arcs from overlapping.
    let mut shortest = f64::INFINITY;
    for index in 0..4 {
        let a = points[index];
        let b = points[(index + 1) % 4];
        shortest = shortest.min((b.0 - a.0).hypot(b.1 - a.1));
    }
    let radius = (radius * scale).clamp(0.0, shortest / 2.0);
    if radius <= 0.5 {
        context.move_to(points[0].0, points[0].1);
        for point in &points[1..] {
            context.line_to(point.0, point.1);
        }
        context.close_path();
        return;
    }
    let direction = |a: (f64, f64), b: (f64, f64)| {
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let length = (dx * dx + dy * dy).sqrt().max(1e-6);
        (dx / length, dy / length)
    };
    let mut first = true;
    for index in 0..4 {
        let previous = points[(index + 3) % 4];
        let corner = points[index];
        let next = points[(index + 1) % 4];
        let incoming = direction(previous, corner);
        let outgoing = direction(corner, next);
        let start = (corner.0 - incoming.0 * radius, corner.1 - incoming.1 * radius);
        let end = (corner.0 + outgoing.0 * radius, corner.1 + outgoing.1 * radius);
        if first {
            context.move_to(start.0, start.1);
            first = false;
        } else {
            context.line_to(start.0, start.1);
        }
        // Cubic equivalent of a quadratic through the corner vertex.
        let control_1 = (
            start.0 + (corner.0 - start.0) * (2.0 / 3.0),
            start.1 + (corner.1 - start.1) * (2.0 / 3.0),
        );
        let control_2 = (
            end.0 + (corner.0 - end.0) * (2.0 / 3.0),
            end.1 + (corner.1 - end.1) * (2.0 / 3.0),
        );
        context.curve_to(control_1.0, control_1.1, control_2.0, control_2.1, end.0, end.1);
    }
    context.close_path();
}

fn paint_perspective_card(
    context: &Context,
    surface: &ImageSurface,
    hw: f64,
    hh: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
    alpha: f64,
    mesh_div: usize,
    filter: Filter,
) {
    let img_w = f64::from(surface.width().max(1));
    let img_h = f64::from(surface.height().max(1));
    let depth = card_depth(hw, hh, transform.perspective);
    // Perspective without rotation projects 1:1 (z == 0 so w == 1). Treat it
    // as flat: one rectangle blit instead of the triangle mesh, so a still
    // with no camera rotation stays pixel-sharp like the Static canvas.
    let bent = transform.rotation_x.abs() + transform.rotation_y.abs() + transform.rotation_z.abs();
    if bent < 0.05 {
        // Flat pose: the card is an axis-aligned rectangle. One rectangle
        // blit is several times cheaper than two clipped textured triangles —
        // Cairo takes a general masked-composite path for triangle clips — so
        // zero rotation avoids that cost entirely.
        let left = cx - hw;
        let top = cy - hh;
        let _ = context.save();
        context.rectangle(left, top, hw * 2.0, hh * 2.0);
        context.clip();
        context.translate(left, top);
        context.scale((hw * 2.0) / img_w, (hh * 2.0) / img_h);
        context.set_source_surface(surface, 0.0, 0.0).ok();
        context.source().set_filter(filter);
        if alpha < 0.999 {
            let _ = context.paint_with_alpha(alpha);
        } else {
            let _ = context.paint();
        }
        let _ = context.restore();
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
            paint_textured_triangle(
                context,
                surface,
                [s00, s10, s01],
                [d00, d10, d01],
                alpha,
                filter,
            );
            paint_textured_triangle(
                context,
                surface,
                [s10, s11, s01],
                [d10, d11, d01],
                alpha,
                filter,
            );
        }
    }
}

fn paint_textured_triangle(
    context: &Context,
    surface: &ImageSurface,
    src: [(f64, f64); 3],
    dest: [(f64, f64); 3],
    alpha: f64,
    filter: Filter,
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
    context.source().set_filter(filter);
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
