use super::super::color::{selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale};
use super::super::selection::{
    action_contains_point_with_padding, action_resize_handle_at_point_with_radius, resize_action,
    translate_action,
};
use super::super::types::{AnnotationAction, Point};
use super::EditorState;

impl EditorState {
    pub fn can_remove_selected_action(&self) -> bool {
        self.selected_action_index
            .is_some_and(|index| index < self.actions.len())
    }

    pub fn selected_action(&self) -> Option<&AnnotationAction> {
        self.selected_action_index
            .and_then(|index| self.actions.get(index))
    }

    pub fn select_action_at_point_with_scale(&mut self, point: Point, view_scale: f64) -> bool {
        let hit_padding = selection_hit_padding_for_scale(view_scale);

        self.selected_action_index = self
            .actions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, action)| action_contains_point_with_padding(action, point, hit_padding))
            .map(|(index, _)| index);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.selected_action_index.is_some()
    }

    pub fn begin_select_drag_with_scale(&mut self, point: Point, view_scale: f64) -> bool {
        let handle_hit_radius = selection_handle_hit_radius_for_scale(view_scale);

        if let Some(selected) = self.selected_action() {
            if let Some(handle) =
                action_resize_handle_at_point_with_radius(selected, point, handle_hit_radius)
            {
                self.select_resize_handle = Some(handle);
                self.select_drag_anchor = Some(point);
                return true;
            }
        }

        self.select_resize_handle = None;
        let selected = self.select_action_at_point_with_scale(point, view_scale);
        if selected {
            self.select_drag_anchor = Some(point);
        }
        selected
    }

    pub fn update_select_drag(&mut self, point: Point) -> bool {
        let Some(anchor) = self.select_drag_anchor else {
            return false;
        };
        let Some(index) = self.selected_action_index else {
            return false;
        };

        let dx = point.x - anchor.x;
        let dy = point.y - anchor.y;
        let img_w = self.base_image.width() as i32;
        let img_h = self.base_image.height() as i32;
        let resize_handle = self.select_resize_handle;
        let (moved, effect_action) = if let Some(action) = self.actions.get_mut(index) {
            let moved = if let Some(handle) = resize_handle {
                resize_action(action, handle, dx, dy)
            } else {
                translate_action(action, dx, dy)
            };

            if moved {
                clamp_action_to_image(action, img_w, img_h);
            }

            let effect_action = matches!(
                action,
                AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
            );
            (moved, effect_action)
        } else {
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            return false;
        };

        if !moved {
            return false;
        }

        self.select_drag_anchor = Some(point);
        self.redo_actions.clear();
        if effect_action {
            self.select_drag_effect_dirty = true;
        }
        true
    }

    #[cfg(test)]
    pub fn end_select_drag(&mut self) -> bool {
        let rebuild = self.select_drag_effect_dirty;
        if rebuild {
            self.rebuild_effect_layer();
            self.select_drag_effect_dirty = false;
        }
        self.end_select_drag_without_rebuild();
        rebuild
    }

    pub fn end_select_drag_without_rebuild(&mut self) {
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.drag_start = None;
        self.drag_current = None;
        self.drag_start_view = None;
        self.drag_path.clear();
    }

    pub fn end_select_drag_without_rebuild_and_check_effect(&mut self) -> bool {
        let rebuild = self.select_drag_effect_dirty;
        self.select_drag_effect_dirty = false;
        self.end_select_drag_without_rebuild();
        rebuild
    }

    pub fn remove_selected_action(&mut self) -> bool {
        if self.remove_selected_action_without_rebuild() {
            self.rebuild_effect_layer();
            true
        } else {
            false
        }
    }

    pub fn remove_selected_action_without_rebuild(&mut self) -> bool {
        let Some(index) = self.selected_action_index.take() else {
            return false;
        };

        if index >= self.actions.len() {
            return false;
        }

        let removed = self.actions.remove(index);
        let next_number_after_remove = match &removed {
            AnnotationAction::Number { number, style, .. } if *style == self.numbering_style => {
                Some(*number)
            }
            _ => None,
        };
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.redo_actions.clear();
        if let Some(next_number) = next_number_after_remove {
            self.next_number = next_number;
        } else {
            self.sync_next_number();
        }
        true
    }
}

/// Clamp an annotation action so it stays within the image bounds.
/// For rect-based actions (Obfuscate, Focus, Box, Circle) the rect is clamped.
/// For point-based actions (Text, Number, Pen, Arrow, Line) each point is clamped.
fn clamp_action_to_image(action: &mut AnnotationAction, img_w: i32, img_h: i32) {
    match action {
        AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect, .. }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Circle { rect, .. } => {
            let w = rect.width.min(img_w);
            let h = rect.height.min(img_h);
            rect.width = w;
            rect.height = h;
            rect.x = rect.x.max(0).min(img_w - w);
            rect.y = rect.y.max(0).min(img_h - h);
        }
        AnnotationAction::Text {
            position,
            text,
            font,
            max_width,
            ..
        } => {
            let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1)
            {
                Ok(s) => s,
                Err(_) => return,
            };
            let context = match gtk4::cairo::Context::new(&surface) {
                Ok(c) => c,
                Err(_) => return,
            };
            let available_width = max_width.unwrap_or(font.size * 1.8).max(font.size * 1.8);
            let bounds = super::super::render::text_action_bounds(
                &context,
                *position,
                text,
                font,
                Some(available_width),
            );
            let box_w = bounds.rect.width as f64;
            let box_h = bounds.rect.height as f64;
            let new_box_left = (bounds.rect.x as f64)
                .max(0.0)
                .min((img_w as f64 - box_w).max(0.0));
            position.x = new_box_left;
            let padding_y = 8.0;
            let new_box_top = (bounds.rect.y as f64)
                .max(0.0)
                .min((img_h as f64 - box_h).max(0.0));
            position.y = new_box_top + font.size + padding_y;
        }
        AnnotationAction::Number { position, .. } => {
            position.x = position.x.max(0.0).min(img_w as f64);
            position.y = position.y.max(0.0).min(img_h as f64);
        }
        AnnotationAction::Pen { points, .. } | AnnotationAction::Highlighter { points, .. } => {
            for p in points {
                p.x = p.x.max(0.0).min(img_w as f64);
                p.y = p.y.max(0.0).min(img_h as f64);
            }
        }
        AnnotationAction::Line { start, end, .. } => {
            start.x = start.x.max(0.0).min(img_w as f64);
            start.y = start.y.max(0.0).min(img_h as f64);
            end.x = end.x.max(0.0).min(img_w as f64);
            end.y = end.y.max(0.0).min(img_h as f64);
        }
        AnnotationAction::Arrow {
            start,
            end,
            control_points,
            stroke_size,
            ..
        } => {
            let iw = img_w as f64;
            let ih = img_h as f64;
            let margin = *stroke_size * 0.5;
            let mut min_x = start.x.min(end.x);
            let mut max_x = start.x.max(end.x);
            let mut min_y = start.y.min(end.y);
            let mut max_y = start.y.max(end.y);

            if let Some(cps) = control_points.as_ref() {
                if cps.len() >= 3 {
                    let p0 = *start;
                    let p1 = cps[1];
                    let p2 = *end;
                    let denom_x = p0.x - 2.0 * p1.x + p2.x;
                    if denom_x.abs() > 1e-10 {
                        let t = (p0.x - p1.x) / denom_x;
                        if t > 0.0 && t < 1.0 {
                            let bx = (1.0 - t).powi(2) * p0.x
                                + 2.0 * (1.0 - t) * t * p1.x
                                + t.powi(2) * p2.x;
                            min_x = min_x.min(bx);
                            max_x = max_x.max(bx);
                        }
                    }
                    let denom_y = p0.y - 2.0 * p1.y + p2.y;
                    if denom_y.abs() > 1e-10 {
                        let t = (p0.y - p1.y) / denom_y;
                        if t > 0.0 && t < 1.0 {
                            let by = (1.0 - t).powi(2) * p0.y
                                + 2.0 * (1.0 - t) * t * p1.y
                                + t.powi(2) * p2.y;
                            min_y = min_y.min(by);
                            max_y = max_y.max(by);
                        }
                    }
                }
            }

            let shift_x = if min_x < margin {
                margin - min_x
            } else if max_x > iw - margin {
                (iw - margin) - max_x
            } else {
                0.0
            };
            let shift_y = if min_y < margin {
                margin - min_y
            } else if max_y > ih - margin {
                (ih - margin) - max_y
            } else {
                0.0
            };
            if shift_x != 0.0 || shift_y != 0.0 {
                start.x += shift_x;
                start.y += shift_y;
                end.x += shift_x;
                end.y += shift_y;
                if let Some(cps) = control_points.as_mut() {
                    for cp in cps.iter_mut() {
                        cp.x += shift_x;
                        cp.y += shift_y;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::color::{DRAW_COLORS, STROKE_WIDTH};
    use crate::capture::editor::types::{AnnotationAction, Point, Rect};

    use super::EditorState;

    #[test]
    fn can_remove_selected_action_requires_valid_selection() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.push_action(AnnotationAction::Box {
            rect: Rect {
                x: 4,
                y: 4,
                width: 10,
                height: 10,
            },
            color: DRAW_COLORS[0],
            stroke_size: STROKE_WIDTH,
            shadow: false,
        });

        assert!(state.can_remove_selected_action());
        state.selected_action_index = None;
        assert!(!state.can_remove_selected_action());
        state.selected_action_index = Some(9);
        assert!(!state.can_remove_selected_action());
    }

    #[test]
    fn selection_drag_marks_effect_actions_dirty_and_clamps_to_image() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.push_action(AnnotationAction::Obfuscate {
            rect: Rect {
                x: 20,
                y: 20,
                width: 8,
                height: 8,
            },
            method: crate::capture::editor::types::ObfuscateMethod::Pixelate,
            amount: 8.0,
        });

        state.select_drag_anchor = Some(Point { x: 24.0, y: 24.0 });
        assert!(state.update_select_drag(Point { x: 44.0, y: 44.0 }));
        assert!(state.select_drag_effect_dirty);
        assert!(state.end_select_drag());

        match &state.actions[0] {
            AnnotationAction::Obfuscate { rect, .. } => {
                assert_eq!(rect.x, 24);
                assert_eq!(rect.y, 24);
            }
            other => panic!("expected obfuscate action, got {other:?}"),
        }
    }

    #[test]
    fn selection_prefers_the_topmost_matching_action() {
        let mut state = EditorState::new(RgbaImage::new(64, 64));
        for (x, color) in [(8, DRAW_COLORS[0]), (14, DRAW_COLORS[1])] {
            state.push_action(AnnotationAction::Box {
                rect: Rect {
                    x,
                    y: x,
                    width: 22,
                    height: 22,
                },
                color,
                stroke_size: STROKE_WIDTH,
                shadow: false,
            });
        }

        assert!(state.select_action_at_point_with_scale(Point { x: 20.0, y: 20.0 }, 1.0));
        assert_eq!(state.selected_action_index, Some(1));
    }
}
