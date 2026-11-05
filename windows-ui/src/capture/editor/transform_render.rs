//! Frozen camera projection, matching the shared Motion card geometry.

use gtk4::cairo::{Context, Filter, ImageSurface, Matrix, Operator};

use super::image_transform::StaticImageTransform;
use super::state::background::Layout;

#[derive(Clone, Copy)]
pub(crate) enum Quality {
    /// A frame painted while the head or pointer moves: coarse enough that a
    /// mesh card stays cheap, and the parked frame is painted again.
    Live,
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

/// Grid for the projective card mesh, per axis.
///
/// The projector is a homography: it sends the card's straight content lines
/// to straight lines. The mesh cannot say that — each cell is painted with one
/// affine transform — so a cell of `cw` by `ch` bends its content by about
/// `(|T_xx|·cw² + 2·|T_xy|·cw·ch + |T_yy|·ch²) / 8`. Sizing the grid from the
/// projector's own second derivatives is what keeps that bend under
/// `tolerance`. The cross term matters as much as the axis terms: a yawed card
/// curves along x, but its vertical scale also changes with x, so a fine
/// column grid with one tall row still ripples.
fn mesh_divisions(
    layout: Layout,
    image_width: f64,
    image_height: f64,
    pose: StaticImageTransform,
    tolerance: f64,
) -> (usize, usize) {
    let (along_x, along_y, mixed) = curvature(layout, image_width, image_height, pose);
    let budget = tolerance.max(1e-6) * 0.5;
    let columns = cell_count(along_x, image_width, budget).clamp(1, 48);
    let rows = cell_count(along_y, image_height, budget).clamp(1, 48);
    let cw = image_width / columns as f64;
    let ch = image_height / rows as f64;
    if mixed <= 1e-12 {
        return (columns, rows);
    }
    let own = (along_x * cw * cw + along_y * ch * ch) / 8.0;
    let remaining = (tolerance - own).max(tolerance * 0.25);
    let widest = (4.0 * remaining / (mixed * cw)).clamp(1.0, ch);
    (
        columns,
        ((image_height / widest).ceil() as usize).clamp(rows, 48),
    )
}

/// Largest second derivative of the projection in each direction, and the
/// cross term, sampled across the card.
fn curvature(
    layout: Layout,
    image_width: f64,
    image_height: f64,
    pose: StaticImageTransform,
) -> (f64, f64, f64) {
    let project = projector(layout, image_width, image_height, pose);
    let hw = image_width / 2.0;
    let hh = image_height / 2.0;
    let step = 1.0;
    let mut along_x: f64 = 0.0;
    let mut along_y: f64 = 0.0;
    let mut mixed: f64 = 0.0;
    for column in 0..=4 {
        for row in 0..=4 {
            let x = -hw + image_width * f64::from(column) / 4.0;
            let y = -hh + image_height * f64::from(row) / 4.0;
            let here = project(x, y);
            along_x = along_x.max(
                second_difference(project(x - step, y), here, project(x + step, y)) / (step * step),
            );
            along_y = along_y.max(
                second_difference(project(x, y - step), here, project(x, y + step)) / (step * step),
            );
            let (plus_plus, plus_minus) =
                (project(x + step, y + step), project(x + step, y - step));
            let (minus_plus, minus_minus) =
                (project(x - step, y + step), project(x - step, y - step));
            mixed = mixed.max(
                (plus_plus.0 - plus_minus.0 - minus_plus.0 + minus_minus.0)
                    .hypot(plus_plus.1 - plus_minus.1 - minus_plus.1 + minus_minus.1)
                    / (4.0 * step * step),
            );
        }
    }
    (along_x, along_y, mixed)
}

fn second_difference(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    ((a.0 - 2.0 * b.0 + c.0).powi(2) + (a.1 - 2.0 * b.1 + c.1).powi(2)).sqrt()
}

fn cell_count(curvature: f64, extent: f64, budget: f64) -> usize {
    if !curvature.is_finite() || curvature <= 1e-9 {
        return 1;
    }
    let widest = (8.0 * budget / curvature).sqrt();
    ((extent / widest).ceil() as usize).max(1)
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
    let (tolerance, filter) = match quality {
        Quality::Live => (1.0, Filter::Bilinear),
        Quality::Preview => (0.35, Filter::Bilinear),
        Quality::Export => (0.15, Filter::Good),
    };
    let project = projector(layout, image_width, image_height, pose);
    let mesh_pose = pose;
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

    let (columns, rows) = mesh_divisions(layout, image_width, image_height, mesh_pose, tolerance);
    let stride = columns + 1;
    let mut grid = vec![(0.0, 0.0); stride * (rows + 1)];
    for j in 0..=rows {
        for i in 0..=columns {
            grid[j * stride + i] = project(
                (i as f64 / columns as f64 * 2.0 - 1.0) * hw,
                (j as f64 / rows as f64 * 2.0 - 1.0) * hh,
            );
        }
    }
    let corners = [
        grid[0],
        grid[columns],
        grid[rows * stride + columns],
        grid[rows * stride],
    ];
    context.push_group();
    context.set_operator(Operator::Source);
    for j in 0..rows {
        for i in 0..columns {
            let x0 = i as f64 * src_w / columns as f64;
            let x1 = (i + 1) as f64 * src_w / columns as f64;
            let y0 = j as f64 * src_h / rows as f64;
            let y1 = (j + 1) as f64 * src_h / rows as f64;
            let (a, b, c, d) = (
                grid[j * stride + i],
                grid[j * stride + i + 1],
                grid[(j + 1) * stride + i],
                grid[(j + 1) * stride + i + 1],
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
    let [(u1, v1), (u2, v2), (u3, v3)] = dst;
    let Some(matrix) = triangle_transform(src, dst) else {
        return;
    };
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
    context.transform(matrix);
    context.set_source_surface(surface, 0.0, 0.0).ok();
    context.source().set_filter(filter);
    context.paint().ok();
    context.restore().ok();
}

pub(crate) fn triangle_transform(src: [(f64, f64); 3], dst: [(f64, f64); 3]) -> Option<Matrix> {
    let [(x1, y1), (x2, y2), (x3, y3)] = src;
    let [(u1, v1), (u2, v2), (u3, v3)] = dst;
    let det = x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2);
    if det.abs() < 1e-8 {
        return None;
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
    Some(Matrix::new(xx, yx, xy, yy, x0, y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::background::Appearance;

    /// Every affine cell has to track the homography it stands in for, or a
    /// yawed card's straight content lines come out bowed and kinked — the
    /// ripple the fixed square grid produced at 24 degrees.
    #[test]
    fn projective_mesh_tracks_the_homography_within_its_tolerance() {
        let (width, height) = (1200.0, 800.0);
        let layout = Appearance::default().layout(1200, 800);
        let tolerance = 0.35;
        for (yaw, pitch, perspective) in [
            (24.0, 0.0, 1.0),
            (24.0, 0.0, 0.18),
            (24.0, 12.0, 1.0),
            (-24.0, -12.0, 1.0),
            (0.0, 0.0, 0.0),
        ] {
            let pose = StaticImageTransform {
                yaw,
                pitch,
                perspective,
                ..Default::default()
            };
            let (columns, rows) = mesh_divisions(layout, width, height, pose, tolerance);
            assert!(columns >= 1 && rows >= 1, "yaw={yaw}");
            let project = projector(layout, width, height, pose);
            let cw = width / columns as f64;
            let ch = height / rows as f64;
            let mut worst: f64 = 0.0;
            for j in 0..rows {
                for i in 0..columns {
                    let x0 = -width / 2.0 + i as f64 * cw;
                    let y0 = -height / 2.0 + j as f64 * ch;
                    let (x1, y1) = (x0 + cw, y0 + ch);
                    let (a, b, c, d) = (
                        project(x0, y0),
                        project(x1, y0),
                        project(x0, y1),
                        project(x1, y1),
                    );
                    let first = triangle_transform([(x0, y0), (x1, y0), (x0, y1)], [a, b, c]);
                    let second = triangle_transform([(x1, y0), (x1, y1), (x0, y1)], [b, d, c]);
                    for column in 1..4 {
                        for row in 1..4 {
                            let u = f64::from(column) / 4.0;
                            let v = f64::from(row) / 4.0;
                            let Some(matrix) = (if u + v <= 1.0 { first } else { second }) else {
                                continue;
                            };
                            let painted = matrix.transform_point(x0 + u * cw, y0 + v * ch);
                            let true_place = project(x0 + u * cw, y0 + v * ch);
                            worst = worst
                                .max((painted.0 - true_place.0).hypot(painted.1 - true_place.1));
                        }
                    }
                }
            }
            // The division estimate is a second-order bound, so it lands a
            // little above the requested tolerance rather than under it.
            assert!(
                worst <= tolerance * 1.5,
                "yaw={yaw} pitch={pitch} perspective={perspective} bends by {worst}px"
            );
        }
    }
}
