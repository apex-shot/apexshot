use super::color::{
    clamp_obfuscate_amount, clamp_stroke_size, clamp_text_size,
    selection_handle_hit_radius_for_scale, selection_hit_padding_for_scale, DEFAULT_COLOR_INDEX,
    DEFAULT_OBFUSCATE_AMOUNT, DRAW_COLORS, SELECT_MIN_RESIZE_SIZE, STROKE_WIDTH, TEXT_SIZE,
};
use super::render::{apply_blur_rect, apply_censor_rect, apply_focus_rect, apply_secure_pixelate};
use super::selection::{
    action_contains_point_with_padding, action_resize_handle_at_point_with_radius, resize_action,
    resize_rect_with_handle, translate_action,
};
use super::types::{
    AnnotationAction, BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor,
    EditorError, MoveHandle, ObfuscateMethod, Point, Rect, ResizeHandle, SelectHandle,
    SizeControlMode, TextEditBounds, Tool,
};
use gtk4;
use image::RgbaImage;
use std::path::Path;

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
    pub text_size: f64,
    pub text_font_family: String,
    pub obfuscate_amount: f64,
    pub next_number: u32,
    pub select_drag_anchor: Option<Point>,
    pub select_resize_handle: Option<super::types::SelectHandle>,
    pub select_effect_rebuild_pending: bool,
    pub active_text_edit: Option<()>,
    pub active_text_entry: Option<gtk4::Entry>,
    pub active_text_bounds: Option<TextEditBounds>,
    pub active_text_is_dragging: bool,
    pub active_text_drag_handle: Option<MoveHandle>,
    pub active_text_drag_start: Option<Point>,
    pub pending_effect_revision: u64,
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
}

fn resize_crop_rect_with_handle(
    rect: &mut Rect,
    handle: SelectHandle,
    dx: f64,
    dy: f64,
    _image_width: i32,
    _image_height: i32,
) -> bool {
    let left = rect.x as f64;
    let top = rect.y as f64;
    let right = left + rect.width as f64;
    let bottom = top + rect.height as f64;

    let updated = match handle {
        SelectHandle::Left | SelectHandle::Right => {
            let width = right - left;
            let expansion = if handle == SelectHandle::Right {
                dx
            } else {
                -dx
            };
            let clamped_expansion = expansion.max((SELECT_MIN_RESIZE_SIZE - width) / 2.0);
            Rect::from_bounds(
                left - clamped_expansion,
                top,
                right + clamped_expansion,
                bottom,
            )
        }
        SelectHandle::Top | SelectHandle::Bottom => {
            let height = bottom - top;
            let expansion = if handle == SelectHandle::Bottom {
                dy
            } else {
                -dy
            };
            let clamped_expansion = expansion.max((SELECT_MIN_RESIZE_SIZE - height) / 2.0);
            Rect::from_bounds(
                left,
                top - clamped_expansion,
                right,
                bottom + clamped_expansion,
            )
        }
        _ => return false,
    };

    let Some(updated) = updated else {
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

fn constrained_crop_point(start: Point, end: Point, aspect_ratio: f64) -> Point {
    if aspect_ratio <= 0.0 {
        return end;
    }

    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let sign_x = if dx < 0.0 { -1.0 } else { 1.0 };
    let sign_y = if dy < 0.0 { -1.0 } else { 1.0 };
    let width = dx.abs().max(dy.abs() * aspect_ratio);
    let height = width / aspect_ratio;

    Point {
        x: start.x + sign_x * width,
        y: start.y + sign_y * height,
    }
}

fn resize_crop_rect_with_fixed_aspect(
    rect: &mut Rect,
    handle: SelectHandle,
    point: Point,
    _image_width: i32,
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
    let half_width = match handle {
        SelectHandle::Left | SelectHandle::Right => (point.x - center.x).abs().max(min_half_width),
        SelectHandle::Top | SelectHandle::Bottom => {
            ((point.y - center.y).abs().max(min_half_height)) * aspect_ratio
        }
        _ => (point.x - center.x)
            .abs()
            .max((point.y - center.y).abs() * aspect_ratio)
            .max(min_half_width),
    };
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
            selected_tool: Tool::Arrow,
            selected_action_index: None,
            selected_color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
            text_size: TEXT_SIZE,
            text_font_family: String::from("Sans"),
            obfuscate_amount: DEFAULT_OBFUSCATE_AMOUNT,
            next_number: 1,
            select_drag_anchor: None,
            select_resize_handle: None,
            select_effect_rebuild_pending: false,
            active_text_edit: None,
            active_text_entry: None,
            active_text_bounds: None,
            active_text_is_dragging: false,
            active_text_drag_handle: None,
            active_text_drag_start: None,
            pending_effect_revision: 0,
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
        self.selected_tool = tool;
        self.clear_drag_without_rebuild_and_check_effect()
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

    pub fn set_obfuscate_amount(&mut self, amount: f64) -> bool {
        let next = clamp_obfuscate_amount(amount);
        if (next - self.obfuscate_amount).abs() <= f64::EPSILON {
            return false;
        }

        self.obfuscate_amount = next;
        true
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
                            .unwrap_or(self.obfuscate_amount),
                    )
                } else {
                    Some(self.obfuscate_amount)
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
                let changed = self.set_obfuscate_amount(size);
                let _ = self
                    .set_selected_obfuscate_action_amount_without_rebuild(self.obfuscate_amount);
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

    pub fn push_action(&mut self, action: AnnotationAction) {
        self.actions.push(action);
        self.redo_actions.clear();
        self.selected_action_index = Some(self.actions.len() - 1);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.sync_next_number();
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
            self.redo_actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.sync_next_number();
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
            self.actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.sync_next_number();
            return true;
        }
        false
    }

    pub fn sync_next_number(&mut self) {
        let max_number = self
            .actions
            .iter()
            .filter_map(|action| match action {
                AnnotationAction::Number { number, .. } => Some(*number),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.next_number = max_number.saturating_add(1);
    }

    pub fn add_number_marker(&mut self, position: Point) {
        let number = self.next_number;
        self.push_action(AnnotationAction::Number {
            position,
            number,
            color: self.selected_color,
        });
    }

    pub fn selected_action(&self) -> Option<&AnnotationAction> {
        self.selected_action_index
            .and_then(|index| self.actions.get(index))
    }

    pub fn selected_text_action_data(&self) -> Option<(usize, String)> {
        let index = self.selected_action_index?;
        let AnnotationAction::Text { text, .. } = self.actions.get(index)? else {
            return None;
        };

        Some((index, text.clone()))
    }

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

    pub fn begin_crop_drag_with_scale(&mut self, point: Point, view_scale: f64) -> bool {
        let Some(crop_rect) = self.crop_selection else {
            return false;
        };

        let crop_action = AnnotationAction::Box {
            rect: crop_rect,
            color: self.selected_color,
            stroke_size: self.stroke_size,
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

        let resize_handle = self.select_resize_handle;
        let (moved, effect_action) = if let Some(action) = self.actions.get_mut(index) {
            let moved = if let Some(handle) = resize_handle {
                resize_action(action, handle, dx, dy)
            } else {
                translate_action(action, dx, dy)
            };
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
            self.select_effect_rebuild_pending = true;
        }
        true
    }

    #[allow(dead_code)]
    pub fn end_select_drag(&mut self) -> bool {
        let rebuild = self.select_effect_rebuild_pending;
        if rebuild {
            self.rebuild_effect_layer();
            self.select_effect_rebuild_pending = false;
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
        let rebuild = self.select_effect_rebuild_pending;
        self.select_effect_rebuild_pending = false;
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

        let _removed = self.actions.remove(index);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.redo_actions.clear();
        self.sync_next_number();
        true
    }

    pub fn rebuild_effect_layer(&mut self) {
        let mut working = self.base_image.clone();
        apply_effect_actions(&mut working, &self.actions);
        self.working_image = working;
        self.select_effect_rebuild_pending = false;
        self.mark_working_image_dirty();
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
                ObfuscateMethod::Blur => {
                    apply_blur_rect(image, *rect, *amount);
                }
                ObfuscateMethod::Pixelate => {
                    apply_censor_rect(image, *rect, *amount);
                }
                ObfuscateMethod::SecurePixelate => {
                    apply_secure_pixelate(image, *rect, *amount);
                }
            },
            _ => {}
        }
    }
}

impl EditorState {
    pub fn begin_drag(&mut self, point: Point) {
        self.selected_action_index = None;
        self.drag_start = Some(point);
        self.drag_current = Some(point);
        self.drag_path.clear();
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter) {
            self.drag_path.push(point);
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
        self.drag_path.clear();
        self.drag_shift_active = false;
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

        match self.selected_tool {
            Tool::Select => None,
            Tool::Crop => None,
            Tool::Background => None,
            Tool::Pen => {
                if self.drag_path.len() >= 2 {
                    Some(AnnotationAction::Pen {
                        points: self.drag_path.clone(),
                        color,
                        stroke_size,
                    })
                } else {
                    None
                }
            }
            Tool::Highlighter => {
                if self.drag_path.len() >= 2 {
                    let points = if self.drag_shift_active {
                        let first = self.drag_path[0];
                        let last = self.drag_path[self.drag_path.len() - 1];
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
                        self.drag_path.clone()
                    };

                    Some(AnnotationAction::Highlighter {
                        points,
                        color,
                        stroke_size,
                    })
                } else {
                    None
                }
            }
            Tool::Circle => Rect::from_points(start, end).map(|rect| AnnotationAction::Circle {
                rect,
                color,
                stroke_size,
            }),
            Tool::Line => Some(AnnotationAction::Line {
                start,
                end,
                color,
                stroke_size,
            }),
            Tool::Arrow => Some(AnnotationAction::Arrow {
                start,
                end,
                color,
                stroke_size,
            }),
            Tool::Box => Rect::from_points(start, end).map(|rect| AnnotationAction::Box {
                rect,
                color,
                stroke_size,
            }),
            Tool::Number => None,
            Tool::Obfuscate => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Obfuscate {
                    rect,
                    method: ObfuscateMethod::Blur,
                    amount: self.obfuscate_amount,
                })
            }
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
            Tool::Text => None,
        }
    }

    pub fn draft_crop_rect(&self) -> Option<Rect> {
        if self.selected_tool != Tool::Crop {
            return None;
        }

        let start = self.drag_start?;
        let end = match self.crop_aspect_ratio_value() {
            Some(aspect_ratio) => constrained_crop_point(start, self.drag_current?, aspect_ratio),
            None => self.drag_current?,
        };
        Rect::from_points(start, end)
    }

    pub fn finalize_drag_action(&mut self) -> Option<AnnotationAction> {
        if matches!(self.selected_tool, Tool::Pen | Tool::Highlighter) {
            let mut points = std::mem::take(&mut self.drag_path);
            let color = self.selected_color;
            let stroke_size = self.stroke_size;
            let tool = self.selected_tool;
            let shift_active = self.drag_shift_active;
            self.clear_drag();
            return if points.len() >= 2 {
                match tool {
                    Tool::Pen => Some(AnnotationAction::Pen {
                        points,
                        color,
                        stroke_size,
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

        match self.selected_tool {
            Tool::Select => None,
            Tool::Crop => None,
            Tool::Background => None,
            Tool::Pen => None,
            Tool::Highlighter => None,
            Tool::Circle => Rect::from_points(start, end).map(|rect| AnnotationAction::Circle {
                rect,
                color,
                stroke_size,
            }),
            Tool::Line => Some(AnnotationAction::Line {
                start,
                end,
                color,
                stroke_size,
            }),
            Tool::Arrow => Some(AnnotationAction::Arrow {
                start,
                end,
                color,
                stroke_size,
            }),
            Tool::Box => Rect::from_points(start, end).map(|rect| AnnotationAction::Box {
                rect,
                color,
                stroke_size,
            }),
            Tool::Number => None,
            Tool::Obfuscate => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Obfuscate {
                    rect,
                    method: ObfuscateMethod::Blur,
                    amount: self.obfuscate_amount,
                })
            }
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
            Tool::Text => None,
        }
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
        self.next_number = 1;
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
