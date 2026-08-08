use super::super::color::{
    selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale, SELECT_MIN_RESIZE_SIZE,
};
use super::super::selection::{
    action_contains_point_with_padding, action_resize_handle_at_point_with_radius,
    resize_rect_with_handle,
};
use super::super::types::{
    AnnotationAction, CropAspectRatio, DrawColor, EditorError, Point, Rect, SelectHandle,
};
use super::EditorState;
use image::RgbaImage;
use std::sync::Arc;

fn resize_crop_rect_with_handle(
    rect: &mut Rect,
    handle: SelectHandle,
    dx: f64,
    dy: f64,
    image_width: i32,
    image_height: i32,
) -> bool {
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

    // Enforce maximum expansion limits (sanity check to prevent runaway/freeze)
    // We allow up to 5000px of padding beyond the image on any side.
    let max_exp = 5000.0;
    left = left.max(-max_exp);
    top = top.max(-max_exp);
    right = right.min(image_width as f64 + max_exp);
    bottom = bottom.min(image_height as f64 + max_exp);

    // Enforce minimum size constraints
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

fn crop_rect_with_aspect_fit(
    image_width: i32,
    image_height: i32,
    aspect_ratio: f64,
) -> Option<Rect> {
    if image_width <= 1 || image_height <= 1 || aspect_ratio <= 0.0 {
        return None;
    }

    let image_ratio = image_width as f64 / image_height as f64;
    let (width, height) = if image_ratio >= aspect_ratio {
        let height = image_height as f64;
        (height * aspect_ratio, height)
    } else {
        let width = image_width as f64;
        (width, width / aspect_ratio)
    };

    let x = (image_width as f64 - width) / 2.0;
    let y = (image_height as f64 - height) / 2.0;
    Rect::from_bounds(x, y, x + width, y + height)
}

fn resize_crop_rect_with_fixed_aspect(
    rect: &mut Rect,
    handle: SelectHandle,
    point: Point,
    image_width: i32,
    _image_height: i32,
    aspect_ratio: f64,
) -> bool {
    if aspect_ratio <= 0.0 {
        return false;
    }

    let center = Point {
        x: rect.x as f64 + rect.width as f64 / 2.0,
        y: rect.y as f64 + rect.height as f64 / 2.0,
    };
    let min_half_width = SELECT_MIN_RESIZE_SIZE / 2.0;
    let min_half_height = min_half_width / aspect_ratio;
    let mut half_width = match handle {
        SelectHandle::Left | SelectHandle::Right => (point.x - center.x).abs().max(min_half_width),
        SelectHandle::Top | SelectHandle::Bottom => {
            ((point.y - center.y).abs().max(min_half_height)) * aspect_ratio
        }
        _ => (point.x - center.x)
            .abs()
            .max((point.y - center.y).abs() * aspect_ratio)
            .max(min_half_width),
    };

    // Sanity check: cap half_width to avoid infinite expansion
    let max_exp = 5000.0;
    let max_half_width = (image_width as f64 + max_exp * 2.0) / 2.0;
    half_width = half_width.min(max_half_width);

    let half_height = half_width / aspect_ratio;

    let Some(updated) = Rect::from_bounds(
        center.x - half_width,
        center.y - half_height,
        center.x + half_width,
        center.y + half_height,
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

fn crop_fill_pixel(fill: DrawColor) -> image::Rgba<u8> {
    image::Rgba([
        (fill.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (fill.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (fill.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (fill.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    ])
}

pub fn crop_image(source: &RgbaImage, crop: Rect, fill: DrawColor) -> RgbaImage {
    let crop_width = crop.width.max(0) as u32;
    let crop_height = crop.height.max(0) as u32;
    if crop_width == 0 || crop_height == 0 {
        return source.clone();
    }

    let mut output = RgbaImage::from_pixel(crop_width, crop_height, crop_fill_pixel(fill));
    let source_x = crop.x.max(0) as u32;
    let source_y = crop.y.max(0) as u32;
    let source_right = (crop.x + crop.width).clamp(0, source.width() as i32) as u32;
    let source_bottom = (crop.y + crop.height).clamp(0, source.height() as i32) as u32;

    if source_right > source_x && source_bottom > source_y {
        let source_width = source_right - source_x;
        let source_height = source_bottom - source_y;
        let source_crop =
            image::imageops::crop_imm(source, source_x, source_y, source_width, source_height)
                .to_image();
        let dest_x = source_x as i64 - crop.x as i64;
        let dest_y = source_y as i64 - crop.y as i64;
        image::imageops::overlay(&mut output, &source_crop, dest_x, dest_y);
    }

    output
}

impl EditorState {
    pub fn crop_aspect_ratio_value(&self) -> Option<f64> {
        self.crop_aspect_ratio.aspect_ratio(
            self.working_image.width() as i32,
            self.working_image.height() as i32,
        )
    }

    pub fn set_crop_aspect_ratio(&mut self, crop_aspect_ratio: CropAspectRatio) -> bool {
        if self.crop_aspect_ratio == crop_aspect_ratio {
            return false;
        }

        self.crop_aspect_ratio = crop_aspect_ratio;

        let Some(rect) = self.crop_selection else {
            return true;
        };

        let image_width = self.working_image.width() as i32;
        let image_height = self.working_image.height() as i32;
        self.crop_selection = match self.crop_aspect_ratio_value() {
            Some(aspect_ratio) => {
                crop_rect_with_aspect_fit(image_width, image_height, aspect_ratio)
            }
            None => Some(rect),
        };
        true
    }

    pub fn set_crop_background_color(&mut self, color: DrawColor) {
        self.crop_background_color = color;
        self.crop_background_color_explicit = true;
    }

    pub fn draft_crop_rect(&self) -> Option<Rect> {
        let start = self.drag_start?;
        let current = self.drag_current?;
        let image_width = self.working_image.width() as i32;
        let image_height = self.working_image.height() as i32;
        let end = if let Some(aspect_ratio) = self.crop_aspect_ratio_value() {
            let dx = current.x - start.x;
            let dy = current.y - start.y;
            if dx.abs() < 0.0001 || dy.abs() < 0.0001 {
                current
            } else {
                let dx_abs = dx.abs();
                let dy_abs = dy.abs();
                let width_from_height = dy_abs * aspect_ratio;
                let height_from_width = dx_abs / aspect_ratio;
                if width_from_height <= dx_abs {
                    Point {
                        x: start.x + dx.signum() * width_from_height,
                        y: current.y,
                    }
                } else {
                    Point {
                        x: current.x,
                        y: start.y + dy.signum() * height_from_width,
                    }
                }
            }
        } else {
            current
        };

        Rect::from_points(start, end).map(|mut rect| {
            rect.x = rect.x.clamp(0, image_width.saturating_sub(1));
            rect.y = rect.y.clamp(0, image_height.saturating_sub(1));
            let max_width = image_width.saturating_sub(rect.x);
            let max_height = image_height.saturating_sub(rect.y);
            rect.width = rect.width.clamp(0, max_width);
            rect.height = rect.height.clamp(0, max_height);
            rect
        })
    }

    pub fn ensure_crop_selection_initialized(&mut self) -> bool {
        if self.crop_selection.is_some() {
            return false;
        }

        let image_width = self.working_image.width() as i32;
        let image_height = self.working_image.height() as i32;
        if image_width <= 1 || image_height <= 1 {
            return false;
        }

        self.crop_selection = match self.crop_aspect_ratio_value() {
            Some(aspect_ratio) => {
                crop_rect_with_aspect_fit(image_width, image_height, aspect_ratio)
            }
            None => Some(Rect {
                x: 0,
                y: 0,
                width: image_width,
                height: image_height,
            }),
        };
        self.crop_selection.is_some()
    }

    pub fn reset_crop_interaction(&mut self) {
        self.crop_selection = None;
        self.clear_drag_without_rebuild();
    }

    pub fn begin_crop_drag_with_scale(&mut self, point: Point, view_scale: f64) -> bool {
        let Some(crop_rect) = self.crop_selection else {
            return false;
        };

        let crop_action = AnnotationAction::Box {
            rect: crop_rect,
            color: self.selected_color,
            stroke_size: self.stroke_size,
            shadow: false,
        };
        let handle_hit_radius = selection_handle_hit_radius_for_scale(view_scale);
        if let Some(handle) =
            action_resize_handle_at_point_with_radius(&crop_action, point, handle_hit_radius)
        {
            self.select_resize_handle = Some(handle);
            self.select_drag_anchor = Some(point);
            return true;
        }

        self.select_resize_handle = None;
        let hit_padding = selection_hit_padding_for_scale(view_scale);
        if action_contains_point_with_padding(&crop_action, point, hit_padding) {
            self.select_drag_anchor = Some(point);
            return true;
        }

        false
    }

    pub fn update_crop_drag(&mut self, point: Point) -> bool {
        let Some(anchor) = self.select_drag_anchor else {
            return false;
        };
        let aspect_ratio = self.crop_aspect_ratio_value();
        let Some(rect) = self.crop_selection.as_mut() else {
            return false;
        };

        let dx = point.x - anchor.x;
        let dy = point.y - anchor.y;
        if dx.abs() < 0.0001 && dy.abs() < 0.0001 {
            return false;
        }

        let image_width = self.working_image.width() as i32;
        let image_height = self.working_image.height() as i32;

        let original = *rect;
        let moved = if let Some(handle) = self.select_resize_handle {
            if let Some(aspect_ratio) = aspect_ratio {
                resize_crop_rect_with_fixed_aspect(
                    rect,
                    handle,
                    point,
                    image_width,
                    image_height,
                    aspect_ratio,
                )
            } else {
                match handle {
                    SelectHandle::Left
                    | SelectHandle::Right
                    | SelectHandle::Top
                    | SelectHandle::Bottom => resize_crop_rect_with_handle(
                        rect,
                        handle,
                        dx,
                        dy,
                        image_width,
                        image_height,
                    ),
                    _ => resize_rect_with_handle(rect, handle, dx, dy),
                }
            }
        } else {
            let dx_i = dx.round() as i32;
            let dy_i = dy.round() as i32;
            if dx_i == 0 && dy_i == 0 {
                false
            } else {
                rect.x += dx_i;
                rect.y += dy_i;
                rect.x != original.x
                    || rect.y != original.y
                    || rect.width != original.width
                    || rect.height != original.height
            }
        };

        if !moved {
            return false;
        }

        self.select_drag_anchor = Some(point);
        true
    }

    pub fn end_crop_drag(&mut self) {
        self.clear_drag();
    }

    pub fn apply_crop_selection(&mut self) -> Result<bool, EditorError> {
        if self.crop_selection.is_none() {
            return Ok(false);
        }

        let cropped_image = self.to_final_image()?;
        if cropped_image.width() == 0 || cropped_image.height() == 0 {
            return Ok(false);
        }

        self.base_image = Arc::new(cropped_image.clone());
        self.working_image = Arc::new(cropped_image);
        self.actions.clear();
        self.redo_actions.clear();
        self.selected_action_index = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.next_number = self.numbering_start;
        self.crop_selection = None;
        self.clear_drag();
        self.mark_working_image_dirty();

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::types::{Point, Rect, SelectHandle};

    use super::EditorState;

    #[test]
    fn reset_crop_interaction_clears_crop_selection_and_drag_handles() {
        let mut state = EditorState::new(RgbaImage::new(32, 32));
        state.crop_selection = Some(Rect {
            x: 2,
            y: 3,
            width: 12,
            height: 14,
        });
        state.drag_start = Some(Point { x: 2.0, y: 3.0 });
        state.drag_current = Some(Point { x: 15.0, y: 18.0 });
        state.drag_start_view = Some(Point { x: 4.0, y: 5.0 });
        state.select_drag_anchor = Some(Point { x: 8.0, y: 9.0 });
        state.select_resize_handle = Some(SelectHandle::BottomRight);

        state.reset_crop_interaction();

        assert!(state.crop_selection.is_none());
        assert!(state.drag_start.is_none());
        assert!(state.drag_current.is_none());
        assert!(state.drag_start_view.is_none());
        assert!(state.select_drag_anchor.is_none());
        assert!(state.select_resize_handle.is_none());
    }
}
