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
    camera: ZoomCameraSettings,
    classic: bool,
    selected: Option<usize>,
    clipboard: Option<ClipClipboard>,
}

impl ZoomSnapshot {
    fn capture(state: &VideoEditState) -> Self {
        Self {
            clips: state.zoom_clips.clone(),
            camera: state.zoom_camera,
            classic: state.zoom_classic,
            selected: state.selected_zoom,
            clipboard: state.clipboard.clone(),
        }
    }

    fn restore(self, state: &mut VideoEditState) {
        state.selected_zoom = self.selected.filter(|index| *index < self.clips.len());
        state.zoom_clips = self.clips;
        state.zoom_camera = self.camera;
        state.zoom_classic = self.classic;
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
    pub fn set_zoom_camera_speed(&mut self, speed: f64) {
        let settings = ZoomCameraSettings {
            speed,
            ..self.zoom_camera
        }
        .clamped();
        if self.zoom_locked || self.zoom_camera == settings {
            return;
        }
        self.record_continuous_zoom_edit();
        self.zoom_camera = settings;
        self.zoom_classic = false;
    }

    pub fn set_zoom_camera_smoothness(&mut self, smoothness: f64) {
        let settings = ZoomCameraSettings {
            smoothness,
            ..self.zoom_camera
        }
        .clamped();
        if self.zoom_locked || self.zoom_camera == settings {
            return;
        }
        self.record_continuous_zoom_edit();
        self.zoom_camera = settings;
        self.zoom_classic = false;
    }

    pub fn set_zoom_camera_start_early(&mut self, start_early: bool) {
        if self.zoom_locked || self.zoom_camera.start_early == start_early {
            return;
        }
        self.record_zoom_command();
        self.zoom_camera.start_early = start_early;
        self.zoom_classic = false;
    }

    pub fn set_zoom_camera_motion_blur(&mut self, motion_blur: f64) {
        let settings = ZoomCameraSettings {
            motion_blur,
            ..self.zoom_camera
        }
        .clamped();
        if self.zoom_locked || self.zoom_camera == settings {
            return;
        }
        self.record_continuous_zoom_edit();
        self.zoom_camera = settings;
        self.zoom_classic = false;
    }

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

#[cfg(test)]
mod zoom_camera_history_tests {
    use super::*;

    fn state() -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/apexshot-zoom-camera-history.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 0,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    #[test]
    fn zoom_camera_sliders_coalesce_and_toggle_gets_a_discrete_step() {
        let mut state = state();
        state.set_zoom_camera_speed(1.5);
        state.set_zoom_camera_speed(2.0);
        assert_eq!(state.zoom_history.undo.len(), 1);
        state.set_zoom_camera_start_early(true);
        assert_eq!(state.zoom_history.undo.len(), 2);
        state.set_zoom_camera_motion_blur(0.4);
        state.set_zoom_camera_motion_blur(0.8);
        assert_eq!(state.zoom_history.undo.len(), 3);

        assert!(state.undo_zoom_edit());
        assert_eq!(state.zoom_camera.motion_blur, 0.0);
        assert!(state.zoom_camera.start_early);
        assert!(state.undo_zoom_edit());
        assert!(!state.zoom_camera.start_early);
        assert_eq!(state.zoom_camera.speed, 2.0);
        assert!(state.undo_zoom_edit());
        assert_eq!(state.zoom_camera, ZoomCameraSettings::default());
        assert!(state.redo_zoom_edit());
        assert_eq!(state.zoom_camera.speed, 2.0);
        assert!(state.redo_zoom_edit());
        assert!(state.zoom_camera.start_early);
        assert!(state.redo_zoom_edit());
        assert_eq!(state.zoom_camera.motion_blur, 0.8);
    }

    #[test]
    fn zoom_camera_edits_migrate_classic_motion_and_undo_restores_it() {
        let edits: [fn(&mut VideoEditState); 4] = [
            |state| state.set_zoom_camera_speed(2.0),
            |state| state.set_zoom_camera_smoothness(0.8),
            |state| state.set_zoom_camera_start_early(true),
            |state| state.set_zoom_camera_motion_blur(0.5),
        ];
        for edit in edits {
            let mut state = state();
            state.zoom_classic = true;
            state.set_zoom_camera_speed(1.0);
            state.set_zoom_camera_smoothness(0.5);
            state.set_zoom_camera_start_early(false);
            state.set_zoom_camera_motion_blur(0.0);
            assert!(state.zoom_classic);
            assert!(state.zoom_history.undo.is_empty());

            edit(&mut state);
            let edited_settings = state.zoom_camera;
            assert!(!state.zoom_classic);
            assert!(state.undo_zoom_edit());
            assert!(state.zoom_classic);
            assert_eq!(state.zoom_camera, ZoomCameraSettings::default());
            assert!(state.redo_zoom_edit());
            assert!(!state.zoom_classic);
            assert_eq!(state.zoom_camera, edited_settings);
        }
    }

    #[test]
    fn zoom_camera_settings_respect_lock_and_skip_unchanged_values() {
        let mut state = state();
        state.set_zoom_camera_speed(1.0);
        state.set_zoom_camera_smoothness(0.5);
        state.set_zoom_camera_start_early(false);
        state.set_zoom_camera_motion_blur(0.0);
        assert!(state.zoom_history.undo.is_empty());

        state.zoom_locked = true;
        state.set_zoom_camera_speed(2.0);
        state.set_zoom_camera_smoothness(0.8);
        state.set_zoom_camera_start_early(true);
        state.set_zoom_camera_motion_blur(0.5);
        assert_eq!(state.zoom_camera, ZoomCameraSettings::default());
        assert!(state.zoom_history.undo.is_empty());
    }

    #[test]
    fn zoom_camera_setters_clamp_and_replace_non_finite_values() {
        let mut state = state();
        state.set_zoom_camera_speed(100.0);
        state.set_zoom_camera_smoothness(-1.0);
        state.set_zoom_camera_motion_blur(2.0);
        assert_eq!(state.zoom_camera.speed, 3.0);
        assert_eq!(state.zoom_camera.smoothness, 0.0);
        assert_eq!(state.zoom_camera.motion_blur, 1.0);
        state.set_zoom_camera_speed(-1.0);
        assert_eq!(state.zoom_camera.speed, 0.25);
        state.set_zoom_camera_speed(f64::NAN);
        state.set_zoom_camera_smoothness(f64::INFINITY);
        state.set_zoom_camera_motion_blur(f64::NEG_INFINITY);
        assert_eq!(state.zoom_camera, ZoomCameraSettings::default());
    }
}
