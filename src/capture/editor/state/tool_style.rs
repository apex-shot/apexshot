use super::super::color::{
    clamp_focus_intensity, clamp_obfuscate_amount, clamp_pixelate_amount, clamp_stroke_size,
    DRAW_COLORS,
};
use super::super::numbering_style::{NumberSize, NumberingStyle};
use super::super::pen_weight::{HighlighterMode, PenWeight};
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
        // Live-update the selected rect so method switches re-tint existing pixels
        // (mirrors set_text_size propagating to the selected text action).
        // ponytail: amount snaps to the new method's current amount; per-method memory stays in the pixelate/blur stores.
        let current = self.current_obfuscate_amount();
        if let Some(index) = self.selected_action_index {
            let before = self.document_snapshot();
            if let Some(AnnotationAction::Obfuscate {
                method: act_method,
                amount: act_amount,
                ..
            }) = self.actions.get_mut(index)
            {
                if *act_method == method && (*act_amount - current).abs() <= f64::EPSILON {
                    return;
                }
                *act_method = method;
                if (*act_amount - current).abs() > f64::EPSILON {
                    *act_amount = clamp_obfuscate_amount(current);
                }
                self.clear_redo_history();
                self.commit_property_edit(before);
            } else if self.actions.get(index).is_none() {
                self.selected_action_index = None;
            }
        }
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

    /// The selected number marker's own style, size, and number.
    ///
    /// The floating number bar edits this marker when it exists; otherwise it
    /// edits the defaults the next marker will be placed with.
    pub fn selected_number_marker(&self) -> Option<(u32, NumberingStyle, NumberSize)> {
        match self.selected_action()? {
            AnnotationAction::Number {
                number,
                style,
                size,
                ..
            } => Some((*number, *style, *size)),
            _ => None,
        }
    }

    pub fn selected_number_style(&self) -> Option<NumberingStyle> {
        self.selected_number_marker().map(|(_, style, _)| style)
    }

    pub fn selected_number_size(&self) -> Option<NumberSize> {
        self.selected_number_marker().map(|(_, _, size)| size)
    }

    /// Numbering style shown by the bar: the selected marker's style, else the
    /// style the next marker will use.
    pub fn active_numbering_style(&self) -> NumberingStyle {
        self.selected_number_style().unwrap_or(self.numbering_style)
    }

    /// Size shown by the bar: the selected marker's size, else the next marker's.
    pub fn active_number_size(&self) -> NumberSize {
        self.selected_number_size().unwrap_or(self.number_size)
    }

    /// The number the bar's stepper edits: the selected marker's own number, or
    /// the number the next placed marker will get.
    pub fn active_number_start(&self) -> u32 {
        match self.selected_number_marker() {
            Some((number, ..)) => number,
            None => self.next_number,
        }
    }

    /// Formatted value for the bar's entry (`3` or `C`, matching the active style).
    pub fn active_number_start_display(&self) -> String {
        match self.selected_number_marker() {
            Some((number, style, _)) => style.format(number),
            None => self.numbering_style.format(self.next_number),
        }
    }

    pub fn set_numbering_style(&mut self, style: NumberingStyle) -> bool {
        let mut changed = false;
        let mut action_changed = false;
        let before = self.document_snapshot();
        if self.numbering_style != style {
            self.numbering_style = style;
            changed = true;
        }
        // Live-update the selected marker (mirrors set_obfuscate_method). Markers
        // keep their own style otherwise, so mixed runs stay intact.
        if let Some(index) = self.selected_action_index {
            match self.actions.get_mut(index) {
                Some(AnnotationAction::Number {
                    style: action_style,
                    ..
                }) => {
                    if *action_style != style {
                        *action_style = style;
                        changed = true;
                        action_changed = true;
                    }
                }
                Some(_) => {}
                None => self.selected_action_index = None,
            }
        }
        if changed {
            // Continue the run of the newly active style rather than restarting it.
            self.sync_next_number();
            self.clear_redo_history();
            if action_changed {
                self.commit_property_edit(before);
            }
        }
        changed
    }

    pub fn set_number_size(&mut self, size: NumberSize) -> bool {
        let mut changed = false;
        let mut action_changed = false;
        let before = self.document_snapshot();
        if self.number_size != size {
            self.number_size = size;
            changed = true;
        }
        // Live-update the selected marker so a size change is visible immediately.
        if let Some(index) = self.selected_action_index {
            match self.actions.get_mut(index) {
                Some(AnnotationAction::Number {
                    size: action_size, ..
                }) => {
                    if *action_size != size {
                        *action_size = size;
                        changed = true;
                        action_changed = true;
                    }
                }
                Some(_) => {}
                None => self.selected_action_index = None,
            }
        }
        if changed {
            self.clear_redo_history();
            if action_changed {
                self.commit_property_edit(before);
            }
        }
        changed
    }

    /// Set the number the bar's stepper shows.
    ///
    /// With a number marker selected this renumbers only that marker.
    /// Followers keep their values so correcting one step never shifts the
    /// rest of the run. With no marker selected it only re-seeds the next
    /// marker, which is what an armed Number tool needs before the first click.
    pub fn set_active_number_start(&mut self, value: u32) -> bool {
        let value = value.max(1);
        let selected = self
            .selected_action_index
            .and_then(|index| match self.actions.get(index) {
                Some(AnnotationAction::Number { style, .. }) => Some((index, *style)),
                Some(_) => None,
                None => {
                    self.selected_action_index = None;
                    None
                }
            });

        let Some((index, _)) = selected else {
            let mut changed = false;
            if self.numbering_start != value {
                self.numbering_start = value;
                changed = true;
            }
            if self.next_number != value {
                self.next_number = value;
                changed = true;
            }
            if changed {
                self.clear_redo_history();
            }
            return changed;
        };

        let current = match self.actions.get(index) {
            Some(AnnotationAction::Number { number, .. }) => *number,
            _ => return false,
        };
        if current == value {
            return false;
        }
        let before = self.document_snapshot();
        if let Some(AnnotationAction::Number { number, .. }) = self.actions.get_mut(index) {
            *number = value;
        }
        // Keep the next marker above the highest existing number so a later
        // add never reuses (duplicates) the edited value.
        self.sync_next_number();
        self.clear_redo_history();
        self.commit_property_edit(before);
        true
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
        let before = self.document_snapshot();
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
        self.clear_redo_history();
        self.commit_property_edit(before);
        true
    }

    /// The selected highlighter stroke's own thickness, if one is selected.
    pub fn selected_highlighter_stroke_size(&self) -> Option<f64> {
        match self.selected_action()? {
            AnnotationAction::Highlighter { stroke_size, .. } => Some(*stroke_size),
            _ => None,
        }
    }

    /// The selected pen stroke's own thickness, if one is selected.
    pub fn selected_pen_stroke_size(&self) -> Option<f64> {
        match self.selected_action()? {
            AnnotationAction::Pen { stroke_size, .. } => Some(*stroke_size),
            _ => None,
        }
    }

    /// Thickness preset shown for the pen: the selected stroke's weight, else the brush.
    pub fn active_pen_weight(&self) -> PenWeight {
        self.selected_pen_stroke_size()
            .map(PenWeight::nearest_for_pen_stroke)
            .unwrap_or(self.pen_weight)
    }

    /// Pick a pen thickness preset: sets the brush and resizes the selected stroke so
    /// the pick shows up immediately. The pen's own thickness is the toolbar slider.
    pub fn set_pen_weight_and_apply(&mut self, weight: PenWeight) -> bool {
        let mut changed = false;
        if self.pen_weight != weight {
            self.pen_weight = weight;
            changed = true;
        }
        if self.selected_pen_stroke_size().is_some()
            && self.set_selected_action_stroke_size(weight.pen_stroke_width())
        {
            changed = true;
        }
        changed
    }

    /// Thickness preset shown for the highlighter: the selected stroke's weight,
    /// else the brush. `None` means text-aware sizing, where the stroke follows the
    /// detected text height instead of a preset.
    pub fn active_highlighter_weight(&self) -> Option<PenWeight> {
        // A selected stroke has a real thickness whatever the brush mode is, so a
        // text-aware stroke still reports the preset closest to it.
        if let Some(size) = self.selected_highlighter_stroke_size() {
            return Some(PenWeight::nearest_for_highlighter_stroke(size));
        }
        if self.highlighter_mode == HighlighterMode::TextAware {
            return None;
        }
        Some(self.pen_weight)
    }

    pub fn set_highlighter_mode_and_check(&mut self, mode: HighlighterMode) -> bool {
        if self.highlighter_mode == mode {
            return false;
        }
        self.highlighter_mode = mode;
        true
    }

    /// Resize the selected highlighter stroke.
    ///
    /// Deliberately not routed through [`Self::set_selected_action_stroke_size`]:
    /// that clamps to `MAX_STROKE_SIZE`, while text-aware strokes store the
    /// detected text height verbatim (which can exceed the slider's ceiling).
    pub fn set_selected_highlighter_stroke_size(&mut self, size: f64) -> bool {
        let size = super::super::color::highlighter_stroke_width(size);
        let Some(index) = self.selected_action_index else {
            return false;
        };
        let before = self.document_snapshot();
        let Some(action) = self.actions.get_mut(index) else {
            self.selected_action_index = None;
            return false;
        };
        let AnnotationAction::Highlighter { stroke_size, .. } = action else {
            return false;
        };
        if (*stroke_size - size).abs() <= f64::EPSILON {
            return false;
        }
        *stroke_size = size;
        self.clear_redo_history();
        self.commit_property_edit(before);
        true
    }

    /// Pick a highlighter thickness preset.
    ///
    /// Choosing a preset leaves text-aware sizing (the sidebar's thickness rows do
    /// the same) and resizes the selected stroke so the pick is visible at once;
    /// the preset also becomes the brush for the next stroke. Freehand thickness
    /// itself comes from the toolbar's stroke-size slider.
    pub fn set_highlighter_weight(&mut self, weight: PenWeight) -> bool {
        let mut changed = false;
        if self.pen_weight != weight {
            self.pen_weight = weight;
            changed = true;
        }
        if self.highlighter_mode != HighlighterMode::Freehand {
            self.highlighter_mode = HighlighterMode::Freehand;
            changed = true;
        }
        if self.selected_highlighter_stroke_size().is_some()
            && self.set_selected_highlighter_stroke_size(weight.highlighter_stroke_width())
        {
            changed = true;
        }
        changed
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
        let before = self.document_snapshot();
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
        self.clear_redo_history();
        self.commit_property_edit(before);
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
        let before = self.document_snapshot();
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
        self.clear_redo_history();
        self.commit_property_edit(before);
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
                // Shapes, the arrow, the pen, and the highlighter all read
                // `stroke_size`, so the slider resizes the next stroke and whatever
                // stroke is selected.
                let changed = self.set_stroke_size(size);
                let _ = self.set_selected_action_stroke_size(self.stroke_size);
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
        let before = self.document_snapshot();
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
        self.clear_redo_history();
        self.commit_property_edit(before);
        true
    }
}
