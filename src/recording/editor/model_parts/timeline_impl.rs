impl VideoEditState {
    pub fn set_trim_start(&mut self, value: f64) {
        if self.video_locked {
            return;
        }
        let duration = self.metadata.duration_seconds.max(0.0);
        let max_start = if duration > MIN_TRIM_DURATION_SECONDS {
            self.trim_end_seconds - MIN_TRIM_DURATION_SECONDS
        } else {
            self.trim_end_seconds
        };
        self.trim_start_seconds = value.clamp(0.0, max_start.max(0.0));
    }

    pub fn shift_trim(&mut self, delta: f64) {
        if self.video_locked {
            return;
        }
        let duration = self.metadata.duration_seconds.max(0.0);
        let span = self.trim_duration();
        if span <= 0.0 || duration <= 0.0 {
            return;
        }
        let max_start = (duration - span).max(0.0);
        let new_start = (self.trim_start_seconds + delta).clamp(0.0, max_start);
        let shift = new_start - self.trim_start_seconds;
        if shift.abs() < f64::EPSILON {
            return;
        }
        self.trim_start_seconds = new_start;
        self.trim_end_seconds = new_start + span;
        for cut in &mut self.cuts {
            *cut += shift;
        }
    }

    pub fn set_trim_end(&mut self, value: f64) {
        if self.video_locked {
            return;
        }
        let duration = self.metadata.duration_seconds.max(0.0);
        let min_end = if duration > MIN_TRIM_DURATION_SECONDS {
            self.trim_start_seconds + MIN_TRIM_DURATION_SECONDS
        } else {
            self.trim_start_seconds
        };
        self.trim_end_seconds = value.clamp(min_end.min(duration), duration);
    }

    pub fn trim_duration(&self) -> f64 {
        (self.trim_end_seconds - self.trim_start_seconds).max(0.0)
    }

    /// Duration of only the kept segments.
    pub fn kept_duration(&self) -> f64 {
        self.ordered_kept_segments()
            .iter()
            .map(|(start, end)| (end - start).max(0.0))
            .sum()
    }

    /// Returns (start, end) pairs for each segment.
    pub fn segment_boundaries(&self) -> Vec<(f64, f64)> {
        let mut boundaries = Vec::with_capacity(self.cuts.len() + 1);
        let mut prev = self.trim_start_seconds;
        for &cut in &self.cuts {
            boundaries.push((prev, cut));
            prev = cut;
        }
        boundaries.push((prev, self.trim_end_seconds));
        boundaries
    }

    /// Add a cut at the given time.
    pub fn add_cut(&mut self, seconds: f64) {
        if self.video_locked {
            return;
        }
        if seconds <= self.trim_start_seconds + 0.1 || seconds >= self.trim_end_seconds - 0.1 {
            return;
        }
        // Don't add duplicate cuts (within 0.1s of existing)
        if self.cuts.iter().any(|&c| (c - seconds).abs() < 0.1) {
            return;
        }
        let insert_pos = self.cuts.partition_point(|&c| c < seconds);
        self.cuts.insert(insert_pos, seconds);
        // The segment at insert_pos gets split — new segment inherits kept state
        let was_kept = self.segments_kept.get(insert_pos).copied().unwrap_or(true);
        self.segments_kept.insert(insert_pos + 1, was_kept);
        let speed = self.segment_speeds.get(insert_pos).copied().unwrap_or(1.0);
        self.segment_speeds.insert(insert_pos + 1, speed);
        let muted = self.segment_muted.get(insert_pos).copied().unwrap_or(false);
        self.segment_muted.insert(insert_pos + 1, muted);
        // Update segment_order: shift indices >= insert_pos+1, insert new segment after original
        for idx in self.segment_order.iter_mut() {
            if *idx > insert_pos {
                *idx += 1;
            }
        }
        // Find where insert_pos is in segment_order and insert insert_pos+1 right after
        let order_pos = self
            .segment_order
            .iter()
            .position(|&i| i == insert_pos)
            .unwrap_or(self.segment_order.len());
        self.segment_order.insert(order_pos + 1, insert_pos + 1);
        let left_start = self.segment_start(insert_pos);
        let left_src = self
            .segment_boundaries()
            .get(insert_pos)
            .map(|(start, _)| *start)
            .unwrap_or(0.0);
        let right_start = (left_start + (seconds - left_src)).max(0.0);
        if insert_pos + 1 > self.segment_starts.len() {
            self.segment_starts.resize(insert_pos + 1, left_start);
        }
        self.segment_starts.insert(insert_pos + 1, right_start);
        self.selected_segment = Some(insert_pos);
        self.selected_zoom = None;
    }

    /// Remove a cut point by index.
    pub fn remove_cut(&mut self, cut_index: usize) {
        if self.video_locked {
            return;
        }
        if cut_index >= self.cuts.len() {
            return;
        }
        self.cuts.remove(cut_index);
        // Merge the two segments — keep if either was kept
        let merged_seg = cut_index; // segment that remains
        let removed_seg = cut_index + 1; // segment that's absorbed
        let kept = self.segments_kept.get(merged_seg).copied().unwrap_or(true)
            || self.segments_kept.get(removed_seg).copied().unwrap_or(true);
        self.segments_kept.remove(removed_seg);
        if let Some(seg) = self.segments_kept.get_mut(merged_seg) {
            *seg = kept;
        }
        // Update segment_order: remove the absorbed segment, fix indices
        self.segment_order.retain(|&i| i != removed_seg);
        for idx in self.segment_order.iter_mut() {
            if *idx > removed_seg {
                *idx -= 1;
            }
        }
        if removed_seg < self.segment_starts.len() {
            self.segment_starts.remove(removed_seg);
        }
        if removed_seg < self.segment_speeds.len() {
            self.segment_speeds.remove(removed_seg);
        }
        if removed_seg < self.segment_muted.len() {
            self.segment_muted.remove(removed_seg);
        }
        if let Some(sel) = self.selected_segment {
            self.selected_segment = if sel == removed_seg {
                Some(merged_seg)
            } else if sel > removed_seg {
                Some(sel - 1)
            } else {
                Some(sel)
            };
        }
    }

    /// Move a cut point without crossing its neighboring cuts.
    pub fn move_cut(&mut self, cut_index: usize, seconds: f64) {
        if self.video_locked {
            return;
        }
        if cut_index >= self.cuts.len() {
            return;
        }

        let min = if cut_index == 0 {
            self.trim_start_seconds + 0.1
        } else {
            self.cuts[cut_index - 1] + 0.1
        };
        let max = if cut_index + 1 >= self.cuts.len() {
            self.trim_end_seconds - 0.1
        } else {
            self.cuts[cut_index + 1] - 0.1
        };

        if min <= max {
            self.cuts[cut_index] = seconds.clamp(min, max);
        }
    }

    /// Toggle keep/remove for a segment.
    pub fn toggle_segment(&mut self, segment_index: usize) {
        if self.video_locked {
            return;
        }
        if let Some(kept) = self.segments_kept.get_mut(segment_index) {
            *kept = !*kept;
        }
    }

    /// Clear all cuts.
    pub fn clear_cuts(&mut self) {
        if self.video_locked {
            return;
        }
        self.cuts.clear();
        self.segments_kept = vec![true];
        self.segment_order = vec![0];
        self.segment_starts = vec![self.timeline_offset_seconds.max(0.0)];
        self.segment_speeds = vec![1.0];
        self.segment_muted = vec![false];
        self.selected_segment = None;
    }

    /// Move a segment from one position in the output order to another.
    pub fn move_segment(&mut self, from_order_pos: usize, to_order_pos: usize) {
        if self.video_locked {
            return;
        }
        if from_order_pos >= self.segment_order.len()
            || to_order_pos >= self.segment_order.len()
            || from_order_pos == to_order_pos
        {
            return;
        }
        let seg = self.segment_order.remove(from_order_pos);
        self.segment_order.insert(to_order_pos, seg);
    }

    /// Kept segments as (composition_start, source_start, source_end), left-to-right.
    pub fn ordered_placed_segments(&self) -> Vec<(f64, f64, f64)> {
        let boundaries = self.segment_boundaries();
        let mut placed: Vec<(f64, f64, f64)> = self
            .segment_order
            .iter()
            .filter(|&&i| self.segments_kept.get(i).copied().unwrap_or(true))
            .filter_map(|&i| {
                boundaries
                    .get(i)
                    .map(|(start, end)| (self.segment_start(i), *start, *end))
            })
            .collect();
        placed.sort_by(|a, b| a.0.total_cmp(&b.0));
        placed
    }

    /// Returns kept segments in composition order (for export).
    pub fn ordered_kept_segments(&self) -> Vec<(f64, f64)> {
        self.ordered_placed_segments()
            .into_iter()
            .map(|(_, start, end)| (start, end))
            .collect()
    }

    pub fn has_segment_gaps(&self) -> bool {
        let placed = self.ordered_placed_segments();
        placed.windows(2).any(|pair| {
            let (left_comp, left_src, left_end) = pair[0];
            let (right_comp, _, _) = pair[1];
            right_comp > left_comp + (left_end - left_src).max(0.0) + 0.001
        })
    }

    /// Returns whether segments have been reordered from their default.
    pub fn is_reordered(&self) -> bool {
        self.segment_order
            .iter()
            .enumerate()
            .any(|(pos, &seg)| pos != seg)
    }

}
