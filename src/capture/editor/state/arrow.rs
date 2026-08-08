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

    pub fn set_selected_arrow_style(&mut self, style: ArrowStyle) -> bool {
        let Some(index) = self.selected_action_index else {
            return false;
        };

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
        self.redo_actions.clear();
        true
    }

    pub fn reverse_selected_arrow_action(&mut self) -> bool {
        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Arrow {
            start,
            end,
            control_points,
            ..
        } = action
        else {
            return false;
        };

        std::mem::swap(start, end);
        if let Some(points) = control_points.as_mut() {
            points.reverse();
        }
        self.redo_actions.clear();
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
        let Some(action) = self.actions.get_mut(action_index) else {
            return;
        };
        let iw = self.base_image.width() as f64;
        let ih = self.base_image.height() as f64;
        if let AnnotationAction::Arrow {
            control_points: Some(handles),
            start,
            end,
            ..
        } = action
        {
            if handles.len() >= 3 {
                let clamp_point = |mut point: Point| {
                    point.x = point.x.max(0.0).min(iw);
                    point.y = point.y.max(0.0).min(ih);
                    point
                };
                match index {
                    0 => {
                        let clamped = clamp_point(new_pos);
                        *start = clamped;
                        handles[0] = clamped;
                        handles[1] = clamp_point(handles[1]);
                    }
                    1 => {
                        // new_pos is the desired on-curve midpoint B(0.5).
                        // Invert: P1 = 2*B(0.5) - 0.5*P0 - 0.5*P2
                        handles[1] = clamp_point(Point {
                            x: 2.0 * new_pos.x - 0.5 * handles[0].x - 0.5 * handles[2].x,
                            y: 2.0 * new_pos.y - 0.5 * handles[0].y - 0.5 * handles[2].y,
                        });
                    }
                    2 => {
                        let clamped = clamp_point(new_pos);
                        *end = clamped;
                        handles[2] = clamped;
                        handles[1] = clamp_point(handles[1]);
                    }
                    _ => {}
                }
            } else {
                match index {
                    0 => {
                        let mut clamped = new_pos;
                        clamped.x = clamped.x.max(0.0).min(iw);
                        clamped.y = clamped.y.max(0.0).min(ih);
                        *start = clamped;
                        handles[0] = clamped;
                    }
                    1 => {
                        let mut clamped = new_pos;
                        clamped.x = clamped.x.max(0.0).min(iw);
                        clamped.y = clamped.y.max(0.0).min(ih);
                        *end = clamped;
                        handles[1] = clamped;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn finalize_arrow_control_editing(&mut self) {
        self.arrow_editing_controls = false;
        self.arrow_control_dragging = None;
    }

    pub fn finalize_arrow_interaction_cleanup(&mut self) {
        self.clear_drag_without_rebuild();
        self.arrow_editing_controls = self
            .selected_action_index
            .and_then(|index| self.actions.get(index))
            .is_some_and(|action| matches!(action, AnnotationAction::Arrow { .. }));
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
}
