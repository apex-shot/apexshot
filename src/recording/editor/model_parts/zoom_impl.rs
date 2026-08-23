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
            mode: ZoomMode::Auto,
        });
        self.zoom_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .zoom_clips
            .iter()
            .position(|clip| (clip.start - start).abs() < 1e-6)?;
        self.selected_zoom = Some(index);
        self.selected_segment = None;
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
        self.segment_speeds[index] = speed.clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED);
    }

    pub fn selected_clip_muted(&self) -> Option<bool> {
        self.active_segment_index()
            .map(|index| self.segment_is_muted(index))
    }

    pub fn set_selected_clip_muted(&mut self, muted: bool) {
        if !self.has_audio_track() {
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
        self.segment_speeds.get(index).copied().unwrap_or(1.0)
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
            .position(|&(start, end)| source_t + 1e-9 >= start && source_t <= end + 1e-9)
    }

    pub fn selected_zoom_clip(&self) -> Option<&ZoomClip> {
        self.selected_zoom
            .and_then(|index| self.zoom_clips.get(index))
    }

    pub fn set_selected_zoom_mode(&mut self, mode: ZoomMode) {
        if self.zoom_locked {
            return;
        }
        if let Some(clip) = self
            .selected_zoom
            .and_then(|index| self.zoom_clips.get_mut(index))
        {
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
        self.zoom_classic = false;
        self.zoom_blur_samples = DEFAULT_ZOOM_BLUR_SAMPLES;
        self.zoom_blur_shutter = DEFAULT_ZOOM_BLUR_SHUTTER;
    }

    pub fn set_zoom_blur_samples(&mut self, samples: u32) {
        self.zoom_blur_samples = samples.clamp(MIN_ZOOM_BLUR_SAMPLES, MAX_ZOOM_BLUR_SAMPLES);
    }

    pub fn set_zoom_blur_shutter(&mut self, shutter: f64) {
        self.zoom_blur_shutter = shutter.clamp(0.0, 1.0);
    }

    pub fn zoom_blur_mix_frames(&self) -> u32 {
        // ponytail: tmix averages output frames, not crop-path samples. Multi-crop blend if it looks wrong.
        let frames = (self.zoom_blur_samples as f64 * self.zoom_blur_shutter).round() as u32;
        frames.clamp(1, MAX_ZOOM_BLUR_SAMPLES)
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
