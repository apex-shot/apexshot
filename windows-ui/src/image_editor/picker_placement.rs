//! Keep the custom color picker clear of visible annotations where possible.

use gtk4::prelude::*;
use gtk4::{Grid, Orientation, Revealer};

use super::annotate::EditorHost;
use super::canvas::Canvas;
use crate::capture::editor::selection::action_bounds_with_padding;
use crate::capture::editor::types::{AnnotationAction, Point, Rect, ViewTransform};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn view_bounds(rect: Rect, transform: ViewTransform) -> Bounds {
    let point = transform.image_to_view(Point {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
    });
    Bounds {
        x: point.x,
        y: point.y,
        width: f64::from(rect.width) * transform.scale,
        height: f64::from(rect.height) * transform.scale,
    }
}

fn overlap(a: Bounds, b: Bounds) -> f64 {
    let width = ((a.x + a.width).min(b.x + b.width) - a.x.max(b.x)).max(0.0);
    let height = ((a.y + a.height).min(b.y + b.height) - a.y.max(b.y)).max(0.0);
    width * height
}

fn picker_x(view_width: f64, picker: Bounds, obstacles: &[Bounds], gap: f64) -> f64 {
    let edge = gap.min((view_width - picker.width).max(0.0) / 2.0);
    let max_x = (view_width - picker.width - edge).max(edge);
    let centered = picker.x.clamp(edge, max_x);
    let score = |x| {
        let candidate = Bounds { x, ..picker };
        obstacles
            .iter()
            .map(|&rect| overlap(candidate, rect))
            .sum::<f64>()
    };
    let mut best = centered;
    let mut best_score = score(best);
    if best_score == 0.0 {
        return best;
    }
    let mut candidates = vec![edge, max_x];
    for rect in obstacles {
        if rect.y < picker.y + picker.height && rect.y + rect.height > picker.y {
            candidates.push(rect.x - picker.width - gap);
            candidates.push(rect.x + rect.width + gap);
        }
    }
    for x in candidates {
        let x = x.clamp(edge, max_x);
        let candidate_score = score(x);
        if candidate_score < best_score - 0.01
            || ((candidate_score - best_score).abs() < 0.01
                && (x - centered).abs() < (best - centered).abs())
        {
            best = x;
            best_score = candidate_score;
        }
    }
    best
}

/// Position against the canvas, not the palette's allocation, so the picker
/// can move beyond the palette without shifting the tool controls.
pub(super) fn position(
    canvas: &Canvas,
    host: &EditorHost,
    grid: &Grid,
    panel: &Revealer,
    scale: f64,
) {
    let Some(anchor) = grid.compute_bounds(&canvas.area) else {
        return;
    };
    let Some(card) = panel.child() else {
        return;
    };
    let view_width = f64::from(canvas.area.width());
    let view_height = f64::from(canvas.area.height());
    if view_width <= 1.0 || view_height <= 1.0 {
        return;
    }
    let (_, width, _, _) = card.measure(Orientation::Horizontal, -1);
    let (_, height, _, _) = card.measure(Orientation::Vertical, width);
    let width = f64::from(width).min(view_width);
    let height = f64::from(height).min(view_height);
    let y = f64::from(anchor.y() + anchor.height()).clamp(0.0, (view_height - height).max(0.0));
    let picker = Bounds {
        x: f64::from(anchor.x() + anchor.width() / 2.0) - width / 2.0,
        y,
        width,
        height,
    };
    let transform = canvas.view_transform();
    let padding = 6.0 * scale / transform.scale.max(0.001);
    let obstacles = host
        .read(|st| {
            let mut bounds = Vec::new();
            for action in &st.actions {
                if matches!(
                    action,
                    AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                ) {
                    continue;
                }
                if let Some(rect) = action_bounds_with_padding(action, padding)
                    .and_then(|r| r.clamp_to(st.working_image.width(), st.working_image.height()))
                {
                    bounds.push(view_bounds(rect, transform));
                }
            }
            if let Some(text) = st.active_text_bounds.as_ref() {
                bounds.push(view_bounds(text.rect, transform));
            }
            bounds
        })
        .unwrap_or_default();
    let x = picker_x(view_width, picker, &obstacles, 8.0 * scale);
    panel.set_margin_start(x.round() as i32);
    panel.set_margin_top(y.round() as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn centered_picker_stays_put_if_drawings_do_not_overlap() {
        let picker = rect(350.0, 100.0, 300.0, 240.0);
        assert_eq!(
            picker_x(1000.0, picker, &[rect(700.0, 140.0, 80.0, 80.0)], 8.0),
            350.0
        );
        assert_eq!(
            picker_x(1000.0, picker, &[rect(400.0, 400.0, 80.0, 80.0)], 8.0),
            350.0
        );
    }

    #[test]
    fn moves_to_nearest_clear_side_and_avoids_other_drawings() {
        let picker = rect(350.0, 100.0, 300.0, 240.0);
        let center = rect(420.0, 150.0, 80.0, 100.0);
        let left = rect(50.0, 120.0, 220.0, 180.0);
        assert_eq!(picker_x(1000.0, picker, &[center], 8.0), 508.0);
        assert_eq!(picker_x(1000.0, picker, &[center, left], 8.0), 508.0);
    }

    #[test]
    fn constrained_view_chooses_least_overlap_and_stays_inside() {
        let picker = rect(50.0, 0.0, 300.0, 200.0);
        let obstacle = rect(0.0, 0.0, 250.0, 200.0);
        assert_eq!(picker_x(400.0, picker, &[obstacle], 8.0), 92.0);
        assert_eq!(picker_x(200.0, picker, &[obstacle], 8.0), 0.0);
    }

    #[test]
    fn annotation_bounds_follow_pan_and_zoom() {
        let mut transform = ViewTransform::for_image(800.0, 600.0);
        transform.scale = 1.5;
        transform.offset_x = -20.0;
        transform.offset_y = 40.0;
        let bounds = view_bounds(
            Rect {
                x: 100,
                y: 20,
                width: 40,
                height: 10,
            },
            transform,
        );
        assert_eq!(bounds, rect(130.0, 70.0, 60.0, 15.0));
    }
}
