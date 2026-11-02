//! Frozen camera projection, matching the shared Motion card geometry.

use gtk4::cairo::{Context, Filter, ImageSurface, Matrix, Operator};

use super::image_transform::StaticImageTransform;
use super::state::background::Layout;

#[derive(Clone, Copy)]
pub(crate) enum Quality {
    Preview,
    Export,
}

pub(crate) fn projector(
    layout: Layout,
    image_width: f64,
    image_height: f64,
    pose: StaticImageTransform,
) -> impl Fn(f64, f64) -> (f64, f64) {
    let pose = pose.sanitized().motion_transform();
    let hw = image_width / 2.0;
    let hh = image_height / 2.0;
    let cx = layout.image_x + hw - pose.pos_x * layout.width / 2.0;
    let cy = layout.image_y + hh - pose.pos_y * layout.height / 2.0;
    let depth = hw.hypot(hh).max(1.0) * (2.44 - pose.perspective * 1.30).max(1.15);
    let flat = pose.rotation_x.abs() + pose.rotation_y.abs() + pose.rotation_z.abs() < 0.05;
    let (sy, cyaw) = pose.rotation_y.to_radians().sin_cos();
    let (sp, cp) = pose.rotation_x.to_radians().sin_cos();
    let (sr, cr) = pose.rotation_z.to_radians().sin_cos();
    move |x, y| {
        if flat {
            return (cx + x, cy + y);
        }
        let x1 = x * cyaw;
        let z1 = -x * sy;
        let y1 = y * cp - z1 * sp;
        let z2 = y * sp + z1 * cp;
        let factor = 1.0 / (1.0 + z2 / depth.max(1.0)).clamp(0.35, 2.5);
        (
            cx + (x1 * cr - y1 * sr) * factor,
            cy + (x1 * sr + y1 * cr) * factor,
        )
    }
}

pub(crate) fn card_corners(
    layout: Layout,
    image_width: f64,
    image_height: f64,
    pose: StaticImageTransform,
) -> [(f64, f64); 4] {
    let project = projector(layout, image_width, image_height, pose);
    let (hw, hh) = (image_width / 2.0, image_height / 2.0);
    [
        project(-hw, -hh),
        project(hw, -hh),
        project(hw, hh),
        project(-hw, hh),
    ]
}

/// Paint a precomposited card in composition coordinates.
pub(crate) fn paint_card(
    context: &Context,
    surface: &ImageSurface,
    layout: Layout,
    image_width: f64,
    image_height: f64,
    pose: StaticImageTransform,
    quality: Quality,
) {
    let (div, filter) = match quality {
        Quality::Preview => (8, Filter::Bilinear),
        Quality::Export => (16, Filter::Good),
    };
    let project = projector(layout, image_width, image_height, pose);
    let pose = pose.sanitized().motion_transform();
    let hw = image_width / 2.0;
    let hh = image_height / 2.0;
    let cx = layout.image_x + hw - pose.pos_x * layout.width / 2.0;
    let cy = layout.image_y + hh - pose.pos_y * layout.height / 2.0;
    let src_w = f64::from(surface.width());
    let src_h = f64::from(surface.height());
    let _ = context.save();
    context.rectangle(0.0, 0.0, layout.width, layout.height);
    context.clip();
    if pose.rotation_x.abs() + pose.rotation_y.abs() + pose.rotation_z.abs() < 0.05 {
        context.translate(cx - hw, cy - hh);
        context.scale(image_width / src_w, image_height / src_h);
        context.set_source_surface(surface, 0.0, 0.0).ok();
        context.source().set_filter(filter);
        context.paint().ok();
        context.restore().ok();
        return;
    }

    let columns = div + 1;
    let mut grid = vec![(0.0, 0.0); columns * columns];
    for j in 0..=div {
        for i in 0..=div {
            grid[j * columns + i] = project(
                (i as f64 / div as f64 * 2.0 - 1.0) * hw,
                (j as f64 / div as f64 * 2.0 - 1.0) * hh,
            );
        }
    }
    let corners = [
        grid[0],
        grid[div],
        grid[div * columns + div],
        grid[div * columns],
    ];
    context.push_group();
    context.set_operator(Operator::Source);
    for j in 0..div {
        for i in 0..div {
            let x0 = i as f64 * src_w / div as f64;
            let x1 = (i + 1) as f64 * src_w / div as f64;
            let y0 = j as f64 * src_h / div as f64;
            let y1 = (j + 1) as f64 * src_h / div as f64;
            let (a, b, c, d) = (
                grid[j * columns + i],
                grid[j * columns + i + 1],
                grid[(j + 1) * columns + i],
                grid[(j + 1) * columns + i + 1],
            );
            paint_triangle(
                context,
                surface,
                [(x0, y0), (x1, y0), (x0, y1)],
                [a, b, c],
                filter,
            );
            paint_triangle(
                context,
                surface,
                [(x1, y0), (x1, y1), (x0, y1)],
                [b, d, c],
                filter,
            );
        }
    }
    let texture = context.pop_group();
    if let Ok(texture) = texture {
        context.set_operator(Operator::Over);
        context.move_to(corners[0].0, corners[0].1);
        for corner in &corners[1..] {
            context.line_to(corner.0, corner.1);
        }
        context.close_path();
        context.clip();
        context.set_source(&texture).ok();
        context.paint().ok();
    }
    context.restore().ok();
}

fn paint_triangle(
    context: &Context,
    surface: &ImageSurface,
    src: [(f64, f64); 3],
    dst: [(f64, f64); 3],
    filter: Filter,
) {
    let [(x1, y1), (x2, y2), (x3, y3)] = src;
    let [(u1, v1), (u2, v2), (u3, v3)] = dst;
    let det = x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2);
    if det.abs() < 1e-8 {
        return;
    }
    let coefficients = |a: f64, b: f64, c: f64| {
        (
            (a * (y2 - y3) + b * (y3 - y1) + c * (y1 - y2)) / det,
            (a * (x3 - x2) + b * (x1 - x3) + c * (x2 - x1)) / det,
            (a * (x2 * y3 - x3 * y2) + b * (x3 * y1 - x1 * y3) + c * (x1 * y2 - x2 * y1)) / det,
        )
    };
    let (xx, xy, x0) = coefficients(u1, u2, u3);
    let (yx, yy, y0) = coefficients(v1, v2, v3);
    let center = ((u1 + u2 + u3) / 3.0, (v1 + v2 + v3) / 3.0);
    let expanded = dst.map(|(x, y)| {
        let (dx, dy) = (x - center.0, y - center.1);
        let length = dx.hypot(dy).max(1e-6);
        (x + dx / length * 0.6, y + dy / length * 0.6)
    });
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::None);
    context.move_to(expanded[0].0, expanded[0].1);
    context.line_to(expanded[1].0, expanded[1].1);
    context.line_to(expanded[2].0, expanded[2].1);
    context.close_path();
    context.clip();
    context.transform(Matrix::new(xx, yx, xy, yy, x0, y0));
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(filter);
    context.paint().ok();
    context.restore().ok();
}
