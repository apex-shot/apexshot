impl VideoEditState {
    pub fn add_cursor_hide_at_playhead(&mut self) -> Option<usize> {
        self.add_cursor_hide_at(self.playhead_seconds)
    }

    pub fn add_cursor_hide_at(&mut self, start: f64) -> Option<usize> {
        let start = start.max(0.0);
        // The hide block may not run past the video's last frame.
        let (start, end) =
            fit_effect_span(self, start, start + DEFAULT_CURSOR_HIDE_DURATION_SECONDS)?;
        if self
            .cursor_hide_clips
            .iter()
            .any(|clip| ranges_overlap(start, end, clip.start, clip.end))
        {
            return None;
        }
        self.cursor_hide_clips.push(CursorHideClip {
            start,
            end,
            hidden: false,
        });
        self.cursor_hide_clips
            .sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .cursor_hide_clips
            .iter()
            .position(|clip| (clip.start - start).abs() < 1e-6)?;
        self.selected_cursor_hide = Some(index);
        self.selected_zoom = None;
        self.selected_segment = None;
        self.selected_tool = EditorTool::Timeline;
        Some(index)
    }

    pub fn remove_selected_cursor_hide(&mut self) {
        if let Some(index) = self.selected_cursor_hide.take() {
            if index < self.cursor_hide_clips.len() {
                self.cursor_hide_clips.remove(index);
            }
        }
    }

    pub fn selected_cursor_hide_clip(&self) -> Option<&CursorHideClip> {
        self.selected_cursor_hide
            .and_then(|index| self.cursor_hide_clips.get(index))
    }

    pub fn move_cursor_hide_clip(&mut self, index: usize, start: f64) {
        let Some(clip) = self.cursor_hide_clips.get(index).cloned() else {
            return;
        };
        // Keep the block's length but pull it back inside the video.
        let (start, end) = fit_effect_move(self, start, clip.duration());
        if self
            .cursor_hide_clips
            .iter()
            .enumerate()
            .any(|(other, existing)| {
                other != index && ranges_overlap(start, end, existing.start, existing.end)
            })
        {
            return;
        }
        if let Some(clip) = self.cursor_hide_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
    }

    pub fn set_cursor_hide_range(&mut self, index: usize, start: f64, end: f64) {
        if self.cursor_hide_clips.get(index).is_none() {
            return;
        }
        let Some((start, end)) = fit_effect_span(self, start, end) else {
            return;
        };
        if self
            .cursor_hide_clips
            .iter()
            .enumerate()
            .any(|(other, existing)| {
                other != index && ranges_overlap(start, end, existing.start, existing.end)
            })
        {
            return;
        }
        if let Some(clip) = self.cursor_hide_clips.get_mut(index) {
            clip.start = start;
            clip.end = end;
        }
    }

    pub fn cursor_hide_alpha(&self, timeline_t: f64) -> f64 {
        if self
            .cursor_hide_clips
            .iter()
            .any(|clip| !clip.hidden && clip.contains(timeline_t))
        {
            0.0
        } else {
            1.0
        }
    }

    pub fn cursor_hide_alpha_for_source(&self, source_t: f64) -> f64 {
        self.cursor_hide_alpha(self.source_to_timeline(source_t))
    }
}
