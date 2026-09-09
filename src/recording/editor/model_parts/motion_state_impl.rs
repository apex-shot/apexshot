impl MotionState {
    pub fn clamp_duration(duration: f64) -> f64 {
        duration.clamp(MIN_MOTION_DURATION_SECONDS, MAX_MOTION_DURATION_SECONDS)
    }

    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty() || !self.text_segments.is_empty()
    }

    pub fn set_duration(&mut self, duration: f64) {
        let previous = self.duration;
        self.duration = Self::clamp_duration(duration);
        if self.playhead > self.duration {
            self.playhead = self.duration;
        }
        if self.segments.len() == 1 {
            let segment = &mut self.segments[0];
            if segment.start.abs() < 1e-6 && (segment.end - previous).abs() < 1e-6 {
                segment.end = self.duration;
            }
        }
        self.segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.segments {
            segment.end = segment.end.min(self.duration);
        }
        if let Some(index) = self.selected {
            if index >= self.segments.len() {
                self.selected = None;
            }
        }
        self.text_segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.text_segments {
            segment.end = segment.end.min(self.duration);
        }
        if let Some(index) = self.selected_text {
            if index >= self.text_segments.len() {
                self.selected_text = None;
            }
        }
        self.reconcile_effect_segments();
    }

    pub fn add_segment_at(&mut self, start: f64) -> Option<usize> {
        let start = start.clamp(0.0, self.duration);
        if self.segment_index_at(start).is_some() {
            return None;
        }
        let mut end = (start + DEFAULT_MOTION_SEGMENT_SECONDS).min(self.duration);
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            let start = (self.duration - MIN_MOTION_SEGMENT_SECONDS).max(0.0);
            end = self.duration;
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
            return self.insert_segment(start, end);
        }
        if self
            .segments
            .iter()
            .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
        {
            if let Some(next_start) = self
                .segments
                .iter()
                .filter(|segment| segment.start >= start)
                .map(|segment| segment.start)
                .min_by(|a, b| a.total_cmp(b))
            {
                end = next_start;
            }
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
        }
        self.insert_segment(start, end)
    }

    fn insert_segment(&mut self, start: f64, end: f64) -> Option<usize> {
        self.segments.push(MotionSegment {
            start,
            end,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: 0.5,
            zoom_anchor_y: 0.5,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: DEFAULT_MOTION_ZOOM,
                ..MotionTransform::default()
            },
        });
        self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.reconcile_effect_segments();
        let index = self
            .segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)?;
        self.selected = Some(index);
        self.selected_text = None;
        Some(index)
    }

    pub fn segment_index_at(&self, time: f64) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| time >= segment.start && time <= segment.end)
    }

    /// Find the nearest meaningful timeline edge for an effect segment. The
    /// visual timeline deliberately stays uncluttered; this supplies the
    /// Shotbase-style magnetic behavior behind it.
    pub fn snap_effect_time(&self, time: f64, tolerance: f64, excluding: Option<usize>) -> f64 {
        self.snap_time(time, tolerance, excluding, None)
    }

    /// Text uses the same shared timeline magnetism as camera effects, while
    /// ignoring the segment currently being dragged or trimmed.
    pub fn snap_text_time(&self, time: f64, tolerance: f64, excluding: Option<usize>) -> f64 {
        self.snap_time(time, tolerance, None, excluding)
    }

    pub fn remove_selected(&mut self) -> bool {
        if let Some(index) = self.selected_text.take() {
            if index < self.text_segments.len() {
                self.text_segments.remove(index);
                if !self.text_segments.is_empty() {
                    self.selected_text = Some(index.min(self.text_segments.len() - 1));
                }
                return true;
            }
        }
        let Some(index) = self.selected.take() else {
            return false;
        };
        if index >= self.segments.len() {
            return false;
        }
        self.segments.remove(index);
        self.reconcile_effect_segments();
        if !self.segments.is_empty() {
            self.selected = Some(index.min(self.segments.len() - 1));
        }
        true
    }

    pub fn set_segment_range(&mut self, index: usize, start: f64, end: f64) {
        if self.segments.get(index).is_none() {
            return;
        }
        let mut start = start.clamp(0.0, self.duration);
        let mut end = end.clamp(0.0, self.duration);
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            return;
        }
        if self.segments.iter().enumerate().any(|(other, clip)| {
            other != index && motion_ranges_overlap(start, end, clip.start, clip.end)
        }) {
            return;
        }
        if let Some(segment) = self.segments.get_mut(index) {
            segment.start = start;
            segment.end = end;
        }
        self.segments.sort_by(|a, b| a.start.total_cmp(&b.start));
        self.reconcile_effect_segments();
        let index = self
            .segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)
            .unwrap_or(index.min(self.segments.len().saturating_sub(1)));
        self.selected = Some(index);
        self.selected_text = None;
    }

    pub fn move_segment(&mut self, index: usize, start: f64) {
        let Some(segment) = self.segments.get(index).cloned() else {
            return;
        };
        let span = segment.duration();
        let start = start.clamp(0.0, (self.duration - span).max(0.0));
        self.set_segment_range(index, start, start + span);
    }

    pub fn selected_segment(&self) -> Option<&MotionSegment> {
        self.selected.and_then(|index| self.segments.get(index))
    }

    pub fn selected_segment_mut(&mut self) -> Option<&mut MotionSegment> {
        self.selected.and_then(|index| self.segments.get_mut(index))
    }

    pub fn set_selected_end_scale(&mut self, scale: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.scale = scale.clamp(MIN_MOTION_ZOOM, MAX_MOTION_ZOOM);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_zoom_mode(&mut self, zoom_mode: MotionZoomMode) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.zoom_mode = zoom_mode;
        }
    }

    pub fn set_selected_intensity(&mut self, intensity: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.intensity = intensity.clamp(0.0, 1.0);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_zoom_anchor(&mut self, x: f64, y: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.zoom_anchor_x = x.clamp(0.0, 1.0);
            segment.zoom_anchor_y = y.clamp(0.0, 1.0);
        }
    }

    pub fn set_selected_disabled(&mut self, is_disabled: bool) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.is_disabled = is_disabled;
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_yaw(&mut self, yaw: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_y = yaw.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    /// Shotbase stores one transition duration for the whole Motion effects
    /// track. The inspector's millisecond slider writes through to the global
    /// timing; there is no per-segment ease.
    pub fn set_selected_transition_ms(&mut self, transition_ms: u32) {
        let transition_ms = transition_ms.clamp(MIN_ZOOM_EASE_MS, MAX_ZOOM_EASE_MS);
        self.transform_timing.transition_duration = transition_ms as f64 / 1000.0;
    }

    pub fn set_transform_timing(&mut self, timing: MotionEffectTransformTiming) {
        self.transform_timing = timing.clamped();
    }
    pub fn set_selected_end_pitch(&mut self, pitch: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_x = pitch.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_roll(&mut self, roll: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.rotation_z = roll.clamp(MIN_MOTION_YAW, MAX_MOTION_YAW);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_perspective(&mut self, perspective: f64) {
        self.perspective_intensity = perspective.clamp(0.0, 1.0);
        let perspective_intensity = self.perspective_intensity;
        if let Some(segment) = self.selected_segment_mut() {
            // Retain this value for legacy in-memory segment consumers, but
            // rendering reads the global compositor perspective above.
            segment.to.perspective = perspective_intensity;
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_pos_x(&mut self, pos_x: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.pos_x = pos_x.clamp(MIN_MOTION_POS, MAX_MOTION_POS);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_selected_end_pos_y(&mut self, pos_y: f64) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.to.pos_y = pos_y.clamp(MIN_MOTION_POS, MAX_MOTION_POS);
        }
        self.reconcile_effect_segments();
    }

    pub fn set_motion_blur(&mut self, motion_blur: f64) {
        self.motion_blur = motion_blur.clamp(0.0, 1.0);
        self.motion_blur_settings.enabled = self.motion_blur > 0.0;
        self.motion_blur_settings.zoom_strength = self.motion_blur;
    }

    /// The scalar used by ApexShot's current still-image renderer. It is a
    /// compatibility projection of Shotbase's separate blur strengths.
    pub fn effective_motion_blur(&self) -> f64 {
        if self.motion_blur_settings.enabled {
            self.motion_blur_settings.zoom_strength.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn text_index_at(&self, time: f64) -> Option<usize> {
        self.text_segments
            .iter()
            .position(|segment| time >= segment.start && time <= segment.end)
    }

    pub fn add_text_at(&mut self, start: f64) -> Option<usize> {
        let start = start.clamp(0.0, self.duration);
        if self.text_index_at(start).is_some() {
            return None;
        }
        let mut end = (start + DEFAULT_MOTION_TEXT_SECONDS).min(self.duration);
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            let start = (self.duration - MIN_MOTION_SEGMENT_SECONDS).max(0.0);
            end = self.duration;
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .text_segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
            return self.insert_text(start, end);
        }
        if self
            .text_segments
            .iter()
            .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
        {
            if let Some(next_start) = self
                .text_segments
                .iter()
                .filter(|segment| segment.start >= start)
                .map(|segment| segment.start)
                .min_by(|a, b| a.total_cmp(b))
            {
                end = next_start;
            }
            if end - start < MIN_MOTION_SEGMENT_SECONDS
                || self
                    .text_segments
                    .iter()
                    .any(|segment| motion_ranges_overlap(start, end, segment.start, segment.end))
            {
                return None;
            }
        }
        self.insert_text(start, end)
    }

    fn insert_text(&mut self, start: f64, end: f64) -> Option<usize> {
        self.text_segments.push(MotionTextSegment {
            start,
            end,
            text: "Title".into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Character,
            typewriter_time: 0.6,
            is_disabled: false,
            annotation_coordinate_space: MotionTextCoordinateSpace::MotionCanvasLocal,
            pos_x: DEFAULT_MOTION_TEXT_POS_X,
            pos_y: DEFAULT_MOTION_TEXT_POS_Y,
            size: DEFAULT_MOTION_TEXT_SIZE,
        });
        self.text_segments
            .sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = self
            .text_segments
            .iter()
            .position(|segment| (segment.start - start).abs() < 1e-6)?;
        self.selected_text = Some(index);
        self.selected = None;
        Some(index)
    }

    pub fn set_text_range(&mut self, index: usize, start: f64, end: f64) {
        if self.text_segments.get(index).is_none() {
            return;
        }
        let mut start = start.clamp(0.0, self.duration);
        let mut end = end.clamp(0.0, self.duration);
        if end < start {
            std::mem::swap(&mut start, &mut end);
        }
        if end - start < MIN_MOTION_SEGMENT_SECONDS {
            return;
        }
        if self.text_segments.iter().enumerate().any(|(other, clip)| {
            other != index && motion_ranges_overlap(start, end, clip.start, clip.end)
        }) {
            return;
        }
        if let Some(segment) = self.text_segments.get_mut(index) {
            segment.start = start;
            segment.end = end;
        }
        self.selected_text = Some(index);
        self.selected = None;
    }

    pub fn move_text(&mut self, index: usize, start: f64) {
        let Some(segment) = self.text_segments.get(index).cloned() else {
            return;
        };
        let span = segment.duration();
        let start = start.clamp(0.0, (self.duration - span).max(0.0));
        self.set_text_range(index, start, start + span);
    }

    pub fn selected_text_segment(&self) -> Option<&MotionTextSegment> {
        self.selected_text
            .and_then(|index| self.text_segments.get(index))
    }

    pub fn set_selected_text_value(&mut self, text: String) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.text = text;
            }
        }
    }

    pub fn set_selected_text_animation(&mut self, animation: MotionTextAnimation) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.animation = animation;
            }
        }
    }

    pub fn set_selected_text_scope(&mut self, scope: MotionTextScope) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.scope = scope;
            }
        }
    }

    pub fn set_selected_text_typewriter_time(&mut self, typewriter_time: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.typewriter_time = typewriter_time.max(0.0);
            }
        }
    }

    pub fn set_selected_text_disabled(&mut self, is_disabled: bool) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.is_disabled = is_disabled;
            }
        }
    }

    pub fn set_selected_text_pos(&mut self, pos_x: f64, pos_y: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.pos_x = pos_x.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS);
                segment.pos_y = pos_y.clamp(MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS);
            }
        }
    }

    pub fn set_selected_text_size(&mut self, size: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.size = size.clamp(MIN_MOTION_TEXT_SIZE, MAX_MOTION_TEXT_SIZE);
            }
        }
    }

    pub fn sample(&self, time: f64) -> MotionTransform {
        let time = time.clamp(0.0, self.duration.max(0.0));
        let transform = if let Some(segment) = self
            .segments
            .iter()
            .find(|segment| time >= segment.start && time <= segment.end)
        {
            segment.sample(time, self.transform_timing)
        } else {
            self.segments
                .iter()
                .rev()
                .find(|segment| time > segment.end && !segment.is_disabled)
                .map(MotionSegment::target_transform)
                .unwrap_or_default()
        };
        MotionTransform {
            perspective: self.perspective_intensity,
            ..transform
        }
    }

    /// Source-artboard zoom focus for the active camera move. The anchor is
    /// deliberately sampled alongside the transform because Shotbase keeps it
    /// on `MotionEffectSegment`, not in the global compositor configuration.
    pub fn zoom_anchor_at(&self, time: f64) -> (f64, f64) {
        let time = time.clamp(0.0, self.duration.max(0.0));
        self.segments
            .iter()
            .find(|segment| time >= segment.start && time <= segment.end && !segment.is_disabled)
            .or_else(|| {
                self.segments
                    .iter()
                    .rev()
                    .find(|segment| time > segment.end && !segment.is_disabled)
            })
            .map(|segment| {
                (
                    segment.zoom_anchor_x.clamp(0.0, 1.0),
                    segment.zoom_anchor_y.clamp(0.0, 1.0),
                )
            })
            .unwrap_or((0.5, 0.5))
    }

    /// Preserve a single camera through every effect segment. Shotbase stores
    /// effect segments as a reconciled track: the next segment begins at the
    /// pose left by the preceding segment, including across an intentional
    /// timeline gap. Without this, adding a second move visibly snaps the card
    /// back to its identity transform.
    fn reconcile_effect_segments(&mut self) {
        let mut previous = MotionTransform::default();
        for segment in &mut self.segments {
            segment.from = previous;
            if !segment.is_disabled {
                previous = segment.target_transform();
            }
        }
    }

    fn snap_time(
        &self,
        time: f64,
        tolerance: f64,
        excluding_effect: Option<usize>,
        excluding_text: Option<usize>,
    ) -> f64 {
        let time = time.clamp(0.0, self.duration);
        let tolerance = tolerance.max(0.0);
        let mut nearest = time;
        let mut distance = tolerance;
        let mut consider = |candidate: f64| {
            let candidate = candidate.clamp(0.0, self.duration);
            let candidate_distance = (candidate - time).abs();
            if candidate_distance <= distance {
                nearest = candidate;
                distance = candidate_distance;
            }
        };
        consider(0.0);
        consider(self.duration);
        consider(self.playhead);
        for (index, segment) in self.segments.iter().enumerate() {
            if Some(index) != excluding_effect {
                consider(segment.start);
                consider(segment.end);
            }
        }
        for (index, segment) in self.text_segments.iter().enumerate() {
            if Some(index) != excluding_text {
                consider(segment.start);
                consider(segment.end);
            }
        }
        nearest
    }
}

fn motion_ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}
