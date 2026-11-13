use super::super::types::{AnnotationAction, ArrowStyle, Point};
use super::EditorState;

impl EditorState {
    pub fn set_arrow_style(&mut self, style: ArrowStyle) {
        self.arrow_style = style;
    }

    pub fn selected_arrow_style(&self) -> Option<ArrowStyle> {
        let AnnotationAction::Arrow { style, .. } = self.selected_action()? else {
            return None;
        };

        Some(*style)
    }

    pub fn selected_arrow_reversed(&self) -> Option<bool> {
        match self.selected_action()? {
            AnnotationAction::Arrow { reversed, .. } => Some(*reversed),
            _ => None,
        }
    }

    pub fn set_selected_arrow_style(&mut self, style: ArrowStyle) -> bool {
        let Some(index) = self.selected_action_index else {
            return false;
        };

        let before = self.document_snapshot();
        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Arrow {
            style: current_style,
            ..
        } = action
        else {
            return false;
        };

        if *current_style == style {
            return false;
        }

        *current_style = style;
        self.clear_redo_history();
        self.commit_history_change(before, super::history::HistoryKind::Edit);
        true
    }

    pub fn reverse_selected_arrow_action(&mut self) -> bool {
        let Some(index) = self.selected_action_index else {
            return false;
        };

        let before = self.document_snapshot();
        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Arrow {
            start,
            end,
            control_points,
            reversed,
            ..
        } = action
        else {
            return false;
        };

        std::mem::swap(start, end);
        if let Some(points) = control_points.as_mut() {
            points.reverse();
        }
        *reversed = !*reversed;
        self.clear_redo_history();
        self.commit_history_change(before, super::history::HistoryKind::Edit);
        true
    }

    const CONTROL_HANDLE_HIT_RADIUS: f64 = 10.0;

    pub fn arrow_control_handle_at(&self, point: Point) -> Option<usize> {
        let action = self.selected_action()?;
        if let AnnotationAction::Arrow {
            control_points: Some(handles),
            ..
        } = action
        {
            if handles.len() >= 3 {
                // Curved/Double: hit-test against on-curve midpoint B(0.5)
                let mid_on_curve = Point {
                    x: 0.25 * handles[0].x + 0.5 * handles[1].x + 0.25 * handles[2].x,
                    y: 0.25 * handles[0].y + 0.5 * handles[1].y + 0.25 * handles[2].y,
                };
                let test_points = [handles[0], mid_on_curve, handles[2]];
                for (i, handle) in test_points.iter().enumerate() {
                    let dx = point.x - handle.x;
                    let dy = point.y - handle.y;
                    if (dx * dx + dy * dy).sqrt() < Self::CONTROL_HANDLE_HIT_RADIUS {
                        return Some(i);
                    }
                }
            } else {
                // Standard/Fancy: hit-test against start and end
                for (i, handle) in handles.iter().enumerate() {
                    let dx = point.x - handle.x;
                    let dy = point.y - handle.y;
                    if (dx * dx + dy * dy).sqrt() < Self::CONTROL_HANDLE_HIT_RADIUS {
                        return Some(i);
                    }
                }
            }
        }
        None
    }

    pub fn move_arrow_control_handle(&mut self, index: usize, new_pos: Point) {
        let Some(action_index) = self.selected_action_index else {
            return;
        };
        if self.history_interaction_before.is_none() {
            let before = self.document_snapshot();
            self.history_interaction_before = Some(before);
        }
        let bounds =
            self.canvas_bounds_for_dimensions(self.base_image.width(), self.base_image.height());
        let Some(action) = self.actions.get_mut(action_index) else {
            return;
        };
        if let AnnotationAction::Arrow {
            control_points: Some(handles),
            start,
            end,
            ..
        } = action
        {
            if handles.len() >= 3 {
                let clamp_point = |mut point: Point| {
                    point.x = point.x.max(bounds.0).min(bounds.2);
                    point.y = point.y.max(bounds.1).min(bounds.3);
                    point
                };
                match index {
                    0 => {
                        let clamped = clamp_point(new_pos);
                        *start = clamped;
                        handles[0] = clamped;
                        handles[1] =
                            clamp_curve_control(handles[0], handles[1], handles[2], bounds);
                    }
                    1 => {
                        // new_pos is the desired on-curve midpoint B(0.5).
                        // Invert: P1 = 2*B(0.5) - 0.5*P0 - 0.5*P2
                        let desired = Point {
                            x: 2.0 * new_pos.x - 0.5 * handles[0].x - 0.5 * handles[2].x,
                            y: 2.0 * new_pos.y - 0.5 * handles[0].y - 0.5 * handles[2].y,
                        };
                        handles[1] = clamp_curve_control(handles[0], desired, handles[2], bounds);
                    }
                    2 => {
                        let clamped = clamp_point(new_pos);
                        *end = clamped;
                        handles[2] = clamped;
                        handles[1] =
                            clamp_curve_control(handles[0], handles[1], handles[2], bounds);
                    }
                    _ => {}
                }
            } else {
                match index {
                    0 => {
                        let mut clamped = new_pos;
                        clamped.x = clamped.x.max(bounds.0).min(bounds.2);
                        clamped.y = clamped.y.max(bounds.1).min(bounds.3);
                        *start = clamped;
                        handles[0] = clamped;
                    }
                    1 => {
                        let mut clamped = new_pos;
                        clamped.x = clamped.x.max(bounds.0).min(bounds.2);
                        clamped.y = clamped.y.max(bounds.1).min(bounds.3);
                        *end = clamped;
                        handles[1] = clamped;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn finalize_arrow_control_editing(&mut self) {
        self.finish_history_interaction();
        self.arrow_editing_controls = false;
        self.arrow_control_dragging = None;
    }

    pub fn finalize_arrow_interaction_cleanup(&mut self) {
        self.clear_drag_without_rebuild();
        self.finish_history_interaction();
        self.arrow_editing_controls = self
            .selected_action_index
            .and_then(|index| self.actions.get(index))
            .is_some_and(|action| matches!(action, AnnotationAction::Arrow { .. }));
    }
}

/// Clamps a quadratic Bézier control point so the curve itself, not the
/// off-curve control point, stays inside `bounds`. On each axis the curve's
/// extreme only reaches an edge `hi` when the control point sits at
/// `hi + sqrt((hi - a) * (hi - b))`, so the control point may lie past the edge.
fn clamp_curve_control(p0: Point, p1: Point, p2: Point, bounds: (f64, f64, f64, f64)) -> Point {
    let axis = |a: f64, c: f64, b: f64, lo: f64, hi: f64| {
        let min = lo - ((a - lo).max(0.0) * (b - lo).max(0.0)).sqrt();
        let max = hi + ((hi - a).max(0.0) * (hi - b).max(0.0)).sqrt();
        c.max(min).min(max)
    };
    Point {
        x: axis(p0.x, p1.x, p2.x, bounds.0, bounds.2),
        y: axis(p0.y, p1.y, p2.y, bounds.1, bounds.3),
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::types::{AnnotationAction, ArrowStyle, DrawColor, Point};

    use super::EditorState;

    #[test]
    fn selected_arrow_style_updates_selected_arrow_immediately() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.actions.push(AnnotationAction::Arrow {
            start: Point { x: 2.0, y: 3.0 },
            end: Point { x: 24.0, y: 26.0 },
            color: DrawColor::new(1.0, 0.5, 0.0, 1.0),
            stroke_size: 4.0,
            style: ArrowStyle::Standard,
            control_points: Some(vec![
                Point { x: 2.0, y: 3.0 },
                Point { x: 13.0, y: 14.0 },
                Point { x: 24.0, y: 26.0 },
            ]),
            shadow: false,
            reversed: false,
        });
        state.selected_action_index = Some(0);

        assert!(state.set_selected_arrow_style(ArrowStyle::Curved));
        assert_eq!(state.selected_arrow_style(), Some(ArrowStyle::Curved));
        assert!(!state.set_selected_arrow_style(ArrowStyle::Curved));
    }

    #[test]
    fn reverse_selected_arrow_action_swaps_endpoints_and_control_points() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.actions.push(AnnotationAction::Arrow {
            start: Point { x: 1.0, y: 2.0 },
            end: Point { x: 20.0, y: 22.0 },
            color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            stroke_size: 4.0,
            style: ArrowStyle::Curved,
            control_points: Some(vec![
                Point { x: 1.0, y: 2.0 },
                Point { x: 10.0, y: 18.0 },
                Point { x: 20.0, y: 22.0 },
            ]),
            shadow: false,
            reversed: false,
        });
        state.selected_action_index = Some(0);

        assert!(state.reverse_selected_arrow_action());

        match state.selected_action() {
            Some(AnnotationAction::Arrow {
                start,
                end,
                control_points: Some(points),
                ..
            }) => {
                assert_eq!(*start, Point { x: 20.0, y: 22.0 });
                assert_eq!(*end, Point { x: 1.0, y: 2.0 });
                assert_eq!(points[0], Point { x: 20.0, y: 22.0 });
                assert_eq!(points[1], Point { x: 10.0, y: 18.0 });
                assert_eq!(points[2], Point { x: 1.0, y: 2.0 });
            }
            other => panic!("expected selected arrow after reverse, got {other:?}"),
        }
    }

    #[test]
    fn curved_arrow_midpoint_can_bend_to_the_canvas_edge() {
        let mut state = EditorState::new(RgbaImage::new(100, 100));
        state.actions.push(AnnotationAction::Arrow {
            start: Point { x: 40.0, y: 20.0 },
            end: Point { x: 40.0, y: 80.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 4.0,
            style: ArrowStyle::Curved,
            control_points: Some(vec![
                Point { x: 40.0, y: 20.0 },
                Point { x: 40.0, y: 50.0 },
                Point { x: 40.0, y: 80.0 },
            ]),
            shadow: false,
            reversed: false,
        });
        state.selected_action_index = Some(0);
        let bounds = state.canvas_bounds_for_dimensions(100, 100);

        state.move_arrow_control_handle(
            1,
            Point {
                x: bounds.2 + 50.0,
                y: 50.0,
            },
        );

        let AnnotationAction::Arrow {
            control_points: Some(points),
            ..
        } = &state.actions[0]
        else {
            panic!("expected a curved arrow");
        };
        let (p0, p1, p2) = (points[0], points[1], points[2]);
        let curve: Vec<Point> = (0..=400)
            .map(|i| {
                let t = i as f64 / 400.0;
                let u = 1.0 - t;
                Point {
                    x: u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
                    y: u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
                }
            })
            .collect();
        for point in &curve {
            assert!(point.x >= bounds.0 - 1e-6 && point.x <= bounds.2 + 1e-6);
            assert!(point.y >= bounds.1 - 1e-6 && point.y <= bounds.3 + 1e-6);
        }
        let rightmost = curve.iter().map(|point| point.x).fold(f64::MIN, f64::max);
        assert!(
            (rightmost - bounds.2).abs() < 0.5,
            "curve should bend all the way to the edge, reached {rightmost} of {}",
            bounds.2
        );
    }
}
