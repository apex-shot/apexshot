use super::super::composition::BackgroundComposition;
use super::super::selection::translate_action;
use super::super::types::{AnnotationAction, CropAspectRatio, CropHandle, Point, Rect};
use super::history::HistoryKind;
use super::EditorState;
use crate::capture::editor::text_detect::{spawn_text_detection, TextDetector};
#[cfg(test)]
use image::RgbaImage;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub(super) struct CropDrag {
    handle: CropHandle,
    start: Point,
    initial_rect: Option<Rect>,
}

impl EditorState {
    pub(super) fn canvas_bounds_for_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> (f64, f64, f64, f64) {
        let layout = BackgroundComposition::new(width as f64, height as f64)
            .with_style(self.background_style.clone())
            .with_padding(self.background_padding)
            .with_shadow(self.background_shadow)
            .with_shadow_profile(
                self.shadow_opacity,
                self.shadow_blur,
                self.shadow_offset_x,
                self.shadow_offset_y,
            )
            .with_insert(self.background_insert)
            .with_alignment(self.background_alignment)
            .with_corner_radius(self.background_corner_radius)
            .with_aspect_ratio(self.background_aspect_ratio)
            .with_frame_style(self.frame_style)
            .with_frame_border_thickness(self.border_thickness)
            .compute();
        let scale = layout.draw_scale.max(0.0001);
        (
            -layout.image_rect.x / scale,
            -layout.image_rect.y / scale,
            (layout.canvas_width - layout.image_rect.x) / scale,
            (layout.canvas_height - layout.image_rect.y) / scale,
        )
    }

    /// Begin creating, moving, or resizing the pending crop at an image point.
    pub fn begin_crop_drag(&mut self, point: Point, view_scale: f64) {
        let image_w = self.base_image.width() as f64;
        let image_h = self.base_image.height() as f64;
        if image_w < 2.0 || image_h < 2.0 {
            return;
        }
        let start = clamp_point(point, image_w, image_h);
        let initial_rect = self.crop_rect;
        let handle = initial_rect
            .and_then(|rect| crop_handle_at(rect, start, view_scale))
            .unwrap_or_else(|| {
                if initial_rect.is_some_and(|rect| {
                    point_in_rect(start, rect)
                        && (rect.x != 0
                            || rect.y != 0
                            || rect.width != image_w as i32
                            || rect.height != image_h as i32)
                }) {
                    CropHandle::Move
                } else {
                    CropHandle::Create
                }
            });
        self.crop_drag = Some(CropDrag {
            handle,
            start,
            initial_rect,
        });
    }

    /// Update the pending crop using bounded screenshot-pixel coordinates.
    pub fn update_crop_drag(&mut self, point: Point, shift_pressed: bool) {
        let Some(drag) = self.crop_drag else {
            return;
        };
        let image_w = self.base_image.width() as f64;
        let image_h = self.base_image.height() as f64;
        let current = clamp_point(point, image_w, image_h);
        let rect = match drag.handle {
            CropHandle::Create => {
                let aspect = self.crop_ratio.aspect_ratio(
                    self.base_image.width() as i32,
                    self.base_image.height() as i32,
                );
                rect_from_drag(
                    drag.start,
                    current,
                    if shift_pressed { Some(1.0) } else { aspect },
                    image_w,
                    image_h,
                )
            }
            CropHandle::Move => drag.initial_rect.map(|rect| {
                let dx = current.x - drag.start.x;
                let dy = current.y - drag.start.y;
                let x = (rect.x as f64 + dx).clamp(0.0, image_w - rect.width as f64);
                let y = (rect.y as f64 + dy).clamp(0.0, image_h - rect.height as f64);
                Rect {
                    x: x.round() as i32,
                    y: y.round() as i32,
                    ..rect
                }
            }),
            handle => drag.initial_rect.and_then(|rect| {
                resize_crop_rect(
                    rect,
                    handle,
                    current,
                    if shift_pressed {
                        Some(rect.width as f64 / rect.height.max(1) as f64)
                    } else {
                        self.crop_ratio.aspect_ratio(
                            self.base_image.width() as i32,
                            self.base_image.height() as i32,
                        )
                    },
                    image_w,
                    image_h,
                )
            }),
        };
        if let Some(rect) = rect.filter(|rect| rect.width >= 2 && rect.height >= 2) {
            self.crop_rect = Some(rect);
        }
    }

    /// Finish the current crop gesture without applying it.
    pub fn end_crop_drag(&mut self) {
        self.crop_drag = None;
    }

    /// Set the crop ratio and fit the pending rectangle to it.
    pub fn set_crop_ratio(&mut self, ratio: CropAspectRatio) {
        self.crop_ratio = ratio;
        let Some(rect) = self.crop_rect else {
            return;
        };
        let Some(aspect) = ratio.aspect_ratio(
            self.base_image.width() as i32,
            self.base_image.height() as i32,
        ) else {
            return;
        };
        let image_w = self.base_image.width() as f64;
        let image_h = self.base_image.height() as f64;
        let cx = rect.x as f64 + rect.width as f64 / 2.0;
        let cy = rect.y as f64 + rect.height as f64 / 2.0;
        let Some((width, height)) = fit_aspect(
            rect.width as f64,
            rect.height as f64,
            aspect,
            image_w,
            image_h,
        ) else {
            return;
        };
        self.crop_rect = rounded_crop_rect(
            cx - width / 2.0,
            cy - height / 2.0,
            width,
            height,
            image_w,
            image_h,
        );
    }

    /// Discard the pending crop selection without changing image history.
    pub fn cancel_crop(&mut self) {
        self.crop_rect = None;
        self.crop_drag = None;
    }

    /// Apply a non-empty crop and record an undoable document transaction.
    pub fn apply_pending_crop(&mut self) -> bool {
        let Some(rect) = self
            .crop_rect
            .and_then(|rect| rect.clamp_to(self.base_image.width(), self.base_image.height()))
        else {
            return false;
        };
        if rect.x == 0
            && rect.y == 0
            && rect.width == self.base_image.width() as i32
            && rect.height == self.base_image.height() as i32
        {
            self.cancel_crop();
            return false;
        }

        self.finish_history_interaction();
        self.cancel_text_input();
        self.cancel_text_edit();
        self.history_interaction_before = None;
        let before = self.document_snapshot_with_current_effects();
        let old_image = Arc::clone(&self.base_image);
        let old_width = old_image.width();
        let old_height = old_image.height();
        let cropped = image::imageops::crop_imm(
            old_image.as_ref(),
            rect.x as u32,
            rect.y as u32,
            rect.width as u32,
            rect.height as u32,
        )
        .to_image();
        let mut actions = Vec::with_capacity(self.actions.len());
        let canvas_bounds =
            self.canvas_bounds_for_dimensions(rect.width as u32, rect.height as u32);
        for mut action in self.actions.drain(..) {
            translate_action(&mut action, -(rect.x as f64), -(rect.y as f64));
            if let AnnotationAction::Obfuscate { rect, .. } | AnnotationAction::Focus { rect, .. } =
                &mut action
            {
                let Some(clipped) = rect.clamp_to(cropped.width(), cropped.height()) else {
                    continue;
                };
                *rect = clipped;
            }
            let Some(bounds) = super::super::selection::action_bounds_with_padding(&action, 0.0)
            else {
                continue;
            };
            if bounds.x as f64 >= canvas_bounds.2
                || bounds.y as f64 >= canvas_bounds.3
                || (bounds.x + bounds.width) as f64 <= canvas_bounds.0
                || (bounds.y + bounds.height) as f64 <= canvas_bounds.1
            {
                continue;
            }
            actions.push(action);
        }

        self.base_image = Arc::new(cropped);
        self.actions = actions;
        self.selected_action_index = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.clear_drag_without_rebuild();
        self.crop_rect = None;
        self.crop_drag = None;
        self.pending_effect_revision = self.pending_effect_revision.wrapping_add(1);
        self.last_applied_effect_revision = self.pending_effect_revision;
        self.select_effect_rebuild_pending = false;
        self.select_effect_rebuild_dirty = false;
        self.select_drag_effect_dirty = false;
        self.text_detector = Arc::new(Mutex::new(TextDetector::new_pending()));
        self.text_detection_ready = Arc::new(AtomicBool::new(false));
        self.text_detection_handle = None;
        self.text_detection_restart_pending = false;
        self.rebuild_effect_layer();
        self.commit_history_change(
            before,
            HistoryKind::Crop {
                rect,
                old_width,
                old_height,
            },
        );
        true
    }

    /// Start text detection against the current base image dimensions.
    pub fn restart_text_detection(&mut self) {
        let detector = Arc::clone(&self.text_detector);
        let ready = Arc::clone(&self.text_detection_ready);
        self.text_detection_handle = Some(spawn_text_detection(
            Arc::clone(&self.base_image),
            detector,
            ready,
        ));
        self.text_detection_restart_pending = false;
    }
}

fn clamp_point(point: Point, width: f64, height: f64) -> Point {
    Point {
        x: point.x.clamp(0.0, width),
        y: point.y.clamp(0.0, height),
    }
}

pub fn point_in_rect(point: Point, rect: Rect) -> bool {
    point.x >= rect.x as f64
        && point.x <= (rect.x + rect.width) as f64
        && point.y >= rect.y as f64
        && point.y <= (rect.y + rect.height) as f64
}

pub fn crop_handle_at(rect: Rect, point: Point, scale: f64) -> Option<CropHandle> {
    let x = rect.x as f64;
    let y = rect.y as f64;
    let right = x + rect.width as f64;
    let bottom = y + rect.height as f64;
    let mid_x = (x + right) / 2.0;
    let mid_y = (y + bottom) / 2.0;
    let radius = (9.0 / scale.max(0.1)).min(rect.width.min(rect.height) as f64 / 3.0);
    [
        (CropHandle::TopLeft, x, y),
        (CropHandle::Top, mid_x, y),
        (CropHandle::TopRight, right, y),
        (CropHandle::Left, x, mid_y),
        (CropHandle::Right, right, mid_y),
        (CropHandle::BottomLeft, x, bottom),
        (CropHandle::Bottom, mid_x, bottom),
        (CropHandle::BottomRight, right, bottom),
    ]
    .into_iter()
    .find_map(|(handle, hx, hy)| {
        ((point.x - hx).abs() <= radius && (point.y - hy).abs() <= radius).then_some(handle)
    })
}

fn rect_from_drag(
    start: Point,
    end: Point,
    aspect: Option<f64>,
    image_w: f64,
    image_h: f64,
) -> Option<Rect> {
    if let Some(aspect) = aspect {
        let direction_x = if end.x < start.x { -1.0 } else { 1.0 };
        let direction_y = if end.y < start.y { -1.0 } else { 1.0 };
        let max_w = if direction_x < 0.0 {
            start.x
        } else {
            image_w - start.x
        };
        let max_h = if direction_y < 0.0 {
            start.y
        } else {
            image_h - start.y
        };
        let (width, height) = fit_aspect(
            (end.x - start.x).abs(),
            (end.y - start.y).abs(),
            aspect,
            max_w,
            max_h,
        )?;
        let x = if direction_x < 0.0 {
            start.x - width
        } else {
            start.x
        };
        let y = if direction_y < 0.0 {
            start.y - height
        } else {
            start.y
        };
        return rounded_crop_rect(x, y, width, height, image_w, image_h);
    }
    Rect::from_points(start, end).and_then(|rect| rect.clamp_to(image_w as u32, image_h as u32))
}

fn fit_aspect(width: f64, height: f64, aspect: f64, max_w: f64, max_h: f64) -> Option<(f64, f64)> {
    let max_w = max_w.floor();
    let max_h = max_h.floor();
    if max_w < 2.0 || max_h < 2.0 {
        return None;
    }
    let width = width.max(2.0);
    let height = height.max(2.0);
    let desired_width = if width / height > aspect {
        height * aspect
    } else {
        width
    };
    let max_width_for_height = max_h * aspect;
    let max_width = max_w.min(max_width_for_height);
    let width = desired_width.min(max_width);
    let height = width / aspect;
    (width >= 2.0 && height >= 2.0).then_some((width, height))
}

fn rounded_crop_rect(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    image_w: f64,
    image_h: f64,
) -> Option<Rect> {
    let max_width = image_w.floor() as i32;
    let max_height = image_h.floor() as i32;
    if max_width < 2 || max_height < 2 {
        return None;
    }
    let width = (width.round() as i32).clamp(2, max_width);
    let height = (height.round() as i32).clamp(2, max_height);
    Some(Rect {
        x: (x.round() as i32).clamp(0, max_width - width),
        y: (y.round() as i32).clamp(0, max_height - height),
        width,
        height,
    })
}

fn resize_crop_rect(
    rect: Rect,
    handle: CropHandle,
    point: Point,
    aspect: Option<f64>,
    image_w: f64,
    image_h: f64,
) -> Option<Rect> {
    let left = rect.x as f64;
    let top = rect.y as f64;
    let right = left + rect.width as f64;
    let bottom = top + rect.height as f64;
    let mut x0 = left;
    let mut y0 = top;
    let mut x1 = right;
    let mut y1 = bottom;
    match handle {
        CropHandle::TopLeft => {
            x0 = point.x.min(right - 2.0);
            y0 = point.y.min(bottom - 2.0);
        }
        CropHandle::Top => y0 = point.y.min(bottom - 2.0),
        CropHandle::TopRight => {
            x1 = point.x.max(left + 2.0);
            y0 = point.y.min(bottom - 2.0);
        }
        CropHandle::Left => x0 = point.x.min(right - 2.0),
        CropHandle::Right => x1 = point.x.max(left + 2.0),
        CropHandle::BottomLeft => {
            x0 = point.x.min(right - 2.0);
            y1 = point.y.max(top + 2.0);
        }
        CropHandle::Bottom => y1 = point.y.max(top + 2.0),
        CropHandle::BottomRight => {
            x1 = point.x.max(left + 2.0);
            y1 = point.y.max(top + 2.0);
        }
        CropHandle::Move | CropHandle::Create => return None,
    }
    if let Some(aspect) = aspect {
        let (anchor_x, anchor_y, direction_x, direction_y, desired_w, desired_h, max_w, max_h) =
            match handle {
                CropHandle::TopLeft => (
                    right,
                    bottom,
                    -1.0,
                    -1.0,
                    right - point.x,
                    bottom - point.y,
                    right,
                    bottom,
                ),
                CropHandle::TopRight => (
                    left,
                    bottom,
                    1.0,
                    -1.0,
                    point.x - left,
                    bottom - point.y,
                    image_w - left,
                    bottom,
                ),
                CropHandle::BottomLeft => (
                    right,
                    top,
                    -1.0,
                    1.0,
                    right - point.x,
                    point.y - top,
                    right,
                    image_h - top,
                ),
                CropHandle::BottomRight => (
                    left,
                    top,
                    1.0,
                    1.0,
                    point.x - left,
                    point.y - top,
                    image_w - left,
                    image_h - top,
                ),
                CropHandle::Top => {
                    let center_x = (left + right) / 2.0;
                    let max_width = 2.0 * center_x.min(image_w - center_x);
                    (
                        center_x,
                        bottom,
                        0.0,
                        -1.0,
                        (bottom - point.y) * aspect,
                        bottom - point.y,
                        max_width,
                        bottom,
                    )
                }
                CropHandle::Bottom => {
                    let center_x = (left + right) / 2.0;
                    let max_width = 2.0 * center_x.min(image_w - center_x);
                    (
                        center_x,
                        top,
                        0.0,
                        1.0,
                        (point.y - top) * aspect,
                        point.y - top,
                        max_width,
                        image_h - top,
                    )
                }
                CropHandle::Left => {
                    let center_y = (top + bottom) / 2.0;
                    let max_height = 2.0 * center_y.min(image_h - center_y);
                    (
                        right,
                        center_y,
                        -1.0,
                        0.0,
                        right - point.x,
                        (right - point.x) / aspect,
                        right,
                        max_height,
                    )
                }
                CropHandle::Right => {
                    let center_y = (top + bottom) / 2.0;
                    let max_height = 2.0 * center_y.min(image_h - center_y);
                    (
                        left,
                        center_y,
                        1.0,
                        0.0,
                        point.x - left,
                        (point.x - left) / aspect,
                        image_w - left,
                        max_height,
                    )
                }
                CropHandle::Move | CropHandle::Create => return None,
            };
        let (width, height) = fit_aspect(desired_w, desired_h, aspect, max_w, max_h)?;
        let x = match direction_x {
            -1.0 => anchor_x - width,
            1.0 => anchor_x,
            _ => anchor_x - width / 2.0,
        };
        let y = match direction_y {
            -1.0 => anchor_y - height,
            1.0 => anchor_y,
            _ => anchor_y - height / 2.0,
        };
        return rounded_crop_rect(x, y, width, height, image_w, image_h);
    }

    x0 = x0.clamp(0.0, image_w - 2.0);
    y0 = y0.clamp(0.0, image_h - 2.0);
    x1 = x1.clamp(x0 + 2.0, image_w);
    y1 = y1.clamp(y0 + 2.0, image_h);
    Rect::from_bounds(x0, y0, x1, y1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::numbering_style::{NumberSize, NumberingStyle};
    use crate::capture::editor::types::{ArrowStyle, DrawColor, FontSettings, ObfuscateMethod};
    use crate::recording::editor::model::MotionState;

    fn assert_crop_rect(actual: Option<Rect>, expected: Rect) {
        let actual = actual.expect("crop rectangle");
        assert_eq!(
            (actual.x, actual.y, actual.width, actual.height),
            (expected.x, expected.y, expected.width, expected.height)
        );
    }

    #[test]
    fn activating_crop_shows_source_bounds_and_allows_drawing_a_region() {
        let mut state = EditorState::new(RgbaImage::new(200, 100));
        state.set_tool_without_rebuild(super::super::super::types::Tool::Crop);
        assert_crop_rect(
            state.crop_rect,
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        assert!(!state.history_availability().0);
        state.begin_crop_drag(Point { x: 40.0, y: 25.0 }, 1.0);
        state.update_crop_drag(Point { x: 120.0, y: 75.0 }, false);
        state.end_crop_drag();
        assert_crop_rect(
            state.crop_rect,
            Rect {
                x: 40,
                y: 25,
                width: 80,
                height: 50,
            },
        );
        state.set_tool_without_rebuild(super::super::super::types::Tool::Background);
        assert!(state.crop_rect.is_none());
        assert!(!state.history_availability().0);
    }

    #[test]
    fn crop_is_bounded_rebases_annotations_and_restores_full_snapshot() {
        let mut image = RgbaImage::new(10, 8);
        for y in 0..8 {
            for x in 0..10 {
                image.put_pixel(x, y, image::Rgba([x as u8, y as u8, 0, 255]));
            }
        }
        let mut state = EditorState::new(image);
        state.push_action(AnnotationAction::Text {
            position: Point { x: 6.0, y: 3.0 },
            text: "editable".into(),
            color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            font: FontSettings::default(),
            max_width: Some(20.0),
            shadow: false,
            background_color: None,
        });
        state.crop_rect = Some(Rect {
            x: 3,
            y: 2,
            width: 5,
            height: 4,
        });

        assert!(state.apply_pending_crop());
        assert_eq!(state.base_image.dimensions(), (5, 4));
        assert_eq!(state.base_image.get_pixel(0, 0).0, [3, 2, 0, 255]);
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Text { position, .. } if position.x == 3.0 && position.y == 1.0
        ));
        assert_eq!(state.to_motion_card_image().unwrap().dimensions(), (5, 4));
        assert!(state.undo());
        assert_eq!(state.base_image.dimensions(), (10, 8));
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Text { position, .. } if position.x == 6.0 && position.y == 3.0
        ));
        assert!(state.redo());
        assert_eq!(state.base_image.dimensions(), (5, 4));
    }

    #[test]
    fn crop_rebases_every_static_annotation_variant() {
        let mut state = EditorState::new(RgbaImage::new(40, 40));
        let color = DrawColor::new(0.8, 0.4, 0.2, 1.0);
        state.actions = vec![
            AnnotationAction::Pen {
                points: vec![Point { x: 12.0, y: 13.0 }, Point { x: 19.0, y: 20.0 }],
                color,
                stroke_size: 3.0,
            },
            AnnotationAction::Highlighter {
                points: vec![Point { x: 13.0, y: 14.0 }, Point { x: 18.0, y: 19.0 }],
                color,
                stroke_size: 5.0,
            },
            AnnotationAction::Circle {
                rect: Rect {
                    x: 12,
                    y: 11,
                    width: 6,
                    height: 5,
                },
                color,
                stroke_size: 2.0,
                shadow: false,
            },
            AnnotationAction::Line {
                start: Point { x: 13.0, y: 15.0 },
                end: Point { x: 18.0, y: 19.0 },
                color,
                stroke_size: 2.0,
                shadow: false,
            },
            AnnotationAction::Arrow {
                start: Point { x: 12.0, y: 12.0 },
                end: Point { x: 16.0, y: 17.0 },
                color,
                stroke_size: 2.0,
                style: ArrowStyle::Curved,
                control_points: Some(vec![
                    Point { x: 12.0, y: 12.0 },
                    Point { x: 14.0, y: 15.0 },
                    Point { x: 16.0, y: 17.0 },
                ]),
                shadow: false,
            },
            AnnotationAction::Box {
                rect: Rect {
                    x: 11,
                    y: 13,
                    width: 5,
                    height: 4,
                },
                color,
                stroke_size: 2.0,
                shadow: false,
            },
            AnnotationAction::Text {
                position: Point { x: 14.0, y: 18.0 },
                text: "editable".into(),
                color,
                font: FontSettings::default(),
                max_width: Some(12.0),
                shadow: false,
                background_color: None,
            },
            AnnotationAction::Number {
                position: Point { x: 16.0, y: 14.0 },
                number: 7,
                color,
                style: NumberingStyle::Numeric,
                size: NumberSize::default(),
                shadow: false,
            },
            AnnotationAction::Obfuscate {
                rect: Rect {
                    x: 12,
                    y: 13,
                    width: 8,
                    height: 7,
                },
                method: ObfuscateMethod::Blackout,
                amount: 10.0,
            },
            AnnotationAction::Focus {
                rect: Rect {
                    x: 20,
                    y: 20,
                    width: 8,
                    height: 8,
                },
                intensity: 40.0,
            },
        ];
        state.crop_rect = Some(Rect {
            x: 10,
            y: 10,
            width: 20,
            height: 20,
        });

        assert!(state.apply_pending_crop());
        assert_eq!(state.actions.len(), 10);
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Pen { points, .. } if points[0].x == 2.0 && points[0].y == 3.0
        ));
        assert!(matches!(
            &state.actions[1],
            AnnotationAction::Highlighter { points, .. } if points[0].x == 3.0 && points[0].y == 4.0
        ));
        assert!(matches!(
            &state.actions[2],
            AnnotationAction::Circle {
                rect: Rect { x: 2, y: 1, .. },
                ..
            }
        ));
        assert!(matches!(
            &state.actions[3],
            AnnotationAction::Line { start, .. } if start.x == 3.0 && start.y == 5.0
        ));
        assert!(matches!(
            &state.actions[4],
            AnnotationAction::Arrow { start, control_points: Some(points), .. }
                if start.x == 2.0 && start.y == 2.0 && points[1].x == 4.0 && points[1].y == 5.0
        ));
        assert!(matches!(
            &state.actions[5],
            AnnotationAction::Box {
                rect: Rect { x: 1, y: 3, .. },
                ..
            }
        ));
        assert!(matches!(
            &state.actions[6],
            AnnotationAction::Text { position, .. } if position.x == 4.0 && position.y == 8.0
        ));
        assert!(matches!(
            &state.actions[7],
            AnnotationAction::Number { position, .. } if position.x == 6.0 && position.y == 4.0
        ));
        assert!(matches!(
            &state.actions[8],
            AnnotationAction::Obfuscate {
                rect: Rect { x: 2, y: 3, .. },
                ..
            }
        ));
        assert!(matches!(
            &state.actions[9],
            AnnotationAction::Focus {
                rect: Rect { x: 10, y: 10, .. },
                ..
            }
        ));
    }

    #[test]
    fn crop_noop_and_cancel_do_not_change_history_or_pixels() {
        let mut state = EditorState::new(RgbaImage::from_pixel(6, 4, image::Rgba([3, 4, 5, 255])));
        state.crop_rect = Some(Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        });
        assert!(!state.apply_pending_crop());
        assert_eq!(state.history_availability(), (false, false));
        state.crop_rect = Some(Rect {
            x: 2,
            y: 1,
            width: 3,
            height: 2,
        });
        state.cancel_crop();
        assert_eq!(state.base_image.dimensions(), (6, 4));
        assert_eq!(state.history_availability(), (false, false));
    }

    #[test]
    fn crop_history_replaces_dimension_bound_text_detectors_and_requests_restarts() {
        let mut state = EditorState::new(RgbaImage::new(20, 12));
        state.crop_rect = Some(Rect {
            x: 2,
            y: 1,
            width: 14,
            height: 9,
        });
        assert!(state.apply_pending_crop());
        let cropped_detector = Arc::clone(&state.text_detector);

        assert!(state.undo());
        assert!(!Arc::ptr_eq(&cropped_detector, &state.text_detector));
        assert!(state.take_text_detection_restart());
        let restored_detector = Arc::clone(&state.text_detector);

        assert!(state.redo());
        assert!(!Arc::ptr_eq(&restored_detector, &state.text_detector));
        assert!(state.take_text_detection_restart());
    }

    #[test]
    fn crop_drag_creates_moves_and_resizes_inside_source_bounds() {
        let mut state = EditorState::new(RgbaImage::new(10, 8));
        state.begin_crop_drag(Point { x: 2.0, y: 2.0 }, 1.0);
        state.update_crop_drag(Point { x: 8.0, y: 6.0 }, false);
        state.end_crop_drag();
        assert_crop_rect(
            state.crop_rect,
            Rect {
                x: 2,
                y: 2,
                width: 6,
                height: 4,
            },
        );

        state.begin_crop_drag(Point { x: 5.0, y: 4.0 }, 1.0);
        state.update_crop_drag(Point { x: 9.0, y: 7.0 }, false);
        state.end_crop_drag();
        assert_crop_rect(
            state.crop_rect,
            Rect {
                x: 4,
                y: 4,
                width: 6,
                height: 4,
            },
        );

        state.begin_crop_drag(Point { x: 4.0, y: 4.0 }, 1.0);
        state.update_crop_drag(Point { x: 1.0, y: 1.0 }, false);
        state.end_crop_drag();
        assert_crop_rect(
            state.crop_rect,
            Rect {
                x: 1,
                y: 1,
                width: 9,
                height: 7,
            },
        );
    }

    #[test]
    fn locked_ratio_resize_stays_bounded_for_every_handle_at_image_edges() {
        let rect = Rect {
            x: 20,
            y: 20,
            width: 20,
            height: 20,
        };
        let cases = [
            (CropHandle::TopLeft, Point { x: 0.0, y: 0.0 }),
            (CropHandle::Top, Point { x: 50.0, y: 0.0 }),
            (CropHandle::TopRight, Point { x: 100.0, y: 0.0 }),
            (CropHandle::Left, Point { x: 0.0, y: 30.0 }),
            (CropHandle::Right, Point { x: 100.0, y: 30.0 }),
            (CropHandle::BottomLeft, Point { x: 0.0, y: 60.0 }),
            (CropHandle::Bottom, Point { x: 50.0, y: 60.0 }),
            (CropHandle::BottomRight, Point { x: 100.0, y: 60.0 }),
        ];
        for (handle, point) in cases {
            let resized = resize_crop_rect(rect, handle, point, Some(1.0), 100.0, 60.0)
                .expect("bounded ratio rectangle");
            assert!(resized.width >= 2 && resized.height >= 2);
            assert!(resized.x >= 0 && resized.y >= 0);
            assert!(resized.x + resized.width <= 100);
            assert!(resized.y + resized.height <= 60);
            assert!((resized.width - resized.height).abs() <= 1);
        }
    }

    #[test]
    fn selecting_ratio_near_image_edges_keeps_rounded_bounds_inside_source() {
        let mut state = EditorState::new(RgbaImage::new(101, 61));
        state.crop_rect = Some(Rect {
            x: 80,
            y: 40,
            width: 21,
            height: 21,
        });
        state.set_crop_ratio(CropAspectRatio::SixteenNine);
        let rect = state.crop_rect.expect("ratio rectangle");
        assert!(rect.width >= 2 && rect.height >= 2);
        assert!(rect.x >= 0 && rect.y >= 0);
        assert!(rect.x + rect.width <= 101);
        assert!(rect.y + rect.height <= 61);
        assert!((rect.width as f64 / rect.height as f64 - 16.0 / 9.0).abs() < 0.1);
    }

    #[test]
    fn crop_filters_effects_to_screenshot_and_keeps_partial_overlap() {
        let mut state = EditorState::new(RgbaImage::from_pixel(
            12,
            8,
            image::Rgba([220, 220, 220, 255]),
        ));
        state.actions.push(AnnotationAction::Focus {
            rect: Rect {
                x: 2,
                y: 1,
                width: 7,
                height: 5,
            },
            intensity: 50.0,
        });
        state.actions.push(AnnotationAction::Obfuscate {
            rect: Rect {
                x: 10,
                y: 0,
                width: 2,
                height: 4,
            },
            method: crate::capture::editor::types::ObfuscateMethod::Blackout,
            amount: 10.0,
        });
        state.crop_rect = Some(Rect {
            x: 4,
            y: 0,
            width: 5,
            height: 7,
        });
        assert!(state.apply_pending_crop());
        assert_eq!(state.actions.len(), 1);
        assert!(matches!(
            state.actions[0],
            AnnotationAction::Focus {
                rect: Rect { x: 0, width: 5, .. },
                ..
            }
        ));
    }

    #[test]
    fn annotation_history_remains_ordered_around_crop_snapshots() {
        let mut state = EditorState::new(RgbaImage::new(24, 16));
        state.push_action(AnnotationAction::Line {
            start: Point { x: 2.0, y: 2.0 },
            end: Point { x: 10.0, y: 10.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        state.crop_rect = Some(Rect {
            x: 4,
            y: 3,
            width: 16,
            height: 10,
        });
        assert!(state.apply_pending_crop());
        state.push_action(AnnotationAction::Box {
            rect: Rect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            color: DrawColor::new(0.0, 1.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });

        assert!(state.undo());
        assert_eq!(state.actions.len(), 1);
        assert!(state.undo());
        assert_eq!(state.base_image.dimensions(), (24, 16));
        assert!(matches!(state.actions[0], AnnotationAction::Line { .. }));
        assert!(state.redo());
        assert_eq!(state.base_image.dimensions(), (16, 10));
        assert!(state.redo());
        assert_eq!(state.actions.len(), 2);
    }

    #[test]
    fn loaded_annotations_keep_redo_order_across_crop_history() {
        let mut state = EditorState::new(RgbaImage::new(24, 16));
        state.actions.push(AnnotationAction::Line {
            start: Point { x: 6.0, y: 4.0 },
            end: Point { x: 18.0, y: 12.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        state.crop_rect = Some(Rect {
            x: 4,
            y: 2,
            width: 16,
            height: 12,
        });
        assert!(state.apply_pending_crop());

        assert!(state.undo());
        assert_eq!(state.base_image.dimensions(), (24, 16));
        assert!(state.undo());
        assert!(state.actions.is_empty());
        assert!(state.redo());
        assert!(matches!(state.actions[0], AnnotationAction::Line { .. }));
        assert_eq!(state.base_image.dimensions(), (24, 16));
        assert!(state.redo());
        assert_eq!(state.base_image.dimensions(), (16, 12));
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Line { start, .. } if start.x == 2.0 && start.y == 2.0
        ));
    }

    #[test]
    fn deletion_after_undo_discards_stale_crop_redo_without_losing_document_state() {
        let mut state = EditorState::new(RgbaImage::new(20, 12));
        state.push_action(AnnotationAction::Line {
            start: Point { x: 2.0, y: 2.0 },
            end: Point { x: 8.0, y: 8.0 },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        state.crop_rect = Some(Rect {
            x: 2,
            y: 1,
            width: 14,
            height: 9,
        });
        assert!(state.apply_pending_crop());
        assert!(state.undo());
        state.selected_action_index = Some(0);
        assert!(state.remove_selected_action_without_rebuild());
        assert!(!state.history_availability().1);
        assert_eq!(state.base_image.dimensions(), (20, 12));
        assert!(state.actions.is_empty());
    }

    #[test]
    fn action_style_edits_are_ordered_after_crop_history() {
        let mut state = EditorState::new(RgbaImage::new(20, 12));
        state.push_action(AnnotationAction::Box {
            rect: Rect {
                x: 6,
                y: 3,
                width: 8,
                height: 5,
            },
            color: DrawColor::new(1.0, 0.0, 0.0, 1.0),
            stroke_size: 2.0,
            shadow: false,
        });
        state.crop_rect = Some(Rect {
            x: 2,
            y: 1,
            width: 14,
            height: 9,
        });
        assert!(state.apply_pending_crop());
        state.selected_action_index = Some(0);
        assert!(state.set_selected_action_color(DrawColor::new(0.0, 1.0, 0.0, 1.0)));

        assert!(state.undo());
        assert_eq!(state.base_image.dimensions(), (14, 9));
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Box { color: DrawColor { r, g, .. }, .. } if *r == 1.0 && *g == 0.0
        ));
        assert!(state.undo());
        assert_eq!(state.base_image.dimensions(), (20, 12));
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Box {
                rect: Rect { x: 6, y: 3, .. },
                ..
            }
        ));
        assert!(state.redo());
        assert!(state.redo());
        assert!(matches!(
            &state.actions[0],
            AnnotationAction::Box { color: DrawColor { r, g, .. }, .. } if *r == 0.0 && *g == 1.0
        ));
    }

    #[test]
    fn crop_transaction_carries_exact_motion_state_for_undo_and_redo() {
        let mut state = EditorState::new(RgbaImage::new(20, 12));
        let before = MotionState::default();
        let mut after = before.clone();
        after.watermark.position = (0.2, 0.7);
        state.crop_rect = Some(Rect {
            x: 2,
            y: 1,
            width: 14,
            height: 9,
        });
        assert!(state.apply_pending_crop());
        state.attach_motion_history_to_latest_crop(before.clone(), after.clone());
        assert!(state.undo());
        let undo = state.take_motion_history_state().expect("motion crop undo");
        assert!(undo.undo);
        assert_eq!(undo.before, before);
        assert_eq!(undo.after, after);
        assert_eq!(
            undo.rect,
            Rect {
                x: 2,
                y: 1,
                width: 14,
                height: 9
            }
        );
        assert!(state.redo());
        let redo = state.take_motion_history_state().expect("motion crop redo");
        assert!(!redo.undo);
        assert_eq!(redo.before, before);
        assert_eq!(redo.after, after);
    }
}
