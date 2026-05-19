use super::api::SelectionArea;
use super::background::BackgroundFrame;
use super::layout::{BORDER_HANDLE_THRESHOLD, MIN_SELECTION_HEIGHT, MIN_SELECTION_WIDTH};
use super::state::{DragMode, ResizeHandle, SelectorState};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SelectionRectF {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) right: f64,
    pub(crate) bottom: f64,
}

impl SelectionRectF {
    pub(crate) fn from_points(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            left: x0.min(x1),
            top: y0.min(y1),
            right: x0.max(x1),
            bottom: y0.max(y1),
        }
    }

    pub(crate) fn width(&self) -> f64 {
        self.right - self.left
    }

    pub(crate) fn height(&self) -> f64 {
        self.bottom - self.top
    }
}

pub(crate) fn current_selection_rect(state: &SelectorState) -> SelectionRectF {
    SelectionRectF::from_points(
        state.start_x,
        state.start_y,
        state.current_x,
        state.current_y,
    )
}

pub(crate) fn set_selection_rect(state: &mut SelectorState, rect: SelectionRectF) {
    state.start_x = rect.left;
    state.start_y = rect.top;
    state.current_x = rect.right;
    state.current_y = rect.bottom;
}

pub(crate) fn clamp_point_to_bounds(
    x: f64,
    y: f64,
    bounds_width: f64,
    bounds_height: f64,
) -> (f64, f64) {
    (
        x.clamp(0.0, bounds_width.max(1.0)),
        y.clamp(0.0, bounds_height.max(1.0)),
    )
}

pub(crate) fn detect_resize_handle(x: f64, y: f64, rect: SelectionRectF) -> Option<ResizeHandle> {
    let left = rect.left;
    let right = rect.right;
    let top = rect.top;
    let bottom = rect.bottom;

    let near_left = (x - left).abs() <= BORDER_HANDLE_THRESHOLD
        && y >= top - BORDER_HANDLE_THRESHOLD
        && y <= bottom + BORDER_HANDLE_THRESHOLD;
    let near_right = (x - right).abs() <= BORDER_HANDLE_THRESHOLD
        && y >= top - BORDER_HANDLE_THRESHOLD
        && y <= bottom + BORDER_HANDLE_THRESHOLD;
    let near_top = (y - top).abs() <= BORDER_HANDLE_THRESHOLD
        && x >= left - BORDER_HANDLE_THRESHOLD
        && x <= right + BORDER_HANDLE_THRESHOLD;
    let near_bottom = (y - bottom).abs() <= BORDER_HANDLE_THRESHOLD
        && x >= left - BORDER_HANDLE_THRESHOLD
        && x <= right + BORDER_HANDLE_THRESHOLD;

    if near_left && near_top {
        return Some(ResizeHandle::NorthWest);
    }
    if near_right && near_top {
        return Some(ResizeHandle::NorthEast);
    }
    if near_left && near_bottom {
        return Some(ResizeHandle::SouthWest);
    }
    if near_right && near_bottom {
        return Some(ResizeHandle::SouthEast);
    }

    if near_top {
        return Some(ResizeHandle::North);
    }
    if near_bottom {
        return Some(ResizeHandle::South);
    }
    if near_left {
        return Some(ResizeHandle::West);
    }
    if near_right {
        return Some(ResizeHandle::East);
    }

    None
}

/// Returns `true` when `(x, y)` is strictly inside the selection rectangle,
/// far enough from every edge that it is not on a resize handle.
/// This is used to decide whether a drag should move the whole rect.
pub(crate) fn is_inside_selection(x: f64, y: f64, rect: SelectionRectF) -> bool {
    x > rect.left + BORDER_HANDLE_THRESHOLD
        && x < rect.right - BORDER_HANDLE_THRESHOLD
        && y > rect.top + BORDER_HANDLE_THRESHOLD
        && y < rect.bottom - BORDER_HANDLE_THRESHOLD
}

pub(crate) fn cursor_name_for_handle(handle: ResizeHandle) -> &'static str {
    match handle {
        ResizeHandle::North | ResizeHandle::South => "ns-resize",
        ResizeHandle::East | ResizeHandle::West => "ew-resize",
        ResizeHandle::NorthEast | ResizeHandle::SouthWest => "nesw-resize",
        ResizeHandle::NorthWest | ResizeHandle::SouthEast => "nwse-resize",
    }
}

pub(crate) fn resize_rect_from_handle(
    initial: SelectionRectF,
    handle: ResizeHandle,
    pointer_x: f64,
    pointer_y: f64,
    bounds_width: f64,
    bounds_height: f64,
) -> SelectionRectF {
    let mut left = initial.left;
    let mut top = initial.top;
    let mut right = initial.right;
    let mut bottom = initial.bottom;

    let move_left = matches!(
        handle,
        ResizeHandle::West | ResizeHandle::NorthWest | ResizeHandle::SouthWest
    );
    let move_right = matches!(
        handle,
        ResizeHandle::East | ResizeHandle::NorthEast | ResizeHandle::SouthEast
    );
    let move_top = matches!(
        handle,
        ResizeHandle::North | ResizeHandle::NorthWest | ResizeHandle::NorthEast
    );
    let move_bottom = matches!(
        handle,
        ResizeHandle::South | ResizeHandle::SouthWest | ResizeHandle::SouthEast
    );

    if move_left {
        left = pointer_x;
    }
    if move_right {
        right = pointer_x;
    }
    if move_top {
        top = pointer_y;
    }
    if move_bottom {
        bottom = pointer_y;
    }

    let min_width = MIN_SELECTION_WIDTH.min(bounds_width.max(1.0));
    let min_height = MIN_SELECTION_HEIGHT.min(bounds_height.max(1.0));

    if (right - left) < min_width {
        if move_left {
            left = right - min_width;
        } else {
            right = left + min_width;
        }
    }

    if (bottom - top) < min_height {
        if move_top {
            top = bottom - min_height;
        } else {
            bottom = top + min_height;
        }
    }

    left = left.clamp(0.0, (bounds_width - min_width).max(0.0));
    top = top.clamp(0.0, (bounds_height - min_height).max(0.0));
    right = right.clamp(min_width, bounds_width.max(min_width));
    bottom = bottom.clamp(min_height, bounds_height.max(min_height));

    if (right - left) < min_width {
        if move_left {
            left = (right - min_width).max(0.0);
        } else {
            right = (left + min_width).min(bounds_width.max(min_width));
        }
    }

    if (bottom - top) < min_height {
        if move_top {
            top = (bottom - min_height).max(0.0);
        } else {
            bottom = (top + min_height).min(bounds_height.max(min_height));
        }
    }

    SelectionRectF {
        left,
        top,
        right,
        bottom,
    }
}

pub(crate) fn update_selection_for_drag(
    state: &mut SelectorState,
    drag_offset_x: f64,
    drag_offset_y: f64,
    bounds_width: f64,
    bounds_height: f64,
) {
    match state.drag_mode {
        Some(DragMode::NewSelection) => {
            let (next_x, next_y) = clamp_point_to_bounds(
                state.drag_origin_x + drag_offset_x,
                state.drag_origin_y + drag_offset_y,
                bounds_width,
                bounds_height,
            );
            state.current_x = next_x;
            state.current_y = next_y;
        }
        Some(DragMode::Move) => {
            if let Some(initial_rect) = state.initial_rect {
                let w = initial_rect.width();
                let h = initial_rect.height();
                // Translate the whole rect by the drag delta, keeping it
                // fully within the screen bounds.
                let new_left =
                    (initial_rect.left + drag_offset_x).clamp(0.0, (bounds_width - w).max(0.0));
                let new_top =
                    (initial_rect.top + drag_offset_y).clamp(0.0, (bounds_height - h).max(0.0));
                set_selection_rect(
                    state,
                    SelectionRectF {
                        left: new_left,
                        top: new_top,
                        right: new_left + w,
                        bottom: new_top + h,
                    },
                );
                state.completed = true;
            }
        }
        Some(DragMode::Resize(handle)) => {
            if let Some(initial_rect) = state.initial_rect {
                let (pointer_x, pointer_y) = clamp_point_to_bounds(
                    state.drag_origin_x + drag_offset_x,
                    state.drag_origin_y + drag_offset_y,
                    bounds_width,
                    bounds_height,
                );
                let resized = resize_rect_from_handle(
                    initial_rect,
                    handle,
                    pointer_x,
                    pointer_y,
                    bounds_width,
                    bounds_height,
                );
                set_selection_rect(state, resized);
                state.completed = true;
            }
        }
        None => {}
    }
}

pub(crate) fn selection_area_from_state(
    state: &SelectorState,
    screen_width: i32,
    screen_height: i32,
    background: Option<&BackgroundFrame>,
) -> SelectionArea {
    if state.fullscreen_mode {
        let mut full = SelectionArea {
            x: 0,
            y: 0,
            width: screen_width,
            height: screen_height,
        };
        if let Some(background) = background {
            full = map_selection_to_image(
                full,
                background.width,
                background.height,
                screen_width,
                screen_height,
            );
        }
        return full;
    }

    let rect = current_selection_rect(state);
    let area = SelectionArea {
        x: rect.left.floor() as i32,
        y: rect.top.floor() as i32,
        width: rect.width().round() as i32,
        height: rect.height().round() as i32,
    };
    if let Some(background) = background {
        map_selection_to_image(
            area,
            background.width,
            background.height,
            screen_width,
            screen_height,
        )
    } else {
        area
    }
}

pub(crate) fn map_selection_to_image(
    area: SelectionArea,
    image_width: i32,
    image_height: i32,
    view_width: i32,
    view_height: i32,
) -> SelectionArea {
    if image_width <= 0 || image_height <= 0 || view_width <= 0 || view_height <= 0 {
        return area;
    }

    let scale_x = image_width as f64 / view_width as f64;
    let scale_y = image_height as f64 / view_height as f64;

    let x0 = (area.x as f64 * scale_x).floor() as i32;
    let y0 = (area.y as f64 * scale_y).floor() as i32;
    let x1 = ((area.x + area.width) as f64 * scale_x).ceil() as i32;
    let y1 = ((area.y + area.height) as f64 * scale_y).ceil() as i32;

    let clamped_x0 = x0.clamp(0, image_width.saturating_sub(1));
    let clamped_y0 = y0.clamp(0, image_height.saturating_sub(1));
    let clamped_x1 = x1.clamp(clamped_x0 + 1, image_width);
    let clamped_y1 = y1.clamp(clamped_y0 + 1, image_height);

    SelectionArea {
        x: clamped_x0,
        y: clamped_y0,
        width: clamped_x1 - clamped_x0,
        height: clamped_y1 - clamped_y0,
    }
}
