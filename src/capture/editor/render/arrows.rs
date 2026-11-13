use super::super::types::{ArrowStyle, DrawColor, Point};
use super::{
    selection_outline_stroke_width, MOVE_HANDLE_OUTLINE_WIDTH, MOVE_HANDLE_RADIUS,
    TEXT_EDIT_BORDER_COLOR,
};

/// Centerline geometry shared by drawing, selection chrome, and hit testing.
pub struct ArrowGeometry {
    /// Sampled centerline of the shaft, tail first.
    pub body: Vec<Point>,
    /// Each head as `[wing, tip, wing]`, stroked as an open chevron.
    pub heads: Vec<[Point; 3]>,
    /// Stroke width shared by the shaft and the heads.
    pub line_width: f64,
}

pub fn arrow_line_width(stroke_size: f64) -> f64 {
    stroke_size.max(0.5) * 1.5 + 1.0
}

fn full_head_length(line_width: f64) -> f64 {
    line_width * 2.6 + 6.0
}

/// Shortest arrow, end to end, that still reads as an arrow at this stroke
/// size: one and a half times its full head length.
pub fn arrow_min_length(stroke_size: f64) -> f64 {
    full_head_length(arrow_line_width(stroke_size)) * 1.5
}

fn bezier_point(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let u = 1.0 - t;
    Point {
        x: u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
        y: u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
    }
}

fn bezier_tangent(p0: Point, p1: Point, p2: Point, t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let dx = 2.0 * u * (p1.x - p0.x) + 2.0 * t * (p2.x - p1.x);
    let dy = 2.0 * u * (p1.y - p0.y) + 2.0 * t * (p2.y - p1.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.0001 {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

fn curved_outline_steps(curve_length: f64) -> usize {
    // The shaft is stroked along a sampled Bezier centerline. Keep its
    // segments short enough that the curve does not look faceted.
    ((curve_length / 3.0).ceil() as usize).clamp(48, 256)
}

fn chevron(tip: Point, direction: (f64, f64), length: f64, spread: f64) -> [Point; 3] {
    let angle = direction.1.atan2(direction.0);
    let wing = |a: f64| Point {
        x: tip.x - length * a.cos(),
        y: tip.y - length * a.sin(),
    };
    [wing(angle - spread), tip, wing(angle + spread)]
}

pub fn arrow_geometry(
    start: Point,
    end: Point,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: &Option<Vec<Point>>,
) -> Option<ArrowGeometry> {
    let control = control_points
        .as_ref()
        .filter(|points| points.len() >= 3)
        .map(|points| points[1]);
    let (body, line_length) = match control {
        Some(p1) => {
            let mut length = 0.0;
            let mut prev = start;
            for i in 1..=20 {
                let pt = bezier_point(start, p1, end, i as f64 / 20.0);
                length += (pt.x - prev.x).hypot(pt.y - prev.y);
                prev = pt;
            }
            let steps = curved_outline_steps(length);
            let body = (0..=steps)
                .map(|i| bezier_point(start, p1, end, i as f64 / steps as f64))
                .collect();
            (body, length)
        }
        None => (vec![start, end], (end.x - start.x).hypot(end.y - start.y)),
    };
    if line_length < 0.1 {
        return None;
    }

    let line_width = arrow_line_width(stroke_size);
    let (length_factor, spread) = match style {
        ArrowStyle::Fancy => (1.3, 0.45),
        _ => (1.0, 0.68),
    };
    let max_share = if matches!(style, ArrowStyle::Double) {
        0.3
    } else {
        0.45
    };
    let head_length = (full_head_length(line_width) * length_factor).min(line_length * max_share);

    let chord = {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let len = dx.hypot(dy).max(0.0001);
        (dx / len, dy / len)
    };
    let direction_at = |t: f64| match control {
        Some(p1) => {
            let (dx, dy) = bezier_tangent(start, p1, end, t);
            if dx.abs() + dy.abs() < 0.0001 {
                chord
            } else {
                (dx, dy)
            }
        }
        None => chord,
    };

    let mut heads = vec![chevron(end, direction_at(1.0), head_length, spread)];
    if matches!(style, ArrowStyle::Double) {
        let (dx, dy) = direction_at(0.0);
        heads.push(chevron(start, (-dx, -dy), head_length, spread));
    }

    Some(ArrowGeometry {
        body,
        heads,
        line_width,
    })
}

fn append_arrow_path(context: &gtk4::cairo::Context, geometry: &ArrowGeometry, with_heads: bool) {
    context.new_path();
    let Some(first) = geometry.body.first() else {
        return;
    };
    context.move_to(first.x, first.y);
    for pt in &geometry.body[1..] {
        context.line_to(pt.x, pt.y);
    }
    if with_heads {
        for [left, tip, right] in &geometry.heads {
            context.move_to(left.x, left.y);
            context.line_to(tip.x, tip.y);
            context.line_to(right.x, right.y);
        }
    }
}

pub fn draw_arrow_selection_outline(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
    view_scale: f64,
) {
    let Some(geometry) = arrow_geometry(start, end, stroke_size, style, &control_points) else {
        return;
    };
    let scale = view_scale.max(0.01);
    let _ = context.save();
    append_arrow_path(context, &geometry, false);
    context.set_line_width(selection_outline_stroke_width(scale).min(geometry.line_width * 0.4));
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_dash(&[], 0.0);
    context.set_source_rgba(
        TEXT_EDIT_BORDER_COLOR.0,
        TEXT_EDIT_BORDER_COLOR.1,
        TEXT_EDIT_BORDER_COLOR.2,
        0.95,
    );
    let _ = context.stroke();
    let _ = context.restore();
}

#[allow(clippy::too_many_arguments)]
pub fn draw_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
    shadow: bool,
) {
    super::draw_shadow_layer(context, shadow, color, |ctx, draw_color| {
        draw_arrow_internal(
            ctx,
            start,
            end,
            draw_color,
            stroke_size,
            style,
            control_points.clone(),
        );
    });
}

fn draw_arrow_internal(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
) {
    let Some(geometry) = arrow_geometry(start, end, stroke_size, style, &control_points) else {
        return;
    };
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);
    append_arrow_path(context, &geometry, true);
    // One stroke covers the shaft and the heads, so a translucent colour does
    // not darken where they overlap.
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(geometry.line_width);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_dash(&[], 0.0);
    let _ = context.stroke();
    let _ = context.restore();
}

fn append_rounded_square(context: &gtk4::cairo::Context, center: Point, side: f64, radius: f64) {
    let half = side / 2.0;
    let (left, top, right, bottom) = (
        center.x - half,
        center.y - half,
        center.x + half,
        center.y + half,
    );
    let radius = radius.min(half);
    context.new_path();
    context.arc(
        right - radius,
        top + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        left + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        left + radius,
        top + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
}

pub fn draw_arrow_control_handles(
    context: &gtk4::cairo::Context,
    handles: Vec<Point>,
    _color: DrawColor,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);
    let display_handles = if handles.len() >= 3 {
        let (p0, p1, p2) = (handles[0], handles[1], handles[2]);
        // On-curve midpoint B(0.5) = 0.25*P0 + 0.5*P1 + 0.25*P2
        let mid_on_curve = Point {
            x: 0.25 * p0.x + 0.5 * p1.x + 0.25 * p2.x,
            y: 0.25 * p0.y + 0.5 * p1.y + 0.25 * p2.y,
        };
        vec![p0, mid_on_curve, p2]
    } else if handles.len() == 2 {
        handles
    } else {
        return;
    };

    let side = (MOVE_HANDLE_RADIUS * 2.0 - 2.0) / scale;
    let border = MOVE_HANDLE_OUTLINE_WIDTH / scale;
    let corner = 2.5 / scale;
    let _ = context.save();
    for handle in display_handles {
        append_rounded_square(context, handle, side, corner);
        context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let _ = context.fill();
        append_rounded_square(
            context,
            handle,
            side - border * 2.0,
            (corner - border).max(0.5 / scale),
        );
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            1.0,
        );
        let _ = context.fill();
    }
    let _ = context.restore();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curved_arrow_body_uses_dense_length_adaptive_sampling() {
        assert_eq!(curved_outline_steps(60.0), 48);
        assert_eq!(curved_outline_steps(300.0), 100);
        assert_eq!(curved_outline_steps(3_000.0), 256);

        let geometry = arrow_geometry(
            Point { x: 10.0, y: 100.0 },
            Point { x: 310.0, y: 100.0 },
            4.0,
            ArrowStyle::Curved,
            &Some(vec![
                Point { x: 10.0, y: 100.0 },
                Point { x: 160.0, y: 0.0 },
                Point { x: 310.0, y: 100.0 },
            ]),
        )
        .expect("curved arrow geometry");

        assert!(
            geometry.body.len() > 100,
            "curved shafts need substantially more than 20 segments"
        );
        assert_eq!(geometry.heads.len(), 1);
    }

    #[test]
    fn heads_are_open_chevrons_at_the_tips() {
        let start = Point { x: 10.0, y: 100.0 };
        let end = Point { x: 310.0, y: 100.0 };

        let standard = arrow_geometry(start, end, 4.0, ArrowStyle::Standard, &None).unwrap();
        assert_eq!(standard.body, vec![start, end]);
        assert_eq!(standard.line_width, 7.0);
        assert_eq!(standard.heads.len(), 1);
        let [left, tip, right] = standard.heads[0];
        assert_eq!(tip, end);
        assert!(left.x < end.x && right.x < end.x);
        assert!((left.y - end.y) * (right.y - end.y) < 0.0);

        let double = arrow_geometry(start, end, 4.0, ArrowStyle::Double, &None).unwrap();
        assert_eq!(double.heads.len(), 2);
        assert_eq!(double.heads[0][1], end);
        assert_eq!(double.heads[1][1], start);
        assert!(double.heads[1][0].x > start.x && double.heads[1][2].x > start.x);

        let fancy = arrow_geometry(start, end, 4.0, ArrowStyle::Fancy, &None).unwrap();
        let spread = |head: [Point; 3]| (head[0].y - head[2].y).abs();
        let reach = |head: [Point; 3]| head[1].x - head[0].x;
        assert!(spread(fancy.heads[0]) < spread(standard.heads[0]));
        assert!(reach(fancy.heads[0]) > reach(standard.heads[0]));
    }

    #[test]
    fn zero_length_arrow_has_no_geometry() {
        let point = Point { x: 40.0, y: 40.0 };
        for style in ArrowStyle::ALL {
            assert!(arrow_geometry(point, point, 4.0, style, &None).is_none());
        }
    }
}
