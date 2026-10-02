// Zoom clips a generation pass is about to place.
//
// A pass works out every placement first and commits it after, so the trim,
// cut, crop, and overlap rules all run before a clip is written and the whole
// pass lands as one undo step.

/// A zoom the generator placed, before it becomes a clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomCandidate {
    pub start: f64,
    pub end: f64,
    pub center: (f64, f64),
    pub scale: f64,
    /// Footage the candidate covers, carried through to the clip it becomes.
    pub(crate) anchor: ZoomAnchor,
}

impl ZoomCandidate {
    /// The generated clip this candidate becomes.
    fn into_clip(self, mode: ZoomMode) -> ZoomClip {
        ZoomClip {
            start: self.start,
            end: self.end,
            scale: self.scale,
            center: self.center,
            ease_ms: DEFAULT_ZOOM_EASE_MS,
            // Auto zooms must launch and settle at zero velocity, or the pulse
            // snaps at both clip edges.
            easing: ZoomEasing::Smooth,
            mode,
            origin: ZoomOrigin::Generated,
            anchor: Some(self.anchor),
            ..Default::default()
        }
    }
}

impl VideoEditState {
    /// Turn placed candidates into clips.
    ///
    /// The overlap check runs here as well as at placement: a clip added after
    /// the pass was computed must not be overrun by it, and overlapping zooms
    /// have no defined blending.
    pub(super) fn commit_zoom_candidates(&mut self, candidates: Vec<ZoomCandidate>) -> usize {
        let mode = if self.supports_auto_zoom() {
            ZoomMode::Auto
        } else {
            ZoomMode::Manual
        };
        let mut added = 0;
        for candidate in candidates {
            if self.zoom_clips.iter().any(|clip| {
                ranges_overlap(candidate.start, candidate.end, clip.start, clip.end)
            }) {
                continue;
            }
            self.zoom_clips.push(candidate.into_clip(mode));
            added += 1;
        }
        if added > 0 {
            self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        added
    }
}
