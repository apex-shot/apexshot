use std::time::{Duration, Instant};

/// Undo steps kept for the zoom track.
///
/// A step is the clip list plus the selection and the pending clipboard —
/// plain data, a few hundred bytes each — so the whole stack stays far below
/// the video's decode buffers. Past this bound the oldest step is dropped:
/// undo is a safety net for the last few edits, not a document history.
pub const ZOOM_HISTORY_LIMIT: usize = 64;

/// Continuous edits (one timeline drag, one slider sweep) arriving closer
/// together than this share a step instead of pushing one per motion event.
const ZOOM_EDIT_COALESCE: Duration = Duration::from_millis(350);

/// The zoom track as one undo step restores it.
///
/// The clipboard travels with the track: Cut removes the clip and holds it for
/// a paste, so undoing a cut has to put the clip back *and* drop the pending
/// paste — otherwise the editor stays dimmed waiting for a clip the user just
/// took back.
#[derive(Debug, Clone, PartialEq)]
struct ZoomSnapshot {
    clips: Vec<ZoomClip>,
    selected: Option<usize>,
    clipboard: Option<ClipClipboard>,
}

impl ZoomSnapshot {
    fn capture(state: &VideoEditState) -> Self {
        Self {
            clips: state.zoom_clips.clone(),
            selected: state.selected_zoom,
            clipboard: state.clipboard.clone(),
        }
    }

    fn restore(self, state: &mut VideoEditState) {
        state.selected_zoom = self.selected.filter(|index| *index < self.clips.len());
        state.zoom_clips = self.clips;
        state.clipboard = self.clipboard;
    }
}

/// Bounded undo/redo for the zoom track.
///
/// This covers the track automatic generation writes to, which is the one it
/// may rewrite wholesale. A generation pass is recorded as a single step and a
/// drag or slider sweep coalesces into one, so undo undoes what the user did
/// rather than how many motion events it took.
///
/// Recorded state is what the caller holds *before* the edit: undo swaps in
/// that state and hands back the current one, which is what redo replays. The
/// stacks are runtime-only — they are not part of the saved project, so a
/// reopened recording starts with nothing to undo.
#[derive(Debug, Clone, Default)]
pub struct ZoomHistory {
    undo: Vec<ZoomSnapshot>,
    redo: Vec<ZoomSnapshot>,
    last_continuous_edit: Option<Instant>,
}

impl ZoomHistory {
    /// Record `before` for an edit that arrives as a stream of updates. The
    /// first update of a burst takes the step; the rest leave it alone so a
    /// whole drag collapses into one.
    fn record_continuous(&mut self, before: ZoomSnapshot) {
        let fresh_burst = self
            .last_continuous_edit
            .is_none_or(|at| at.elapsed() > ZOOM_EDIT_COALESCE);
        self.last_continuous_edit = Some(Instant::now());
        if fresh_burst {
            self.push(before);
        }
    }

    /// Record `before` for one discrete command, which always gets its own
    /// step. It also ends any burst in progress, so a command that lands
    /// mid-drag is not folded into the drag's step.
    fn record_command(&mut self, before: ZoomSnapshot) {
        self.last_continuous_edit = None;
        self.push(before);
    }

    fn push(&mut self, before: ZoomSnapshot) {
        // A command that could not change anything records the state already
        // on top; keeping a duplicate would only cost a step.
        if self.undo.last() == Some(&before) {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > ZOOM_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self, current: ZoomSnapshot) -> Option<ZoomSnapshot> {
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

    fn redo(&mut self, current: ZoomSnapshot) -> Option<ZoomSnapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.last_continuous_edit = None;
        Some(next)
    }
}

impl VideoEditState {
    /// Take the step a continuous edit is about to change.
    pub(super) fn record_continuous_zoom_edit(&mut self) {
        let before = ZoomSnapshot::capture(self);
        self.zoom_history.record_continuous(before);
    }

    /// Take the step one discrete zoom command is about to change.
    pub(super) fn record_zoom_command(&mut self) {
        let before = ZoomSnapshot::capture(self);
        self.zoom_history.record_command(before);
    }

    /// Restore the zoom track to how it stood before the last zoom edit.
    ///
    /// Returns `false` when there is nothing left to undo, which leaves the
    /// track untouched.
    pub fn undo_zoom_edit(&mut self) -> bool {
        let current = ZoomSnapshot::capture(self);
        let Some(previous) = self.zoom_history.undo(current) else {
            return false;
        };
        previous.restore(self);
        // A composition edit since the step was taken has already moved the
        // clips that are anchored to their footage, and the step is older than
        // that edit. Re-projecting puts the restored clips on the footage
        // their anchors name instead of where they sat when it was captured.
        self.reproject_anchored_zooms();
        true
    }

    /// Replay the zoom edit the last [`Self::undo_zoom_edit`] took back.
    pub fn redo_zoom_edit(&mut self) -> bool {
        let current = ZoomSnapshot::capture(self);
        let Some(next) = self.zoom_history.redo(current) else {
            return false;
        };
        next.restore(self);
        self.reproject_anchored_zooms();
        true
    }
}
