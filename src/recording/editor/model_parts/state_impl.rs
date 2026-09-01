impl VideoEditState {
    pub fn new(metadata: VideoMetadata) -> Self {
        let sidecar = PointerSidecar::load_next_to_video(&metadata.path);
        let project_media = seed_project_media(&metadata);
        let title = title_from_path(&metadata.path);
        Self {
            trim_start_seconds: 0.0,
            trim_end_seconds: metadata.duration_seconds,
            playhead_seconds: 0.0,
            custom_width: metadata.width,
            custom_height: metadata.height,
            metadata,
            dimension_preset: DimensionPreset::Original,
            quality: 70,
            audio_mode: AudioMode::Unchanged,
            cuts: Vec::new(),
            segments_kept: vec![true],
            segment_order: vec![0],
            segment_starts: vec![0.0],
            segment_speeds: vec![1.0],
            segment_muted: vec![false],
            zoom_clips: Vec::new(),
            selected_zoom: None,
            cursor_hide_clips: Vec::new(),
            selected_cursor_hide: None,
            selected_segment: None,
            background: VideoBackground::None,
            background_padding: 24.0,
            background_corner_radius: 18.0,
            background_shadow: 15.0,
            crop: None,
            sidecar,
            cursor: CursorSettings::default(),
            selected_tool: EditorTool::Cursor,
            project_media,
            title,
            video_locked: false,
            video_hidden: false,
            audio_locked: false,
            audio_removed: false,
            zoom_locked: false,
            zoom_hidden: false,
            zoom_classic: false,
            timeline_scale: 0.0,
            timeline_offset_seconds: 0.0,
            timeline_scroll_seconds: 0.0,
        }
    }

    pub fn composition_duration(&self) -> f64 {
        self.video_end_seconds().max(0.001)
    }

    pub fn source_duration(&self) -> f64 {
        self.metadata.duration_seconds.max(0.001)
    }

    pub fn content_end_seconds(&self) -> f64 {
        let zoom_end = self
            .zoom_clips
            .iter()
            .map(|clip| clip.end)
            .fold(0.0_f64, f64::max);
        let hide_end = self
            .cursor_hide_clips
            .iter()
            .map(|clip| clip.end)
            .fold(0.0_f64, f64::max);
        self.video_end_seconds()
            .max(zoom_end)
            .max(hide_end)
    }

    fn video_end_seconds(&self) -> f64 {
        let bounds = self.segment_boundaries();
        self.segment_order
            .iter()
            .filter(|&&i| self.segments_kept.get(i).copied().unwrap_or(true))
            .filter_map(|&i| {
                bounds
                    .get(i)
                    .map(|_| self.segment_start(i) + self.segment_timeline_duration(i))
            })
            .fold(0.0_f64, f64::max)
    }

    pub fn visible_span_seconds(&self) -> f64 {
        let factor = 1.0 + (self.timeline_scale.clamp(0.0, 100.0) / 100.0) * 7.0;
        self.source_duration() / factor
    }

    /// Scrollable / drawable length. Always keeps a full viewport of empty
    /// time after the last frame so the ruler never stops at the clip.
    pub fn timeline_canvas_seconds(&self) -> f64 {
        self.composition_duration() + self.visible_span_seconds()
    }

    pub fn max_timeline_scroll(&self) -> f64 {
        (self.timeline_canvas_seconds() - self.visible_span_seconds()).max(0.0)
    }

    pub fn source_to_timeline(&self, source_t: f64) -> f64 {
        let bounds = self.segment_boundaries();
        for (index, &(start, end)) in bounds.iter().enumerate().rev() {
            if !self.segments_kept.get(index).copied().unwrap_or(true) {
                continue;
            }
            if source_t + 1e-9 >= start && source_t <= end + 1e-9 {
                return self.segment_start(index) + (source_t - start) / self.segment_speed(index);
            }
        }
        if let Some((index, &(start, _))) = bounds
            .iter()
            .enumerate()
            .find(|(index, _)| self.segments_kept.get(*index).copied().unwrap_or(true))
        {
            if source_t < start {
                return self.segment_start(index) + (source_t - start) / self.segment_speed(index);
            }
        }
        if let Some((index, &(start, _))) = bounds.iter().enumerate().rev().find(|(index, _)| {
            self.segments_kept.get(*index).copied().unwrap_or(true)
        }) {
            return self.segment_start(index) + (source_t - start) / self.segment_speed(index);
        }
        source_t.max(0.0)
    }

    pub fn timeline_to_source(&self, timeline_t: f64) -> f64 {
        let bounds = self.segment_boundaries();
        for (index, &(start, end)) in bounds.iter().enumerate() {
            if !self.segments_kept.get(index).copied().unwrap_or(true) {
                continue;
            }
            let comp = self.segment_start(index);
            let comp_end = comp + self.segment_timeline_duration(index);
            if timeline_t + 1e-9 >= comp && timeline_t <= comp_end + 1e-9 {
                return (start + (timeline_t - comp) * self.segment_speed(index)).min(end);
            }
        }
        timeline_t - self.timeline_offset_seconds
    }

    pub fn segment_start(&self, index: usize) -> f64 {
        self.segment_starts
            .get(index)
            .copied()
            .unwrap_or(0.0)
            .max(0.0)
    }

    pub fn set_segment_start(&mut self, index: usize, start: f64) {
        if self.video_locked || index >= self.segment_starts.len() {
            return;
        }
        self.segment_starts[index] = if start.is_finite() {
            start.max(0.0)
        } else {
            0.0
        };
        self.sync_offset_from_segments();
        self.clamp_timeline_scroll();
    }

    pub fn settle_segment_start(&mut self, index: usize) {
        if self.video_locked || index >= self.segment_starts.len() {
            return;
        }
        self.segment_starts[index] = self.unoverlap_segment_start(index, self.segment_start(index));
        self.sync_offset_from_segments();
        self.clamp_timeline_scroll();
    }

    fn unoverlap_segment_start(&self, index: usize, start: f64) -> f64 {
        let bounds = self.segment_boundaries();
        let Some(&(src_start, src_end)) = bounds.get(index) else {
            return start.max(0.0);
        };
        let duration = (src_end - src_start).max(0.0) / self.segment_speed(index);
        let mut start = if start.is_finite() {
            start.max(0.0)
        } else {
            0.0
        };
        for _ in 0..self.segment_starts.len() {
            let end = start + duration;
            let Some((other_start, other_end)) =
                bounds.iter().enumerate().find_map(|(other, &(s0, s1))| {
                    if other == index || !self.segments_kept.get(other).copied().unwrap_or(true) {
                        return None;
                    }
                    let other_start = self.segment_start(other);
                    let other_end = other_start + (s1 - s0).max(0.0) / self.segment_speed(other);
                    ranges_overlap(start, end, other_start, other_end)
                        .then_some((other_start, other_end))
                })
            else {
                break;
            };
            let left = (other_start - duration).max(0.0);
            let right = other_end;
            let prefer_right = start + duration * 0.5 >= (other_start + other_end) * 0.5;
            start = if prefer_right || left + duration > other_start + 1e-9 {
                right
            } else {
                left
            };
        }
        start
    }

    pub fn source_playhead(&self) -> f64 {
        self.timeline_to_source(self.playhead_seconds)
            .clamp(0.0, self.source_duration())
    }

    pub fn source_to_x(&self, source_t: f64, width: f64) -> f64 {
        self.time_to_x(self.source_to_timeline(source_t), width)
    }

    pub fn set_timeline_offset(&mut self, value: f64) {
        if self.video_locked {
            return;
        }
        let next = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        let delta = next - self.timeline_offset_seconds;
        self.timeline_offset_seconds = next;
        if self.segment_starts.len() <= 1 {
            if let Some(start) = self.segment_starts.get_mut(0) {
                *start = next;
            }
        } else {
            for start in &mut self.segment_starts {
                *start = (*start + delta).max(0.0);
            }
        }
        self.clamp_timeline_scroll();
    }

    pub(super) fn sync_offset_from_segments(&mut self) {
        if let Some(min) = self.segment_starts.iter().copied().reduce(f64::min) {
            self.timeline_offset_seconds = min.max(0.0);
        }
    }

    pub fn set_timeline_scroll(&mut self, value: f64) {
        self.timeline_scroll_seconds = value;
        self.clamp_timeline_scroll();
    }

    pub(super) fn clamp_timeline_scroll(&mut self) {
        let max_scroll = self.max_timeline_scroll();
        self.timeline_scroll_seconds = self.timeline_scroll_seconds.clamp(0.0, max_scroll);
    }

    /// Keep a moving clip inside the fit-zoom window by panning, without
    /// changing pixels-per-second.
    pub fn follow_clip_on_timeline(&mut self) {
        if self.timeline_scale > 0.001 {
            return;
        }
        let start = self.source_to_timeline(self.trim_start_seconds);
        let end = self.source_to_timeline(self.trim_end_seconds);
        let visible = self.visible_span_seconds();
        if end > self.timeline_scroll_seconds + visible {
            self.timeline_scroll_seconds = end - visible;
        }
        if start < self.timeline_scroll_seconds {
            self.timeline_scroll_seconds = start;
        }
        self.clamp_timeline_scroll();
    }

    pub fn timeline_view(&self) -> (f64, f64) {
        let visible = self.visible_span_seconds().max(0.001);
        let start = self.timeline_scroll_seconds.max(0.0);
        (start, visible)
    }

    pub fn time_to_x(&self, seconds: f64, width: f64) -> f64 {
        let (start, span) = self.timeline_view();
        ((seconds - start) / span.max(1e-6)) * width.max(1.0)
    }

    pub fn x_to_time(&self, x: f64, width: f64) -> f64 {
        let (start, span) = self.timeline_view();
        start + (x / width.max(1.0)) * span
    }

    pub fn frac_to_x(&self, frac: f64, width: f64) -> f64 {
        self.time_to_x(frac * self.source_duration(), width)
    }

    pub fn x_to_frac(&self, x: f64, width: f64) -> f64 {
        self.x_to_time(x, width) / self.source_duration()
    }

    pub fn has_source_video(&self) -> bool {
        self.metadata.duration_seconds > 0.0
    }

    pub fn has_audio_track(&self) -> bool {
        self.metadata.has_audio && !self.audio_removed
    }

    pub fn has_zoom_track(&self) -> bool {
        !self.zoom_clips.is_empty()
    }

    pub fn video_tracks(&self) -> Vec<&ProjectMedia> {
        self.project_media
            .iter()
            .filter(|item| item.kind == ProjectMediaKind::Video)
            .collect()
    }

    pub fn extra_video_tracks(&self) -> Vec<&ProjectMedia> {
        self.video_tracks()
            .into_iter()
            .filter(|item| item.path != self.metadata.path)
            .collect()
    }

    pub fn remove_project_media(&mut self, path: &Path, kind: ProjectMediaKind) {
        if path == self.metadata.path && kind == ProjectMediaKind::Video {
            return;
        }
        self.project_media
            .retain(|item| !(item.path == path && item.kind == kind));
    }

    pub fn video_has_edits(&self) -> bool {
        self.trim_start_seconds > f64::EPSILON
            || (self.trim_end_seconds - self.metadata.duration_seconds).abs() > 0.001
            || self.timeline_offset_seconds > f64::EPSILON
            || !self.cuts.is_empty()
            || self.segments_kept.iter().any(|kept| !kept)
            || self
                .segment_speeds
                .iter()
                .any(|speed| (*speed - 1.0).abs() > 1e-6)
            || self.segment_muted.iter().any(|muted| *muted)
    }

    pub fn is_muted(&self) -> bool {
        self.audio_mode == AudioMode::Muted
    }

    pub fn toggle_mute(&mut self) {
        if self.audio_locked {
            return;
        }
        self.audio_mode = if self.is_muted() {
            AudioMode::Unchanged
        } else {
            AudioMode::Muted
        };
    }

    pub fn reset_video_edits(&mut self) {
        if self.video_locked {
            return;
        }
        self.trim_start_seconds = 0.0;
        self.trim_end_seconds = self.metadata.duration_seconds;
        self.timeline_offset_seconds = 0.0;
        self.timeline_scroll_seconds = 0.0;
        self.clear_cuts();
    }

    pub fn remove_audio_track(&mut self) {
        if self.audio_locked || !self.metadata.has_audio {
            return;
        }
        self.audio_removed = true;
        self.audio_mode = AudioMode::Muted;
    }

    pub fn clear_zoom_clips(&mut self) {
        if self.zoom_locked {
            return;
        }
        self.zoom_clips.clear();
        self.selected_zoom = None;
        self.zoom_hidden = false;
    }

    pub fn set_title(&mut self, raw: &str) {
        let title = sanitize_title(raw);
        if self.title == title {
            return;
        }
        let old = self.title.clone();
        self.title = title.clone();
        for item in &mut self.project_media {
            if item.path != self.metadata.path {
                continue;
            }
            match item.kind {
                ProjectMediaKind::Video if item.display_name == old => {
                    item.display_name = title.clone();
                }
                ProjectMediaKind::Audio if item.display_name == format!("{old} audio") => {
                    item.display_name = format!("{title} audio");
                }
                _ => {}
            }
        }
    }

    pub fn export_path(&self) -> PathBuf {
        unique_edited_path(
            self.metadata.path.parent().unwrap_or_else(|| Path::new("")),
            &self.title,
        )
    }

    pub fn add_project_media(&mut self, item: ProjectMedia) {
        if self
            .project_media
            .iter()
            .any(|existing| existing.path == item.path && existing.kind == item.kind)
        {
            return;
        }
        self.project_media.push(item);
    }
}
