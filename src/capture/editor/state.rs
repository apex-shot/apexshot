use super::color::{
    clamp_blur_secure_amount, clamp_blur_smooth_amount, clamp_obfuscate_amount,
    clamp_pixelate_amount, clamp_stroke_size, clamp_text_size,
    selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale, DEFAULT_COLOR_INDEX,
    DEFAULT_OBFUSCATE_AMOUNT, DRAW_COLORS, SELECT_MIN_RESIZE_SIZE, STROKE_WIDTH, TEXT_SIZE,
};
use super::numbering_style::{NumberSize, NumberingStyle};
use super::pen_weight::{HighlighterMode, PenWeight};
use super::render::{
    apply_blackout_rect, apply_blur_rect, apply_censor_rect, apply_focus_rect, apply_secure_blur,
    layout_wrapped_text,
};
use super::selection::{
    action_bounds_with_padding, action_contains_point_with_padding,
    action_resize_handle_at_point_with_radius, resize_action, resize_rect_with_handle,
    translate_action,
};
use super::text_detect::{BackgroundTextDetection, TextDetector};
use super::types::{
    AnnotationAction, ArrowStyle, BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor,
    EditorError, FontSettings, FontStyle, MoveHandle, ObfuscateMethod, Point, Rect, SelectHandle,
    SizeControlMode, TextAlignment, TextDecoration, TextEditBounds, Tool,
};
use gtk4;
use image::RgbaImage;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct EditorState {
    pub base_image: RgbaImage,
    pub working_image: RgbaImage,
    pub working_image_revision: u64,
    pub crop_selection: Option<Rect>,
    pub crop_aspect_ratio: CropAspectRatio,
    pub crop_background_color: DrawColor,
    pub crop_background_color_explicit: bool,
    pub actions: Vec<AnnotationAction>,
    pub redo_actions: Vec<AnnotationAction>,
    pub selected_tool: Tool,
    pub selected_action_index: Option<usize>,
    pub selected_color: DrawColor,
    pub stroke_size: f64,
    pub smooth_drawing_enabled: bool,
    pub draw_object_shadow: bool,
    pub auto_expand_canvas: bool,
    pub inverse_arrow_direction: bool,
    pub text_size: f64,
    pub text_font_family: String,
    pub obfuscate_method: ObfuscateMethod,
    pub obfuscate_pixelate_amount: f64,
    pub obfuscate_blur_secure_amount: f64,
    pub obfuscate_blur_smooth_amount: f64,
    pub arrow_style: ArrowStyle,
    pub arrow_editing_controls: bool,
    pub arrow_control_dragging: Option<usize>,
    pub next_number: u32,
    pub select_drag_anchor: Option<Point>,
    pub select_resize_handle: Option<super::types::SelectHandle>,
    pub select_effect_rebuild_pending: bool,
    pub select_effect_rebuild_dirty: bool,
    pub select_drag_effect_dirty: bool,
    pub active_text_edit: Option<()>,
    pub active_text_entry: Option<gtk4::Entry>,
    pub active_text_bounds: Option<TextEditBounds>,
    pub active_text_is_dragging: bool,
    pub active_text_drag_handle: Option<MoveHandle>,
    pub active_text_drag_start: Option<Point>,
    pub pending_effect_revision: u64,
    pub last_applied_effect_revision: u64,
    pub last_effect_request_time_us: i64,
    pub drag_start: Option<Point>,
    pub drag_current: Option<Point>,
    pub drag_start_view: Option<Point>,
    pub drag_path: Vec<Point>,
    pub drag_shift_active: bool,
    pub background_style: BackgroundStyle,
    pub background_padding: f64,
    pub background_shadow: f64,
    pub background_insert: f64,
    pub auto_balance: bool,
    pub background_alignment: BackgroundAlignment,
    pub background_corner_radius: f64,
    pub background_aspect_ratio: CropAspectRatio,
    pub active_text_drag_start_bounds: Option<Rect>,
    pub active_text_is_resizing: bool,
    pub hovered_text_action_index: Option<usize>,
    #[allow(dead_code)]
    pub active_text_input: Option<TextInputState>,

    // Text detection for highlighter
    pub text_detector: Arc<Mutex<TextDetector>>,
    #[allow(dead_code)]
    pub text_detection_ready: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub text_detection_handle: Option<BackgroundTextDetection>,

    // Highlighter mode
    pub highlighter_mode: HighlighterMode,
    pub pen_weight: PenWeight,
    pub locked_highlighter_stroke_size: Option<f64>,

    // Number tool options
    pub numbering_style: NumberingStyle,
    pub numbering_start: u32,
    pub number_size: NumberSize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TextInputState {
    pub text: String,
    pub cursor_position: usize,
    pub cursor_visible: bool,
    pub cursor_blink_timer: u32,
    pub color: DrawColor,
    pub editing_action_index: Option<usize>,
}

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

fn simplify_drag_path(points: &[Point], epsilon: f64) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    simplify_drag_path_range(points, 0, points.len() - 1, epsilon, &mut keep);

    points
        .iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

fn simplify_drag_path_range(
    points: &[Point],
    start: usize,
    end: usize,
    epsilon: f64,
    keep: &mut [bool],
) {
    if end <= start + 1 {
        return;
    }

    let first = points[start];
    let last = points[end];
    let mut max_distance = 0.0;
    let mut max_index = None;

    for (index, point) in points.iter().enumerate().take(end).skip(start + 1) {
        let distance = perpendicular_distance(*point, first, last);
        if distance > max_distance {
            max_distance = distance;
            max_index = Some(index);
        }
    }

    if max_distance > epsilon {
        if let Some(index) = max_index {
            keep[index] = true;
            simplify_drag_path_range(points, start, index, epsilon, keep);
            simplify_drag_path_range(points, index, end, epsilon, keep);
        }
    }
}

fn perpendicular_distance(point: Point, start: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() <= f64::EPSILON && dy.abs() <= f64::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }

    let numerator = ((dy * point.x) - (dx * point.y) + (end.x * start.y) - (end.y * start.x)).abs();
    let denominator = (dx * dx + dy * dy).sqrt();
    numerator / denominator
}

fn expand_rgba_image(
    image: &RgbaImage,
    new_width: u32,
    new_height: u32,
    offset_x: u32,
    offset_y: u32,
) -> RgbaImage {
    if new_width == image.width() && new_height == image.height() && offset_x == 0 && offset_y == 0
    {
        return image.clone();
    }

    let mut expanded = RgbaImage::from_pixel(new_width, new_height, image::Rgba([0, 0, 0, 0]));
    image::imageops::overlay(&mut expanded, image, offset_x as i64, offset_y as i64);
    expanded
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

impl EditorState {
    pub fn new(base_image: RgbaImage) -> Self {
        Self {
            working_image: base_image.clone(),
            base_image,
            working_image_revision: 1,
            crop_selection: None,
            crop_aspect_ratio: CropAspectRatio::Freeform,
            crop_background_color: DrawColor::new(1.0, 1.0, 1.0, 1.0),
            crop_background_color_explicit: false,
            actions: Vec::new(),
            redo_actions: Vec::new(),
            selected_tool: Tool::Background,
            selected_action_index: None,
            selected_color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
            smooth_drawing_enabled: false,
            draw_object_shadow: false,
            auto_expand_canvas: false,
            inverse_arrow_direction: false,
            text_size: TEXT_SIZE,
            text_font_family: String::from("Sans"),
            obfuscate_method: ObfuscateMethod::Pixelate,
            obfuscate_pixelate_amount: DEFAULT_OBFUSCATE_AMOUNT,
            obfuscate_blur_secure_amount: DEFAULT_OBFUSCATE_AMOUNT,
            obfuscate_blur_smooth_amount: DEFAULT_OBFUSCATE_AMOUNT,
            arrow_style: ArrowStyle::Standard,
            arrow_editing_controls: false,
            arrow_control_dragging: None,
            next_number: 1,
            select_drag_anchor: None,
            select_resize_handle: None,
            select_effect_rebuild_pending: false,
            select_effect_rebuild_dirty: false,
            select_drag_effect_dirty: false,
            active_text_edit: None,
            active_text_entry: None,
            active_text_bounds: None,
            active_text_is_dragging: false,
            active_text_drag_handle: None,
            active_text_drag_start: None,
            pending_effect_revision: 0,
            last_applied_effect_revision: 0,
            last_effect_request_time_us: 0,
            drag_start: None,
            drag_current: None,
            drag_start_view: None,
            drag_path: Vec::new(),
            drag_shift_active: false,
            background_style: BackgroundStyle::None,
            background_padding: 24.0,
            background_shadow: 15.0,
            background_insert: 20.0,
            auto_balance: true,
            background_alignment: BackgroundAlignment::Center,
            background_corner_radius: 18.0,
            background_aspect_ratio: CropAspectRatio::Original,
            active_text_drag_start_bounds: None,
            active_text_is_resizing: false,
            hovered_text_action_index: None,
            active_text_input: None,

            text_detector: Arc::new(Mutex::new(TextDetector::new_pending())),
            text_detection_ready: Arc::new(AtomicBool::new(false)),
            text_detection_handle: None,
            highlighter_mode: HighlighterMode::default(),
            pen_weight: PenWeight::default(),
            locked_highlighter_stroke_size: None,
            numbering_style: NumberingStyle::default(),
            numbering_start: 1,
            number_size: NumberSize::default(),
        }
    }

    pub fn set_tool(&mut self, tool: Tool) -> bool {
        let rebuild = self.set_tool_without_rebuild(tool);
        if rebuild {
            self.rebuild_effect_layer();
        }
        rebuild
    }

    pub fn set_tool_without_rebuild(&mut self, tool: Tool) -> bool {
        if self.selected_tool == Tool::Crop && tool != Tool::Crop {
            self.crop_selection = None;
        }
        if tool != Tool::Select {
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
        }
        if tool != Tool::Text {
            self.cancel_text_input();
            self.hovered_text_action_index = None;
        }
        if tool != Tool::Arrow {
            self.finalize_arrow_control_editing();
        }
        self.selected_tool = tool;
        self.clear_drag_without_rebuild_and_check_effect()
    }

    fn existing_text_bounds(&self, skip_index: Option<usize>) -> Vec<Rect> {
        self.actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                if Some(index) == skip_index || !matches!(action, AnnotationAction::Text { .. }) {
                    return None;
                }
                action_bounds_with_padding(action, 0.0)
            })
            .collect()
    }

    fn text_obstacle_limits(
        &self,
        bounds: &TextEditBounds,
        skip_index: Option<usize>,
    ) -> (f64, f64) {
        let image_width = self.base_image.width() as f64;
        let image_height = self.base_image.height() as f64;
        let mut right_limit = image_width - bounds.rect.x as f64;
        let mut bottom_limit = image_height - bounds.rect.y as f64;

        for obstacle in self.existing_text_bounds(skip_index) {
            let vertical_overlap = bounds.rect.y < obstacle.y + obstacle.height
                && bounds.rect.y + bounds.rect.height > obstacle.y;
            if vertical_overlap && obstacle.x >= bounds.rect.x {
                right_limit = right_limit.min((obstacle.x - bounds.rect.x).max(50) as f64);
            }

            let horizontal_overlap = bounds.rect.x < obstacle.x + obstacle.width
                && bounds.rect.x + bounds.rect.width > obstacle.x;
            if horizontal_overlap && obstacle.y >= bounds.rect.y {
                bottom_limit = bottom_limit.min((obstacle.y - bounds.rect.y).max(44) as f64);
            }
        }

        (right_limit.max(50.0), bottom_limit.max(44.0))
    }

    pub fn begin_text_input(&mut self, position: Point, width: f64, height: f64) {
        let image_width = self.base_image.width() as f64;
        let image_height = self.base_image.height() as f64;
        let baseline_y = position.y.clamp(self.text_size + 8.0, image_height - 8.0);
        let max_width = (image_width - position.x).max(50.0);
        let constrained_width = width.clamp(50.0, max_width);
        let max_height = (image_height - (baseline_y - self.text_size - 8.0)).max(44.0);
        let constrained_height = height.clamp(44.0, max_height);
        let top_left = Point {
            x: position.x.clamp(0.0, image_width - 50.0),
            y: (baseline_y - self.text_size - 8.0).clamp(0.0, image_height - constrained_height),
        };
        let bounds = TextEditBounds::new(top_left, constrained_width, constrained_height);
        self.active_text_bounds = Some(bounds);
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.active_text_drag_start_bounds = None;
        self.active_text_is_resizing = false;
        self.start_text_input();
    }

    #[allow(dead_code)]
    pub fn start_text_input(&mut self) {
        self.active_text_input = Some(TextInputState {
            text: String::new(),
            cursor_position: 0,
            cursor_visible: true,
            cursor_blink_timer: 0,
            color: self.selected_color,
            editing_action_index: None,
        });
    }

    #[allow(dead_code)]
    pub fn add_text_input_char(&mut self, c: char) {
        if let Some(ref mut state) = self.active_text_input {
            state.text.insert(state.cursor_position, c);
            state.cursor_position += 1;
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    pub fn reset_text_cursor_blink(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    pub fn set_text_cursor_position(&mut self, position: usize) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_position = position.min(state.text.chars().count());
            state.cursor_visible = true;
            state.cursor_blink_timer = 0;
        }
    }

    #[allow(dead_code)]
    pub fn delete_text_input_char(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position > 0 {
                state.cursor_position -= 1;
                state.text.remove(state.cursor_position);
                state.cursor_blink_timer = 0;
            }
        }
    }

    #[allow(dead_code)]
    pub fn move_cursor_left(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position > 0 {
                state.cursor_position -= 1;
                state.cursor_visible = true;
                state.cursor_blink_timer = 0;
            }
        }
    }

    #[allow(dead_code)]
    pub fn move_cursor_right(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            if state.cursor_position < state.text.len() {
                state.cursor_position += 1;
                state.cursor_visible = true;
                state.cursor_blink_timer = 0;
            }
        }
    }

    #[allow(dead_code)]
    pub fn tick_cursor_blink(&mut self) {
        if let Some(ref mut state) = self.active_text_input {
            state.cursor_blink_timer += 1;
            if state.cursor_blink_timer >= 1 {
                state.cursor_blink_timer = 0;
                state.cursor_visible = !state.cursor_visible;
            }
        }
    }

    #[allow(dead_code)]
    pub fn commit_text_input(&mut self) -> Option<AnnotationAction> {
        if let Some(input_state) = self.active_text_input.take() {
            let trimmed_text = input_state.text.trim().to_string();
            let bounds = self.active_text_bounds.take();
            self.active_text_is_dragging = false;
            self.active_text_drag_handle = None;
            self.active_text_drag_start = None;
            self.active_text_drag_start_bounds = None;
            self.active_text_is_resizing = false;

            if let Some(index) = input_state.editing_action_index {
                if trimmed_text.is_empty() {
                    if index < self.actions.len()
                        && matches!(self.actions[index], AnnotationAction::Text { .. })
                    {
                        self.actions.remove(index);
                        self.selected_action_index = None;
                        self.select_drag_anchor = None;
                        self.select_resize_handle = None;
                        self.redo_actions.clear();
                    }
                    return None;
                }

                if let Some(b) = bounds {
                    let Some(AnnotationAction::Text {
                        position,
                        text,
                        color,
                        font,
                        max_width,
                        ..
                    }) = self.actions.get_mut(index)
                    else {
                        return None;
                    };
                    position.x = b.rect.x as f64;
                    position.y = (b.rect.y as f64 + self.text_size + 8.0)
                        .clamp(self.text_size + 8.0, self.base_image.height() as f64 - 8.0);
                    *text = trimmed_text;
                    *color = input_state.color;
                    font.family = self.text_font_family.clone();
                    font.size = self.text_size;
                    font.style = FontStyle::Normal;
                    font.decoration = TextDecoration::None;
                    font.alignment = TextAlignment::Left;
                    *max_width = Some(b.rect.width as f64);
                    self.selected_action_index = Some(index);
                    self.redo_actions.clear();
                }
                return None;
            }

            if trimmed_text.is_empty() {
                self.clear_text_edit_state();
                return None;
            }

            if let Some(b) = bounds {
                let position = Point {
                    x: b.rect.x as f64,
                    y: (b.rect.y as f64 + self.text_size + 8.0)
                        .clamp(self.text_size + 8.0, self.base_image.height() as f64 - 8.0),
                };
                let font = FontSettings {
                    family: self.text_font_family.clone(),
                    size: self.text_size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                let clamped_position = Point {
                    x: position.x.clamp(
                        0.0,
                        (self.base_image.width() as f64 - font.size * 1.8).max(0.0),
                    ),
                    y: position.y.clamp(font.size, self.base_image.height() as f64),
                };
                let clamped_width = (b.rect.width as f64).min(
                    (self.base_image.width() as f64 - clamped_position.x).max(font.size * 1.8),
                );
                return Some(AnnotationAction::Text {
                    position: clamped_position,
                    text: trimmed_text,
                    color: input_state.color,
                    font,
                    max_width: Some(clamped_width),
                    shadow: self.draw_object_shadow,
                });
            }
        }
        None
    }

    #[allow(dead_code)]
    pub fn cancel_text_input(&mut self) {
        self.active_text_input = None;
        self.clear_text_edit_state();
    }

    #[allow(dead_code)]
    fn clear_text_edit_state(&mut self) {
        self.active_text_bounds = None;
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.active_text_drag_start_bounds = None;
        self.active_text_is_resizing = false;
    }

    #[allow(dead_code)]
    pub fn get_text_input(&self) -> Option<&TextInputState> {
        self.active_text_input.as_ref()
    }

    #[allow(dead_code)]
    pub fn get_text_bounds(&self) -> Option<&TextEditBounds> {
        self.active_text_bounds.as_ref()
    }

    pub fn fit_active_text_to_layout_with_constraints(
        &mut self,
        preserve_width: bool,
        preserve_height: bool,
        preserve_font_size: bool,
    ) {
        let Some(input) = self.active_text_input.as_ref() else {
            return;
        };
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };

        let skip_index = input.editing_action_index;
        let text = input.text.clone();
        let family = self.text_font_family.clone();
        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(surface) => surface,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(context) => context,
            Err(_) => return,
        };

        let mut fitted_size = self.text_size;
        loop {
            let (available_width_limit, available_height_limit) =
                self.text_obstacle_limits(&bounds, skip_index);
            let available_height = if preserve_height {
                bounds.rect.height.max(1) as f64
            } else {
                available_height_limit
            };
            // When not preserving width, allow the box to grow up to the full
            // available space (image edge or next obstacle). This lets text
            // stay on one line and only wrap when it truly runs out of room.
            // When preserving width, cap at the current box width.
            let mut max_width = if preserve_width {
                (bounds.rect.width.max(1) as f64).min(available_width_limit)
            } else {
                available_width_limit
            };

            let measure = |size: f64, width: f64| {
                let font = FontSettings {
                    family: family.clone(),
                    size,
                    style: FontStyle::Normal,
                    decoration: TextDecoration::None,
                    alignment: TextAlignment::Left,
                };
                let content_width = (width - 20.0).max(font.size * 0.8);
                let layout = layout_wrapped_text(&context, &text, &font, content_width);
                let line_height = (font.size * 1.2).max(font.size + 4.0);
                // Include top+bottom padding and border inset so the box is
                // always tall enough that the bottom border never clips text.
                // border_inset mirrors TEXT_EDIT_BORDER_WIDTH/2 + 1 from render.rs
                let padding_y = 8.0;
                let border_inset = 2.0; // = TEXT_EDIT_BORDER_WIDTH / 2.0 + 1.0
                let text_block_height =
                    (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
                let height = (text_block_height + (padding_y + border_inset) * 2.0).max(44.0);
                (layout, height)
            };

            if !preserve_font_size {
                while fitted_size < 120.0 {
                    let next_size = (fitted_size + 1.0).min(120.0);
                    let (_, next_height) = measure(next_size, max_width);
                    if next_height > available_height {
                        break;
                    }
                    fitted_size = next_size;
                }
            }

            let (layout, mut height) = measure(fitted_size, max_width);
            if !preserve_font_size {
                while fitted_size > 10.0 && height > available_height {
                    fitted_size = (fitted_size - 1.0).max(10.0);
                    let measured = measure(fitted_size, max_width);
                    height = measured.1;
                }
            }

            if preserve_font_size {
                if height > available_height {
                    let mut low = max_width;
                    let mut high = available_width_limit;
                    while high - low > 1.0 {
                        let mid = (low + high) / 2.0;
                        let measured = measure(fitted_size, mid);
                        if measured.1 > available_height {
                            low = mid;
                        } else {
                            high = mid;
                        }
                    }
                    max_width = high;
                    height = measure(fitted_size, max_width).1;
                }
            }

            let old_width = bounds.rect.width;
            let old_height = bounds.rect.height;
            let target_width = if preserve_width {
                // Preserving width: keep the current box width (capped at available).
                max_width.round().max(fitted_size * 1.8) as i32
            } else {
                // Not preserving width: size the box to the actual text width
                // (with padding), only growing as wide as the text needs.
                // Add padding_x * 2 to match draw_active_text_input's padding.
                let padding_x = 10.0;
                (layout.max_width + padding_x * 2.0)
                    .max(fitted_size * 1.8)
                    .min(max_width)
                    .round() as i32
            };
            let target_height = if preserve_height {
                bounds.rect.height
            } else {
                height.min(available_height.max(44.0)).round().max(1.0) as i32
            };
            bounds.rect.width = target_width;
            bounds.rect.height = target_height;
            bounds.sync_handles();

            if bounds.rect.width == old_width && bounds.rect.height == old_height {
                break;
            }
        }

        self.text_size = fitted_size;
        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    #[allow(dead_code)]
    pub fn fit_active_text_to_layout_preserving_height(&mut self, preserve_height: bool) {
        self.fit_active_text_to_layout_with_constraints(false, preserve_height, false);
    }

    #[allow(dead_code)]
    pub fn fit_active_text_to_layout_preserving_font_size(&mut self) {
        self.fit_active_text_to_layout_with_constraints(true, false, true);
    }

    #[allow(dead_code)]
    pub fn fit_active_text_to_layout_preserving_box(&mut self) {
        self.fit_active_text_to_layout_with_constraints(true, true, false);
    }

    pub fn fit_active_text_to_layout(&mut self) {
        self.fit_active_text_to_layout_with_constraints(false, false, false);
    }

    /// Reflow only the box height to fit the current text at the current width
    /// and font size. Does NOT touch x, y, or width — safe to call during a
    /// Left/Right handle drag where the user is explicitly controlling width.
    pub fn fit_active_text_height_only(&mut self) {
        let Some(input) = self.active_text_input.as_ref() else {
            return;
        };
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };

        let text = input.text.clone();
        let family = self.text_font_family.clone();
        let size = self.text_size;

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return,
        };

        let font = FontSettings {
            family,
            size,
            style: FontStyle::Normal,
            decoration: TextDecoration::None,
            alignment: TextAlignment::Left,
        };
        let content_width = (bounds.rect.width as f64 - 20.0).max(font.size * 0.8);
        let layout = layout_wrapped_text(&context, &text, &font, content_width);
        let line_height = (font.size * 1.2).max(font.size + 4.0);
        let padding_y = 8.0;
        let border_inset = 2.0;
        let text_block_height =
            (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
        let new_height = (text_block_height + (padding_y + border_inset) * 2.0)
            .max(44.0)
            .round() as i32;

        // Only update height — x, y, width are untouched.
        bounds.rect.height = new_height;
        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    /// Like fit_active_text_height_only but reads text/font from the selected
    /// committed action instead of active_text_input. Used during circle-handle
    /// resizes of committed text actions (no active edit session open).
    pub fn fit_committed_text_height_only(&mut self) {
        let Some(mut bounds) = self.active_text_bounds.clone() else {
            return;
        };
        let Some(index) = self.selected_action_index else {
            return;
        };
        let (text, font) = match self.actions.get(index) {
            Some(AnnotationAction::Text { text, font, .. }) => (text.clone(), font.clone()),
            _ => return,
        };

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return,
        };

        let content_width = (bounds.rect.width as f64 - 20.0).max(font.size * 0.8);
        let layout = layout_wrapped_text(&context, &text, &font, content_width);
        let line_height = (font.size * 1.2).max(font.size + 4.0);
        let padding_y = 8.0;
        let border_inset = 2.0;
        let text_block_height =
            (layout.lines.len().max(1) as f64 - 1.0).max(0.0) * line_height + font.size;
        let new_height = (text_block_height + (padding_y + border_inset) * 2.0)
            .max(44.0)
            .round() as i32;

        bounds.rect.height = new_height;
        bounds.sync_handles();
        self.active_text_bounds = Some(bounds);
    }

    /// Compute the minimum box width needed to display the committed text
    /// action without any word being cut off. Returns the width of the longest
    /// single word (plus padding), or a font-size-based floor if no action.
    pub fn committed_text_min_width(&self) -> f64 {
        let Some(index) = self.selected_action_index else {
            return 50.0;
        };
        let (text, font) = match self.actions.get(index) {
            Some(AnnotationAction::Text { text, font, .. }) => (text.as_str(), font),
            _ => return 50.0,
        };

        let surface = match gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1) {
            Ok(s) => s,
            Err(_) => return 50.0,
        };
        let context = match gtk4::cairo::Context::new(&surface) {
            Ok(c) => c,
            Err(_) => return 50.0,
        };

        // Measure the width of each word; the widest word is the minimum.
        let padding_x = 10.0;
        let max_word_width = text
            .split_whitespace()
            .map(|word| super::render::measure_text_width(&context, word, font))
            .fold(0.0_f64, f64::max);

        // Add padding on both sides, floor at font_size * 1.8.
        (max_word_width + padding_x * 2.0)
            .max(font.size * 1.8)
            .max(50.0)
    }

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

    pub fn set_color_index(&mut self, index: usize) {
        if let Some(color) = DRAW_COLORS.get(index).copied() {
            self.selected_color = color;
            if let Some(input) = self.active_text_input.as_mut() {
                input.color = color;
            }
        }
    }

    pub fn set_crop_background_color(&mut self, color: DrawColor) {
        self.crop_background_color = color;
        self.crop_background_color_explicit = true;
    }

    pub fn set_stroke_size(&mut self, size: f64) -> bool {
        let next = clamp_stroke_size(size);
        if (next - self.stroke_size).abs() <= f64::EPSILON {
            return false;
        }

        self.stroke_size = next;
        true
    }

    pub fn set_obfuscate_method(&mut self, method: ObfuscateMethod) {
        self.obfuscate_method = method;
    }

    #[allow(dead_code)]
    pub fn obfuscate_method(&self) -> ObfuscateMethod {
        self.obfuscate_method
    }

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

    pub fn current_obfuscate_amount(&self) -> f64 {
        match self.obfuscate_method {
            ObfuscateMethod::Pixelate => self.obfuscate_pixelate_amount,
            ObfuscateMethod::BlurSecure => self.obfuscate_blur_secure_amount,
            ObfuscateMethod::BlurSmooth => self.obfuscate_blur_smooth_amount,
            ObfuscateMethod::Blackout => 0.0,
        }
    }

    pub fn set_current_obfuscate_amount(&mut self, amount: f64) {
        match self.obfuscate_method {
            ObfuscateMethod::Pixelate => {
                self.obfuscate_pixelate_amount = clamp_pixelate_amount(amount)
            }
            ObfuscateMethod::BlurSecure => {
                self.obfuscate_blur_secure_amount = clamp_blur_secure_amount(amount)
            }
            ObfuscateMethod::BlurSmooth => {
                self.obfuscate_blur_smooth_amount = clamp_blur_smooth_amount(amount)
            }
            ObfuscateMethod::Blackout => {}
        }
    }

    /// Like set_current_obfuscate_amount but returns true if the value actually changed.
    pub fn set_current_obfuscate_amount_and_check(&mut self, amount: f64) -> bool {
        let before = self.current_obfuscate_amount();
        self.set_current_obfuscate_amount(amount);
        let after = self.current_obfuscate_amount();
        (after - before).abs() > f64::EPSILON
    }

    pub fn selected_action_stroke_size(&self) -> Option<f64> {
        match self.selected_action()? {
            AnnotationAction::Pen { stroke_size, .. }
            | AnnotationAction::Highlighter { stroke_size, .. }
            | AnnotationAction::Circle { stroke_size, .. }
            | AnnotationAction::Line { stroke_size, .. }
            | AnnotationAction::Arrow { stroke_size, .. }
            | AnnotationAction::Box { stroke_size, .. } => Some(*stroke_size),
            AnnotationAction::Text { .. }
            | AnnotationAction::Number { .. }
            | AnnotationAction::Obfuscate { .. }
            | AnnotationAction::Focus { .. } => None,
        }
    }

    pub fn set_selected_action_stroke_size(&mut self, size: f64) -> bool {
        let next = clamp_stroke_size(size);

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let target = match action {
            AnnotationAction::Pen { stroke_size, .. }
            | AnnotationAction::Highlighter { stroke_size, .. }
            | AnnotationAction::Circle { stroke_size, .. }
            | AnnotationAction::Line { stroke_size, .. }
            | AnnotationAction::Arrow { stroke_size, .. }
            | AnnotationAction::Box { stroke_size, .. } => stroke_size,
            AnnotationAction::Text { .. }
            | AnnotationAction::Number { .. }
            | AnnotationAction::Obfuscate { .. }
            | AnnotationAction::Focus { .. } => return false,
        };

        if (*target - next).abs() <= f64::EPSILON {
            return false;
        }

        *target = next;
        self.redo_actions.clear();
        true
    }

    #[allow(dead_code)]
    pub fn set_text_size(&mut self, size: f64) -> bool {
        let next = clamp_text_size(size);
        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if (font.size - next).abs() <= f64::EPSILON {
                return false;
            }
            font.size = next;
            self.text_size = next;
            self.redo_actions.clear();
            return true;
        }

        if self.active_text_input.is_some() {
            if (next - self.text_size).abs() <= f64::EPSILON {
                return false;
            }
            self.text_size = next;
            return true;
        }

        if self.selected_action_index.is_some() {
            if self.set_selected_text_action_size(next) {
                self.text_size = next;
                return true;
            }
            return false;
        }

        if (next - self.text_size).abs() <= f64::EPSILON {
            return false;
        }

        self.text_size = next;
        true
    }

    pub fn selected_text_action_size(&self) -> Option<f64> {
        let AnnotationAction::Text { font, .. } = self.selected_action()? else {
            return None;
        };

        Some(font.size)
    }

    pub fn set_selected_text_action_size(&mut self, size: f64) -> bool {
        let next = clamp_text_size(size);

        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if (font.size - next).abs() <= f64::EPSILON {
                return false;
            }
            font.size = next;
            self.redo_actions.clear();
            return true;
        }

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Text { font, .. } = action else {
            return false;
        };

        if (font.size - next).abs() <= f64::EPSILON {
            return false;
        }

        font.size = next;
        self.redo_actions.clear();
        true
    }

    pub fn selected_text_font_family(&self) -> Option<String> {
        let AnnotationAction::Text { font, .. } = self.selected_action()? else {
            return None;
        };

        Some(font.family.clone())
    }

    pub fn set_selected_text_font_family(&mut self, family: String) -> bool {
        if let Some(index) = self
            .active_text_input
            .as_ref()
            .and_then(|input| input.editing_action_index)
        {
            let Some(AnnotationAction::Text { font, .. }) = self.actions.get_mut(index) else {
                return false;
            };
            if font.family == family {
                return false;
            }
            font.family = family;
            self.redo_actions.clear();
            return true;
        }

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Text { font, .. } = action else {
            return false;
        };

        if font.family == family {
            return false;
        }

        font.family = family;
        self.redo_actions.clear();
        true
    }

    pub fn selected_obfuscate_action_amount(&self) -> Option<f64> {
        let AnnotationAction::Obfuscate { amount, .. } = self.selected_action()? else {
            return None;
        };

        Some(*amount)
    }

    #[allow(dead_code)]
    pub fn set_selected_obfuscate_action_amount(&mut self, amount: f64) -> bool {
        if self.set_selected_obfuscate_action_amount_without_rebuild(amount) {
            self.rebuild_effect_layer();
            true
        } else {
            false
        }
    }

    pub fn set_selected_obfuscate_action_amount_without_rebuild(&mut self, amount: f64) -> bool {
        let next = clamp_obfuscate_amount(amount);

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let AnnotationAction::Obfuscate {
            amount: act_amount, ..
        } = action
        else {
            return false;
        };

        if (*act_amount - next).abs() <= f64::EPSILON {
            return false;
        }

        *act_amount = next;
        self.redo_actions.clear();
        true
    }

    pub fn active_size_control_mode(&self) -> Option<SizeControlMode> {
        if self.selected_tool == Tool::Select {
            if self.selected_action_stroke_size().is_some() {
                return Some(SizeControlMode::Stroke);
            }
            if self.selected_obfuscate_action_amount().is_some() {
                return Some(SizeControlMode::Obfuscate);
            }
            return None;
        }

        if self.selected_tool == Tool::Text {
            return None;
        }

        if self.selected_tool == Tool::Obfuscate {
            return Some(SizeControlMode::Obfuscate);
        }

        if super::types::tool_uses_stroke_size(self.selected_tool) {
            return Some(SizeControlMode::Stroke);
        }

        None
    }

    pub fn active_size_value(&self) -> Option<f64> {
        match self.active_size_control_mode()? {
            SizeControlMode::Stroke => {
                if self.selected_tool == Tool::Select {
                    Some(
                        self.selected_action_stroke_size()
                            .unwrap_or(self.stroke_size),
                    )
                } else {
                    Some(self.stroke_size)
                }
            }
            SizeControlMode::Obfuscate => {
                if self.selected_tool == Tool::Select {
                    Some(
                        self.selected_obfuscate_action_amount()
                            .unwrap_or_else(|| self.current_obfuscate_amount()),
                    )
                } else {
                    Some(self.current_obfuscate_amount())
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_active_size(&mut self, size: f64) -> bool {
        if self.set_active_size_without_rebuild(size) {
            self.rebuild_effect_layer();
            true
        } else {
            false
        }
    }

    pub fn set_active_size_without_rebuild(&mut self, size: f64) -> bool {
        match self.active_size_control_mode() {
            Some(SizeControlMode::Stroke) => {
                let changed = self.set_stroke_size(size);
                let _ = self.set_selected_action_stroke_size(self.stroke_size);
                changed
            }
            Some(SizeControlMode::Obfuscate) => {
                // Update the per-method amount for the current method only.
                // This ensures Pixelate, BlurSecure, and BlurSmooth each have
                // independent intensity values and don't interfere with each other.
                let changed = self.set_current_obfuscate_amount_and_check(size);
                // Also update any currently selected obfuscate action in-place.
                let current_amount = self.current_obfuscate_amount();
                let _ = self.set_selected_obfuscate_action_amount_without_rebuild(current_amount);
                changed
            }
            None => false,
        }
    }

    pub fn selected_action_color(&self) -> Option<DrawColor> {
        match self.selected_action()? {
            AnnotationAction::Pen { color, .. }
            | AnnotationAction::Highlighter { color, .. }
            | AnnotationAction::Circle { color, .. }
            | AnnotationAction::Line { color, .. }
            | AnnotationAction::Arrow { color, .. }
            | AnnotationAction::Box { color, .. }
            | AnnotationAction::Text { color, .. }
            | AnnotationAction::Number { color, .. } => Some(*color),
            AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. } => None,
        }
    }

    pub fn set_selected_action_color(&mut self, color: DrawColor) -> bool {
        if let Some(input) = self.active_text_input.as_mut() {
            input.color = color;
            return true;
        }

        let Some(index) = self.selected_action_index else {
            return false;
        };

        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };

        let target = match action {
            AnnotationAction::Pen { color, .. }
            | AnnotationAction::Highlighter { color, .. }
            | AnnotationAction::Circle { color, .. }
            | AnnotationAction::Line { color, .. }
            | AnnotationAction::Arrow { color, .. }
            | AnnotationAction::Box { color, .. }
            | AnnotationAction::Text { color, .. }
            | AnnotationAction::Number { color, .. } => color,
            AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. } => return false,
        };

        if *target == color {
            return false;
        }

        *target = color;
        self.redo_actions.clear();
        true
    }

    pub fn history_availability(&self) -> (bool, bool) {
        (!self.actions.is_empty(), !self.redo_actions.is_empty())
    }

    pub fn can_remove_selected_action(&self) -> bool {
        self.selected_action_index
            .is_some_and(|index| index < self.actions.len())
    }

    pub fn mark_working_image_dirty(&mut self) {
        self.working_image_revision = self.working_image_revision.wrapping_add(1);
    }

    pub fn push_action(&mut self, mut action: AnnotationAction) {
        self.expand_canvas_for_action_if_needed(&mut action);

        let next_number_after_push = match &action {
            AnnotationAction::Number { number, style, .. } if *style == self.numbering_style => {
                Some(number.saturating_add(1))
            }
            _ => None,
        };

        self.actions.push(action);
        self.redo_actions.clear();
        self.selected_action_index = Some(self.actions.len() - 1);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;

        if let Some(next_number) = next_number_after_push {
            self.next_number = next_number;
        } else {
            self.sync_next_number();
        }
        // NOTE: Effect-requiring actions (Obfuscate, Focus) should NOT rebuild here
        // synchronously as it blocks the UI. The caller should use the async pipeline
        // via rebuild_effects_async callback after calling this method.
    }

    /// Check if an action modifies pixels and requires effect layer rebuild
    pub fn action_requires_effect_rebuild(action: &AnnotationAction) -> bool {
        matches!(
            action,
            AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
        )
    }

    pub fn undo(&mut self) -> bool {
        if self.undo_without_rebuild() {
            // Check if any remaining actions require effect rebuild
            if self
                .actions
                .iter()
                .any(|a| Self::action_requires_effect_rebuild(a))
            {
                self.rebuild_effect_layer();
            }
            true
        } else {
            false
        }
    }

    pub fn undo_without_rebuild(&mut self) -> bool {
        if let Some(action) = self.actions.pop() {
            let next_number_after_undo = match &action {
                AnnotationAction::Number { number, style, .. }
                    if *style == self.numbering_style =>
                {
                    Some(*number)
                }
                _ => None,
            };

            self.redo_actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;

            if let Some(next_number) = next_number_after_undo {
                self.next_number = next_number;
            } else {
                self.sync_next_number();
            }
            return true;
        }
        false
    }

    pub fn redo(&mut self) -> bool {
        if self.redo_without_rebuild() {
            // Only rebuild if the redone action requires it
            if let Some(action) = self.actions.last() {
                if Self::action_requires_effect_rebuild(action) {
                    self.rebuild_effect_layer();
                }
            }
            true
        } else {
            false
        }
    }

    pub fn redo_without_rebuild(&mut self) -> bool {
        if let Some(action) = self.redo_actions.pop() {
            let next_number_after_redo = match &action {
                AnnotationAction::Number { number, style, .. }
                    if *style == self.numbering_style =>
                {
                    Some(number.saturating_add(1))
                }
                _ => None,
            };

            self.actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;

            if let Some(next_number) = next_number_after_redo {
                self.next_number = next_number;
            } else {
                self.sync_next_number();
            }
            return true;
        }
        false
    }

    pub fn sync_next_number(&mut self) {
        let max_number = self
            .actions
            .iter()
            .filter_map(|action| match action {
                AnnotationAction::Number { number, style, .. } => {
                    // Only consider numbers with the same style
                    if *style == self.numbering_style {
                        Some(*number)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        // Use the user-specified starting number if no numbers exist yet
        self.next_number = if max_number == 0 {
            self.numbering_start
        } else {
            max_number.saturating_add(1)
        };
    }

    pub fn add_number_marker(&mut self, position: Point) {
        let number = self.next_number;
        let radius = self.number_size.radius();
        let image_width = self.working_image.width() as f64;
        let image_height = self.working_image.height() as f64;

        let clamped_x = if image_width <= radius * 2.0 {
            image_width / 2.0
        } else {
            position.x.clamp(radius, image_width - radius)
        };
        let clamped_y = if image_height <= radius * 2.0 {
            image_height / 2.0
        } else {
            position.y.clamp(radius, image_height - radius)
        };

        self.push_action(AnnotationAction::Number {
            position: Point {
                x: clamped_x,
                y: clamped_y,
            },
            number,
            color: self.selected_color,
            style: self.numbering_style,
            size: self.number_size,
            shadow: self.draw_object_shadow,
        });
    }

    pub fn selected_action(&self) -> Option<&AnnotationAction> {
        self.selected_action_index
            .and_then(|index| self.actions.get(index))
    }

    pub fn selected_text_action_data(
        &self,
    ) -> Option<(usize, String, DrawColor, FontSettings, Option<f64>, Point)> {
        let index = self.selected_action_index?;
        let AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width,
            ..
        } = self.actions.get(index)?
        else {
            return None;
        };

        Some((
            index,
            text.clone(),
            *color,
            font.clone(),
            *max_width,
            *position,
        ))
    }

    #[allow(dead_code)]
    pub fn commit_active_text_input(&mut self) -> bool {
        if let Some(action) = self.commit_text_input() {
            self.push_action(action);
            return true;
        }
        false
    }

    pub fn begin_editing_selected_text(&mut self) -> bool {
        let Some((index, text, color, font, max_width, position)) =
            self.selected_text_action_data()
        else {
            return false;
        };
        let Some(width) = max_width else {
            return false;
        };

        // Use the stored max_width as the box width directly.
        // Do NOT recompute from text_action_bounds() — that would shrink the box
        // to fit the text tightly, then commit_text_input() would write that
        // smaller width back, permanently changing the action's max_width.
        let padding_y = 8.0;
        let bounds_position = Point {
            x: position.x,
            y: position.y - font.size - padding_y,
        };
        let height = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1)
            .ok()
            .and_then(|surface| gtk4::cairo::Context::new(&surface).ok())
            .map(|context| {
                let content_width = (width - 20.0).max(font.size * 0.8);
                let layout = layout_wrapped_text(&context, &text, &font, content_width);
                let line_height = (font.size * 1.2).max(font.size + 4.0);
                (layout.lines.len().max(1) as f64 * line_height + font.size * 0.2 + padding_y * 2.0)
                    .max(44.0)
            })
            .unwrap_or_else(|| (font.size * 1.45 + 16.0).max(44.0));
        let bounds = TextEditBounds::new(bounds_position, width, height);
        self.active_text_bounds = Some(bounds);
        self.active_text_input = Some(TextInputState {
            cursor_position: text.chars().count(),
            text,
            cursor_visible: true,
            cursor_blink_timer: 0,
            color,
            editing_action_index: Some(index),
        });
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
        self.text_font_family = font.family.clone();
        self.text_size = font.size;
        self.selected_color = color;
        true
    }

    #[allow(dead_code)]
    pub fn update_text_action(&mut self, index: usize, new_text: String) -> bool {
        if index >= self.actions.len() {
            return false;
        }

        let trimmed = new_text.trim().to_string();
        if trimmed.is_empty() {
            let removed = self.actions.remove(index);
            if !matches!(removed, AnnotationAction::Text { .. }) {
                self.actions.insert(index, removed);
                return false;
            }

            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.redo_actions.clear();
            return true;
        }

        let Some(AnnotationAction::Text { text, .. }) = self.actions.get_mut(index) else {
            return false;
        };

        if *text == trimmed {
            return false;
        }

        *text = trimmed;
        self.redo_actions.clear();
        true
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

    #[allow(dead_code)]
    pub fn select_text_action_at_point_with_scale(
        &mut self,
        point: Point,
        _view_scale: f64,
    ) -> bool {
        self.selected_action_index = self
            .actions
            .iter()
            .enumerate()
            .rev()
            .find(|(_, action)| {
                matches!(action, AnnotationAction::Text { .. })
                    && action_contains_point_with_padding(action, point, 0.0)
            })
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
            let resized = if let Some(aspect_ratio) = aspect_ratio {
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
            };
            resized
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

            // Clamp the action so it cannot be moved/resized outside the image bounds.
            if moved {
                clamp_action_to_image(action, img_w, img_h);
            }

            let effect_action = matches!(action, AnnotationAction::Obfuscate { .. });
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

    #[allow(dead_code)]
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

    pub fn rebuild_effect_layer(&mut self) {
        let mut working = self.base_image.clone();
        apply_effect_actions(&mut working, &self.actions);
        self.working_image = working;
        self.select_effect_rebuild_pending = false;
        self.mark_working_image_dirty();
    }

    #[allow(dead_code)]
    pub fn commit_text_edit(
        &mut self,
        bounds: &TextEditBounds,
        text: String,
        color: DrawColor,
        font: FontSettings,
    ) {
        if text.trim().is_empty() {
            self.cancel_text_edit();
            return;
        }

        let position = Point {
            x: bounds.rect.x as f64,
            y: bounds.rect.y as f64,
        };

        self.actions.push(AnnotationAction::Text {
            position,
            text,
            color,
            font,
            max_width: Some(bounds.rect.width as f64),
            shadow: self.draw_object_shadow,
        });

        self.active_text_edit = None;
        self.active_text_entry = None;
        self.active_text_bounds = None;
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
    }

    pub fn cancel_text_edit(&mut self) {
        self.active_text_edit = None;
        self.active_text_entry = None;
        self.active_text_bounds = None;
        self.active_text_is_dragging = false;
        self.active_text_drag_handle = None;
        self.active_text_drag_start = None;
    }
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::capture::editor::types::{
        AnnotationAction, ArrowStyle, DrawColor, Point, Rect, SelectHandle,
    };

    use super::EditorState;

    #[test]
    fn editor_state_defaults_to_background_tool() {
        let source = include_str!("state.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("selected_tool: Tool::Background,"),
            "Editor state should default to the Background tool so startup inspector width matches the initial tool surface",
        );
    }

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

/// Clamp an annotation action so it stays within the image bounds.
/// For rect-based actions (Obfuscate, Focus, Box, Circle) the rect is clamped.
/// For point-based actions (Text, Number, Pen, Arrow, Line) each point is clamped.
fn clamp_action_to_image(action: &mut AnnotationAction, img_w: i32, img_h: i32) {
    match action {
        AnnotationAction::Obfuscate { rect, .. }
        | AnnotationAction::Focus { rect }
        | AnnotationAction::Box { rect, .. }
        | AnnotationAction::Circle { rect, .. } => {
            // Keep the rect fully inside the image.
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
            // Compute the real rendered bounds so we clamp correctly for
            // any number of lines at any font size.
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
            let bounds = super::render::text_action_bounds(
                &context,
                *position,
                text,
                font,
                Some(available_width),
            );
            let box_w = bounds.rect.width as f64;
            let box_h = bounds.rect.height as f64;

            // Clamp box_left to [0, img_w - box_w]
            let new_box_left = (bounds.rect.x as f64)
                .max(0.0)
                .min((img_w as f64 - box_w).max(0.0));
            position.x = new_box_left; // position.x == box_left for Text

            // Clamp box_top to [0, img_h - box_h], then recompute baseline
            // position.y = box_top + font.size + padding_y
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
            // Account for stroke width — the arrow's visual bounds extend
            // beyond the curve centerline by roughly half the stroke size.
            let margin = *stroke_size * 0.5;
            // Compute the actual visual bounds of the arrow including Bezier
            // curve extrema, not just the endpoints. A quadratic Bezier can
            // bulge well beyond its start/end points.
            let mut min_x = start.x.min(end.x);
            let mut max_x = start.x.max(end.x);
            let mut min_y = start.y.min(end.y);
            let mut max_y = start.y.max(end.y);

            if let Some(cps) = control_points.as_ref() {
                if cps.len() >= 3 {
                    let p0 = *start;
                    let p1 = cps[1]; // middle control point
                    let p2 = *end;
                    // Quadratic Bezier extrema: t = (P0 - P1) / (P0 - 2*P1 + P2)
                    // Check x-axis extremum
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
                    // Check y-axis extremum
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

pub fn apply_effect_actions(image: &mut RgbaImage, actions: &[AnnotationAction]) {
    for action in actions {
        match action {
            AnnotationAction::Obfuscate {
                rect,
                method,
                amount,
            } => match method {
                ObfuscateMethod::Pixelate => {
                    apply_censor_rect(image, *rect, *amount);
                }
                ObfuscateMethod::BlurSecure => {
                    apply_secure_blur(image, *rect, *amount);
                }
                ObfuscateMethod::BlurSmooth => {
                    apply_blur_rect(image, *rect, *amount);
                }
                ObfuscateMethod::Blackout => {
                    apply_blackout_rect(image, rect);
                }
            },
            _ => {}
        }
    }
}

impl EditorState {
    fn current_highlighter_stroke_size(&self) -> f64 {
        self.locked_highlighter_stroke_size
            .unwrap_or_else(|| match self.highlighter_mode {
                HighlighterMode::TextAware => self.stroke_size,
                HighlighterMode::Freehand => self.pen_weight.stroke_width(),
            })
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
        let end = super::types::constrained_drag_endpoint(
            self.selected_tool,
            start,
            self.drag_current?,
            self.drag_shift_active,
        );
        let color = self.selected_color;
        let stroke_size = self.stroke_size;

        let result = match self.selected_tool {
            Tool::Select => None,
            Tool::Crop => None,
            Tool::Background => None,
            Tool::Pen => {
                let points = self.processed_drag_path(self.drag_path.clone());
                if points.len() >= 2 {
                    Some(AnnotationAction::Pen {
                        points,
                        color,
                        stroke_size: self.pen_weight.stroke_width(),
                    })
                } else {
                    None
                }
            }
            Tool::Highlighter => {
                let source_points = self.processed_drag_path(self.drag_path.clone());
                if source_points.len() >= 2 {
                    let points = if self.drag_shift_active {
                        let first = source_points[0];
                        let last = source_points[source_points.len() - 1];
                        vec![
                            first,
                            super::types::constrained_drag_endpoint(
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
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
            Tool::Text => None,
        };

        result
    }

    pub fn finalize_drag_action(&mut self) -> Option<AnnotationAction> {
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter) {
            let drag_path = std::mem::take(&mut self.drag_path);
            let mut points = self.processed_drag_path(drag_path);
            let color = self.selected_color;
            let tool = self.selected_tool;
            let shift_active = self.drag_shift_active;
            let pen_stroke_size = self.pen_weight.stroke_width();
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
                            let constrained_last = super::types::constrained_drag_endpoint(
                                Tool::Highlighter,
                                first,
                                last,
                                true,
                            );
                            points = vec![first, constrained_last];
                        }

                        let stroke_size = highlighter_stroke_size
                            .unwrap_or_else(|| self.pen_weight.stroke_width());

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
        let end = super::types::constrained_drag_endpoint(
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
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
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

    fn arrow_points(&self, start: Point, end: Point) -> (Point, Point) {
        if self.inverse_arrow_direction {
            (end, start)
        } else {
            (start, end)
        }
    }

    fn processed_drag_path(&self, points: Vec<Point>) -> Vec<Point> {
        if !self.smooth_drawing_enabled {
            return points;
        }

        simplify_drag_path(&points, 0.6)
    }

    fn expand_canvas_for_action_if_needed(&mut self, action: &mut AnnotationAction) {
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

        self.base_image = expand_rgba_image(
            &self.base_image,
            next_width,
            next_height,
            expand_left,
            expand_top,
        );
        self.working_image = expand_rgba_image(
            &self.working_image,
            next_width,
            next_height,
            expand_left,
            expand_top,
        );

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

    pub fn apply_crop_selection(&mut self) -> Result<bool, EditorError> {
        if self.crop_selection.is_none() {
            return Ok(false);
        }

        let cropped_image = self.to_final_image()?;
        if cropped_image.width() == 0 || cropped_image.height() == 0 {
            return Ok(false);
        }

        self.base_image = cropped_image.clone();
        self.working_image = cropped_image;
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

    pub fn to_rendered_image(&self) -> Result<RgbaImage, EditorError> {
        let (width, height) = self.working_image.dimensions();
        if width == 0 || height == 0 {
            return Err(EditorError::ImageSave(
                "image has invalid dimensions".into(),
            ));
        }

        let stride = gtk4::cairo::Format::ARgb32
            .stride_for_width(width)
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        let mut source_image = self.working_image.clone();
        for action in &self.actions {
            if let AnnotationAction::Focus { rect } = action {
                apply_focus_rect(&mut source_image, *rect);
            }
        }

        let data = super::render::rgba_to_cairo_argb_bytes(&source_image);
        let mut surface = gtk4::cairo::ImageSurface::create_for_data(
            data,
            gtk4::cairo::Format::ARgb32,
            width as i32,
            height as i32,
            stride,
        )
        .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        {
            let context = gtk4::cairo::Context::new(&surface)
                .map_err(|e| EditorError::ImageSave(e.to_string()))?;

            for action in &self.actions {
                if matches!(
                    action,
                    AnnotationAction::Obfuscate { .. } | AnnotationAction::Focus { .. }
                ) {
                    continue;
                }
                super::render::draw_annotation_action(&context, action);
            }
        }

        surface.flush();
        let surface_data = surface
            .data()
            .map_err(|e| EditorError::ImageSave(e.to_string()))?;

        Ok(super::render::cairo_argb_to_rgba_image(
            width,
            height,
            stride as usize,
            surface_data.as_ref(),
        ))
    }

    pub fn to_final_image(&self) -> Result<RgbaImage, EditorError> {
        let rendered = self.to_rendered_image()?;

        let final_base = if self.background_style != BackgroundStyle::None {
            self.render_with_background(&rendered)?
        } else {
            rendered
        };

        if let Some(crop) = self.crop_selection {
            let crop_width = crop.width.max(0) as u32;
            let crop_height = crop.height.max(0) as u32;
            if crop_width == 0 || crop_height == 0 {
                return Ok(final_base);
            }

            let background = image::Rgba([
                (self.crop_background_color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
                (self.crop_background_color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
                (self.crop_background_color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
                (self.crop_background_color.a.clamp(0.0, 1.0) * 255.0).round() as u8,
            ]);
            let mut output = RgbaImage::from_pixel(crop_width, crop_height, background);

            let source_x = crop.x.max(0) as u32;
            let source_y = crop.y.max(0) as u32;
            let source_right = (crop.x + crop.width).clamp(0, final_base.width() as i32) as u32;
            let source_bottom = (crop.y + crop.height).clamp(0, final_base.height() as i32) as u32;

            if source_right > source_x && source_bottom > source_y {
                let source_width = source_right - source_x;
                let source_height = source_bottom - source_y;
                let source = image::imageops::crop_imm(
                    &final_base,
                    source_x,
                    source_y,
                    source_width,
                    source_height,
                )
                .to_image();
                let dest_x = source_x as i64 - crop.x as i64;
                let dest_y = source_y as i64 - crop.y as i64;
                image::imageops::overlay(&mut output, &source, dest_x, dest_y);
            }

            return Ok(output);
        }

        Ok(final_base)
    }

    fn render_with_background(&self, screenshot: &RgbaImage) -> Result<RgbaImage, EditorError> {
        let screenshot_w = screenshot.width() as f64;
        let screenshot_h = screenshot.height() as f64;

        // Base scaling factor for padding based on screenshot size
        let ref_size = screenshot_w.max(screenshot_h);
        let scale_factor = ref_size / 400.0;

        // Padding increases the CANVAS size
        let padding_px = self.background_padding * scale_factor;
        let mut canvas_w = screenshot_w + padding_px * 2.0;
        let mut canvas_h = screenshot_h + padding_px * 2.0;

        // Apply background aspect ratio expansion if set
        if let Some(ratio) = self
            .background_aspect_ratio
            .aspect_ratio(canvas_w as i32, canvas_h as i32)
        {
            let current_ratio = canvas_w / canvas_h;
            if current_ratio < ratio {
                canvas_w = canvas_h * ratio;
            } else {
                canvas_h = canvas_w / ratio;
            }
        }

        // Insert shrinks the SCREENSHOT within the canvas
        // (0.0 means 100% size, 100.0 means 50% size for safety)
        let insert_ratio = self.background_insert / 200.0;
        let draw_scale = 1.0 - insert_ratio;
        let draw_w = screenshot_w * draw_scale;
        let draw_h = screenshot_h * draw_scale;

        let mut canvas = match &self.background_style {
            BackgroundStyle::PlainColor(color) => {
                let pixel = image::Rgba([
                    (color.r.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.g.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.b.clamp(0.0, 1.0) * 255.0) as u8,
                    (color.a.clamp(0.0, 1.0) * 255.0) as u8,
                ]);
                RgbaImage::from_pixel(canvas_w as u32, canvas_h as u32, pixel)
            }
            BackgroundStyle::Gradient(idx) => {
                let file_name = crate::capture::editor::window::background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES[*idx];
                let path = crate::capture::editor::window::background_panel::background_gradient_asset_path(file_name);
                self.load_and_resize_background(&path, canvas_w as u32, canvas_h as u32)?
            }
            BackgroundStyle::Wallpaper(path) => {
                self.load_and_resize_background(path, canvas_w as u32, canvas_h as u32)?
            }
            BackgroundStyle::Blurred(_idx) => {
                let mut blurred = screenshot.clone();
                apply_blur_rect(
                    &mut blurred,
                    Rect {
                        x: 0,
                        y: 0,
                        width: screenshot_w as i32,
                        height: screenshot_h as i32,
                    },
                    30.0,
                );
                image::imageops::resize(
                    &blurred,
                    canvas_w as u32,
                    canvas_h as u32,
                    image::imageops::FilterType::Triangle,
                )
            }
            BackgroundStyle::None => return Ok(screenshot.clone()),
        };

        // Draw screenshot onto background with alignment
        // (Alignment is calculated based on the available space in the canvas)
        let (dest_x, dest_y) = match self.background_alignment {
            BackgroundAlignment::TopLeft => (padding_px, padding_px),
            BackgroundAlignment::TopCenter => ((canvas_w - draw_w) / 2.0, padding_px),
            BackgroundAlignment::TopRight => (canvas_w - draw_w - padding_px, padding_px),
            BackgroundAlignment::CenterLeft => (padding_px, (canvas_h - draw_h) / 2.0),
            BackgroundAlignment::Center => ((canvas_w - draw_w) / 2.0, (canvas_h - draw_h) / 2.0),
            BackgroundAlignment::CenterRight => {
                (canvas_w - draw_w - padding_px, (canvas_h - draw_h) / 2.0)
            }
            BackgroundAlignment::BottomLeft => (padding_px, canvas_h - draw_h - padding_px),
            BackgroundAlignment::BottomCenter => {
                ((canvas_w - draw_w) / 2.0, canvas_h - draw_h - padding_px)
            }
            BackgroundAlignment::BottomRight => (
                canvas_w - draw_w - padding_px,
                canvas_h - draw_h - padding_px,
            ),
        };

        // Scale and handle corner radius
        let scaled_screenshot = if (draw_scale - 1.0).abs() > 0.001 {
            image::imageops::resize(
                screenshot,
                draw_w as u32,
                draw_h as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            screenshot.clone()
        };

        let mut final_screenshot = scaled_screenshot;
        if self.background_corner_radius > 0.0 {
            let radius = self.background_corner_radius * scale_factor * draw_scale;
            apply_corner_radius(&mut final_screenshot, radius);
        }

        // Draw shadow if requested
        if self.background_shadow > 0.0 {
            let shadow_color = image::Rgba([0, 0, 0, 100]);
            let shadow_offset = (self.background_shadow * 0.15 * scale_factor * draw_scale) as i64;

            let mut shadow_layer =
                RgbaImage::from_pixel(draw_w as u32, draw_h as u32, shadow_color);
            if self.background_corner_radius > 0.0 {
                let radius = self.background_corner_radius * scale_factor * draw_scale;
                apply_corner_radius(&mut shadow_layer, radius);
            }
            image::imageops::overlay(
                &mut canvas,
                &shadow_layer,
                dest_x as i64,
                (dest_y + shadow_offset as f64) as i64,
            );
        }

        image::imageops::overlay(&mut canvas, &final_screenshot, dest_x as i64, dest_y as i64);

        Ok(canvas)
    }

    fn load_and_resize_background(
        &self,
        path: &Path,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, EditorError> {
        let img = image::open(path).map_err(|e| EditorError::ImageLoad(e.to_string()))?;
        let rgba = img.into_rgba8();
        Ok(image::imageops::resize(
            &rgba,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ))
    }

    pub fn set_highlighter_mode(&mut self, mode: HighlighterMode) {
        self.highlighter_mode = mode;
    }

    pub fn set_pen_weight(&mut self, weight: PenWeight) {
        self.pen_weight = weight;
    }
}

fn apply_corner_radius(image: &mut RgbaImage, radius: f64) {
    let (width, height) = image.dimensions();
    if radius <= 0.0 {
        return;
    }

    let r2 = radius * radius;
    for y in 0..height {
        for x in 0..width {
            let fx = x as f64;
            let fy = y as f64;
            let mut alpha_scale = 1.0;

            // Top-left
            if fx < radius && fy < radius {
                let dx = radius - fx;
                let dy = radius - fy;
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    alpha_scale = (radius - (dist2.sqrt() - radius)).clamp(0.0, 1.0); // Anti-aliasing hack
                    if dist2 > (radius + 1.0) * (radius + 1.0) {
                        alpha_scale = 0.0;
                    }
                }
            }
            // Top-right
            else if fx > (width as f64 - radius) && fy < radius {
                let dx = fx - (width as f64 - radius);
                let dy = radius - fy;
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    alpha_scale = 0.0;
                }
            }
            // Bottom-left
            else if fx < radius && fy > (height as f64 - radius) {
                let dx = radius - fx;
                let dy = fy - (height as f64 - radius);
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    alpha_scale = 0.0;
                }
            }
            // Bottom-right
            else if fx > (width as f64 - radius) && fy > (height as f64 - radius) {
                let dx = fx - (width as f64 - radius);
                let dy = fy - (height as f64 - radius);
                let dist2 = dx * dx + dy * dy;
                if dist2 > r2 {
                    alpha_scale = 0.0;
                }
            }

            if alpha_scale < 1.0 {
                let pixel = image.get_pixel_mut(x, y);
                pixel[3] = (pixel[3] as f64 * alpha_scale) as u8;
            }
        }
    }
}
