// Zoom suggestions staged for review, before they become clips.
//
// Detection used to apply its suggestions the moment the user asked for them,
// so the only way to judge a pass was to look at the timeline it had already
// rewritten. A staged candidate is the same placement held aside: it is not a
// clip, so it takes no part in the preview, the export, or the saved project
// until `apply_zoom_candidates` commits it.

/// A zoom the generator placed and the user has not accepted yet.
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
    /// Suggestions waiting to be applied or thrown away, in timeline order.
    pub fn zoom_candidates(&self) -> &[ZoomCandidate] {
        &self.zoom_candidates
    }

    /// Whether anything is waiting for review.
    pub fn has_zoom_candidates(&self) -> bool {
        !self.zoom_candidates.is_empty()
    }

    /// Detect into a staged review. Returns how many candidates are waiting.
    ///
    /// An explicit Detect is the pass running, so it counts as reviewed: a
    /// discarded review does not come back on the next open.
    pub fn stage_zoom_candidates(&mut self) -> usize {
        if self.zoom_locked {
            return 0;
        }
        self.zoom_suggestions_reviewed = true;
        // The review replaces the selection: the panel that shows it is the
        // one that appears when no clip is selected.
        self.selected_zoom = None;
        self.zoom_candidates = self.placed_zoom_candidates();
        self.zoom_candidates.len()
    }

    /// Commit the staged candidates as clips, as one undo step. Returns how
    /// many were added.
    pub fn apply_zoom_candidates(&mut self) -> usize {
        if self.zoom_locked || self.zoom_candidates.is_empty() {
            return 0;
        }
        self.record_zoom_command();
        let candidates = std::mem::take(&mut self.zoom_candidates);
        self.commit_zoom_candidates(candidates)
    }

    /// Throw the staged review away without touching the timeline. Returns
    /// whether there was anything to throw away.
    pub fn discard_zoom_candidates(&mut self) -> bool {
        !std::mem::take(&mut self.zoom_candidates).is_empty()
    }

    /// Turn placed candidates into clips.
    ///
    /// The overlap check runs here as well as at placement: a clip added after
    /// the review was staged must not be overrun by it, and overlapping zooms
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
