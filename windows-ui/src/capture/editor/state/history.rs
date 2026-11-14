use super::super::image_transform::StaticImageTransform;
use super::super::text_detect::TextDetector;
use super::super::types::AnnotationAction;
use super::background::{Appearance, Fill};
use super::EditorState;
use image::RgbaImage;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct DocumentSnapshot {
    base_image: Arc<RgbaImage>,
    working_image: Arc<RgbaImage>,
    actions: Vec<AnnotationAction>,
    next_number: u32,
    static_image_transform: StaticImageTransform,
    scene_shadow: crate::recording::editor::model::MotionSceneShadow,
    frame_style: super::super::types::FrameStyle,
    background: Appearance,
}

/// Two fills are the same edit when they paint the same source. Both the
/// image and the custom painter are shared handles, so their identity is the
/// only cheap test: a freshly built painter is a new fill.
fn same_fill(left: &Fill, right: &Fill) -> bool {
    match (left, right) {
        (Fill::None, Fill::None) => true,
        (Fill::Image(left), Fill::Image(right)) => Arc::ptr_eq(left, right),
        (Fill::Custom(left), Fill::Custom(right)) => Rc::ptr_eq(left, right),
        _ => false,
    }
}

/// The revision is a paint cache key, not document content, so it is left
/// out: a repaint of an unchanged appearance is not a history step.
fn same_background(left: &Appearance, right: &Appearance) -> bool {
    same_fill(&left.fill, &right.fill)
        && left.padding == right.padding
        && left.blur == right.blur
        && left.radius == right.radius
        && left.noise == right.noise
        && left.frame_aspect == right.frame_aspect
        && left.frame_style == right.frame_style
}

impl DocumentSnapshot {
    fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.base_image, &other.base_image)
            && self.actions == other.actions
            && self.next_number == other.next_number
            && self.static_image_transform == other.static_image_transform
            && self.scene_shadow == other.scene_shadow
            && self.frame_style == other.frame_style
            && same_background(&self.background, &other.background)
    }
}

#[derive(Clone)]
pub(super) enum HistoryKind {
    Annotation(AnnotationAction),
    Edit,
    PropertyEdit(Option<usize>),
    Crop,
    StaticTransform,
    FrameStyle {
        before_radius: f64,
        after_radius: f64,
    },
}

#[derive(Clone)]
pub(crate) struct HistoryTransaction {
    before: DocumentSnapshot,
    after: DocumentSnapshot,
    kind: HistoryKind,
}

impl EditorState {
    pub fn history_availability(&self) -> (bool, bool) {
        (
            !self.undo_history.is_empty() || !self.actions.is_empty(),
            !self.redo_history.is_empty() || !self.redo_actions.is_empty(),
        )
    }

    pub(super) fn document_snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot {
            base_image: Arc::clone(&self.base_image),
            working_image: Arc::clone(&self.working_image),
            actions: self.actions.clone(),
            next_number: self.next_number,
            static_image_transform: self.static_image_transform,
            scene_shadow: self.scene_shadow.clone(),
            frame_style: self.background.frame_style,
            background: self.background.clone(),
        }
    }

    pub(super) fn document_snapshot_with_current_effects(&self) -> DocumentSnapshot {
        let mut snapshot = self.document_snapshot();
        let mut working = (*snapshot.base_image).clone();
        super::apply_effect_actions(&mut working, &snapshot.actions);
        snapshot.working_image = Arc::new(working);
        snapshot
    }

    pub(super) fn commit_history_change(&mut self, before: DocumentSnapshot, kind: HistoryKind) {
        let after = self.document_snapshot();
        if before.same_document(&after) {
            return;
        }
        self.undo_history.push(HistoryTransaction {
            before,
            after,
            kind,
        });
        self.redo_history.clear();
        self.redo_actions.clear();
    }

    pub(super) fn commit_property_edit(&mut self, before: DocumentSnapshot) {
        let target = self.selected_action_index;
        let after = self.document_snapshot();
        if before.same_document(&after) {
            return;
        }
        if let Some(transaction) = self.undo_history.last_mut() {
            if matches!(&transaction.kind, HistoryKind::PropertyEdit(previous) if *previous == target)
            {
                transaction.after = after;
                self.redo_history.clear();
                self.redo_actions.clear();
                return;
            }
        }
        self.undo_history.push(HistoryTransaction {
            before,
            after,
            kind: HistoryKind::PropertyEdit(target),
        });
        self.redo_history.clear();
        self.redo_actions.clear();
    }

    pub(crate) fn begin_history_interaction(&mut self) {
        if self.history_interaction_before.is_none() {
            let before = self.document_snapshot();
            self.history_interaction_before = Some(before);
        }
    }

    pub(crate) fn finish_history_interaction(&mut self) {
        if let Some(before) = self.history_interaction_before.take() {
            self.commit_history_change(before, HistoryKind::Edit);
        }
    }

    pub(crate) fn clear_redo_history(&mut self) {
        self.redo_actions.clear();
        self.redo_history.clear();
    }

    fn restore_document_snapshot(&mut self, snapshot: DocumentSnapshot) {
        let dimensions_changed = self.base_image.dimensions() != snapshot.base_image.dimensions();
        self.base_image = snapshot.base_image;
        self.working_image = snapshot.working_image;
        self.actions = snapshot.actions;
        self.next_number = snapshot.next_number;
        self.static_image_transform = snapshot.static_image_transform;
        self.scene_shadow = snapshot.scene_shadow;
        let revision = self.background.revision;
        let background_changed = !same_background(&self.background, &snapshot.background);
        self.background = snapshot.background;
        self.background.revision = revision;
        if background_changed {
            // The revision is the paint cache key, so bumping it drops the
            // rasterized fill the same way every live background edit does.
            self.background.changed();
        }
        self.selected_action_index = None;
        self.select_drag_anchor = None;
        self.select_resize_handle = None;
        self.crop_rect = None;
        self.crop_drag = None;
        self.history_interaction_before = None;
        self.cancel_text_input();
        self.cancel_text_edit();
        if dimensions_changed {
            self.text_detector = Arc::new(Mutex::new(TextDetector::new_pending()));
            self.text_detection_ready = Arc::new(AtomicBool::new(false));
            self.text_detection_handle = None;
            self.text_detection_restart_pending = true;
        }
        self.clear_drag_without_rebuild();
        self.pending_effect_revision = self.pending_effect_revision.wrapping_add(1);
        self.last_applied_effect_revision = self.pending_effect_revision;
        self.select_effect_rebuild_pending = false;
        self.select_effect_rebuild_dirty = false;
        self.select_drag_effect_dirty = false;
        self.mark_working_image_dirty();
    }

    pub fn mark_working_image_dirty(&mut self) {
        self.working_image_revision = self.working_image_revision.wrapping_add(1);
    }

    pub fn push_action(&mut self, mut action: AnnotationAction) {
        self.history_interaction_before = None;
        let before = self.document_snapshot();
        self.expand_canvas_for_action_if_needed(&mut action);

        let next_number_after_push = match &action {
            AnnotationAction::Number { number, style, .. } if *style == self.numbering_style => {
                Some(number.saturating_add(1))
            }
            _ => None,
        };

        self.actions.push(action.clone());
        self.selected_action_index = Some(self.actions.len() - 1);
        self.select_drag_anchor = None;
        self.select_resize_handle = None;

        if let Some(next_number) = next_number_after_push {
            self.next_number = next_number;
        } else {
            self.sync_next_number();
        }
        self.commit_history_change(before, HistoryKind::Annotation(action));
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

    pub fn can_undo(&self) -> bool {
        !self.undo_history.is_empty() || !self.redo_actions.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_history.is_empty() || !self.redo_actions.is_empty()
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
        if let Some(transaction) = self.undo_history.pop() {
            if let HistoryKind::Annotation(action) = &transaction.kind {
                self.redo_actions.push(action.clone());
            }
            let live_radius = self.background.radius;
            self.restore_document_snapshot(transaction.before.clone());
            if let HistoryKind::FrameStyle {
                before_radius,
                after_radius,
            } = &transaction.kind
            {
                // A style step owns the radius it moved; the radius the
                // document carried into this undo stays put, and the check
                // below reverts only the style's own change.
                self.background.radius = live_radius;
                if before_radius != after_radius && self.background.radius == *after_radius {
                    self.background.radius = *before_radius;
                    self.background.changed();
                }
            }
            self.redo_history.push(transaction);
            return true;
        }
        if let Some(action) = self.actions.last().cloned() {
            let after = self.document_snapshot();
            let mut before = after.clone();
            before.actions.pop();
            let next_number_after_undo = match &action {
                AnnotationAction::Number { number, style, .. }
                    if *style == self.numbering_style =>
                {
                    Some(*number)
                }
                _ => None,
            };
            if let Some(next_number) = next_number_after_undo {
                before.next_number = next_number;
            }
            let transaction = HistoryTransaction {
                before: before.clone(),
                after,
                kind: HistoryKind::Annotation(action.clone()),
            };
            self.redo_actions.push(action);
            self.restore_document_snapshot(before);
            self.redo_history.push(transaction);
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
        if let Some(transaction) = self.redo_history.pop() {
            if matches!(&transaction.kind, HistoryKind::Annotation(_)) {
                self.redo_actions.pop();
            }
            let live_radius = self.background.radius;
            self.restore_document_snapshot(transaction.after.clone());
            if let HistoryKind::FrameStyle {
                before_radius,
                after_radius,
            } = &transaction.kind
            {
                // Same rule as undo: only the radius the style itself moved
                // is replayed; the live one otherwise stays.
                self.background.radius = live_radius;
                if before_radius != after_radius && self.background.radius == *before_radius {
                    self.background.radius = *after_radius;
                    self.background.changed();
                }
            }
            self.undo_history.push(transaction);
            return true;
        }
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

    pub(crate) fn take_text_detection_restart(&mut self) -> bool {
        std::mem::take(&mut self.text_detection_restart_pending)
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::EditorState;

    #[test]
    fn background_padding_undoes_with_the_edit_that_changed_it() {
        let mut state = EditorState::new(RgbaImage::from_pixel(120, 80, Rgba([30, 60, 90, 255])));
        assert_eq!(state.background.padding, 0.0);
        // The background sliders bracket a drag with these two calls.
        state.begin_history_interaction();
        state.background.set_padding(24.0);
        state.background.changed();
        state.finish_history_interaction();
        assert_eq!(state.background.padding, 24.0);
        assert!(state.undo_without_rebuild());
        assert_eq!(state.background.padding, 0.0);
        assert!(state.redo_without_rebuild());
        assert_eq!(state.background.padding, 24.0);
    }
}
