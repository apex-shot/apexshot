use super::super::color::{
    clamp_focus_intensity, clamp_obfuscate_amount, clamp_pixelate_amount, clamp_stroke_size,
    DRAW_COLORS,
};
use super::super::types::{AnnotationAction, DrawColor, ObfuscateMethod, SizeControlMode, Tool};
use super::EditorState;

impl EditorState {
    pub fn set_color_index(&mut self, index: usize) {
        if let Some(color) = DRAW_COLORS.get(index).copied() {
            self.selected_color = color;
            if let Some(input) = self.active_text_input.as_mut() {
                input.color = color;
            }
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

    pub fn set_obfuscate_method(&mut self, method: ObfuscateMethod) {
        self.obfuscate_method = method;
    }

    pub fn obfuscate_method(&self) -> ObfuscateMethod {
        self.obfuscate_method
    }

    pub fn current_obfuscate_amount(&self) -> f64 {
        match self.obfuscate_method {
            ObfuscateMethod::Pixelate => self.obfuscate_pixelate_amount,
            ObfuscateMethod::Blur => self.obfuscate_blur_amount,
            ObfuscateMethod::Blackout => 0.0,
        }
    }

    pub fn set_current_obfuscate_amount(&mut self, amount: f64) {
        match self.obfuscate_method {
            ObfuscateMethod::Pixelate => {
                self.obfuscate_pixelate_amount = clamp_pixelate_amount(amount)
            }
            ObfuscateMethod::Blur => self.obfuscate_blur_amount = clamp_obfuscate_amount(amount),
            ObfuscateMethod::Blackout => {}
        }
    }

    pub fn set_current_obfuscate_amount_and_check(&mut self, amount: f64) -> bool {
        let before = self.current_obfuscate_amount();
        self.set_current_obfuscate_amount(amount);
        (self.current_obfuscate_amount() - before).abs() > f64::EPSILON
    }

    pub fn current_focus_intensity(&self) -> f64 {
        clamp_focus_intensity(self.focus_intensity)
    }

    pub fn set_current_focus_intensity_and_check(&mut self, intensity: f64) -> bool {
        let next = clamp_focus_intensity(intensity);
        if (self.focus_intensity - next).abs() <= f64::EPSILON {
            return false;
        }
        self.focus_intensity = next;
        true
    }

    pub fn selected_focus_action_intensity(&self) -> Option<f64> {
        let AnnotationAction::Focus { intensity, .. } = self.selected_action()? else {
            return None;
        };
        Some(*intensity)
    }

    pub fn set_selected_focus_action_intensity_without_rebuild(&mut self, intensity: f64) -> bool {
        let next = clamp_focus_intensity(intensity);
        let Some(index) = self.selected_action_index else {
            return false;
        };
        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };
        let AnnotationAction::Focus {
            intensity: act_intensity,
            ..
        } = action
        else {
            return false;
        };
        if (*act_intensity - next).abs() <= f64::EPSILON {
            return false;
        }
        *act_intensity = next;
        self.redo_actions.clear();
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

    pub fn selected_obfuscate_action_amount(&self) -> Option<f64> {
        let AnnotationAction::Obfuscate { amount, .. } = self.selected_action()? else {
            return None;
        };
        Some(*amount)
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
            if self.selected_focus_action_intensity().is_some() {
                return Some(SizeControlMode::Focus);
            }
            return None;
        }
        if self.selected_tool == Tool::Text {
            return None;
        }
        if self.selected_tool == Tool::Obfuscate {
            return Some(SizeControlMode::Obfuscate);
        }
        if self.selected_tool == Tool::Focus {
            return Some(SizeControlMode::Focus);
        }
        super::super::types::tool_uses_stroke_size(self.selected_tool)
            .then_some(SizeControlMode::Stroke)
    }

    pub fn active_size_value(&self) -> Option<f64> {
        match self.active_size_control_mode()? {
            SizeControlMode::Stroke => (self.selected_tool == Tool::Select)
                .then(|| {
                    self.selected_action_stroke_size()
                        .unwrap_or(self.stroke_size)
                })
                .or(Some(self.stroke_size)),
            SizeControlMode::Obfuscate => (self.selected_tool == Tool::Select)
                .then(|| {
                    self.selected_obfuscate_action_amount()
                        .unwrap_or_else(|| self.current_obfuscate_amount())
                })
                .or_else(|| Some(self.current_obfuscate_amount())),
            SizeControlMode::Focus => (self.selected_tool == Tool::Select)
                .then(|| {
                    self.selected_focus_action_intensity()
                        .unwrap_or_else(|| self.current_focus_intensity())
                })
                .or_else(|| Some(self.current_focus_intensity())),
        }
    }

    pub fn set_active_size_without_rebuild(&mut self, size: f64) -> bool {
        match self.active_size_control_mode() {
            Some(SizeControlMode::Stroke) => {
                let changed = self.set_stroke_size(size);
                let is_highlighter = self
                    .selected_action()
                    .is_some_and(|action| matches!(action, AnnotationAction::Highlighter { .. }));
                if !is_highlighter {
                    let _ = self.set_selected_action_stroke_size(self.stroke_size);
                }
                changed
            }
            Some(SizeControlMode::Obfuscate) => {
                let changed = self.set_current_obfuscate_amount_and_check(size);
                let _ = self.set_selected_obfuscate_action_amount_without_rebuild(
                    self.current_obfuscate_amount(),
                );
                changed
            }
            Some(SizeControlMode::Focus) => {
                let changed = self.set_current_focus_intensity_and_check(size);
                let _ = self.set_selected_focus_action_intensity_without_rebuild(
                    self.current_focus_intensity(),
                );
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
}
