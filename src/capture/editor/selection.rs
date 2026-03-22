#[cfg(test)]
use super::color::SELECT_HANDLE_HIT_RADIUS;
use super::color::{highlighter_stroke_width, SELECT_MIN_RESIZE_SIZE};
use super::render::text_action_bounds;
use super::types::{AnnotationAction, Point, Rect, SelectHandle};

pub fn action_bounds_with_padding(action: &AnnotationAction, padding: f64) -> Option<Rect> {
    match action {
        AnnotationAction::Pen {
            points,
            stroke_size,
            ..
        } => stroke_bounds(points, *stroke_size + padding),
        AnnotationAction::Highlighter {
            points,
            stroke_size,
            ..
        } => stroke_bounds(
            points,
            highlighter_stroke_width(*stroke_size) * 0.5 + padding,
        ),
        AnnotationAction::Circle { rect, .. }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect } => Rect::from_bounds(
            rect.x as f64 - padding,
            rect.y as f64 - padding,
            rect.x as f64 + rect.width as f64 + padding,
            rect.y as f64 + rect.height as f64 + padding,
        ),
        AnnotationAction::Line {
            start,
            end,
            stroke_size,
            ..
        } => {
            let padding = *stroke_size + padding;
            Rect::from_bounds(
                start.x.min(end.x) - padding,
                start.y.min(end.y) - padding,
                start.x.max(end.x) + padding,
                start.y.max(end.y) + padding,
            )
        }
        AnnotationAction::Arrow {
            start,
            end,
            stroke_size,
            control_points,
            ..
        } => {
            let pad = *stroke_size + padding;
            // Sample the Bezier curve to get a tight bounding box for curved arrows
            let mid = control_points.as_ref().and_then(|v| {
                if v.len() >= 3 {
                    v.get(1).copied()
                } else {
                    None
                }
            });
            if let Some(ctrl) = mid {
                let mut min_x = start.x.min(end.x).min(ctrl.x);
                let mut min_y = start.y.min(end.y).min(ctrl.y);
                let mut max_x = start.x.max(end.x).max(ctrl.x);
                let mut max_y = start.y.max(end.y).max(ctrl.y);
                for i in 0..=16u32 {
                    let t = i as f64 / 16.0;
                    let u = 1.0 - t;
                    let px = u * u * start.x + 2.0 * u * t * ctrl.x + t * t * end.x;
                    let py = u * u * start.y + 2.0 * u * t * ctrl.y + t * t * end.y;
                    min_x = min_x.min(px);
                    min_y = min_y.min(py);
                    max_x = max_x.max(px);
                    max_y = max_y.max(py);
                }
                Rect::from_bounds(min_x - pad, min_y - pad, max_x + pad, max_y + pad)
            } else {
                Rect::from_bounds(
                    start.x.min(end.x) - pad,
                    start.y.min(end.y) - pad,
                    start.x.max(end.x) + pad,
                    start.y.max(end.y) + pad,
                )
            }
        }
        AnnotationAction::Text {
            position,
            text,
            font,
            max_width,
            ..
        } => {
            let surface =
                gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok()?;
            let context = gtk4::cairo::Context::new(&surface).ok()?;
            let available_width = max_width.unwrap_or(4096.0);
            let bounds = text_action_bounds(&context, *position, text, font, Some(available_width));
            Rect::from_bounds(
                bounds.rect.x as f64 - padding,
                bounds.rect.y as f64 - padding,
                (bounds.rect.x + bounds.rect.width) as f64 + padding,
                (bounds.rect.y + bounds.rect.height) as f64 + padding,
            )
        }
        AnnotationAction::Number { position, .. } => {
            let radius = 15.0 + padding; // NUMBER_RADIUS
            Rect::from_bounds(
                position.x - radius,
                position.y - radius,
                position.x + radius,
                position.y + radius,
            )
        }
    }
}

pub fn action_contains_point_with_padding(
    action: &AnnotationAction,
    point: Point,
    padding: f64,
) -> bool {
    match action {
        AnnotationAction::Pen {
            points,
            stroke_size,
            ..
        } => stroke_contains_point(points, point, *stroke_size + padding),
        AnnotationAction::Highlighter {
            points,
            stroke_size,
            ..
        } => stroke_contains_point(
            points,
            point,
            (highlighter_stroke_width(*stroke_size) * 0.5) + padding,
        ),
        AnnotationAction::Line {
            start,
            end,
            stroke_size,
            ..
        } => distance_to_segment(point, *start, *end) <= *stroke_size + padding + 2.0,
        AnnotationAction::Arrow {
            start,
            end,
            stroke_size,
            control_points,
            ..
        } => {
            let threshold = *stroke_size + padding + 8.0;
            let mid = control_points.as_ref().and_then(|v| {
                if v.len() >= 3 {
                    v.get(1).copied()
                } else {
                    None
                }
            });
            if let Some(ctrl) = mid {
                // Sample the quadratic Bezier and check distance to each segment
                let steps = 20u32;
                let mut prev = *start;
                for i in 1..=steps {
                    let t = i as f64 / steps as f64;
                    let u = 1.0 - t;
                    let cur = Point {
                        x: u * u * start.x + 2.0 * u * t * ctrl.x + t * t * end.x,
                        y: u * u * start.y + 2.0 * u * t * ctrl.y + t * t * end.y,
                    };
                    if distance_to_segment(point, prev, cur) <= threshold {
                        return true;
                    }
                    prev = cur;
                }
                false
            } else {
                distance_to_segment(point, *start, *end) <= threshold
            }
        }
        _ => action_bounds_with_padding(action, padding)
            .map(|rect| rect_contains_point(rect, point, 0.0))
            .unwrap_or(false),
    }
}

pub fn action_resize_handles(action: &AnnotationAction) -> Vec<(SelectHandle, Point)> {
    match action {
        AnnotationAction::Circle { rect, .. }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect } => {
            let left = rect.x as f64;
            let top = rect.y as f64;
            let right = left + rect.width as f64;
            let bottom = top + rect.height as f64;
            let center_x = (left + right) * 0.5;
            let center_y = (top + bottom) * 0.5;

            vec![
                (SelectHandle::TopLeft, Point { x: left, y: top }),
                (
                    SelectHandle::Top,
                    Point {
                        x: center_x,
                        y: top,
                    },
                ),
                (SelectHandle::TopRight, Point { x: right, y: top }),
                (
                    SelectHandle::Left,
                    Point {
                        x: left,
                        y: center_y,
                    },
                ),
                (
                    SelectHandle::Right,
                    Point {
                        x: right,
                        y: center_y,
                    },
                ),
                (SelectHandle::BottomLeft, Point { x: left, y: bottom }),
                (
                    SelectHandle::Bottom,
                    Point {
                        x: center_x,
                        y: bottom,
                    },
                ),
                (
                    SelectHandle::BottomRight,
                    Point {
                        x: right,
                        y: bottom,
                    },
                ),
            ]
        }
        AnnotationAction::Line { start, end, .. } | AnnotationAction::Arrow { start, end, .. } => {
            vec![(SelectHandle::Start, *start), (SelectHandle::End, *end)]
        }
        _ => Vec::new(),
    }
}

pub fn action_resize_handle_at_point_with_radius(
    action: &AnnotationAction,
    point: Point,
    hit_radius: f64,
) -> Option<SelectHandle> {
    action_resize_handles(action)
        .into_iter()
        .filter_map(|(handle, center)| {
            let handle_hit_radius = if matches!(
                handle,
                SelectHandle::Top | SelectHandle::Bottom | SelectHandle::Left | SelectHandle::Right
            ) {
                (hit_radius * 0.65).max(4.0) // SELECT_HANDLE_SIZE * 0.5
            } else {
                hit_radius
            };
            let radius_sq = handle_hit_radius * handle_hit_radius;
            let dx = point.x - center.x;
            let dy = point.y - center.y;
            let dist_sq = dx * dx + dy * dy;
            (dist_sq <= radius_sq).then_some((handle, dist_sq))
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(handle, _)| handle)
}

#[cfg(test)]
pub fn action_resize_handle_at_point(
    action: &AnnotationAction,
    point: Point,
) -> Option<SelectHandle> {
    action_resize_handle_at_point_with_radius(action, point, SELECT_HANDLE_HIT_RADIUS)
}

pub fn resize_rect_with_handle(rect: &mut Rect, handle: SelectHandle, dx: f64, dy: f64) -> bool {
    let mut left = rect.x as f64;
    let mut top = rect.y as f64;
    let mut right = left + rect.width as f64;
    let mut bottom = top + rect.height as f64;

    let move_left = matches!(
        handle,
        SelectHandle::TopLeft | SelectHandle::Left | SelectHandle::BottomLeft
    );
    let move_right = matches!(
        handle,
        SelectHandle::TopRight | SelectHandle::Right | SelectHandle::BottomRight
    );
    let move_top = matches!(
        handle,
        SelectHandle::TopLeft | SelectHandle::Top | SelectHandle::TopRight
    );
    let move_bottom = matches!(
        handle,
        SelectHandle::BottomLeft | SelectHandle::Bottom | SelectHandle::BottomRight
    );

    if !move_left && !move_right && !move_top && !move_bottom {
        return false;
    }

    if move_left {
        left += dx;
    }
    if move_right {
        right += dx;
    }
    if move_top {
        top += dy;
    }
    if move_bottom {
        bottom += dy;
    }

    if move_left && right - left < SELECT_MIN_RESIZE_SIZE {
        left = right - SELECT_MIN_RESIZE_SIZE;
    }
    if move_right && right - left < SELECT_MIN_RESIZE_SIZE {
        right = left + SELECT_MIN_RESIZE_SIZE;
    }
    if move_top && bottom - top < SELECT_MIN_RESIZE_SIZE {
        top = bottom - SELECT_MIN_RESIZE_SIZE;
    }
    if move_bottom && bottom - top < SELECT_MIN_RESIZE_SIZE {
        bottom = top + SELECT_MIN_RESIZE_SIZE;
    }

    let Some(updated) = Rect::from_bounds(
        left.min(right),
        top.min(bottom),
        left.max(right),
        top.max(bottom),
    ) else {
        return false;
    };

    let changed = updated.x != rect.x
        || updated.y != rect.y
        || updated.width != rect.width
        || updated.height != rect.height;

    if changed {
        *rect = updated;
    }

    changed
}

pub fn resize_action(
    action: &mut AnnotationAction,
    handle: SelectHandle,
    dx: f64,
    dy: f64,
) -> bool {
    if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
        return false;
    }

    match action {
        AnnotationAction::Circle { rect, .. }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect } => resize_rect_with_handle(rect, handle, dx, dy),
        AnnotationAction::Line { start, end, .. } | AnnotationAction::Arrow { start, end, .. } => {
            let target = match handle {
                SelectHandle::Start => start,
                SelectHandle::End => end,
                _ => return false,
            };
            target.x += dx;
            target.y += dy;
            true
        }
        _ => false,
    }
}

pub fn translate_action(action: &mut AnnotationAction, dx: f64, dy: f64) -> bool {
    if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
        return false;
    }

    match action {
        AnnotationAction::Pen { points, .. } | AnnotationAction::Highlighter { points, .. } => {
            for point in points {
                point.x += dx;
                point.y += dy;
            }
            true
        }
        AnnotationAction::Line { start, end, .. } => {
            start.x += dx;
            start.y += dy;
            end.x += dx;
            end.y += dy;
            true
        }
        AnnotationAction::Arrow {
            start,
            end,
            control_points,
            ..
        } => {
            start.x += dx;
            start.y += dy;
            end.x += dx;
            end.y += dy;
            // Keep control_points in sync so the handle circles move with the arrow
            if let Some(pts) = control_points {
                for pt in pts.iter_mut() {
                    pt.x += dx;
                    pt.y += dy;
                }
            }
            true
        }
        AnnotationAction::Text { position, .. } | AnnotationAction::Number { position, .. } => {
            position.x += dx;
            position.y += dy;
            true
        }
        AnnotationAction::Circle { rect, .. }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect } => {
            let dx_i = dx.round() as i32;
            let dy_i = dy.round() as i32;
            if dx_i == 0 && dy_i == 0 {
                return false;
            }
            rect.x += dx_i;
            rect.y += dy_i;
            true
        }
    }
}

fn rect_contains_point(rect: Rect, point: Point, padding: f64) -> bool {
    let min_x = rect.x as f64 - padding;
    let min_y = rect.y as f64 - padding;
    let max_x = rect.x as f64 + rect.width as f64 + padding;
    let max_y = rect.y as f64 + rect.height as f64 + padding;

    point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y
}

fn distance_to_segment(point: Point, start: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len_sq = dx * dx + dy * dy;

    if len_sq <= f64::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }

    let t = (((point.x - start.x) * dx) + ((point.y - start.y) * dy)) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = start.x + t * dx;
    let proj_y = start.y + t * dy;

    ((point.x - proj_x).powi(2) + (point.y - proj_y).powi(2)).sqrt()
}

fn stroke_bounds(points: &[Point], padding: f64) -> Option<Rect> {
    let mut iter = points.iter();
    let first = iter.next()?;

    let mut min_x = first.x;
    let mut max_x = first.x;
    let mut min_y = first.y;
    let mut max_y = first.y;

    for point in iter {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }

    Rect::from_bounds(
        min_x - padding,
        min_y - padding,
        max_x + padding,
        max_y + padding,
    )
}

fn stroke_contains_point(points: &[Point], point: Point, threshold: f64) -> bool {
    if points.is_empty() {
        return false;
    }

    if points.len() == 1 {
        return ((point.x - points[0].x).powi(2) + (point.y - points[0].y).powi(2)).sqrt()
            <= threshold;
    }

    points
        .windows(2)
        .any(|pair| distance_to_segment(point, pair[0], pair[1]) <= threshold)
}
