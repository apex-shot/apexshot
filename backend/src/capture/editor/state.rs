use super::color::{
    clamp_stroke_size, clamp_text_size, selection_handle_hit_radius_for_scale,
    selection_hit_padding_for_scale, DEFAULT_COLOR_INDEX, DRAW_COLORS, STROKE_SIZE_STEP,
    STROKE_WIDTH, TEXT_SIZE, TEXT_SIZE_STEP,
};
use super::color::{BLUR_RADIUS, CENSOR_BLOCK_SIZE};
use super::render::{apply_blur_rect, apply_censor_rect, apply_focus_rect};
use super::selection::{
    action_contains_point_with_padding, action_resize_handle_at_point_with_radius, resize_action,
    translate_action,
};
use super::types::{AnnotationAction, DrawColor, EditorError, Point, Rect, SizeControlMode, Tool};
use image::RgbaImage;

pub struct EditorState {
    pub base_image: RgbaImage,
    pub working_image: RgbaImage,
    pub working_image_revision: u64,
    pub crop_selection: Option<Rect>,
    pub actions: Vec<AnnotationAction>,
    pub redo_actions: Vec<AnnotationAction>,
    pub selected_tool: Tool,
    pub selected_action_index: Option<usize>,
    pub selected_color: DrawColor,
    pub stroke_size: f64,
    pub text_size: f64,
    pub next_number: u32,
    pub select_drag_anchor: Option<Point>,
    pub select_resize_handle: Option<super::types::SelectHandle>,
    pub drag_start: Option<Point>,
    pub drag_current: Option<Point>,
    pub drag_start_view: Option<Point>,
    pub drag_path: Vec<Point>,
    pub drag_shift_active: bool,
}

impl EditorState {
    pub fn new(base_image: RgbaImage) -> Self {
        Self {
            working_image: base_image.clone(),
            base_image,
            working_image_revision: 1,
            crop_selection: None,
            actions: Vec::new(),
            redo_actions: Vec::new(),
            selected_tool: Tool::Arrow,
            selected_action_index: None,
            selected_color: DRAW_COLORS[DEFAULT_COLOR_INDEX],
            stroke_size: STROKE_WIDTH,
            text_size: TEXT_SIZE,
            next_number: 1,
            select_drag_anchor: None,
            select_resize_handle: None,
            drag_start: None,
            drag_current: None,
            drag_start_view: None,
            drag_path: Vec::new(),
            drag_shift_active: false,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if self.selected_tool == Tool::Crop && tool != Tool::Crop {
            self.crop_selection = None;
        }
        if tool != Tool::Select {
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
        }
        self.selected_tool = tool;
        self.clear_drag();
    }

    pub fn set_color_index(&mut self, index: usize) {
        if let Some(color) = DRAW_COLORS.get(index).copied() {
            self.selected_color = color;
        }
    }

    pub fn set_stroke_size(&mut self, size: f64) -> bool {
        let next = clamp_stroke_size(size);
        if (next - self.stroke_size).abs() <= f64::EPSILON {
            return false;
        }

        self.stroke_size = next;
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
            | AnnotationAction::Blur { .. }
            | AnnotationAction::Focus { .. }
            | AnnotationAction::Censor { .. } => None,
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
            | AnnotationAction::Blur { .. }
            | AnnotationAction::Focus { .. }
            | AnnotationAction::Censor { .. } => return false,
        };

        if (*target - next).abs() <= f64::EPSILON {
            return false;
        }

        *target = next;
        self.redo_actions.clear();
        true
    }

    pub fn adjust_stroke_size(&mut self, delta: f64) -> bool {
        let changed = self.set_stroke_size(self.stroke_size + delta);
        if !changed {
            return false;
        }

        let _ = self.set_selected_action_stroke_size(self.stroke_size);
        true
    }

    pub fn set_text_size(&mut self, size: f64) -> bool {
        let next = clamp_text_size(size);
        if (next - self.text_size).abs() <= f64::EPSILON {
            return false;
        }

        self.text_size = next;
        true
    }

    pub fn selected_text_action_size(&self) -> Option<f64> {
        let AnnotationAction::Text { font_size, .. } = self.selected_action()? else {
            return None;
        };

        Some(*font_size)
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

        let AnnotationAction::Text { font_size, .. } = action else {
            return false;
        };

        if (*font_size - next).abs() <= f64::EPSILON {
            return false;
        }

        *font_size = next;
        self.redo_actions.clear();
        true
    }

    pub fn adjust_text_size(&mut self, delta: f64) -> bool {
        let changed = self.set_text_size(self.text_size + delta);
        if !changed {
            return false;
        }

        let _ = self.set_selected_text_action_size(self.text_size);
        true
    }

    pub fn active_size_control_mode(&self) -> Option<SizeControlMode> {
        if self.selected_tool == Tool::Select {
            if self.selected_text_action_size().is_some() {
                return Some(SizeControlMode::Text);
            }
            if self.selected_action_stroke_size().is_some() {
                return Some(SizeControlMode::Stroke);
            }
            return None;
        }

        if self.selected_tool == Tool::Text {
            return Some(SizeControlMode::Text);
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
            SizeControlMode::Text => {
                if self.selected_tool == Tool::Select {
                    Some(self.selected_text_action_size().unwrap_or(self.text_size))
                } else {
                    Some(self.text_size)
                }
            }
        }
    }

    pub fn adjust_active_size(&mut self, direction: f64) -> bool {
        if direction.abs() <= f64::EPSILON {
            return false;
        }

        let step = direction.signum();
        match self.active_size_control_mode() {
            Some(SizeControlMode::Stroke) => self.adjust_stroke_size(STROKE_SIZE_STEP * step),
            Some(SizeControlMode::Text) => self.adjust_text_size(TEXT_SIZE_STEP * step),
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
            AnnotationAction::Blur { .. }
            | AnnotationAction::Focus { .. }
            | AnnotationAction::Censor { .. } => None,
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
            AnnotationAction::Blur { .. }
            | AnnotationAction::Focus { .. }
            | AnnotationAction::Censor { .. } => return false,
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
        self.selected_action_index = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.sync_next_number();
        self.rebuild_effect_layer();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(action) = self.actions.pop() {
            self.redo_actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.sync_next_number();
            self.rebuild_effect_layer();
            return true;
        }
        false
    }

    pub fn redo(&mut self) -> bool {
        if let Some(action) = self.redo_actions.pop() {
            self.actions.push(action);
            self.selected_action_index = None;
            self.select_drag_anchor = None;
            self.select_resize_handle = None;
            self.sync_next_number();
            self.rebuild_effect_layer();
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
            let effect_action = matches!(
                action,
                AnnotationAction::Blur { .. } | AnnotationAction::Censor { .. }
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
            self.rebuild_effect_layer();
        }
        true
    }

    pub fn end_select_drag(&mut self) {
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.drag_start = None;
        self.drag_current = None;
        self.drag_start_view = None;
        self.drag_path.clear();
    }

    pub fn remove_selected_action(&mut self) -> bool {
        let Some(index) = self.selected_action_index.take() else {
            return false;
        };

        if index >= self.actions.len() {
            return false;
        }

        let removed = self.actions.remove(index);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.redo_actions.clear();
        self.sync_next_number();

        if matches!(
            removed,
            AnnotationAction::Blur { .. } | AnnotationAction::Censor { .. }
        ) {
            self.rebuild_effect_layer();
        }

        true
    }

    pub fn rebuild_effect_layer(&mut self) {
        self.working_image = self.base_image.clone();
        for action in &self.actions {
            match action {
                AnnotationAction::Blur { rect } => {
                    apply_blur_rect(&mut self.working_image, *rect, BLUR_RADIUS);
                }
                AnnotationAction::Censor { rect } => {
                    apply_censor_rect(&mut self.working_image, *rect, CENSOR_BLOCK_SIZE);
                }
                _ => {}
            }
        }
        self.mark_working_image_dirty();
    }

    pub fn begin_drag(&mut self, point: Point) {
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

    pub fn clear_drag(&mut self) {
        self.drag_start = None;
        self.drag_current = None;
        self.drag_start_view = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.drag_path.clear();
        self.drag_shift_active = false;
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
            Tool::Blur => Rect::from_points(start, end).map(|rect| AnnotationAction::Blur { rect }),
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
            Tool::Censor => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Censor { rect })
            }
            Tool::Text => None,
        }
    }

    pub fn draft_crop_rect(&self) -> Option<Rect> {
        if self.selected_tool != Tool::Crop {
            return None;
        }

        Rect::from_points(self.drag_start?, self.drag_current?)
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
            Tool::Blur => Rect::from_points(start, end).map(|rect| AnnotationAction::Blur { rect }),
            Tool::Focus => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Focus { rect })
            }
            Tool::Censor => {
                Rect::from_points(start, end).map(|rect| AnnotationAction::Censor { rect })
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
                    AnnotationAction::Blur { .. }
                        | AnnotationAction::Focus { .. }
                        | AnnotationAction::Censor { .. }
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

        if let Some(crop) = self
            .crop_selection
            .and_then(|rect| rect.clamp_to(rendered.width(), rendered.height()))
        {
            return Ok(image::imageops::crop_imm(
                &rendered,
                crop.x as u32,
                crop.y as u32,
                crop.width as u32,
                crop.height as u32,
            )
            .to_image());
        }

        Ok(rendered)
    }
}
