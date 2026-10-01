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
        self.zoom_clips.push(ZoomClip {
            start,
            end,
            scale: DEFAULT_ZOOM_SCALE,
            center,
            ease_ms: DEFAULT_ZOOM_EASE_MS,
            easing: ZoomEasing::Glide,
            mode: if self.supports_auto_zoom() {
                ZoomMode::Auto
            } else {
                ZoomMode::Manual
            },
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
        if self.zoom_locked {
            return;
        }
        if let Some(index) = self.selected_zoom.take() {
            if index < self.zoom_clips.len() {
                self.zoom_clips.remove(index);
            }
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
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.hidden = hidden;
        }
    }

    pub fn set_cursor_hide_hidden(&mut self, index: usize, hidden: bool) {
        if let Some(clip) = self.cursor_hide_clips.get_mut(index) {
            clip.hidden = hidden;
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
        self.zoom_clips.push(ZoomClip {
            start,
            end,
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
        if !self.copy_zoom_clip(index) {
            return false;
        }
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
                self.zoom_clips.push(ZoomClip { start, end, ..clip });
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
        let Some(sidecar) = &self.sidecar else {
            return 0;
        };
        let mut suggestions = zoom_suggest::suggest_zooms(
            sidecar,
            self.metadata.width as f64,
            self.metadata.height as f64,
            self.source_duration(),
        );
        if suggestions.is_empty() {
            return 0;
        }
        suggestions.sort_by(|a, b| {
            b.priority
                .total_cmp(&a.priority)
                .then_with(|| a.start.total_cmp(&b.start))
        });
        let mode = if self.supports_auto_zoom() {
            ZoomMode::Auto
        } else {
            ZoomMode::Manual
        };
        let crop = self.crop_or_full();
        let segments = self.ordered_placed_segments();
        // The density budget counts candidates that actually land in the kept
        // edit. Candidates the trim, crop, or an existing clip rejects do not
        // spend it, so a lower-ranked valid suggestion can still be placed.
        let limit =
            ((self.source_duration() / zoom_suggest::SECONDS_PER_SUGGESTION).ceil() as usize).max(1);
        let mut added = 0;
        for suggestion in suggestions {
            if added >= limit {
                break;
            }
            // At an exact cut both neighboring source ranges contain the
            // timestamp. Match segment_index_at_source by choosing the range
            // with the later source start rather than attaching the zoom to
            // the segment that just ended.
            let Some(&(composition_start, source_start, source_end)) = segments
                .iter()
                .filter(|&&(_, start, end)| {
                    suggestion.center_time + 1e-9 >= start && suggestion.center_time <= end + 1e-9
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            let mut start = suggestion.start.max(source_start);
            let mut end = suggestion.end.min(source_end);
            // A valid interaction close to a trim/cut boundary can lose most
            // of its pre/post-roll. Refit the minimum useful duration inside
            // the selected segment instead of dropping the detection.
            if end - start < zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS
                && source_end - source_start >= zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS
            {
                let half = zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS * 0.5;
                start = (suggestion.center_time - half).max(source_start);
                end = (start + zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS).min(source_end);
                start = (end - zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS).max(source_start);
            }
            if end - start < zoom_suggest::MIN_SUGGESTED_ZOOM_SECONDS {
                continue;
            }
            let speed = self.speed_for_source(suggestion.center_time);
            let timeline_start = composition_start + (start - source_start) / speed;
            let timeline_end = composition_start + (end - source_start) / speed;
            if self
                .zoom_clips
                .iter()
                .any(|clip| ranges_overlap(timeline_start, timeline_end, clip.start, clip.end))
            {
                continue;
            }
            let scale = suggestion.scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
            let (crop_x, crop_y, crop_w, crop_h) = crop;
            if suggestion.center.0 < crop_x
                || suggestion.center.0 >= crop_x + crop_w
                || suggestion.center.1 < crop_y
                || suggestion.center.1 >= crop_y + crop_h
            {
                continue;
            }
            let center = clamp_zoom_center(crop, scale, suggestion.center);
            self.zoom_clips.push(ZoomClip {
                start: timeline_start,
                end: timeline_end,
                scale,
                center,
                ease_ms: DEFAULT_ZOOM_EASE_MS,
                // Auto zooms must launch and settle at zero velocity, or the
                // pulse snaps at both clip edges.
                easing: ZoomEasing::Smooth,
                mode,
                ..Default::default()
            });
            added += 1;
        }
        if added > 0 {
            self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        added
    }

    /// Drop previously auto-detected zooms and place new ones from this
    /// recording's pointer path. Manual clips are kept.
    pub fn redetect_zoom_clips(&mut self) -> bool {
        if self.zoom_locked {
            return false;
        }
        let before = self.zoom_clips.len();
        self.zoom_clips.retain(|clip| clip.mode != ZoomMode::Auto);
        let removed = before - self.zoom_clips.len();
        self.selected_zoom = None;
        let added = self.suggest_zoom_clips();
        if added > 0 {
            self.selected_zoom = self
                .zoom_clips
                .iter()
                .position(|clip| clip.mode == ZoomMode::Auto);
        }
        removed > 0 || added > 0
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
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            if let Some(center) = visible_center {
                clip.center = clamp_zoom_center(crop, clip.scale, center);
            }
            clip.mode = mode;
        }
    }

    pub fn set_selected_zoom_scale(&mut self, scale: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.scale = scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
        }
    }

    pub fn set_selected_zoom_easing(&mut self, easing: ZoomEasing) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.easing = easing;
        }
    }

    pub fn set_selected_zoom_ease_ms(&mut self, ease_ms: u32) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.ease_ms = ease_ms.clamp(MIN_ZOOM_EASE_MS, MAX_ZOOM_EASE_MS);
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
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.center = center;
        }
    }

    pub fn reset_zoom_animation(&mut self) {
        if self.zoom_locked {
            return;
        }
        self.zoom_classic = false;
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            // Auto zooms are created with Smooth; Reset must not bring back
            // the edge snap. Manual zooms keep the classic Glide default.
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
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
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
        if let Some(clip) = self.zoom_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
    }

    pub fn set_selected_zoom_yaw(&mut self, yaw: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.rotation_y = yaw.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
    }

    pub fn set_selected_zoom_pitch(&mut self, pitch: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.rotation_x = pitch.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
    }

    pub fn set_selected_zoom_roll(&mut self, roll: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.rotation_z = roll.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
    }

    pub fn set_selected_zoom_perspective(&mut self, perspective: f64) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
            clip.perspective = perspective.clamp(0.0, 1.0);
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
    /// locates the cursor for auto-zoom recentering, which inside a hold stays
    /// pinned to the last real frame.
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
        let Some(clip) = self
            .zoom_clips
            .iter()
            .find(|clip| !clip.hidden && timeline_t >= clip.start && timeline_t <= clip.end)
        else {
            return (scale, center);
        };
        if clip.mode != ZoomMode::Auto {
            return (scale, center);
        }
        let cursor = self.cursor.clamped();
        let Some((cursor_x, cursor_y)) = self
            .sidecar
            .as_ref()
            .and_then(|sidecar| sidecar.motion_position_at(source_t, cursor.smooth, cursor.speed))
        else {
            return (scale, center);
        };
        (
            scale,
            recenter_if_near_edge(center, (cursor_x, cursor_y), scale, frame_w, frame_h),
        )
    }
}
