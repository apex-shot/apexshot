//! Bounded undo/redo for the editor's document.
//!
//! The snapshot type belongs to the timeline; this module owns the stacks, the
//! bound, and the rule that a continuous edit — one trim drag, one slider
//! sweep — is one step instead of one step per motion event.

use std::time::{Duration, Instant};

/// Undo steps kept for the document.
///
/// A step is the editable state plus the selection and the pending clipboard,
/// so the whole stack stays far below the video's decode buffers. Past this
/// bound the oldest step is dropped: undo is a safety net for the last few
/// edits, not a document history.
pub(super) const EDIT_HISTORY_LIMIT: usize = 64;

/// Continuous edits arriving closer together than this share a step instead of
/// pushing one per motion event.
const EDIT_COALESCE: Duration = Duration::from_millis(350);

/// The two bounded stacks behind Ctrl+Z and Ctrl+Shift+Z.
///
/// Recorded state is what the caller holds *before* the edit: undo swaps in
/// that state and hands back the current one, which is what redo replays. The
/// stacks are runtime-only — they are not part of the saved edit list, so a
/// reopened recording starts with nothing to undo.
pub(super) struct EditHistory<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    last_continuous_edit: Option<Instant>,
}

impl<T> Default for EditHistory<T> {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last_continuous_edit: None,
        }
    }
}

impl<T: PartialEq> EditHistory<T> {
    /// Record `before` for an edit that arrives as a stream of updates. The
    /// first update of a burst takes the step; the rest leave it alone so a
    /// whole drag collapses into one.
    pub(super) fn record_continuous(&mut self, before: T) {
        let fresh_burst = self
            .last_continuous_edit
            .is_none_or(|at| at.elapsed() > EDIT_COALESCE);
        self.last_continuous_edit = Some(Instant::now());
        if fresh_burst {
            self.push(before);
        }
    }

    /// Record `before` for one discrete command, which always gets its own
    /// step. It also ends any burst in progress, so a command that lands
    /// mid-drag is not folded into the drag's step.
    pub(super) fn record_command(&mut self, before: T) {
        self.last_continuous_edit = None;
        self.push(before);
    }

    fn push(&mut self, before: T) {
        // A command that could not change anything records the state already
        // on top; keeping a duplicate would only cost a step.
        if self.undo.last() == Some(&before) {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > EDIT_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Hand back the state before the last step, taking `current` for redo.
    pub(super) fn undo(&mut self, current: T) -> Option<T> {
        // An edit that changed nothing still leaves its step behind; skip the
        // ones that equal the live state so undo always makes progress.
        while self.undo.last() == Some(&current) {
            self.undo.pop();
        }
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.last_continuous_edit = None;
        Some(previous)
    }

    /// Hand back the state one undone step replaced, taking `current` for the
    /// undo stack again.
    pub(super) fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.last_continuous_edit = None;
        Some(next)
    }

    /// Forget every step. A document that is being swapped has no history to
    /// carry into the next recording.
    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_continuous_edit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny document: the value is the whole state.
    fn edit(history: &mut EditHistory<i32>, before: i32) {
        history.record_command(before);
    }

    #[test]
    fn undo_hands_back_the_step_before_and_takes_the_current_state_for_redo() {
        let mut history = EditHistory::default();
        edit(&mut history, 0);
        assert_eq!(history.undo(1), Some(0));
        assert_eq!(history.undo(0), None);
        assert_eq!(history.redo(0), Some(1));
        assert_eq!(history.redo(1), None);
    }

    #[test]
    fn a_burst_of_continuous_edits_is_one_step() {
        let mut history = EditHistory::default();
        // A drag arrives as a stream of values; they must not each become a
        // step.
        for _ in 0..5 {
            history.record_continuous(0);
        }
        assert_eq!(history.undo(1), Some(0));
        assert_eq!(history.undo(0), None);
    }

    #[test]
    fn a_command_after_a_burst_keeps_its_own_step() {
        let mut history = EditHistory::default();
        history.record_continuous(0);
        history.record_command(1);
        assert_eq!(history.undo(2), Some(1));
        assert_eq!(history.undo(1), Some(0));
    }

    #[test]
    fn undo_skips_steps_that_changed_nothing() {
        let mut history = EditHistory::default();
        edit(&mut history, 0);
        // The document is still 0; the step equals the live state, so undo has
        // nothing to do rather than restoring the same thing.
        assert_eq!(history.undo(0), None);
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut history = EditHistory::default();
        edit(&mut history, 0);
        assert_eq!(history.undo(1), Some(0));
        edit(&mut history, 1);
        assert_eq!(history.redo(1), None);
    }

    #[test]
    fn steps_past_the_bound_drop_the_oldest() {
        let mut history = EditHistory::default();
        for step in 0..EDIT_HISTORY_LIMIT as i32 + 10 {
            edit(&mut history, step);
        }
        // The ten oldest steps are gone; the most recent bound's worth is left.
        let mut taken = Vec::new();
        let mut current = EDIT_HISTORY_LIMIT as i32 + 10;
        while let Some(previous) = history.undo(current) {
            taken.push(previous);
            current = previous;
        }
        assert_eq!(taken.len(), EDIT_HISTORY_LIMIT);
        assert_eq!(taken.last().copied(), Some(10));
    }

    #[test]
    fn clearing_forgets_both_stacks() {
        let mut history = EditHistory::default();
        edit(&mut history, 0);
        history.undo(1);
        history.clear();
        assert_eq!(history.undo(1), None);
        assert_eq!(history.redo(1), None);
    }
}
