use std::hash::{Hash, Hasher};

const ZOOM_CAMERA_STEP: f64 = 1.0 / 60.0;
const ZOOM_BLUR_SAMPLES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct CameraMotion {
    rect: [f64; 4],
    velocity: [f64; 4],
}

impl CameraMotion {
    fn parked(rect: [f64; 4]) -> Self {
        Self {
            rect,
            velocity: [0.0; 4],
        }
    }

    fn advance(self, target: [f64; 4], spring: CameraSpring, seconds: f64) -> Self {
        let mut motion = self;
        let step = (spring_adaptive_step_ms(spring) / 1000.0).min(ZOOM_CAMERA_STEP);
        let steps = (seconds / step).ceil() as usize;
        for index in 0..steps {
            let dt = step.min((seconds - index as f64 * step).max(0.0));
            for (axis, target) in target.iter().enumerate() {
                (motion.rect[axis], motion.velocity[axis]) = spring_step(
                    motion.rect[axis],
                    motion.velocity[axis],
                    *target,
                    spring,
                    dt,
                );
            }
        }
        motion
    }
}

#[derive(Debug, Clone)]
struct CameraZoom {
    clip: ZoomClip,
    centers: Vec<(f64, (f64, f64))>,
    holds_for_next: bool,
    exit: Option<(CameraMotion, CameraMotion)>,
}

#[derive(Debug, Clone, Copy)]
struct CameraSegment {
    composition_start: f64,
    composition_end: f64,
    source_start: f64,
    source_end: f64,
    speed: f64,
}

/// A shared screen-rectangle spring, integrated on a fixed composition-time grid.
/// Standalone exits are fitted to their clip boundary with a Hermite residual
/// correction, so the viewport reaches the full crop with zero velocity there.
/// Close automatic zooms keep the actual viewport and velocity instead.
#[derive(Debug)]
struct ZoomCameraPlan {
    crop: [f64; 4],
    settings: ZoomCameraSettings,
    zooms: Vec<CameraZoom>,
    boundaries: Vec<f64>,
    frames: Vec<CameraMotion>,
}

#[derive(Debug, Default)]
pub(super) struct ZoomCameraCache {
    signature: Option<u64>,
    plan: Option<ZoomCameraPlan>,
}

impl ZoomCameraPlan {
    fn new(state: &VideoEditState) -> Self {
        let crop = state.crop_or_full();
        let settings = state.zoom_camera.clamped();
        let points = state
            .sidecar
            .as_ref()
            .map(|sidecar| {
                sidecar
                    .pointer
                    .iter()
                    .map(|sample| {
                        let (x, y) = sidecar.map_to_video(
                            sample.x,
                            sample.y,
                            state.metadata.width as f64,
                            state.metadata.height as f64,
                        );
                        (sample.t, x, y)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let segments: Vec<CameraSegment> = state
            .segment_boundaries()
            .iter()
            .enumerate()
            .filter(|(index, _)| state.segments_kept.get(*index).copied().unwrap_or(true))
            .map(|(index, &(start, end))| CameraSegment {
                composition_start: state.segment_start(index),
                composition_end: state.segment_start(index)
                    + state.segment_timeline_duration(index),
                source_start: start,
                source_end: end,
                speed: state.segment_speed(index),
            })
            .collect();
        let mut clips: Vec<_> = state
            .zoom_clips
            .iter()
            .filter(|clip| {
                !clip.hidden
                    && clip.start.is_finite()
                    && clip.end.is_finite()
                    && clip.end > clip.start
                    && clip.scale.is_finite()
            })
            .cloned()
            .collect();
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let zooms = clips
            .iter()
            .enumerate()
            .map(|(index, clip)| {
                let mut centers = Vec::new();
                for segment in &segments {
                    let start = clip.start.max(segment.composition_start);
                    let end = clip.end.min(segment.composition_end);
                    if end <= start {
                        continue;
                    }
                    let source_start = (segment.source_start
                        + (start - segment.composition_start) * segment.speed)
                        .min(segment.source_end);
                    let source_end = (segment.source_start
                        + (end - segment.composition_start) * segment.speed)
                        .min(segment.source_end);
                    let window = zoom_follow_samples(&points, source_start, source_end);
                    centers.extend(
                        movement_group_centers(&window, movement_group_budget(crop, clip.scale))
                            .into_iter()
                            .map(|(source_t, center)| {
                                (
                                    (start + (source_t - source_start) / segment.speed).max(start),
                                    center,
                                )
                            }),
                    );
                }
                centers.sort_by(|a, b| a.0.total_cmp(&b.0));
                let holds_for_next = !clip.instant
                    && clip.mode == ZoomMode::Auto
                    && clips.iter().enumerate().any(|(other, next)| {
                        other != index
                            && !next.instant
                            && next.mode == ZoomMode::Auto
                            && next.start >= clip.end
                            && next.start - clip.end <= ZOOM_MORPH_GAP_SECONDS
                    });
                CameraZoom {
                    clip: clip.clone(),
                    centers,
                    holds_for_next,
                    exit: None,
                }
            })
            .collect::<Vec<_>>();
        let mut boundaries = vec![0.0];
        for zoom in &zooms {
            boundaries.extend([
                (zoom.clip.start - settings.lead_in_seconds()).max(0.0),
                zoom.clip.start,
                Self::exit_start(&zoom.clip),
                zoom.clip.end,
                zoom.clip.end + 0.05,
            ]);
        }
        boundaries.sort_by(f64::total_cmp);
        boundaries.dedup();
        Self {
            crop: [crop.0, crop.1, crop.2, crop.3],
            settings,
            zooms,
            boundaries,
            frames: vec![CameraMotion::parked([0.0, 0.0, crop.2, crop.3])],
        }
    }

    fn active_at(&self, t: f64) -> Option<usize> {
        self.zooms
            .iter()
            .rposition(|zoom| t >= zoom.clip.start && t <= zoom.clip.end)
    }

    fn target_index_at(&self, t: f64) -> Option<usize> {
        if let Some(index) = self.active_at(t) {
            return Some(index);
        }
        if self.settings.start_early {
            if let Some(index) = self.zooms.iter().position(|zoom| {
                !zoom.clip.instant
                    && t >= zoom.clip.start - self.settings.lead_in_seconds()
                    && t < zoom.clip.start
            }) {
                return Some(index);
            }
        }
        self.zooms
            .iter()
            .enumerate()
            .filter(|(_, zoom)| {
                zoom.holds_for_next
                    && t > zoom.clip.end
                    && t - zoom.clip.end <= ZOOM_MORPH_GAP_SECONDS
            })
            .max_by(|(_, a), (_, b)| a.clip.end.total_cmp(&b.clip.end))
            .map(|(index, _)| index)
    }

    fn target_rect(&self, index: usize, timeline_t: f64) -> [f64; 4] {
        let zoom = &self.zooms[index];
        let center = if zoom.clip.mode == ZoomMode::Auto {
            let t = if timeline_t < zoom.clip.start {
                zoom.clip.start
            } else {
                timeline_t
            };
            movement_group_center_in(&zoom.centers, t).unwrap_or(zoom.clip.center)
        } else {
            zoom.clip.center
        };
        self.focus_rect(zoom.clip.scale, center)
    }

    fn focus_rect(&self, scale: f64, center: (f64, f64)) -> [f64; 4] {
        let scale = scale.clamp(1.0, MAX_ZOOM_SCALE);
        let width = self.crop[2] * scale;
        let height = self.crop[3] * scale;
        let x = if center.0.is_finite() {
            self.crop[2] / 2.0 - (center.0 - self.crop[0]) * scale
        } else {
            0.0
        };
        let y = if center.1.is_finite() {
            self.crop[3] / 2.0 - (center.1 - self.crop[1]) * scale
        } else {
            0.0
        };
        [
            x.clamp(self.crop[2] - width, 0.0),
            y.clamp(self.crop[3] - height, 0.0),
            width,
            height,
        ]
    }

    fn full_rect(&self) -> [f64; 4] {
        [0.0, 0.0, self.crop[2], self.crop[3]]
    }

    fn exit_start(clip: &ZoomClip) -> f64 {
        clip.end - (clip.ease_ms as f64 / 1000.0).min(clip.duration() / 2.0)
    }

    fn snap_window(&self, t: f64) -> bool {
        self.zooms
            .iter()
            .any(|zoom| zoom.clip.instant && t >= zoom.clip.start && t <= zoom.clip.end + 0.05)
    }

    fn advance(&mut self, mut motion: CameraMotion, mut from: f64, to: f64) -> CameraMotion {
        let events: Vec<_> = self
            .boundaries
            .iter()
            .copied()
            .filter(|event| *event > from && *event < to)
            .chain(std::iter::once(to))
            .collect();
        for end in events {
            let sample_t = (from + end) / 2.0;
            let active = self.active_at(sample_t);
            let target_index = self.target_index_at(sample_t);
            let target = target_index
                .map(|index| self.target_rect(index, sample_t))
                .unwrap_or_else(|| self.full_rect());
            if self.snap_window(sample_t) {
                motion = CameraMotion::parked(target);
            } else if let Some(index) = active.filter(|&index| {
                let zoom = &self.zooms[index];
                !zoom.holds_for_next
                    && zoom.clip.ease_ms > 0
                    && sample_t >= Self::exit_start(&zoom.clip)
            }) {
                let exit_start = Self::exit_start(&self.zooms[index].clip);
                let duration = self.zooms[index].clip.end - exit_start;
                let spring = self.settings.spring();
                let full = self.full_rect();
                let (origin, terminal) = *self.zooms[index]
                    .exit
                    .get_or_insert_with(|| (motion, motion.advance(full, spring, duration)));
                let elapsed = (end - exit_start).clamp(0.0, duration);
                motion = origin.advance(full, spring, elapsed);
                let u = (elapsed / duration).clamp(0.0, 1.0);
                let position_weight = u * u * (3.0 - 2.0 * u);
                let velocity_weight = u * u * (u - 1.0);
                let position_rate = 6.0 * u * (1.0 - u) / duration;
                let velocity_rate = 3.0 * u * u - 2.0 * u;
                for (axis, target) in full.iter().enumerate() {
                    let residual = terminal.rect[axis] - target;
                    motion.rect[axis] -= position_weight * residual
                        + velocity_weight * duration * terminal.velocity[axis];
                    motion.velocity[axis] -=
                        position_rate * residual + velocity_rate * terminal.velocity[axis];
                }
                if end >= self.zooms[index].clip.end {
                    motion = CameraMotion::parked(full);
                }
            } else {
                if let Some(index) = active.filter(|&index| self.zooms[index].clip.ease_ms == 0) {
                    let clip = &self.zooms[index].clip;
                    if from <= clip.start
                        && !self.settings.start_early
                        && !self.zooms.iter().any(|zoom| {
                            zoom.holds_for_next
                                && zoom.clip.end <= clip.start
                                && clip.start - zoom.clip.end <= ZOOM_MORPH_GAP_SECONDS
                        })
                    {
                        motion = CameraMotion::parked(self.focus_rect(clip.scale, clip.center));
                    }
                }
                motion = motion.advance(target, self.settings.spring(), end - from);
            }
            from = end;
        }
        motion
    }

    fn evaluate(&mut self, timeline_t: f64) -> (f64, (f64, f64)) {
        let t = timeline_t.max(0.0);
        if self.snap_window(t) {
            let target = self
                .target_index_at(t)
                .map(|index| self.target_rect(index, t))
                .unwrap_or_else(|| self.full_rect());
            return self.frame_for_rect(target);
        }
        if self.zooms.iter().all(|zoom| t > zoom.clip.end) {
            return self.frame_for_rect(self.full_rect());
        }
        let frame = (t / ZOOM_CAMERA_STEP).floor() as usize;
        while self.frames.len() <= frame {
            let from = (self.frames.len() - 1) as f64 * ZOOM_CAMERA_STEP;
            let motion = self.advance(*self.frames.last().unwrap(), from, from + ZOOM_CAMERA_STEP);
            self.frames.push(motion);
        }
        let from = frame as f64 * ZOOM_CAMERA_STEP;
        let motion = self.advance(self.frames[frame], from, t);
        self.frame_for_rect(motion.rect)
    }

    fn frame_for_rect(&self, rect: [f64; 4]) -> (f64, (f64, f64)) {
        let width = rect[2].clamp(self.crop[2], self.crop[2] * MAX_ZOOM_SCALE);
        let height = width * self.crop[3] / self.crop[2];
        let x = rect[0].clamp(self.crop[2] - width, 0.0);
        let y = rect[1].clamp(self.crop[3] - height, 0.0);
        let scale = width / self.crop[2];
        (
            scale,
            (
                self.crop[0] + (self.crop[2] / 2.0 - x) / scale,
                self.crop[1] + (self.crop[3] / 2.0 - y) / scale,
            ),
        )
    }
}

impl VideoEditState {
    fn zoom_camera_signature(&self) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.metadata.width.hash(&mut hash);
        self.metadata.height.hash(&mut hash);
        let crop = self.crop_or_full();
        for value in [
            crop.0,
            crop.1,
            crop.2,
            crop.3,
            self.trim_start_seconds,
            self.trim_end_seconds,
            self.timeline_offset_seconds,
            self.freeze_tail,
            self.zoom_camera.speed,
            self.zoom_camera.smoothness,
        ] {
            value.to_bits().hash(&mut hash);
        }
        self.zoom_camera.start_early.hash(&mut hash);
        self.frozen_segment.hash(&mut hash);
        self.zoom_clips.len().hash(&mut hash);
        for clip in &self.zoom_clips {
            for value in [
                clip.start,
                clip.end,
                clip.scale,
                clip.center.0,
                clip.center.1,
            ] {
                value.to_bits().hash(&mut hash);
            }
            clip.ease_ms.hash(&mut hash);
            (clip.mode == ZoomMode::Auto).hash(&mut hash);
            clip.hidden.hash(&mut hash);
            clip.instant.hash(&mut hash);
        }
        for values in [&self.cuts, &self.segment_starts, &self.segment_speeds] {
            values.len().hash(&mut hash);
            for value in values {
                value.to_bits().hash(&mut hash);
            }
        }
        self.segments_kept.hash(&mut hash);
        self.segment_order.hash(&mut hash);
        if let Some(sidecar) = &self.sidecar {
            sidecar.pointer.len().hash(&mut hash);
            for value in [
                sidecar.region.x as f64,
                sidecar.region.y as f64,
                sidecar.region.w as f64,
                sidecar.region.h as f64,
            ] {
                value.to_bits().hash(&mut hash);
            }
            for sample in &sidecar.pointer {
                for value in [sample.t, sample.x, sample.y] {
                    value.to_bits().hash(&mut hash);
                }
            }
        }
        hash.finish()
    }

    pub(super) fn eval_spring_zoom_at(&self, timeline_t: f64, _source_t: f64) -> (f64, (f64, f64)) {
        let crop = self.crop_or_full();
        if !timeline_t.is_finite()
            || self.zoom_hidden
            || self.zoom_clips.iter().all(|clip| clip.hidden)
        {
            return (1.0, (crop.0 + crop.2 / 2.0, crop.1 + crop.3 / 2.0));
        }
        let signature = self.zoom_camera_signature();
        let mut cache = self
            .zoom_camera_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.signature != Some(signature) {
            cache.plan = Some(ZoomCameraPlan::new(self));
            cache.signature = Some(signature);
        }
        cache.plan.as_mut().unwrap().evaluate(timeline_t)
    }

    pub fn zoom_blur_sample_count(&self) -> usize {
        if !self.zoom_hidden
            && self.zoom_camera.clamped().motion_blur > 0.0
            && self.zoom_clips.iter().any(|clip| !clip.hidden)
        {
            ZOOM_BLUR_SAMPLES
        } else {
            1
        }
    }

    pub fn eval_zoom_blur_sample_at(
        &self,
        timeline_t: f64,
        source_t: f64,
        sample_index: usize,
    ) -> (f64, (f64, f64)) {
        let samples = self.zoom_blur_sample_count();
        if samples == 1 || sample_index == 0 {
            return self.eval_zoom_at(timeline_t, source_t);
        }
        let exposure =
            0.5 / self.metadata.export_frame_rate() * self.zoom_camera.clamped().motion_blur;
        let mut sample_t = (timeline_t
            - exposure * sample_index.min(samples - 1) as f64 / (samples - 1) as f64)
            .max(0.0);
        for clip in self
            .zoom_clips
            .iter()
            .filter(|clip| !clip.hidden && clip.instant)
        {
            if timeline_t >= clip.start && timeline_t <= clip.end + 0.05 {
                return self.eval_zoom_at(timeline_t, source_t);
            }
            if sample_t < clip.end + 0.05 && timeline_t > clip.end + 0.05 {
                sample_t = clip.end + 0.05;
            }
        }
        self.eval_zoom_at(sample_t, self.timeline_to_source(sample_t))
    }
}

#[cfg(test)]
mod zoom_camera_tests {
    use super::*;
    use crate::recording::editor::sidecar::{CaptureRegion, CursorKind, PointerSample};

    fn state() -> VideoEditState {
        VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/nonexistent/zoom-camera-fixture.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 0,
            has_audio: false,
            frame_rate: 30.0,
        })
    }

    fn parked_pointer(state: &mut VideoEditState, x: f64, y: f64) {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x,
            y,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);
    }

    fn zoom(start: f64, end: f64) -> ZoomClip {
        ZoomClip {
            start,
            end,
            scale: 2.0,
            center: (960.0, 540.0),
            mode: ZoomMode::Auto,
            ..Default::default()
        }
    }

    #[test]
    fn adjoining_zoom_keeps_the_evaluated_viewport_instead_of_restarting() {
        let mut state = state();
        parked_pointer(&mut state, 1300.0, 540.0);
        state.zoom_clips = vec![zoom(0.5, 2.0), zoom(2.3, 4.0)];
        let before = state.eval_zoom_at(2.299, 2.299);
        let boundary = state.eval_zoom_at(2.3, 2.3);
        let after = state.eval_zoom_at(2.301, 2.301);
        assert!(
            (boundary.1 .0 - before.1 .0).abs() < 1.0,
            "{before:?} -> {boundary:?}"
        );
        assert!(
            (after.1 .0 - boundary.1 .0).abs() < 1.0,
            "{boundary:?} -> {after:?}"
        );
        assert!(
            boundary.1 .0 > 1290.0,
            "the followed endpoint must not reset to 960: {boundary:?}"
        );
        assert!((before.0 - boundary.0).abs() < 0.01);
    }

    #[test]
    fn zoom_size_and_position_share_the_same_spring_response() {
        let mut state = state();
        let mut clip = zoom(0.5, 3.0);
        clip.mode = ZoomMode::Manual;
        clip.center = (1300.0, 600.0);
        state.zoom_clips.push(clip);
        let (scale, center) = state.eval_zoom_at(0.65, 0.65);
        let position_progress = (center.0 * scale - 960.0) / 1640.0;
        let vertical_progress = (center.1 * scale - 540.0) / 660.0;
        let size_progress = scale - 1.0;
        assert!(size_progress > 0.0 && size_progress < 1.0);
        assert!((position_progress - size_progress).abs() < 1e-9);
        assert!((vertical_progress - size_progress).abs() < 1e-9);
    }

    #[test]
    fn early_animation_precedes_the_block_but_never_precedes_an_instant_zoom() {
        let mut state = state();
        state.zoom_clips.push(zoom(1.0, 3.0));
        assert_eq!(state.eval_zoom_at(0.9, 0.9).0, 1.0);
        state.zoom_camera.start_early = true;
        assert_eq!(state.eval_zoom_at(0.64, 0.64).0, 1.0);
        assert!(state.eval_zoom_at(0.9, 0.9).0 > 1.1);
        state.zoom_clips[0].instant = true;
        assert_eq!(state.eval_zoom_at(0.9, 0.9).0, 1.0);
        assert_eq!(state.eval_zoom_at(1.0, 1.0).0, 2.0);
    }

    #[test]
    fn camera_pacing_uses_composition_time_when_the_video_speed_changes() {
        let mut slow = state();
        slow.zoom_clips.push(zoom(0.5, 2.0));
        let mut fast = slow.clone();
        fast.segment_speeds[0] = 2.0;
        for t in [0.6, 0.8, 1.0] {
            let normal = slow.eval_zoom_at(t, t);
            let retimed = fast.eval_zoom_at(t, t * 2.0);
            assert_eq!(normal, retimed);
        }
    }

    #[test]
    fn follow_targets_remap_when_a_zoom_crosses_reordered_footage() {
        let mut state = state();
        parked_pointer(&mut state, 400.0, 540.0);
        state.sidecar.as_mut().unwrap().pointer.push(PointerSample {
            t: 5.0,
            x: 1500.0,
            y: 540.0,
            kind: CursorKind::Default,
        });
        state.add_cut(5.0);
        state.segment_order = vec![1, 0];
        state.segment_starts = vec![5.0, 0.0];
        state.zoom_clips.push(zoom(0.5, 9.0));
        let first = state.eval_zoom_at(4.0, 9.0);
        let second = state.eval_zoom_at(6.0, 1.0);
        assert!(first.1 .0 > 1400.0, "later footage plays first: {first:?}");
        assert!(
            second.1 .0 < 700.0,
            "the camera must follow earlier footage after the cut: {second:?}"
        );
        let before = state.eval_zoom_at(4.999, 9.999);
        let after = state.eval_zoom_at(5.001, 0.001);
        assert!(
            (before.1 .0 - after.1 .0).abs() < 1.0,
            "{before:?} -> {after:?}"
        );
    }

    #[test]
    fn cache_tracks_direct_clip_pointer_and_settings_edits() {
        let mut state = state();
        parked_pointer(&mut state, 1300.0, 540.0);
        state.zoom_clips.push(zoom(0.5, 3.0));
        let first = state.eval_zoom_at(1.0, 1.0);
        state.sidecar.as_mut().unwrap().pointer[0].x = 600.0;
        let moved = state.eval_zoom_at(1.0, 1.0);
        assert!(moved.1 .0 < first.1 .0);
        state.zoom_camera.speed = 0.25;
        let slowed = state.eval_zoom_at(1.0, 1.0);
        assert!(slowed.0 < moved.0);
        state.zoom_clips[0].hidden = true;
        assert_eq!(state.eval_zoom_at(1.0, 1.0).0, 1.0);
    }

    #[test]
    fn random_subframe_seeks_match_a_fresh_trajectory() {
        let mut state = state();
        parked_pointer(&mut state, 1300.0, 600.0);
        state.zoom_clips = vec![zoom(0.5, 2.0), zoom(2.3, 4.0)];
        let times = [0.61, 1.51, 2.31, 3.91, 4.01];
        let expected: Vec<_> = times.iter().map(|&t| state.eval_zoom_at(t, t)).collect();
        for index in (0..times.len()).rev() {
            let mut fresh = state.clone();
            fresh.zoom_camera_cache = Default::default();
            assert_eq!(
                fresh.eval_zoom_at(times[index], times[index]),
                expected[index]
            );
        }
    }

    #[test]
    fn blur_exposure_is_bounded_and_never_smears_an_instant_cut() {
        let mut state = state();
        state.zoom_clips.push(zoom(0.5, 3.0));
        assert_eq!(state.zoom_blur_sample_count(), 1);
        state.zoom_camera.motion_blur = 1.0;
        assert_eq!(state.zoom_blur_sample_count(), 8);
        let current = state.eval_zoom_blur_sample_at(0.7, 0.7, 0);
        let previous = state.eval_zoom_blur_sample_at(0.7, 0.7, 7);
        assert!(previous.0 < current.0);
        state.zoom_clips[0].instant = true;
        for t in [0.5, 0.51, 3.0, 3.01] {
            for index in 1..8 {
                assert_eq!(
                    state.eval_zoom_blur_sample_at(t, t, index),
                    state.eval_zoom_blur_sample_at(t, t, 0)
                );
            }
        }
    }

    #[test]
    fn an_instant_zoom_owns_its_shared_start_boundary() {
        let mut state = state();
        let mut instant = zoom(2.0, 4.0);
        instant.mode = ZoomMode::Manual;
        instant.center = (1300.0, 540.0);
        instant.scale = 3.0;
        instant.instant = true;
        state.zoom_clips = vec![zoom(0.5, 2.0), instant];
        state.zoom_camera.start_early = true;
        state.zoom_camera.motion_blur = 1.0;
        assert_eq!(state.eval_zoom_at(2.0, 2.0), (3.0, (1300.0, 540.0)));
        assert_eq!(
            state.eval_zoom_blur_sample_at(2.0, 2.0, 7),
            (3.0, (1300.0, 540.0))
        );
    }

    #[test]
    fn standalone_exit_reaches_the_full_crop_at_its_boundary() {
        let mut state = state();
        parked_pointer(&mut state, 1900.0, 900.0);
        state.zoom_camera.speed = 0.25;
        state.zoom_clips.push(zoom(0.5, 2.0));
        let before = state.eval_zoom_at(1.999, 1.999);
        let after = state.eval_zoom_at(2.001, 2.001);
        assert!((before.0 - 1.0).abs() < 0.001);
        assert_eq!(after, (1.0, (960.0, 540.0)));
    }
}
