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
        let full_motion_lane = self.segments.len() == 1;
        let full_text_lane = self.text_segments.len() == 1;
        self.segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.segments {
            let end = if full_motion_lane && segment.start.abs() < 1e-6 && (segment.end - previous).abs() < 1e-6 {
                self.duration
            } else {
                segment.end.min(self.duration)
            };
            retime_motion_clip(segment, end - segment.start);
            segment.end = end;
        }
        if let Some(index) = self.selected {
            if index >= self.segments.len() {
                self.selected = None;
            }
        }
        self.text_segments
            .retain(|segment| segment.start < self.duration);
        for segment in &mut self.text_segments {
            let end = if full_text_lane && segment.start.abs() < 1e-6 && (segment.end - previous).abs() < 1e-6 {
                self.duration
            } else {
                segment.end.min(self.duration)
            };
            retime_text_clip(segment, end - segment.start);
            segment.end = end;
        }
        if let Some(index) = self.selected_text {
            if index >= self.text_segments.len() {
                self.selected_text = None;
            }
        }
        self.reconcile_effect_segments();
    }

    /// The exact span a click at `start` would create, or `None` when the
    /// click is inside an existing clip or the surrounding free gap is
    /// shorter than a default clip. Inside a gap the clip is nudged back so
    /// it fits flush against the following clip instead of being rejected.
    /// The timeline's add ghost shares this so the affordance can never
    /// promise a clip the click cannot create.
    pub fn motion_add_span(&self, start: f64) -> Option<(f64, f64)> {
        let start = start.clamp(0.0, self.duration);
        if self.segment_index_at(start).is_some() {
            return None;
        }
        let gap_start = self
            .segments
            .iter()
            .filter(|segment| segment.end <= start + 1e-9)
            .map(|segment| segment.end)
            .fold(0.0, f64::max);
        let gap_end = self
            .segments
            .iter()
            .filter(|segment| segment.start >= start - 1e-9)
            .map(|segment| segment.start)
            .fold(self.duration, f64::min);
        let length = DEFAULT_MOTION_SEGMENT_SECONDS;
        if gap_end - gap_start + 1e-9 < length {
            return None;
        }
        let fitted = start.clamp(gap_start, gap_end - length);
        Some((fitted, fitted + length))
    }

    pub fn add_segment_at(&mut self, start: f64) -> Option<usize> {
        let (start, end) = self.motion_add_span(start)?;
        self.insert_segment(start, end)
    }

    pub fn motion_duplicate_span(&self, index: usize) -> Option<(f64, f64)> {
        let segment = self.segments.get(index)?;
        duplicate_span_after(
            segment.end,
            segment.duration(),
            self.duration,
            self.segments
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, segment)| (segment.start, segment.end)),
        )
    }

    pub fn duplicate_motion_segment(&mut self, index: usize) -> Option<usize> {
        let (start, end) = self.motion_duplicate_span(index)?;
        let mut duplicate = self.segments.get(index)?.clone();
        duplicate.start = start;
        duplicate.end = end;
        self.segments.push(duplicate);
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

    fn insert_segment(&mut self, start: f64, end: f64) -> Option<usize> {
        // Default the new move to the opposite of where the camera already
        // is: pull back to identity when the move it chains from left the
        // camera at the default zoom, otherwise push in to the default zoom.
        // Across a gap the camera has already released to identity, so the
        // new move always starts there and pushes in.
        let incoming_scale = self
            .segments
            .iter()
            .find(|segment| !segment.is_disabled && (segment.end - start).abs() <= 1e-9)
            .map(|segment| segment.target_transform().scale)
            .unwrap_or(MIN_MOTION_ZOOM);
        let target_scale = if (incoming_scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6 {
            MIN_MOTION_ZOOM
        } else {
            DEFAULT_MOTION_ZOOM
        };
        // A new clip inherits the curve the user is currently working with;
        // with no selection it starts from the recovered defaults.
        let timing = self
            .selected
            .and_then(|index| self.segments.get(index))
            .map(|segment| segment.timing)
            .unwrap_or_default();
        // Keep the zoom anchor fixed across a flush chain: a new clip inherits
        // the anchor of the move it chains from (else the selected clip's
        // anchor, else center). Resetting to center here would hard-cut the
        // zoom focus at every boundary of a Position-driven edge tour.
        let (anchor_x, anchor_y) = self
            .segments
            .iter()
            .find(|segment| !segment.is_disabled && (segment.end - start).abs() <= 1e-9)
            .map(|segment| (segment.zoom_anchor_x, segment.zoom_anchor_y))
            .or_else(|| {
                self.selected
                    .and_then(|index| self.segments.get(index))
                    .map(|segment| (segment.zoom_anchor_x, segment.zoom_anchor_y))
            })
            .unwrap_or((0.5, 0.5));
        self.segments.push(MotionSegment {
            start,
            end,
            zoom_mode: MotionZoomMode::Manual,
            intensity: 1.0,
            zoom_anchor_x: anchor_x,
            zoom_anchor_y: anchor_y,
            is_disabled: false,
            from: MotionTransform::default(),
            to: MotionTransform {
                scale: target_scale,
                ..MotionTransform::default()
            },
            timing,
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

    /// Half-open on the end: a click exactly at a clip's edge starts a new
    /// clip instead of grabbing the one that just ended.
    pub fn segment_index_at(&self, time: f64) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| time >= segment.start && time < segment.end)
    }

    /// Find the nearest meaningful timeline edge for an effect segment. The
    /// visual timeline deliberately stays uncluttered; this supplies the
    /// magnetic behavior behind it.
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
        self.set_segment_range_inner(index, start, end, false);
    }

    pub fn resize_segment_range(&mut self, index: usize, start: f64, end: f64) {
        self.set_segment_range_inner(index, start, end, true);
    }

    fn set_segment_range_inner(&mut self, index: usize, start: f64, end: f64, retime: bool) {
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
            if retime {
                retime_motion_clip(segment, end - start);
            }
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

    /// Writes the transition duration into the selected clip's timing, so a
    /// slider nudge no longer rewrites every move on the track.
    pub fn set_selected_transition_ms(&mut self, transition_ms: u32) {
        let transition_ms = transition_ms.clamp(
            (MIN_MOTION_TRANSITION_SECONDS * 1000.0) as u32,
            (MAX_MOTION_TRANSITION_SECONDS * 1000.0) as u32,
        );
        if let Some(segment) = self.selected_segment_mut() {
            segment.timing.transition_duration = transition_ms as f64 / 1000.0;
        }
    }

    /// The timing of the clip the inspector is editing; falls back to the
    /// defaults when nothing is selected.
    pub fn selected_transform_timing(&self) -> MotionEffectTransformTiming {
        self.selected_segment()
            .map(|segment| segment.timing)
            .unwrap_or_default()
    }

    pub fn set_transform_timing(&mut self, timing: MotionEffectTransformTiming) {
        if let Some(segment) = self.selected_segment_mut() {
            segment.timing = timing.clamped();
        }
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
    /// compatibility projection of the separate blur strengths.
    pub fn effective_motion_blur(&self) -> f64 {
        if self.motion_blur_settings.enabled {
            self.motion_blur_settings.zoom_strength.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Half-open on the end, like [`Self::segment_index_at`].
    pub fn text_index_at(&self, time: f64) -> Option<usize> {
        self.text_segments
            .iter()
            .position(|segment| time >= segment.start && time < segment.end)
    }

    /// Text counterpart of [`Self::motion_add_span`].
    pub fn text_add_span(&self, start: f64) -> Option<(f64, f64)> {
        let start = start.clamp(0.0, self.duration);
        if self.text_index_at(start).is_some() {
            return None;
        }
        let gap_start = self
            .text_segments
            .iter()
            .filter(|segment| segment.end <= start + 1e-9)
            .map(|segment| segment.end)
            .fold(0.0, f64::max);
        let gap_end = self
            .text_segments
            .iter()
            .filter(|segment| segment.start >= start - 1e-9)
            .map(|segment| segment.start)
            .fold(self.duration, f64::min);
        let length = DEFAULT_MOTION_TEXT_SECONDS;
        if gap_end - gap_start + 1e-9 < length {
            return None;
        }
        let fitted = start.clamp(gap_start, gap_end - length);
        Some((fitted, fitted + length))
    }

    pub fn add_text_at(&mut self, start: f64) -> Option<usize> {
        let (start, end) = self.text_add_span(start)?;
        self.insert_text(start, end)
    }

    pub fn text_duplicate_span(&self, index: usize) -> Option<(f64, f64)> {
        let segment = self.text_segments.get(index)?;
        duplicate_span_after(
            segment.end,
            segment.duration(),
            self.duration,
            self.text_segments
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, segment)| (segment.start, segment.end)),
        )
    }

    pub fn duplicate_text_segment(&mut self, index: usize) -> Option<usize> {
        let (start, end) = self.text_duplicate_span(index)?;
        let mut duplicate = self.text_segments.get(index)?.clone();
        duplicate.start = start;
        duplicate.end = end;
        self.text_segments.push(duplicate);
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

    fn insert_text(&mut self, start: f64, end: f64) -> Option<usize> {
        self.text_segments.push(MotionTextSegment {
            start,
            end,
            text: "Title".into(),
            animation: MotionTextAnimation::None,
            scope: MotionTextScope::Character,
            typewriter_time: DEFAULT_MOTION_TEXT_TYPEWRITER_SECONDS,
            is_disabled: false,
            annotation_coordinate_space: MotionTextCoordinateSpace::Canvas,
            pos_x: DEFAULT_MOTION_TEXT_POS_X,
            pos_y: DEFAULT_MOTION_TEXT_POS_Y,
            size: DEFAULT_MOTION_TEXT_SIZE,
            format: MotionTextFormat::canvas(),
            transition_duration: DEFAULT_MOTION_TEXT_TRANSITION_SECONDS,
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
        self.set_text_range_inner(index, start, end, false);
    }

    pub fn resize_text_range(&mut self, index: usize, start: f64, end: f64) {
        self.set_text_range_inner(index, start, end, true);
    }

    fn set_text_range_inner(&mut self, index: usize, start: f64, end: f64, retime: bool) {
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
            if retime {
                retime_text_clip(segment, end - start);
            }
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
                segment.typewriter_time = typewriter_time.clamp(0.0, MAX_MOTION_TEXT_TRANSITION_SECONDS);
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
                let (min, max) = motion_text_pos_band(segment.annotation_coordinate_space);
                segment.pos_x = pos_x.clamp(min, max);
                segment.pos_y = pos_y.clamp(min, max);
            }
        }
    }

    /// Switch the selected title between the composition canvas and the moving
    /// card. The caller supplies the position to keep, because only the
    /// renderer knows how the two references map on screen.
    pub fn set_selected_text_attachment(
        &mut self,
        space: MotionTextCoordinateSpace,
        pos_x: f64,
        pos_y: f64,
    ) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.annotation_coordinate_space = if space.is_canvas() {
                    MotionTextCoordinateSpace::Canvas
                } else {
                    MotionTextCoordinateSpace::image()
                };
                let (min, max) = motion_text_pos_band(segment.annotation_coordinate_space);
                segment.pos_x = pos_x.clamp(min, max);
                segment.pos_y = pos_y.clamp(min, max);
            }
        }
    }

    pub fn set_selected_text_font_family(&mut self, font_family: String) {
        self.edit_selected_text_format(|format| format.font_family = font_family);
    }

    pub fn set_selected_text_bold(&mut self, bold: bool) {
        self.edit_selected_text_format(|format| format.bold = bold);
    }

    pub fn set_selected_text_italic(&mut self, italic: bool) {
        self.edit_selected_text_format(|format| format.italic = italic);
    }

    pub fn set_selected_text_color(&mut self, color: [f64; 4]) {
        self.edit_selected_text_format(|format| format.color = color);
    }

    pub fn set_selected_text_opacity(&mut self, opacity: f64) {
        self.edit_selected_text_format(|format| format.color[3] = opacity.clamp(0.0, 1.0));
    }

    pub fn set_selected_text_alignment(&mut self, alignment: MotionTextAlignment) {
        self.edit_selected_text_format(|format| format.alignment = alignment);
    }

    pub fn set_selected_text_wrap_width(&mut self, wrap_width: f64) {
        self.edit_selected_text_format(|format| {
            format.wrap_width = if wrap_width <= 0.0 {
                0.0
            } else {
                wrap_width.clamp(MIN_MOTION_TEXT_WIDTH, 1.0)
            };
        });
    }

    pub fn set_selected_text_line_spacing(&mut self, line_spacing: f64) {
        self.edit_selected_text_format(|format| {
            format.line_spacing = line_spacing
                .clamp(MIN_MOTION_TEXT_LINE_SPACING, MAX_MOTION_TEXT_LINE_SPACING);
        });
    }

    pub fn set_selected_text_letter_spacing(&mut self, letter_spacing: f64) {
        self.edit_selected_text_format(|format| {
            format.letter_spacing = letter_spacing
                .clamp(MIN_MOTION_TEXT_LETTER_SPACING, MAX_MOTION_TEXT_LETTER_SPACING);
        });
    }

    pub fn set_selected_text_rotation(&mut self, rotation: f64) {
        self.edit_selected_text_format(|format| {
            let rotation = if rotation.is_finite() { rotation } else { 0.0 };
            format.rotation = rotation.clamp(MIN_MOTION_TEXT_ROTATION, MAX_MOTION_TEXT_ROTATION);
        });
    }

    pub fn set_selected_text_outline_width(&mut self, outline_width: f64) {
        self.edit_selected_text_format(|format| {
            format.outline_width = outline_width.clamp(0.0, MAX_MOTION_TEXT_OUTLINE);
        });
    }

    pub fn set_selected_text_shadow(&mut self, shadow: bool) {
        self.edit_selected_text_format(|format| format.shadow = shadow);
    }

    pub fn set_selected_text_transition_duration(&mut self, transition_duration: f64) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                segment.transition_duration = transition_duration.clamp(
                    MIN_MOTION_TEXT_TRANSITION_SECONDS,
                    MAX_MOTION_TEXT_TRANSITION_SECONDS,
                );
            }
        }
    }

    fn edit_selected_text_format(&mut self, edit: impl FnOnce(&mut MotionTextFormat)) {
        if let Some(index) = self.selected_text {
            if let Some(segment) = self.text_segments.get_mut(index) {
                edit(&mut segment.format);
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
        // No camera moves: stay perfectly flat so the Motion still matches
        // the Static canvas. Applying `perspective_intensity` here would force
        // the mesh path (and a shadow lift) for an identity pose whose
        // projection is exactly 1:1 — a visible warp/softening with no clip.
        if self.segments.iter().all(|segment| segment.is_disabled) {
            return MotionTransform::default();
        }
        let transform = if let Some(segment) = self
            .segments
            .iter()
            .find(|segment| time >= segment.start && time <= segment.end && !segment.is_disabled)
        {
            segment.sample(time)
        } else if let Some((index, previous)) = self
            .segments
            .iter()
            .enumerate()
            .rev()
            .find(|(_, segment)| time > segment.end && !segment.is_disabled)
        {
            // The camera always eases back to the initial framing in a gap.
            // The gap's length sets the release speed, capped by the clip's
            // own transition duration; the following move starts from identity
            // when the gap ends. Flush moves never enter this branch: they
            // chain directly through `reconcile_effect_segments`.
            let release_end = self.segments[index + 1..]
                .iter()
                .find(|segment| !segment.is_disabled)
                .map(|segment| segment.start)
                .unwrap_or(self.duration);
            previous.release_after(time, release_end - previous.end)
        } else {
            MotionTransform::default()
        };
        MotionTransform {
            perspective: self.perspective_intensity,
            ..transform
        }
    }

    /// Source-artboard zoom focus for the active camera move. The anchor is
    /// deliberately sampled alongside the transform because it lives
    /// on `MotionEffectSegment`, not in the global compositor configuration.
    pub fn zoom_anchor_at(&self, time: f64) -> (f64, f64) {
        let time = time.clamp(0.0, self.duration.max(0.0));
        let Some((index, segment)) = self.segments
            .iter()
            .enumerate()
            .find(|(_, segment)| time >= segment.start && time <= segment.end && !segment.is_disabled)
            .or_else(|| {
                self.segments
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, segment)| time > segment.end && !segment.is_disabled)
            })
        else {
            return (0.5, 0.5);
        };
        let authored_anchor = |segment: &MotionSegment| (
            segment.zoom_anchor_x.clamp(0.0, 1.0),
            segment.zoom_anchor_y.clamp(0.0, 1.0),
        );
        let blend = |from: (f64, f64), to: (f64, f64), progress: f64| (
            from.0 + (to.0 - from.0) * progress,
            from.1 + (to.1 - from.1) * progress,
        );
        let mut previous_end = None;
        let mut previous_anchor = (0.5, 0.5);
        for previous in self.segments[..index].iter().filter(|segment| !segment.is_disabled) {
            let authored = authored_anchor(previous);
            previous_anchor = if previous_end.is_some_and(|end: f64| (end - previous.start).abs() <= 1e-9) {
                blend(previous_anchor, authored, previous.intensity.clamp(0.0, 1.0))
            } else {
                authored
            };
            previous_end = Some(previous.end);
        }
        let authored = authored_anchor(segment);
        let from = if previous_end.is_some_and(|end| (end - segment.start).abs() <= 1e-9) {
            previous_anchor
        } else {
            authored
        };
        let target = blend(from, authored, segment.intensity.clamp(0.0, 1.0));
        if time > segment.end {
            return target;
        }
        let timing = segment.timing.clamped();
        let ease = timing.transition_duration.min(segment.duration());
        let progress = if ease <= f64::EPSILON {
            1.0
        } else {
            timing.apply(((time - segment.start) / ease).clamp(0.0, 1.0))
        };
        blend(from, target, progress)
    }

    /// Each move starts from the pose the timeline leaves at its start. A
    /// flush (adjacent) move chains from the previous target, so two moves
    /// placed together read as one camera transition; a move after a gap
    /// starts from identity, because the gap released the camera back to the
    /// initial framing first.
    fn reconcile_effect_segments(&mut self) {
        let mut previous_end = 0.0;
        let mut previous_pose = MotionTransform::default();
        for segment in self.segments.iter_mut().filter(|segment| !segment.is_disabled) {
            segment.from = if segment.start <= previous_end + 1e-9 {
                previous_pose
            } else {
                MotionTransform::default()
            };
            previous_end = segment.end;
            previous_pose = segment.target_transform();
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

fn retime_motion_clip(segment: &mut MotionSegment, new_span: f64) {
    let old_span = segment.duration();
    if old_span <= f64::EPSILON || (new_span - old_span).abs() < 1e-9 {
        return;
    }
    segment.timing.transition_duration =
        segment.timing.clamped().transition_duration.min(old_span) * new_span / old_span;
}

fn retime_text_clip(segment: &mut MotionTextSegment, new_span: f64) {
    let old_span = segment.duration();
    if old_span <= f64::EPSILON || (new_span - old_span).abs() < 1e-9 {
        return;
    }
    segment.transition_duration = segment.transition_duration.max(0.0).min(old_span) * new_span / old_span;
    segment.typewriter_time = segment.typewriter_time.max(0.0).min(old_span) * new_span / old_span;
}

fn duplicate_span_after(
    earliest_start: f64,
    length: f64,
    duration: f64,
    ranges: impl Iterator<Item = (f64, f64)>,
) -> Option<(f64, f64)> {
    if length <= 0.0 {
        return None;
    }
    let mut ranges: Vec<_> = ranges.collect();
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut start = earliest_start;
    for (clip_start, clip_end) in ranges {
        if clip_end <= start {
            continue;
        }
        if clip_start - start >= length {
            return Some((start, start + length));
        }
        start = start.max(clip_end);
    }
    (start + length <= duration).then_some((start, start + length))
}

fn motion_ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

/// The position band a title's reference rectangle allows: the whole
/// composition for a Canvas title, the recovered inset band for a card title.
fn motion_text_pos_band(space: MotionTextCoordinateSpace) -> (f64, f64) {
    if space.is_canvas() {
        (MIN_MOTION_TEXT_CANVAS_POS, MAX_MOTION_TEXT_CANVAS_POS)
    } else {
        (MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS)
    }
}
