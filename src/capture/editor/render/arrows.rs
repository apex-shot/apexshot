use super::super::types::{ArrowStyle, DrawColor, Point};
use super::{MOVE_HANDLE_OUTLINE_WIDTH, MOVE_HANDLE_RADIUS, TEXT_EDIT_BORDER_COLOR};

pub fn draw_arrow_selection_outline(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    style: ArrowStyle,
    control_points: Option<Vec<Point>>,
    view_scale: f64,
) {
    let _ = context.save();

    let path_built = match style {
        ArrowStyle::Double => {
            build_double_arrow_path(context, start, end, stroke_size, &control_points)
        }
        ArrowStyle::Fancy => {
            build_thorn_arrow_path(context, start, end, stroke_size, false, &control_points)
        }
        ArrowStyle::Curved => {
            build_thorn_arrow_path(context, start, end, stroke_size, false, &control_points)
        }
        _ => build_thorn_arrow_path(context, start, end, stroke_size, true, &control_points),
    };

    if path_built {
        let scale = view_scale.max(0.01);
        // Slightly expand the stroke so it sits just outside the fill
        context.set_line_width(
            (stroke_size.max(0.5) * 0.2 + 1.0) + super::selection_outline_stroke_width(scale) * 2.0,
        );
        context.set_line_join(gtk4::cairo::LineJoin::Round);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        context.set_dash(&[], 0.0);
        context.set_source_rgba(
            TEXT_EDIT_BORDER_COLOR.0,
            TEXT_EDIT_BORDER_COLOR.1,
            TEXT_EDIT_BORDER_COLOR.2,
            0.9,
        );
        let _ = context.stroke();
    }

    let _ = context.restore();
}
fn draw_arrow_head(
    context: &gtk4::cairo::Context,
    tip: Point,
    angle: f64,
    head_length: f64,
    spread: f64,
    color: DrawColor,
) {
    let left_x = tip.x - head_length * (angle - spread).cos();
    let left_y = tip.y - head_length * (angle - spread).sin();
    let right_x = tip.x - head_length * (angle + spread).cos();
    let right_y = tip.y - head_length * (angle + spread).sin();

    context.move_to(tip.x, tip.y);
    context.line_to(left_x, left_y);
    context.line_to(right_x, right_y);
    context.close_path();
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill();
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
fn build_thorn_arrow_path(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    is_smooth: bool,
    control_points: &Option<Vec<Point>>,
) -> bool {
    let outline = thorn_arrow_outline_points(start, end, stroke_size, is_smooth, control_points);
    if outline.is_empty() {
        return false;
    }
    context.new_path();
    context.move_to(outline[0].x, outline[0].y);
    for pt in &outline[1..] {
        context.line_to(pt.x, pt.y);
    }
    context.close_path();
    true
}
pub fn thorn_arrow_outline_points(
    start: Point,
    end: Point,
    stroke_size: f64,
    is_smooth: bool,
    control_points: &Option<Vec<Point>>,
) -> Vec<Point> {
    let is_curved = control_points
        .as_ref()
        .map(|v| v.len() >= 3)
        .unwrap_or(false);
    let p0 = start;
    let p2 = end;
    let p1 = control_points
        .as_ref()
        .and_then(|c| c.get(1).copied())
        .unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });

    let mut line_length = 0.0;
    if is_curved {
        let mut prev = p0;
        for i in 1..=20 {
            let t = i as f64 / 20.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dx = pt.x - prev.x;
            let dy = pt.y - prev.y;
            line_length += (dx * dx + dy * dy).sqrt();
            prev = pt;
        }
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        line_length = (dx * dx + dy * dy).sqrt();
    }

    if line_length < 0.1 {
        return Vec::new();
    }

    let stroke = stroke_size.max(0.5);
    // Body / head proportions tuned so Medium (4px) reads cleanly and
    // Very Thick (12px) stays bold without ballooning into the whole crop.
    let w = stroke * 2.4 + 2.0;
    let h = (stroke * 5.0 + 8.0)
        .clamp(10.0, 96.0)
        .min((line_length * 0.68).max(8.0));
    let w_h = h * 1.1;
    let s = h * 0.32;

    let t_neck = if is_curved {
        let mut best_t = 0.0;
        for i in (0..=100).rev() {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p2.x - pt.x) * (p2.x - pt.x) + (p2.y - pt.y) * (p2.y - pt.y)).sqrt();
            if dist >= h {
                best_t = t;
                break;
            }
        }
        best_t
    } else {
        1.0 - (h / line_length).min(1.0)
    };

    let (vx, vy) = if is_curved {
        bezier_tangent(p0, p1, p2, 1.0)
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        (dx / line_length, dy / line_length)
    };
    let nx = -vy;
    let ny = vx;

    let neck_pt = if is_curved {
        bezier_point(p0, p1, p2, t_neck)
    } else {
        Point {
            x: end.x - vx * h,
            y: end.y - vy * h,
        }
    };

    let wing_center_x = neck_pt.x - vx * s;
    let wing_center_y = neck_pt.y - vy * s;
    let right_wing_x = wing_center_x + nx * (w_h / 2.0);
    let right_wing_y = wing_center_y + ny * (w_h / 2.0);
    let left_wing_x = wing_center_x - nx * (w_h / 2.0);
    let left_wing_y = wing_center_y - ny * (w_h / 2.0);

    let (tail_vx, tail_vy) = if is_curved {
        bezier_tangent(p0, p1, p2, 0.0)
    } else {
        (vx, vy)
    };
    let tail_angle = tail_vy.atan2(tail_vx);

    let mut outline = Vec::new();

    if is_curved {
        let steps = 20;
        let mut left_body = Vec::new();
        let mut right_body = Vec::new();

        for i in 0..=steps {
            let t = (i as f64 / steps as f64) * t_neck;
            let pt = bezier_point(p0, p1, p2, t);
            let (tvx, tvy) = bezier_tangent(p0, p1, p2, t);
            let tnx = -tvy;
            let tny = tvx;
            let current_w = w * (t / t_neck);
            left_body.push(Point {
                x: pt.x - tnx * current_w / 2.0,
                y: pt.y - tny * current_w / 2.0,
            });
            right_body.push(Point {
                x: pt.x + tnx * current_w / 2.0,
                y: pt.y + tny * current_w / 2.0,
            });
        }

        for pt in right_body.iter().rev() {
            outline.push(*pt);
        }

        if is_smooth {
            let tail_r = (stroke * 0.5).max(1.0);
            let tail_cx = p0.x + tail_vx * tail_r;
            let tail_cy = p0.y + tail_vy * tail_r;
            let arc_start = tail_angle + std::f64::consts::FRAC_PI_2;
            let arc_end = tail_angle + 3.0 * std::f64::consts::FRAC_PI_2;
            let arc_steps = 12;
            for i in 0..=arc_steps {
                let a = arc_start + (i as f64 / arc_steps as f64) * (arc_end - arc_start);
                outline.push(Point {
                    x: tail_cx + tail_r * a.cos(),
                    y: tail_cy + tail_r * a.sin(),
                });
            }
        } else {
            outline.push(p0);
        }

        for pt in left_body.iter().skip(1) {
            outline.push(*pt);
        }
    } else {
        let right_neck_x = neck_pt.x + nx * (w / 2.0);
        let right_neck_y = neck_pt.y + ny * (w / 2.0);
        let left_neck_x = neck_pt.x - nx * (w / 2.0);
        let left_neck_y = neck_pt.y - ny * (w / 2.0);

        outline.push(Point {
            x: right_neck_x,
            y: right_neck_y,
        });

        if is_smooth {
            let tail_r = (stroke * 0.5).max(1.0);
            let tail_cx = p0.x + tail_vx * tail_r;
            let tail_cy = p0.y + tail_vy * tail_r;
            let arc_start = tail_angle + std::f64::consts::FRAC_PI_2;
            let arc_end = tail_angle + 3.0 * std::f64::consts::FRAC_PI_2;
            let arc_steps = 12;
            for i in 0..=arc_steps {
                let a = arc_start + (i as f64 / arc_steps as f64) * (arc_end - arc_start);
                outline.push(Point {
                    x: tail_cx + tail_r * a.cos(),
                    y: tail_cy + tail_r * a.sin(),
                });
            }
        } else {
            outline.push(p0);
        }

        outline.push(Point {
            x: left_neck_x,
            y: left_neck_y,
        });
    }

    outline.push(Point {
        x: left_wing_x,
        y: left_wing_y,
    });

    if is_smooth {
        let head_r = (stroke * 0.4).max(1.0);
        let head_cx = end.x - vx * head_r;
        let head_cy = end.y - vy * head_r;
        let left_dx = head_cx - left_wing_x;
        let left_dy = head_cy - left_wing_y;
        let left_angle = left_dy.atan2(left_dx) - std::f64::consts::FRAC_PI_2;
        let right_dx = head_cx - right_wing_x;
        let right_dy = head_cy - right_wing_y;
        let right_angle = right_dy.atan2(right_dx) + std::f64::consts::FRAC_PI_2;
        let arc_steps = 12;
        for i in 0..=arc_steps {
            let a = left_angle + (i as f64 / arc_steps as f64) * (right_angle - left_angle);
            outline.push(Point {
                x: head_cx + head_r * a.cos(),
                y: head_cy + head_r * a.sin(),
            });
        }
    } else {
        outline.push(end);
    }

    outline.push(Point {
        x: right_wing_x,
        y: right_wing_y,
    });

    outline
}
fn draw_thorn_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    is_smooth: bool,
    control_points: Option<Vec<Point>>,
) {
    if !build_thorn_arrow_path(context, start, end, stroke_size, is_smooth, &control_points) {
        return;
    }
    let stroke = stroke_size.max(0.5);
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);

    // Fill only — optional drop shadows are applied by `super::draw_shadow_layer`
    // when the user enables object shadows, so arrows stay crisp by default.
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill_preserve();

    // Soft edge outline that darkens the fill slightly instead of a hard
    // black stroke, which used to make arrows look muddy / outlined.
    let outline_alpha = (0.28 * color.a).clamp(0.08, 0.35);
    context.set_source_rgba(
        (color.r * 0.35).clamp(0.0, 1.0),
        (color.g * 0.35).clamp(0.0, 1.0),
        (color.b * 0.35).clamp(0.0, 1.0),
        outline_alpha,
    );
    context.set_line_width((stroke * 0.12 + 0.6).max(0.75));
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    let _ = context.stroke();
    let _ = context.restore();
}
pub fn double_arrow_outline_points(
    start: Point,
    end: Point,
    stroke_size: f64,
    control_points: &Option<Vec<Point>>,
) -> Vec<Point> {
    let is_curved = control_points.is_some();
    let p0 = start;
    let p2 = end;
    let p1 = control_points
        .as_ref()
        .and_then(|c| c.get(1).copied())
        .unwrap_or_else(|| Point {
            x: (start.x + end.x) / 2.0,
            y: (start.y + end.y) / 2.0,
        });

    let mut line_length = 0.0;
    if is_curved {
        let mut prev = p0;
        for i in 1..=20 {
            let t = i as f64 / 20.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dx = pt.x - prev.x;
            let dy = pt.y - prev.y;
            line_length += (dx * dx + dy * dy).sqrt();
            prev = pt;
        }
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        line_length = (dx * dx + dy * dy).sqrt();
    }

    if line_length < 0.1 {
        return Vec::new();
    }

    let stroke = stroke_size.max(0.5);
    let w = stroke * 2.4 + 2.0;
    let h = (stroke * 5.0 + 8.0)
        .clamp(10.0, 96.0)
        .min((line_length * 0.38).max(8.0));
    let w_h = h * 1.1;
    let s = h * 0.32;

    let (t_neck_start, t_neck_end) = if is_curved {
        let mut t_end = 0.0;
        for i in (0..=100).rev() {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p2.x - pt.x) * (p2.x - pt.x) + (p2.y - pt.y) * (p2.y - pt.y)).sqrt();
            if dist >= h {
                t_end = t;
                break;
            }
        }
        let mut t_start = 0.0;
        for i in 0..=100 {
            let t = i as f64 / 100.0;
            let pt = bezier_point(p0, p1, p2, t);
            let dist = ((p0.x - pt.x) * (p0.x - pt.x) + (p0.y - pt.y) * (p0.y - pt.y)).sqrt();
            if dist >= h {
                t_start = t;
                break;
            }
        }
        (t_start, t_end)
    } else {
        ((h / line_length).min(0.5), (1.0 - h / line_length).max(0.5))
    };

    let (vx_e, vy_e) = if is_curved {
        bezier_tangent(p0, p1, p2, 1.0)
    } else {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        (dx / line_length, dy / line_length)
    };
    let nx_e = -vy_e;
    let ny_e = vx_e;

    let neck_pt_e = if is_curved {
        bezier_point(p0, p1, p2, t_neck_end)
    } else {
        Point {
            x: end.x - vx_e * h,
            y: end.y - vy_e * h,
        }
    };

    let wing_ce_x = neck_pt_e.x - vx_e * s;
    let wing_ce_y = neck_pt_e.y - vy_e * s;

    let right_wing_e = Point {
        x: wing_ce_x + nx_e * (w_h / 2.0),
        y: wing_ce_y + ny_e * (w_h / 2.0),
    };
    let left_wing_e = Point {
        x: wing_ce_x - nx_e * (w_h / 2.0),
        y: wing_ce_y - ny_e * (w_h / 2.0),
    };

    let (vx_s, vy_s) = if is_curved {
        bezier_tangent(p0, p1, p2, 0.0)
    } else {
        (vx_e, vy_e)
    };
    let nx_s = -vy_s;
    let ny_s = vx_s;

    let neck_pt_s = if is_curved {
        bezier_point(p0, p1, p2, t_neck_start)
    } else {
        Point {
            x: start.x + vx_s * h,
            y: start.y + vy_s * h,
        }
    };

    let wing_cs_x = neck_pt_s.x + vx_s * s;
    let wing_cs_y = neck_pt_s.y + vy_s * s;

    let left_wing_s = Point {
        x: wing_cs_x - nx_s * (w_h / 2.0),
        y: wing_cs_y - ny_s * (w_h / 2.0),
    };
    let right_wing_s = Point {
        x: wing_cs_x + nx_s * (w_h / 2.0),
        y: wing_cs_y + ny_s * (w_h / 2.0),
    };

    let mut outline = Vec::new();
    let steps = 20;

    for i in 0..=steps {
        let t = t_neck_start + (i as f64 / steps as f64) * (t_neck_end - t_neck_start);
        let (pt, tvx, tvy) = if is_curved {
            (
                bezier_point(p0, p1, p2, t),
                bezier_tangent(p0, p1, p2, t).0,
                bezier_tangent(p0, p1, p2, t).1,
            )
        } else {
            (
                Point {
                    x: p0.x + vx_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                    y: p0.y + vy_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                },
                vx_s,
                vy_s,
            )
        };
        let tnx = -tvy;
        let tny = tvx;
        outline.push(Point {
            x: pt.x + tnx * (w / 2.0),
            y: pt.y + tny * (w / 2.0),
        });
    }

    outline.push(right_wing_e);

    let head_r = (stroke * 0.4).max(1.0);
    let head_cx_e = p2.x - vx_e * head_r;
    let head_cy_e = p2.y - vy_e * head_r;
    let left_dx_e = head_cx_e - left_wing_e.x;
    let left_dy_e = head_cy_e - left_wing_e.y;
    let left_angle_e = left_dy_e.atan2(left_dx_e) - std::f64::consts::FRAC_PI_2;
    let right_dx_e = head_cx_e - right_wing_e.x;
    let right_dy_e = head_cy_e - right_wing_e.y;
    let right_angle_e = right_dy_e.atan2(right_dx_e) + std::f64::consts::FRAC_PI_2;
    let arc_steps = 12;
    for i in 0..=arc_steps {
        let a = left_angle_e + (i as f64 / arc_steps as f64) * (right_angle_e - left_angle_e);
        outline.push(Point {
            x: head_cx_e + head_r * a.cos(),
            y: head_cy_e + head_r * a.sin(),
        });
    }

    outline.push(left_wing_e);

    for i in (0..=steps).rev() {
        let t = t_neck_start + (i as f64 / steps as f64) * (t_neck_end - t_neck_start);
        let (pt, tvx, tvy) = if is_curved {
            (
                bezier_point(p0, p1, p2, t),
                bezier_tangent(p0, p1, p2, t).0,
                bezier_tangent(p0, p1, p2, t).1,
            )
        } else {
            (
                Point {
                    x: p0.x + vx_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                    y: p0.y + vy_s * (h + (line_length - 2.0 * h) * (i as f64 / steps as f64)),
                },
                vx_s,
                vy_s,
            )
        };
        let tnx = -tvy;
        let tny = tvx;
        outline.push(Point {
            x: pt.x - tnx * (w / 2.0),
            y: pt.y - tny * (w / 2.0),
        });
    }

    outline.push(left_wing_s);

    let head_cx_s = p0.x + vx_s * head_r;
    let head_cy_s = p0.y + vy_s * head_r;
    let left_dx_s = head_cx_s - left_wing_s.x;
    let left_dy_s = head_cy_s - left_wing_s.y;
    let left_angle_s = left_dy_s.atan2(left_dx_s) + std::f64::consts::FRAC_PI_2;
    let right_dx_s = head_cx_s - right_wing_s.x;
    let right_dy_s = head_cy_s - right_wing_s.y;
    let right_angle_s = right_dy_s.atan2(right_dx_s) - std::f64::consts::FRAC_PI_2;
    for i in 0..=arc_steps {
        let a = left_angle_s + (i as f64 / arc_steps as f64) * (right_angle_s - left_angle_s);
        outline.push(Point {
            x: head_cx_s + head_r * a.cos(),
            y: head_cy_s + head_r * a.sin(),
        });
    }

    outline.push(right_wing_s);

    outline
}
fn draw_double_arrow(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    color: DrawColor,
    stroke_size: f64,
    control_points: Option<Vec<Point>>,
) {
    if !build_double_arrow_path(context, start, end, stroke_size, &control_points) {
        return;
    }
    let stroke = stroke_size.max(0.5);
    let _ = context.save();
    context.set_antialias(gtk4::cairo::Antialias::Best);

    context.set_source_rgba(color.r, color.g, color.b, color.a);
    let _ = context.fill_preserve();

    let outline_alpha = (0.28 * color.a).clamp(0.08, 0.35);
    context.set_source_rgba(
        (color.r * 0.35).clamp(0.0, 1.0),
        (color.g * 0.35).clamp(0.0, 1.0),
        (color.b * 0.35).clamp(0.0, 1.0),
        outline_alpha,
    );
    context.set_line_width((stroke * 0.12 + 0.6).max(0.75));
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    let _ = context.stroke();
    let _ = context.restore();
}
fn build_double_arrow_path(
    context: &gtk4::cairo::Context,
    start: Point,
    end: Point,
    stroke_size: f64,
    control_points: &Option<Vec<Point>>,
) -> bool {
    let outline = double_arrow_outline_points(start, end, stroke_size, control_points);
    if outline.is_empty() {
        return false;
    }
    context.new_path();
    context.move_to(outline[0].x, outline[0].y);
    for pt in &outline[1..] {
        context.line_to(pt.x, pt.y);
    }
    context.close_path();
    true
}
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
    if matches!(
        style,
        ArrowStyle::Fancy | ArrowStyle::Standard | ArrowStyle::Curved | ArrowStyle::Double
    ) {
        if matches!(style, ArrowStyle::Double) {
            draw_double_arrow(context, start, end, color, stroke_size, control_points);
        } else {
            let is_smooth = matches!(style, ArrowStyle::Standard);
            draw_thorn_arrow(
                context,
                start,
                end,
                color,
                stroke_size,
                is_smooth,
                control_points,
            );
        }
        return;
    }

    let stroke = stroke_size.max(0.5);
    context.set_source_rgba(color.r, color.g, color.b, color.a);
    context.set_line_width(stroke + 0.6);
    context.set_line_cap(gtk4::cairo::LineCap::Round);

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() < 0.1 && dy.abs() < 0.1 {
        return;
    }

    // Draw the line/curve
    match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            if let Some(ref v) = control_points {
                if let Some(mid) = v.get(1) {
                    context.move_to(start.x, start.y);
                    context.curve_to(mid.x, mid.y, mid.x, mid.y, end.x, end.y);
                } else {
                    context.move_to(start.x, start.y);
                    context.line_to(end.x, end.y);
                }
            } else {
                context.move_to(start.x, start.y);
                context.line_to(end.x, end.y);
            }
        }
        _ => {
            context.move_to(start.x, start.y);
            context.line_to(end.x, end.y);
        }
    }
    let _ = context.stroke();

    // Compute arrowhead dimensions
    let angle = dy.atan2(dx);
    let line_length = (dx * dx + dy * dy).sqrt().max(1.0);
    let head_length = (stroke * 4.8)
        .clamp(12.0, 120.0)
        .min((line_length * 0.75).max(8.0));

    let spread = match style {
        ArrowStyle::Fancy => 0.3,
        _ => 0.55,
    };

    // End arrowhead (all styles)
    let end_angle = match style {
        ArrowStyle::Curved | ArrowStyle::Double => {
            if let Some(ref v) = control_points {
                if let Some(mid) = v.get(1) {
                    (end.y - mid.y).atan2(end.x - mid.x)
                } else {
                    angle
                }
            } else {
                angle
            }
        }
        _ => angle,
    };
    draw_arrow_head(context, end, end_angle, head_length, spread, color);

    // Start arrowhead (Double only)
    if matches!(style, ArrowStyle::Double) {
        let start_angle = if let Some(ref v) = control_points {
            if let Some(mid) = v.get(1) {
                (start.y - mid.y).atan2(start.x - mid.x) + std::f64::consts::PI
            } else {
                angle + std::f64::consts::PI
            }
        } else {
            angle + std::f64::consts::PI
        };
        draw_arrow_head(context, start, start_angle, head_length, spread, color);
    }
}
pub fn draw_arrow_control_handles(
    context: &gtk4::cairo::Context,
    handles: Vec<Point>,
    color: DrawColor,
    view_scale: f64,
) {
    let scale = view_scale.max(0.01);

    if handles.len() >= 3 {
        // Curved/Double: Bezier with 3 handles
        let p0 = handles[0];
        let p1 = handles[1];
        let p2 = handles[2];

        // On-curve midpoint B(0.5) = 0.25*P0 + 0.5*P1 + 0.25*P2
        let mid_on_curve = Point {
            x: 0.25 * p0.x + 0.5 * p1.x + 0.25 * p2.x,
            y: 0.25 * p0.y + 0.5 * p1.y + 0.25 * p2.y,
        };

        // Draw dashed curve from start → mid → end (following the Bezier)
        context.set_source_rgba(color.r, color.g, color.b, 0.4);
        context.set_line_width(1.0 / scale);
        context.set_dash(&[4.0 / scale, 4.0 / scale], 0.0);
        context.move_to(p0.x, p0.y);
        context.curve_to(p1.x, p1.y, p1.x, p1.y, p2.x, p2.y);
        let _ = context.stroke();
        context.set_dash(&[], 0.0);

        // Draw handle circles: start, on-curve mid, end
        let display_handles = [p0, mid_on_curve, p2];
        let _ = context.save();
        for (i, handle) in display_handles.iter().enumerate() {
            let radius = (MOVE_HANDLE_RADIUS + if i == 1 { 1.0 } else { 0.0 }) / scale;
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
            context.arc(handle.x, handle.y, radius, 0.0, std::f64::consts::TAU);
            let _ = context.stroke();
            context.set_source_rgba(
                TEXT_EDIT_BORDER_COLOR.0,
                TEXT_EDIT_BORDER_COLOR.1,
                TEXT_EDIT_BORDER_COLOR.2,
                1.0,
            );
            context.arc(
                handle.x,
                handle.y,
                (radius - MOVE_HANDLE_OUTLINE_WIDTH / scale).max(1.0 / scale),
                0.0,
                std::f64::consts::TAU,
            );
            let _ = context.fill();
        }
        let _ = context.restore();
    } else if handles.len() == 2 {
        // Standard/Fancy: 2 handles at head and tail
        let p0 = handles[0];
        let p1 = handles[1];

        // Draw dashed line from start to end
        context.set_source_rgba(color.r, color.g, color.b, 0.4);
        context.set_line_width(1.0 / scale);
        context.set_dash(&[4.0 / scale, 4.0 / scale], 0.0);
        context.move_to(p0.x, p0.y);
        context.line_to(p1.x, p1.y);
        let _ = context.stroke();
        context.set_dash(&[], 0.0);

        // Draw handle circles at start and end
        let display_handles = [p0, p1];
        let _ = context.save();
        for handle in &display_handles {
            let radius = MOVE_HANDLE_RADIUS / scale;
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            context.set_line_width(MOVE_HANDLE_OUTLINE_WIDTH / scale);
            context.arc(handle.x, handle.y, radius, 0.0, std::f64::consts::TAU);
            let _ = context.stroke();
            context.set_source_rgba(
                TEXT_EDIT_BORDER_COLOR.0,
                TEXT_EDIT_BORDER_COLOR.1,
                TEXT_EDIT_BORDER_COLOR.2,
                1.0,
            );
            context.arc(
                handle.x,
                handle.y,
                (radius - MOVE_HANDLE_OUTLINE_WIDTH / scale).max(1.0 / scale),
                0.0,
                std::f64::consts::TAU,
            );
            let _ = context.fill();
        }
        let _ = context.restore();
    }
}
