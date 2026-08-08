use super::super::types::AnnotationAction;
use super::EditorState;

impl EditorState {
    pub fn history_availability(&self) -> (bool, bool) {
        (!self.actions.is_empty(), !self.redo_actions.is_empty())
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
                .any(Self::action_requires_effect_rebuild)
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
}
