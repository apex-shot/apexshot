use crate::recording::editor::auto_zoom;

impl VideoEditState {
    pub fn add_zoom_at_playhead(&mut self) -> Option<usize> {
        self.add_zoom_at(self.playhead_seconds)
    }

    pub fn add_zoom_at(&mut self, start: f64) -> Option<usize> {
        if self.zoom_locked {
            return None;
        }
        let start = start.max(0.0);
        // A zoom may not run past the last frame of the video, so the default
        // span is trimmed to the boundary and refused when too little is left.
        let (start, end) = fit_effect_span(self, start, start + DEFAULT_ZOOM_DURATION_SECONDS)?;
        if self
            .zoom_clips
            .iter()
            .any(|clip| ranges_overlap(start, end, clip.start, clip.end))
        {
            return None;
        }
        let center = self.default_zoom_center(start);
        self.record_zoom_command();
        let style = self.last_edited_zoom_style.unwrap_or(ZoomStyle {
            scale: DEFAULT_ZOOM_SCALE,
            easing: ZoomEasing::Glide,
            ease_ms: DEFAULT_ZOOM_EASE_MS,
            instant: false,
        });
        self.zoom_clips.push(ZoomClip {
            start,
            end,
            scale: style.scale,
            center,
            ease_ms: style.ease_ms,
            easing: style.easing,
            instant: style.instant,
            mode: if self.supports_auto_zoom() {
                ZoomMode::Auto
            } else {
                ZoomMode::Manual
            },
            // Placed by the user, so automatic generation must not replace it
            // even though the camera may follow the pointer.
            origin: ZoomOrigin::User,
            ..Default::default()
        });
        self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .zoom_clips
            .iter()
            .position(|clip| (clip.start - start).abs() < 1e-6)?;
        self.selected_zoom = Some(index);
        self.selected_cursor_hide = None;
        self.selected_segment = None;
        self.selected_tool = EditorTool::Timeline;
        Some(index)
    }

    pub fn remove_selected_zoom(&mut self) {
        if let Some(index) = self.selected_zoom {
            self.remove_zoom_clip(index);
        }
    }

    /// Index of the zoom clip containing `timeline_t`, if any.
    pub fn zoom_clip_index_at(&self, timeline_t: f64) -> Option<usize> {
        self.zoom_clips
            .iter()
            .position(|clip| timeline_t >= clip.start && timeline_t <= clip.end)
    }

    /// Remove the zoom clip at `index`, for the clip menu which addresses a
    /// clip directly rather than going through the selection.
    pub fn remove_zoom_clip(&mut self, index: usize) {
        if self.zoom_locked || index >= self.zoom_clips.len() {
            return;
        }
        self.record_zoom_command();
        self.zoom_clips.remove(index);
        self.selected_zoom = match self.selected_zoom {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            other => other,
        };
    }

    pub fn remove_cursor_hide_clip(&mut self, index: usize) {
        if index >= self.cursor_hide_clips.len() {
            return;
        }
        self.cursor_hide_clips.remove(index);
        self.selected_cursor_hide = match self.selected_cursor_hide {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            other => other,
        };
    }

    /// Index of the hide clip containing `timeline_t`, if any.
    pub fn cursor_hide_index_at(&self, timeline_t: f64) -> Option<usize> {
        self.cursor_hide_clips
            .iter()
            .position(|clip| timeline_t >= clip.start && timeline_t <= clip.end)
    }

    /// A disabled clip keeps its span but stops feeding the preview and the
    /// export. Both track kinds share the toggle so the menu reads the same on
    /// either.
    pub fn set_zoom_hidden(&mut self, index: usize, hidden: bool) {
        if self.zoom_clips.get(index).is_none_or(|clip| clip.hidden == hidden) {
            return;
        }
        self.record_zoom_command();
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.hidden = hidden;
        }
        // A hidden generated clip must survive regeneration: the mute is a
        // deliberate edit, so it becomes the user's work.
        self.protect_zoom_clip(index);
    }

    pub fn set_cursor_hide_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(clip) = self.cursor_hide_clips.get_mut(index) {
            clip.hidden = hidden;
        }
    }

    /// Take a clip off its source anchor and make it the user's work: they
    /// have said where it belongs by dragging or resizing it, so no later
    /// composition edit may pull it back to the footage the generator picked.
    fn unanchor_zoom_clip(&mut self, index: usize) {
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.anchor = None;
        }
        self.protect_zoom_clip(index);
    }

    /// Promote a clip to the user's work so automatic generation leaves it
    /// alone. Generated clips flip to `User` on the first edit; user and
    /// legacy clips are already protected.
    fn protect_zoom_clip(&mut self, index: usize) {
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            if clip.origin == ZoomOrigin::Generated {
                clip.origin = ZoomOrigin::User;
            }
        }
    }

    /// Insert a copy of `clip` at `start`, keeping every field but the span.
    /// Returns the new index, or `None` when the span would collide with an
    /// existing clip — overlapping zooms have no defined blending, and the
    /// add path already refuses them.
    pub fn duplicate_zoom_clip(&mut self, index: usize, start: f64) -> Option<usize> {
        if self.zoom_locked {
            return None;
        }
        let clip = self.zoom_clips.get(index)?.clone();
        let start = start.max(0.0);
        let (start, end) = fit_effect_move(self, start, clip.duration());
        if self
            .zoom_clips
            .iter()
            .any(|other| ranges_overlap(start, end, other.start, other.end))
        {
            return None;
        }
        self.record_zoom_command();
        self.zoom_clips.push(ZoomClip {
            start,
            end,
            origin: ZoomOrigin::User,
            // The copy sits where the user dropped it, not on the original's
            // footage.
            anchor: None,
            ..clip
        });
        self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let new_index = self
            .zoom_clips
            .iter()
            .position(|other| (other.start - start).abs() < 1e-6)?;
        self.selected_zoom = Some(new_index);
        self.selected_cursor_hide = None;
        self.selected_segment = None;
        self.selected_tool = EditorTool::Timeline;
        Some(new_index)
    }

    pub fn duplicate_cursor_hide_clip(&mut self, index: usize, start: f64) -> Option<usize> {
        let clip = self.cursor_hide_clips.get(index)?.clone();
        let start = start.max(0.0);
        let (start, end) = fit_effect_move(self, start, clip.duration());
        if self
            .cursor_hide_clips
            .iter()
            .any(|other| ranges_overlap(start, end, other.start, other.end))
        {
            return None;
        }
        self.cursor_hide_clips
            .push(CursorHideClip { start, end, ..clip });
        self.cursor_hide_clips
            .sort_by(|a, b| a.start.total_cmp(&b.start));
        let new_index = self
            .cursor_hide_clips
            .iter()
            .position(|other| (other.start - start).abs() < 1e-6)?;
        self.selected_cursor_hide = Some(new_index);
        self.selected_zoom = None;
        self.selected_segment = None;
        self.selected_tool = EditorTool::Timeline;
        Some(new_index)
    }

    /// The clip most recently copied or cut, ready to paste at the playhead.
    pub fn clipboard_clip(&self) -> Option<&ClipClipboard> {
        self.clipboard.as_ref()
    }

    pub fn copy_zoom_clip(&mut self, index: usize) -> bool {
        let Some(clip) = self.zoom_clips.get(index).cloned() else {
            return false;
        };
        self.clipboard = Some(ClipClipboard::Zoom(clip));
        true
    }

    pub fn copy_cursor_hide_clip(&mut self, index: usize) -> bool {
        let Some(clip) = self.cursor_hide_clips.get(index).cloned() else {
            return false;
        };
        self.clipboard = Some(ClipClipboard::Hide(clip));
        true
    }

    pub fn cut_zoom_clip(&mut self, index: usize) -> bool {
        let Some(clip) = self.zoom_clips.get(index).cloned() else {
            return false;
        };
        // Recorded before the copy lands: the pending paste is part of the
        // step, so undoing a cut has to drop the clipboard again.
        self.record_zoom_command();
        self.clipboard = Some(ClipClipboard::Zoom(clip));
        self.zoom_clips.remove(index);
        self.selected_zoom = None;
        true
    }

    pub fn cut_cursor_hide_clip(&mut self, index: usize) -> bool {
        if !self.copy_cursor_hide_clip(index) {
            return false;
        }
        self.cursor_hide_clips.remove(index);
        self.selected_cursor_hide = None;
        true
    }

    /// Paste the clipboard at the playhead. The pasted clip lands where the
    /// playhead is and the playhead does not move, so repeated pastes stack at
    /// the same spot rather than marching along the timeline. A collision with
    /// an existing clip is refused the same way a new clip would be.
    ///
    /// The playhead is read in timeline space, not source space: clip spans are
    /// composition times — the same space `add_zoom_at` writes — so a paste
    /// placed from the source clock would land somewhere else the moment the
    /// composition is trimmed, cut or sped up.
    pub fn paste_clipboard_at_playhead(&mut self) -> Option<usize> {
        let at = self.playhead_seconds.max(0.0);
        let placed = match self.clipboard.clone()? {
            ClipClipboard::Zoom(clip) => {
                let (start, end) = fit_effect_span(self, at.max(0.0), at.max(0.0) + clip.duration())?;
                if self
                    .zoom_clips
                    .iter()
                    .any(|other| ranges_overlap(start, end, other.start, other.end))
                {
                    return None;
                }
                self.record_zoom_command();
                self.zoom_clips.push(ZoomClip {
                    start,
                    end,
                    origin: ZoomOrigin::User,
                    anchor: None,
                    ..clip
                });
                self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
                let index = self
                    .zoom_clips
                    .iter()
                    .position(|other| (other.start - start).abs() < 1e-6)?;
                self.selected_zoom = Some(index);
                self.selected_cursor_hide = None;
                self.selected_segment = None;
                self.selected_tool = EditorTool::Timeline;
                Some(index)
            }
            ClipClipboard::Hide(clip) => {
                let (start, end) = fit_effect_span(self, at.max(0.0), at.max(0.0) + clip.duration())?;
                if self
                    .cursor_hide_clips
                    .iter()
                    .any(|other| ranges_overlap(start, end, other.start, other.end))
                {
                    return None;
                }
                self.cursor_hide_clips.push(CursorHideClip { start, end, ..clip });
                self.cursor_hide_clips
                    .sort_by(|a, b| a.start.total_cmp(&b.start));
                let index = self
                    .cursor_hide_clips
                    .iter()
                    .position(|other| (other.start - start).abs() < 1e-6)?;
                self.selected_cursor_hide = Some(index);
                self.selected_zoom = None;
                self.selected_segment = None;
                self.selected_tool = EditorTool::Timeline;
                Some(index)
            }
        };
        // Placing the clip finishes the copy. Leaving it on the clipboard kept
        // the editor dimmed and the ghost on the lane after the paste, so the
        // next click placed another copy instead of returning to normal
        // editing.
        if placed.is_some() {
            self.clipboard = None;
        }
        placed
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard = None;
    }

    /// Whether a clip is waiting to be placed. The editor dims itself while
    /// this is true, so Copy and Cut read as the start of an action that has
    /// to be finished rather than as a no-op.
    pub fn is_pasting_clip(&self) -> bool {
        self.clipboard.is_some()
    }

    /// The span on the clipboard, when it holds a clip for the track the
    /// caller is drawing. `None` means this track has nothing to place, so the
    /// track falls back to its normal click-to-add behaviour.
    pub fn clipboard_duration_for(&self, is_zoom_track: bool) -> Option<f64> {
        match self.clipboard.as_ref()? {
            ClipClipboard::Zoom(clip) if is_zoom_track => Some(clip.duration()),
            ClipClipboard::Hide(clip) if !is_zoom_track => Some(clip.duration()),
            _ => None,
        }
    }

    /// Paste the clipboard so its start sits at `timeline_t`. Used by the
    /// click-to-place flow, where the pointer position is the destination
    /// rather than the playhead.
    pub fn paste_clipboard_at(&mut self, timeline_t: f64) -> Option<usize> {
        let start = timeline_t.max(0.0);
        let duration = self.clipboard_duration_for(true).or_else(|| {
            self.clipboard_duration_for(false)
        })?;
        let is_zoom_track = matches!(self.clipboard, Some(ClipClipboard::Zoom(_)));
        if !self.paste_spot_is_free(start, duration, is_zoom_track) {
            return None;
        }
        let restore = self.playhead_seconds;
        // The paste body reads the playhead, so the destination is staged
        // there and put back: a click-to-place must not move the playhead.
        self.playhead_seconds = start;
        let pasted = self.paste_clipboard_at_playhead();
        self.playhead_seconds = restore;
        pasted
    }

    /// Whether a clip of `duration` starting at `start` would fit on the track
    /// the clipboard is aimed at. The painter uses this to show a placeable
    /// ghost differently from one that would collide.
    pub fn paste_spot_is_free(&self, start: f64, duration: f64, is_zoom_track: bool) -> bool {
        // A span that cannot fit before the video ends is not placeable, the
        // same as one that would collide.
        if fit_effect_span(self, start, start + duration).is_none() {
            return false;
        }
        let end = start + duration;
        match (self.clipboard.as_ref(), is_zoom_track) {
            (Some(ClipClipboard::Zoom(_)), true) => !self
                .zoom_clips
                .iter()
                .any(|other| ranges_overlap(start, end, other.start, other.end)),
            (Some(ClipClipboard::Hide(_)), false) => !self
                .cursor_hide_clips
                .iter()
                .any(|other| ranges_overlap(start, end, other.start, other.end)),
            _ => false,
        }
    }

    /// Whether a paste would land right now: something is on the clipboard and
    /// the spot under the playhead is free. The menu offers Paste only when
    /// this is true, so the item never appears when pressing it could only
    /// fail.
    pub fn can_paste_clipboard_at_playhead(&self) -> bool {
        let Some(clip) = self.clipboard.as_ref() else {
            return false;
        };
        let start = self.playhead_seconds.max(0.0);
        match clip {
            ClipClipboard::Zoom(clip) => {
                // A paste that cannot fit before the video ends is refused,
                // just like one that would collide.
                if fit_effect_span(self, start, start + clip.duration()).is_none() {
                    return false;
                }
                !self.zoom_clips.iter().any(|other| {
                    ranges_overlap(start, start + clip.duration(), other.start, other.end)
                })
            }
            ClipClipboard::Hide(clip) => {
                if fit_effect_span(self, start, start + clip.duration()).is_none() {
                    return false;
                }
                !self.cursor_hide_clips.iter().any(|other| {
                    ranges_overlap(start, start + clip.duration(), other.start, other.end)
                })
            }
        }
    }

    /// Duplicate the selected clip, placing the copy directly after it so the
    /// two read as one run rather than an overlap.
    pub fn duplicate_selected_clip(&mut self) -> Option<usize> {
        if let Some(index) = self.selected_zoom {
            let after = self.zoom_clips.get(index)?.end;
            return self.duplicate_zoom_clip(index, after);
        }
        let index = self.selected_cursor_hide?;
        let after = self.cursor_hide_clips.get(index)?.end;
        self.duplicate_cursor_hide_clip(index, after)
    }

    /// Flip the hidden flag on whichever clip is selected.
    pub fn toggle_selected_clip_hidden(&mut self) -> bool {
        if let Some(index) = self.selected_zoom {
            let Some(clip) = self.zoom_clips.get(index) else {
                return false;
            };
            let hidden = !clip.hidden;
            self.set_zoom_hidden(index, hidden);
            return true;
        }
        if let Some(index) = self.selected_cursor_hide {
            let Some(clip) = self.cursor_hide_clips.get(index) else {
                return false;
            };
            let hidden = !clip.hidden;
            self.set_cursor_hide_hidden(index, hidden);
            return true;
        }
        false
    }

    /// Delete whichever track clip is selected. The video segment's own
    /// `remove_selected_clip` is a separate thing — it drops a segment from the
    /// composition — so this one is named for the two overlay tracks.
    pub fn remove_selected_track_clip(&mut self) {
        if self.selected_zoom.is_some() {
            self.remove_selected_zoom();
        } else if self.selected_cursor_hide.is_some() {
            self.remove_selected_cursor_hide();
        }
    }

    fn active_segment_index(&self) -> Option<usize> {
        self.selected_segment
            .or_else(|| self.segment_index_at_source(self.source_playhead()))
            .or_else(|| (!self.segment_speeds.is_empty()).then_some(0))
    }

    pub fn selected_clip_speed(&self) -> Option<f64> {
        self.active_segment_index()
            .map(|index| self.segment_speed(index))
    }

    pub fn set_selected_clip_speed(&mut self, speed: f64) {
        if self.video_locked {
            return;
        }
        let Some(index) = self.active_segment_index() else {
            return;
        };
        self.selected_segment = Some(index);
        if index >= self.segment_speeds.len() {
            self.segment_speeds.resize(index + 1, 1.0);
        }
        let old_end = self.segment_start(index) + self.segment_timeline_duration(index);
        let old_duration = self.segment_timeline_duration(index);
        self.segment_speeds[index] = if speed.is_finite() {
            speed.clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED)
        } else {
            1.0
        };
        let shift = self.segment_timeline_duration(index) - old_duration;
        if shift.abs() > 1e-9 {
            for (other, start) in self.segment_starts.iter_mut().enumerate() {
                if other != index && *start + 1e-9 >= old_end {
                    *start = (*start + shift).max(0.0);
                }
            }
        }
        self.sync_offset_from_segments();
        self.composition_changed();
        self.clamp_timeline_scroll();
    }

    pub fn selected_clip_muted(&self) -> Option<bool> {
        self.active_segment_index()
            .map(|index| self.segment_is_muted(index))
    }

    pub fn set_selected_clip_muted(&mut self, muted: bool) {
        if self.audio_locked || !self.has_audio_track() {
            return;
        }
        let Some(index) = self.active_segment_index() else {
            return;
        };
        self.selected_segment = Some(index);
        if index >= self.segment_muted.len() {
            self.segment_muted.resize(index + 1, false);
        }
        self.segment_muted[index] = muted;
    }

    pub fn remove_selected_clip(&mut self) {
        if self.video_locked {
            return;
        }
        let Some(index) = self.selected_segment.take() else {
            return;
        };
        if let Some(kept) = self.segments_kept.get_mut(index) {
            *kept = false;
        }
        self.composition_changed();
    }

    pub fn segment_speed(&self, index: usize) -> f64 {
        let speed = self.segment_speeds.get(index).copied().unwrap_or(1.0);
        if speed.is_finite() {
            speed.clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED)
        } else {
            1.0
        }
    }

    pub fn segment_timeline_duration(&self, index: usize) -> f64 {
        self.segment_boundaries()
            .get(index)
            .map(|(start, end)| (end - start).max(0.0) / self.segment_speed(index))
            .unwrap_or(0.0)
    }

    pub fn segment_is_muted(&self, index: usize) -> bool {
        self.segment_muted.get(index).copied().unwrap_or(false)
    }

    pub fn speed_for_source(&self, source_t: f64) -> f64 {
        self.segment_index_at_source(source_t)
            .map(|index| self.segment_speed(index))
            .unwrap_or(1.0)
    }

    pub fn muted_for_source(&self, source_t: f64) -> bool {
        self.is_muted()
            || self
                .segment_index_at_source(source_t)
                .is_some_and(|index| self.segment_is_muted(index))
    }

    fn segment_index_at_source(&self, source_t: f64) -> Option<usize> {
        self.segment_boundaries()
            .iter()
            .rposition(|&(start, end)| source_t + 1e-9 >= start && source_t <= end + 1e-9)
    }

    pub fn selected_zoom_clip(&self) -> Option<&ZoomClip> {
        self.selected_zoom
            .and_then(|index| self.zoom_clips.get(index))
    }

    pub fn supports_auto_zoom(&self) -> bool {
        self.sidecar
            .as_ref()
            .is_some_and(|sidecar| !sidecar.pointer.is_empty() || !sidecar.clicks.is_empty())
    }

    /// Populate the timeline with zoom clips derived from recorded pointer
    /// interactions. Returns the number of clips added.
    pub fn suggest_zoom_clips(&mut self) -> usize {
        if self.zoom_locked {
            return 0;
        }
        self.record_zoom_command();
        let candidates = self.placed_zoom_candidates();
        self.commit_zoom_candidates(candidates)
    }

    /// Where a generation pass would put its zooms, without placing any.
    ///
    /// The placement half of a pass: the trim, cut, crop, and overlap rules all
    /// run here, so a caller can show the result before it becomes clips. It
    /// records nothing itself — whoever commits the result takes the one step
    /// for the whole pass.
    pub(super) fn placed_zoom_candidates(&self) -> Vec<ZoomCandidate> {
        let Some(sidecar) = &self.sidecar else {
            return Vec::new();
        };
        let width = self.metadata.width as f64;
        let height = self.metadata.height as f64;
        let clicks = auto_zoom::clicks_from_sidecar(sidecar, width, height);
        let pointer = auto_zoom::pointer_from_sidecar(sidecar, width, height);
        let zooms = auto_zoom::automatic_zooms(&clicks, &pointer, self.source_duration());
        if zooms.is_empty() {
            return Vec::new();
        }
        let crop = self.crop_or_full();
        let segments = self.ordered_placed_segments();
        let mut candidates = Vec::with_capacity(zooms.len());
        for zoom in zooms {
            // A window belongs to the footage its interaction sits on; at an
            // exact cut the later source range owns the timestamp, matching
            // reprojection.
            let Some(&(composition_start, source_start, source_end)) = segments
                .iter()
                .filter(|&&(_, start, end)| {
                    zoom.center_time + 1e-9 >= start && zoom.center_time <= end + 1e-9
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            let start = zoom.start.max(source_start);
            let end = zoom.end.min(source_end);
            if end - start < auto_zoom::MIN_WINDOW_SECONDS {
                continue;
            }
            let speed = self.speed_for_source(start);
            let timeline_start = composition_start + (start - source_start) / speed;
            let timeline_end = composition_start + (end - source_start) / speed;
            if self
                .zoom_clips
                .iter()
                .any(|clip| ranges_overlap(timeline_start, timeline_end, clip.start, clip.end))
            {
                continue;
            }
            let (crop_x, crop_y, crop_w, crop_h) = crop;
            if zoom.center.0 < crop_x
                || zoom.center.0 >= crop_x + crop_w
                || zoom.center.1 < crop_y
                || zoom.center.1 >= crop_y + crop_h
            {
                continue;
            }
            let scale = zoom.scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
            let center = clamp_zoom_center(crop, scale, zoom.center);
            candidates.push(ZoomCandidate {
                start: timeline_start,
                end: timeline_end,
                scale,
                center,
                // The generator picked this footage, not this composition
                // time, so the clip it becomes follows it through a trim, a
                // re-cut, or a speed change.
                anchor: ZoomAnchor {
                    source_start: start,
                    source_end: end,
                },
            });
        }
        candidates
    }

    /// Put every anchored clip back on the footage it was placed for.
    ///
    /// A clip span is a composition time, so an edit that changes which
    /// footage plays where — trimming, cutting, unkeeping a segment, moving a
    /// segment, or retiming one — would leave a generated zoom framing
    /// whatever now sits at its old position. Re-deriving the span from the
    /// anchor is what keeps "the zoom on the click" true after the edit.
    ///
    /// A clip whose footage the composition no longer plays is dropped: the
    /// generator made it for a moment that is gone, and keeping it would frame
    /// something else. It is still the clip's own start that owns the source
    /// time at an exact cut, matching how placement resolves the same
    /// timestamp. A clip that spans a newly added cut stays with the piece
    /// that owns its start rather than splitting in two.
    ///
    /// Clips the user placed or dragged carry no anchor and never move.
    pub(crate) fn composition_changed(&mut self) {
        self.reproject_anchored_zooms();
    }

    pub(crate) fn reproject_anchored_zooms(&mut self) {
        if self.zoom_clips.iter().all(|clip| clip.anchor.is_none()) {
            return;
        }
        let segments = self.placed_segment_slots();
        let speeds: Vec<f64> = (0..self.segment_speeds.len())
            .map(|index| self.segment_speed(index))
            .collect();
        let mut reprojected: Vec<ZoomClip> = Vec::with_capacity(self.zoom_clips.len());
        for clip in self.zoom_clips.drain(..) {
            let Some(anchor) = clip.anchor else {
                reprojected.push(clip);
                continue;
            };
            // At an exact cut both neighboring ranges contain the timestamp;
            // the later source start owns it, as in placement.
            let Some(&(segment, composition_start, source_start, source_end)) = segments
                .iter()
                .filter(|&&(_, _, start, end)| {
                    anchor.source_start + 1e-9 >= start && anchor.source_start <= end + 1e-9
                })
                .max_by(|a, b| a.2.total_cmp(&b.2))
            else {
                continue;
            };
            let speed = speeds.get(segment).copied().unwrap_or(1.0);
            let start = anchor.source_start.max(source_start);
            let end = anchor.source_end.min(source_end);
            if end - start <= f64::EPSILON {
                continue;
            }
            reprojected.push(ZoomClip {
                start: composition_start + (start - source_start) / speed,
                end: composition_start + (end - source_start) / speed,
                ..clip
            });
        }
        reprojected.sort_by(|a, b| a.start.total_cmp(&b.start));
        // The move can land an anchored clip on one that did not move — a
        // Manual zoom the user placed further along, say. Overlapping zooms
        // have no defined blending, so the clip that moved gives way; a clip
        // the user placed is never dropped by an edit.
        let mut resolved: Vec<ZoomClip> = Vec::with_capacity(reprojected.len());
        for clip in reprojected {
            if let Some(previous) = resolved.last() {
                if ranges_overlap(clip.start, clip.end, previous.start, previous.end) {
                    if clip.anchor.is_some() {
                        continue;
                    }
                    if previous.anchor.is_some() {
                        resolved.pop();
                    }
                }
            }
            resolved.push(clip);
        }
        self.zoom_clips = resolved;
        self.selected_zoom = self
            .selected_zoom
            .filter(|index| *index < self.zoom_clips.len());
    }

    /// Drop the zooms this generator placed and nobody has touched since, then
    /// place new ones from this recording's pointer path.
    ///
    /// Ownership, not camera mode, decides what is replaced: a user-added Auto
    /// clip and an edited or hidden generated one are both the user's work, so
    /// they survive. Clips loaded from an older project have no origin and are
    /// treated the same way rather than being replaced by default.
    pub fn redetect_zoom_clips(&mut self) -> bool {
        if self.zoom_locked {
            return false;
        }
        // An explicit re-run is itself a review, so opening again stays quiet.
        self.zoom_suggestions_reviewed = true;
        self.record_zoom_command();
        let before = self.zoom_clips.len();
        self.zoom_clips
            .retain(|clip| clip.origin != ZoomOrigin::Generated);
        let removed = before - self.zoom_clips.len();
        self.selected_zoom = None;
        let candidates = self.placed_zoom_candidates();
        let added = self.commit_zoom_candidates(candidates);
        if added > 0 {
            self.selected_zoom = self
                .zoom_clips
                .iter()
                .position(|clip| clip.mode == ZoomMode::Auto);
        }
        removed > 0 || added > 0
    }

    /// Whether the automatic zoom pass has run for this recording.
    pub fn zoom_suggestions_reviewed(&self) -> bool {
        self.zoom_suggestions_reviewed
    }

    /// Record that the automatic pass has run, so it stays quiet on open until
    /// [`Self::reset_zoom_suggestions_reviewed`].
    pub fn mark_zoom_suggestions_reviewed(&mut self) {
        self.zoom_suggestions_reviewed = true;
    }

    /// Re-enable the automatic pass on open. This is the escape hatch for a
    /// user who wants the editor to propose again; the explicit Suggest action
    /// regenerates without it.
    pub fn reset_zoom_suggestions_reviewed(&mut self) {
        self.zoom_suggestions_reviewed = false;
    }

    /// Run the automatic zoom pass the first time the editor opens a
    /// recording. Returns true when suggestions were added.
    ///
    /// The pass runs once and is recorded even when it finds nothing: an empty
    /// clip list is a legitimate result, not proof that the user has not been
    /// asked. Recording it separately from `zoom_clips` is what makes rejecting
    /// every suggestion — or deleting all of them — stick across reopen.
    pub fn suggest_zooms_on_open(&mut self) -> bool {
        if self.zoom_suggestions_reviewed {
            return false;
        }
        self.zoom_suggestions_reviewed = true;
        // A project that already carries clips has been through this pass;
        // there is nothing to fill.
        if !self.zoom_clips.is_empty() {
            return false;
        }
        self.suggest_zoom_clips() > 0
    }

    pub fn set_selected_zoom_mode(&mut self, mode: ZoomMode) {
        if self.zoom_locked {
            return;
        }
        if mode == ZoomMode::Auto && !self.supports_auto_zoom() {
            return;
        }
        let visible_center = (mode == ZoomMode::Manual
            && self
                .selected_zoom_clip()
                .is_some_and(|clip| clip.mode == ZoomMode::Auto))
        .then(|| self.eval_zoom_at(self.playhead_seconds, self.source_playhead()).1);
        let crop = self.crop_or_full();
        if let Some(index) = self.selected_zoom {
            self.record_zoom_command();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                if let Some(center) = visible_center {
                    clip.center = clamp_zoom_center(crop, clip.scale, center);
                }
                clip.mode = mode;
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn set_selected_zoom_scale(&mut self, scale: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.scale = scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
            }
            self.protect_zoom_clip(index);
            self.capture_zoom_style(index);
        }
    }

    pub fn set_selected_zoom_easing(&mut self, easing: ZoomEasing) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_zoom_command();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.easing = easing;
            }
            self.protect_zoom_clip(index);
            self.capture_zoom_style(index);
        }
    }

    /// Flip the selected zoom between a snap and a glide.
    ///
    /// Animated is the follow camera; instant jumps to its target with no
    /// eased scale ramp and no morph from a neighbour. The choice is
    /// per-zoom and remembered like the rest of the style.
    pub fn set_selected_zoom_instant(&mut self, instant: bool) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_zoom_command();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.instant = instant;
            }
            self.protect_zoom_clip(index);
            self.capture_zoom_style(index);
        }
    }

    pub fn set_selected_zoom_ease_ms(&mut self, ease_ms: u32) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.ease_ms = ease_ms.clamp(MIN_ZOOM_EASE_MS, MAX_ZOOM_EASE_MS);
            }
            self.protect_zoom_clip(index);
            self.capture_zoom_style(index);
        }
    }

    /// Remember a zoom's style so the next zoom the user adds opens with it.
    fn capture_zoom_style(&mut self, index: usize) {
        if let Some(clip) = self.zoom_clips.get(index) {
            self.last_edited_zoom_style = Some(ZoomStyle {
                scale: clip.scale,
                easing: clip.easing,
                ease_ms: clip.ease_ms,
                instant: clip.instant,
            });
        }
    }

    pub fn set_selected_zoom_center(&mut self, center: (f64, f64)) {
        if self.zoom_locked {
            return;
        }
        let Some(index) = self.selected_zoom else {
            return;
        };
        let scale = match self.zoom_clips.get(index) {
            Some(clip) if clip.mode == ZoomMode::Manual => clip.scale,
            _ => return,
        };
        let center = clamp_zoom_center(self.crop_or_full(), scale, center);
        self.record_continuous_zoom_edit();
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.center = center;
        }
        self.protect_zoom_clip(index);
    }

    pub fn reset_zoom_animation(&mut self) {
        if self.zoom_locked {
            return;
        }
        self.record_zoom_command();
        self.zoom_classic = false;
        if let Some(index) = self.selected_zoom {
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                // Auto zooms are created with Smooth; Reset must not bring back
                // the edge snap. Manual zooms keep the classic Glide default.
                // Either way the zoom returns to animated: instant is the
                // opt-in snap, not the resting state.
                clip.instant = false;
                clip.easing = match clip.mode {
                    ZoomMode::Auto => ZoomEasing::Smooth,
                    ZoomMode::Manual => ZoomEasing::Glide,
                };
                clip.ease_ms = DEFAULT_ZOOM_EASE_MS;
                clip.rotation_x = 0.0;
                clip.rotation_y = 0.0;
                clip.rotation_z = 0.0;
                clip.perspective = 0.0;
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn move_zoom_clip(&mut self, index: usize, start: f64) {
        if self.zoom_locked {
            return;
        }
        let Some(clip) = self.zoom_clips.get(index).cloned() else {
            return;
        };
        // Keep the clip's length but pull it back inside the video.
        let (start, end) = fit_effect_move(self, start, clip.duration());
        if self.zoom_clips.iter().enumerate().any(|(other, existing)| {
            other != index && ranges_overlap(start, end, existing.start, existing.end)
        }) {
            return;
        }
        self.record_continuous_zoom_edit();
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
        self.unanchor_zoom_clip(index);
    }

    pub fn set_zoom_range(&mut self, index: usize, start: f64, end: f64) {
        if self.zoom_locked {
            return;
        }
        if self.zoom_clips.get(index).is_none() {
            return;
        }
        let Some((start, end)) = fit_effect_span(self, start, end) else {
            return;
        };
        if self.zoom_clips.iter().enumerate().any(|(other, existing)| {
            other != index && ranges_overlap(start, end, existing.start, existing.end)
        }) {
            return;
        }
        self.record_continuous_zoom_edit();
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
        self.unanchor_zoom_clip(index);
    }

    pub fn set_selected_zoom_yaw(&mut self, yaw: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.rotation_y = yaw.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn set_selected_zoom_pitch(&mut self, pitch: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.rotation_x = pitch.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn set_selected_zoom_roll(&mut self, roll: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.rotation_z = roll.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn set_selected_zoom_perspective(&mut self, perspective: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom {
            self.record_continuous_zoom_edit();
            if let Some(clip) = self.zoom_clips.get_mut(index) {
                clip.perspective = perspective.clamp(0.0, 1.0);
            }
            self.protect_zoom_clip(index);
        }
    }

    pub fn eval_zoom_pose(&self, t: f64) -> MotionTransform {
        self.eval_zoom_pose_at(self.source_to_timeline(t))
    }

    /// `eval_zoom_pose` for a caller that already holds the composition time.
    ///
    /// Zoom clips live on the timeline, so a freeze hold is matched by the
    /// playhead's composition position — the pinned source frame would keep the
    /// clip stuck at `footage_end` for the whole hold.
    pub fn eval_zoom_pose_at(&self, timeline_t: f64) -> MotionTransform {
        if self.zoom_hidden {
            return MotionTransform::default();
        }
        let Some(clip) = self
            .zoom_clips
            .iter()
            .find(|clip| !clip.hidden && timeline_t >= clip.start && timeline_t <= clip.end)
        else {
            return MotionTransform::default();
        };
        let target = clip.card_pose();
        let span = (clip.end - clip.start).max(0.0);
        if span <= f64::EPSILON {
            return target;
        }
        let ease = (clip.ease_ms as f64 / 1000.0).clamp(0.0, span / 2.0);
        let alpha = if ease <= f64::EPSILON {
            ((timeline_t - clip.start) / span).clamp(0.0, 1.0)
        } else if timeline_t < clip.start + ease {
            ((timeline_t - clip.start) / ease).clamp(0.0, 1.0)
        } else {
            return target;
        };
        lerp_transform(
            MotionTransform::default(),
            target,
            clip.easing.apply(alpha),
        )
    }

    pub fn eval_zoom(&self, t: f64) -> (f64, (f64, f64)) {
        self.eval_zoom_at(self.source_to_timeline(t), t)
    }

    /// `eval_zoom` for a caller that already holds both times.
    ///
    /// `timeline_t` matches the clip — zoom clips are placed in composition
    /// seconds and must keep advancing through a freeze hold. `source_t`
    /// locates the pointer for the follow camera, which inside a hold stays
    /// pinned to the last real frame.
    ///
    /// The follow camera chases the movement-group centre active at
    /// `source_t` on a damped spring, starting at the clip's stored centre.
    /// The evaluation is pure (no carried state), so random seeks match
    /// sequential playback. A recorded drag selects the stiff drag spring
    /// ahead of the click spring; release stiffness and typing suppression
    /// are intentionally absent, since neither has a recorded signal and key
    /// identities are never collected.
    pub fn eval_zoom_at(&self, timeline_t: f64, source_t: f64) -> (f64, (f64, f64)) {
        let frame_w = self.metadata.width as f64;
        let frame_h = self.metadata.height as f64;
        if self.zoom_hidden {
            return (1.0, (frame_w / 2.0, frame_h / 2.0));
        }
        let (scale, center) = eval_zoom(&self.zoom_clips, timeline_t, frame_w, frame_h);
        if self.zoom_classic || scale <= 1.01 {
            return (scale, center);
        }
        // Inside a clip, follow its own framing. In a morph gap, keep following
        // the clip the camera just left: `eval_zoom` holds that clip's stored
        // framing, and recentering it here continues the camera from the
        // evaluated endpoint instead of snapping back to the stored center.
        let clip_index = self
            .zoom_clips
            .iter()
            .position(|clip| !clip.hidden && timeline_t >= clip.start && timeline_t <= clip.end)
            .or_else(|| zoom_gap_hold_predecessor(&self.zoom_clips, timeline_t));
        let Some(clip) = clip_index.map(|index| &self.zoom_clips[index]) else {
            return (scale, center);
        };
        if clip.mode != ZoomMode::Auto {
            return (scale, center);
        }
        let Some(sidecar) = self.sidecar.as_ref() else {
            return (scale, center);
        };
        // Pointer and clicks in encoded-video pixels, the same space the
        // cursor overlay draws in, so the camera and the cursor never react
        // to different data. Area recordings need the video mapping; without
        // it the follow would chase capture-local coordinates.
        let points: Vec<(f64, f64, f64)> = sidecar
            .pointer
            .iter()
            .map(|sample| {
                let (x, y) = sidecar.map_to_video(sample.x, sample.y, frame_w, frame_h);
                (sample.t, x, y)
            })
            .collect();
        if points.is_empty() {
            return (scale, center);
        }
        let click_times: Vec<f64> = sidecar.clicks.iter().map(|click| click.t).collect();
        let crop = self.crop_or_full();
        let budget = movement_group_budget(crop, clip.scale);
        if clip.instant {
            // An instant zoom snaps to the group centre instead of chasing
            // it: same target, no spring.
            let target =
                movement_group_center_at(&points, source_t, budget).unwrap_or(clip.center);
            return (scale, clamp_zoom_center(crop, scale, target));
        }
        let spring = camera_spring_for_time(&click_times, &sidecar.presses, source_t);
        let from_source = self.timeline_to_source(clip.start);
        if !source_t.is_finite() || !from_source.is_finite() || source_t <= from_source {
            return (scale, clamp_zoom_center(crop, scale, clip.center));
        }
        let followed = evaluate_spring_camera(
            clip.center,
            &points,
            from_source,
            source_t,
            budget,
            spring,
        );
        (scale, clamp_zoom_center(crop, scale, followed))
    }
}
