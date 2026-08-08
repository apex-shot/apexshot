use super::super::pen_weight::HighlighterMode;
use super::super::selection::{action_bounds_with_padding, translate_action};
use super::super::types::{AnnotationAction, ArrowStyle, Point, Rect, Tool};
use super::{expand_rgba_image, simplify_drag_path, EditorState};
use std::sync::Arc;

impl EditorState {
    pub(super) fn current_highlighter_stroke_size(&self) -> f64 {
        self.locked_highlighter_stroke_size
            .unwrap_or_else(|| match self.highlighter_mode {
                HighlighterMode::TextAware => self.stroke_size,
                HighlighterMode::Freehand => self.pen_weight.highlighter_stroke_width(),
            })
    }

    pub(super) fn current_pen_stroke_size(&self) -> f64 {
        self.pen_weight.pen_stroke_width()
    }

    pub fn begin_drag(&mut self, point: Point) {
        self.selected_action_index = None;
        self.drag_start = Some(point);
        self.drag_current = Some(point);
        self.drag_path.clear();
        self.locked_highlighter_stroke_size = None;
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter) {
            self.drag_path.push(point);
        }
        if self.selected_tool == Tool::Highlighter {
            self.locked_highlighter_stroke_size = Some(self.current_highlighter_stroke_size());
            // In TextAware mode, also lock the detected text height at the drag start point
            // so the stroke size matches what the cursor was showing
            if self.highlighter_mode == HighlighterMode::TextAware {
                if let Ok(detector) = self.text_detector.lock() {
                    if detector.is_ready() {
                        if let Some(text_height) = detector.best_text_height_at_point(point) {
                            self.locked_highlighter_stroke_size = Some(text_height);
                        }
                    }
                }
            }
        }
    }

    pub fn update_drag(&mut self, point: Point) {
        self.drag_current = Some(point);
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter)
            && self
                .drag_path
                .last()
                .map(|last| (last.x - point.x).abs() > 0.1 || (last.y - point.y).abs() > 0.1)
                .unwrap_or(true)
        {
            self.drag_path.push(point);
        }
    }

    pub fn clear_drag(&mut self) -> bool {
        let rebuild = self.clear_drag_without_rebuild_and_check_effect();
        if rebuild {
            self.rebuild_effect_layer();
        }
        rebuild
    }

    pub fn clear_drag_without_rebuild(&mut self) {
        self.drag_start = None;
        self.drag_current = None;
        self.drag_start_view = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.arrow_control_dragging = None;
        self.drag_path.clear();
        self.drag_shift_active = false;
        self.locked_highlighter_stroke_size = None;
    }

    pub fn clear_drag_without_rebuild_and_check_effect(&mut self) -> bool {
        let rebuild = self.select_effect_rebuild_pending;
        self.select_effect_rebuild_pending = false;
        self.clear_drag_without_rebuild();
        rebuild
    }

    pub fn draft_action(&self) -> Option<AnnotationAction> {
        let start = self.drag_start?;
        let end = super::super::types::constrained_drag_endpoint(
            self.selected_tool,
            start,
            self.drag_current?,
            self.drag_shift_active,
        );
        let color = self.selected_color;
        let stroke_size = self.stroke_size;

        match self.selected_tool {
            Tool::Select => None,
            Tool::Crop => None,
            Tool::Background => None,
            Tool::Pen => {
                // Skip Douglas–Peucker simplification for the in-progress draft.
                // Simplification is O(n log n) (worst-case O(n²)) and runs on every
                // redraw, which adds visible lag for long pen strokes. The raw path
                // already de-duplicates points within 0.1px in `update_drag`, so the
                // draft renders identically without the per-frame work. The full
                // simplification still runs once in `finalize_drag_action`.
                let points = self.drag_path.clone();
                if points.len() >= 2 {
                    Some(AnnotationAction::Pen {
                        points,
                        color,
                        stroke_size: self.current_pen_stroke_size(),
                    })
                } else {
                    None
                }
            }
            Tool::Highlighter => {
                // See note above on Tool::Pen – skip simplification for the draft.
                let source_points = self.drag_path.clone();
                if source_points.len() >= 2 {
                    let points = if self.drag_shift_active {
                        let first = source_points[0];
                        let last = source_points[source_points.len() - 1];
                        vec![
                            first,
                            super::super::types::constrained_drag_endpoint(
                                Tool::Highlighter,
                                first,
                                last,
                                true,
                            ),
                        ]
                    } else {
                        source_points
                    };

                    Some(AnnotationAction::Highlighter {
                        points,
                        color,
                        stroke_size: self.current_highlighter_stroke_size(),
                    })
                } else {
                    None
                }
            }
            Tool::Circle => Rect::from_points(start, end).map(|rect| AnnotationAction::Circle {
                rect,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Line => Some(AnnotationAction::Line {
                start,
                end,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Arrow => {
                let (start, end) = self.arrow_points(start, end);
                // Reject zero-length arrows (clicks without dragging).
                if (start.x - end.x).abs() < 0.5 && (start.y - end.y).abs() < 0.5 {
                    None
                } else {
                    Some(AnnotationAction::Arrow {
                        start,
                        end,
                        color,
                        stroke_size,
                        style: self.arrow_style,
                        control_points: None,
                        shadow: self.draw_object_shadow,
                    })
                }
            }
            Tool::Box => Rect::from_points(start, end).map(|rect| AnnotationAction::Box {
                rect,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Number => None,
            Tool::Obfuscate => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Obfuscate {
                    rect,
                    method: self.obfuscate_method,
                    amount: self.current_obfuscate_amount(),
                })
            }
            Tool::Focus => Rect::from_points(start, end).map(|rect| AnnotationAction::Focus {
                rect,
                intensity: self.current_focus_intensity(),
            }),
            Tool::Text => None,
        }
    }

    pub fn finalize_drag_action(&mut self) -> Option<AnnotationAction> {
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter) {
            let drag_path = std::mem::take(&mut self.drag_path);
            let mut points = self.processed_drag_path(drag_path);
            let color = self.selected_color;
            let tool = self.selected_tool;
            let shift_active = self.drag_shift_active;
            let pen_stroke_size = self.current_pen_stroke_size();
            let highlighter_stroke_size = if tool == Tool::Highlighter {
                Some(self.current_highlighter_stroke_size())
            } else {
                None
            };
            self.clear_drag();
            return if points.len() >= 2 {
                match tool {
                    Tool::Pen => Some(AnnotationAction::Pen {
                        points,
                        color,
                        stroke_size: pen_stroke_size,
                    }),
                    Tool::Highlighter => {
                        if shift_active {
                            let first = points[0];
                            let last = points[points.len() - 1];
                            let constrained_last = super::super::types::constrained_drag_endpoint(
                                Tool::Highlighter,
                                first,
                                last,
                                true,
                            );
                            points = vec![first, constrained_last];
                        }

                        let stroke_size = highlighter_stroke_size
                            .unwrap_or_else(|| self.pen_weight.highlighter_stroke_width());

                        if points.len() >= 2
                            && ((points[0].x - points[1].x).abs() > 0.1
                                || (points[0].y - points[1].y).abs() > 0.1)
                        {
                            Some(AnnotationAction::Highlighter {
                                points,
                                color,
                                stroke_size,
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };
        }

        let start = self.drag_start?;
        let end = super::super::types::constrained_drag_endpoint(
            self.selected_tool,
            start,
            self.drag_current?,
            self.drag_shift_active,
        );
        let color = self.selected_color;
        let stroke_size = self.stroke_size;
        self.clear_drag();

        let mut result = match self.selected_tool {
            Tool::Select => None,
            Tool::Crop => None,
            Tool::Background => None,
            Tool::Pen => None,
            Tool::Highlighter => None,
            Tool::Circle => Rect::from_points(start, end).map(|rect| AnnotationAction::Circle {
                rect,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Line => Some(AnnotationAction::Line {
                start,
                end,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Arrow => {
                let (start, end) = self.arrow_points(start, end);
                // Reject zero-length arrows (clicks without dragging).
                if (start.x - end.x).abs() < 0.5 && (start.y - end.y).abs() < 0.5 {
                    None
                } else {
                    Some(AnnotationAction::Arrow {
                        start,
                        end,
                        color,
                        stroke_size,
                        style: self.arrow_style,
                        control_points: None,
                        shadow: self.draw_object_shadow,
                    })
                }
            }
            Tool::Box => Rect::from_points(start, end).map(|rect| AnnotationAction::Box {
                rect,
                color,
                stroke_size,
                shadow: self.draw_object_shadow,
            }),
            Tool::Number => None,
            Tool::Obfuscate => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Obfuscate {
                    rect,
                    method: self.obfuscate_method,
                    amount: self.current_obfuscate_amount(),
                })
            }
            Tool::Focus => Rect::from_points(start, end).map(|rect| AnnotationAction::Focus {
                rect,
                intensity: self.current_focus_intensity(),
            }),
            Tool::Text => None,
        };

        // For all arrows, initialize control handles after finalize
        if let Some(AnnotationAction::Arrow {
            style,
            control_points,
            start,
            end,
            ..
        }) = result.as_mut()
        {
            match style {
                ArrowStyle::Curved | ArrowStyle::Double => {
                    let mid = Point {
                        x: (start.x + end.x) / 2.0,
                        y: (start.y + end.y) / 2.0,
                    };
                    *control_points = Some(vec![*start, mid, *end]);
                }
                _ => {
                    *control_points = Some(vec![*start, *end]);
                }
            }
            self.arrow_editing_controls = true;
        }

        result
    }

    pub(super) fn arrow_points(&self, start: Point, end: Point) -> (Point, Point) {
        if self.inverse_arrow_direction {
            (end, start)
        } else {
            (start, end)
        }
    }

    pub(super) fn processed_drag_path(&self, points: Vec<Point>) -> Vec<Point> {
        if !self.smooth_drawing_enabled {
            return points;
        }

        // Light simplification only: large epsilons make strokes jump to
        // angular polylines on mouse-up after a smooth draft. Keep enough
        // points that midpoint curve smoothing still looks continuous.
        let epsilon = (self.current_pen_stroke_size() * 0.08).clamp(0.25, 0.55);
        simplify_drag_path(&points, epsilon)
    }

    pub(super) fn expand_canvas_for_action_if_needed(&mut self, action: &mut AnnotationAction) {
        if !self.auto_expand_canvas {
            return;
        }

        let Some(bounds) = action_bounds_with_padding(action, 0.0) else {
            return;
        };

        let left_padding = (-bounds.x).max(0);
        let top_padding = (-bounds.y).max(0);
        let right_edge = (bounds.x + bounds.width).max(self.working_image.width() as i32);
        let bottom_edge = (bounds.y + bounds.height).max(self.working_image.height() as i32);
        let new_width = (right_edge + left_padding).max(self.working_image.width() as i32);
        let new_height = (bottom_edge + top_padding).max(self.working_image.height() as i32);

        let expand_left = left_padding.max(0) as u32;
        let expand_top = top_padding.max(0) as u32;
        let next_width = new_width.max(1) as u32;
        let next_height = new_height.max(1) as u32;

        if next_width == self.working_image.width()
            && next_height == self.working_image.height()
            && expand_left == 0
            && expand_top == 0
        {
            return;
        }

        self.base_image = Arc::new(expand_rgba_image(
            &self.base_image,
            next_width,
            next_height,
            expand_left,
            expand_top,
        ));
        self.working_image = Arc::new(expand_rgba_image(
            &self.working_image,
            next_width,
            next_height,
            expand_left,
            expand_top,
        ));

        if expand_left > 0 || expand_top > 0 {
            let dx = expand_left as f64;
            let dy = expand_top as f64;

            for existing in &mut self.actions {
                translate_action(existing, dx, dy);
            }
            translate_action(action, dx, dy);

            if let Some(crop) = self.crop_selection.as_mut() {
                crop.x += expand_left as i32;
                crop.y += expand_top as i32;
            }

            if let Some(bounds) = self.active_text_bounds.as_mut() {
                bounds.rect.x += expand_left as i32;
                bounds.rect.y += expand_top as i32;
                bounds.sync_handles();
            }
        }

        self.mark_working_image_dirty();
    }
}
