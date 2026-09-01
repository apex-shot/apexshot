impl VideoEditState {
    pub fn add_zoom_at_playhead(&mut self) -> Option<usize> {
        self.add_zoom_at(self.playhead_seconds)
    }

    pub fn add_zoom_at(&mut self, start: f64) -> Option<usize> {
        if self.zoom_locked {
            return None;
        }
        let start = start.max(0.0);
        let end = start + DEFAULT_ZOOM_DURATION_SECONDS;
        if end - start < 0.2 {
            return None;
        }
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
            .is_some_and(|sidecar| !sidecar.pointer.is_empty())
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
        let suggestions = zoom_suggest::suggest_zooms(
            sidecar,
            self.metadata.width as f64,
            self.metadata.height as f64,
            self.source_duration(),
        );
        if suggestions.is_empty() {
            return 0;
        }
        let mode = if self.supports_auto_zoom() {
            ZoomMode::Auto
        } else {
            ZoomMode::Manual
        };
        let crop = self.crop_or_full();
        let segments = self.ordered_placed_segments();
        let mut added = 0;
        for suggestion in suggestions {
            let Some(&(composition_start, source_start, source_end)) =
                segments.iter().find(|&&(_, start, end)| {
                    suggestion.center_time >= start && suggestion.center_time <= end
                })
            else {
                continue;
            };
            let start = suggestion.start.max(source_start);
            let end = suggestion.end.min(source_end);
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
                easing: ZoomEasing::Glide,
                mode,
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
        .then(|| self.eval_zoom(self.source_playhead()).1);
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
            clip.easing = ZoomEasing::Glide;
            clip.ease_ms = DEFAULT_ZOOM_EASE_MS;
        }
    }

    pub fn move_zoom_clip(&mut self, index: usize, start: f64) {
        if self.zoom_locked {
            return;
        }
        let Some(clip) = self.zoom_clips.get(index).cloned() else {
            return;
        };
        let duration = clip.duration().max(0.2);
        let start = start.max(0.0);
        let end = start + duration;
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
        let mut start = start.max(0.0);
        let mut end = end.max(0.0);
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        if end - start < 0.2 {
            end = start + 0.2;
        }
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

    pub fn eval_zoom(&self, t: f64) -> (f64, (f64, f64)) {
        let frame_w = self.metadata.width as f64;
        let frame_h = self.metadata.height as f64;
        if self.zoom_hidden {
            return (1.0, (frame_w / 2.0, frame_h / 2.0));
        }
        let timeline_t = self.source_to_timeline(t);
        let (scale, center) = eval_zoom(&self.zoom_clips, timeline_t, frame_w, frame_h);
        if self.zoom_classic || scale <= 1.01 {
            return (scale, center);
        }
        let Some(clip) = self
            .zoom_clips
            .iter()
            .find(|clip| timeline_t >= clip.start && timeline_t <= clip.end)
        else {
            return (scale, center);
        };
        if clip.mode != ZoomMode::Auto {
            return (scale, center);
        }
        let Some((cursor_x, cursor_y, _)) = self
            .sidecar
            .as_ref()
            .and_then(|sidecar| sidecar.interpolated_at(t))
        else {
            return (scale, center);
        };
        (
            scale,
            recenter_if_near_edge(center, (cursor_x, cursor_y), scale, frame_w, frame_h),
        )
    }
}
