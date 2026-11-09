//! Zoom framing: inside a clip the camera chases the recorded pointer on a
//! damped spring, and a clip with no pointer samples in its footage window
//! falls back to the static eased framing.
//!
//! The spring integrates in 1 ms steps, matching the studied camera. The
//! result is cached on the composition frame grid anchored at the clip's
//! start, so a scrub backwards replays the same path. [`ZoomCameraSettings`]
//! carries the camera-wide knobs — spring speed and smoothness, plus the early
//! lead-in — that the Zoom panel edits.

use super::pointer_path::{self, PointerEvent, PointerKind};
use super::timeline::{ZoomClip, ZoomMode};
use crate::recording::editor::auto_zoom::release_position;
use crate::recording::editor::sidecar::PointerSidecar;

/// The studied screen spring's precision setting (`CpX:53-58`).
const SCREEN_SPRING_PRECISION: f64 = 0.002;
/// The shared follow spring at speed 1.0 and smoothness 0.5. The camera and
/// the cursor sprite both chase on it.
pub(super) const CAMERA_FOLLOW_SPRING: CameraSpring = CameraSpring {
    stiffness: 125.0,
    damping: 12.0,
    mass: 1.5,
};

/// Camera-wide zoom settings, including the screen motion trail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ZoomCameraSettings {
    pub speed: f64,
    pub smoothness: f64,
    pub start_early: bool,
    pub motion_blur: f64,
    /// Composition frame rate. The studied spring advances once per rendered frame.
    pub frame_rate: f64,
}

impl Default for ZoomCameraSettings {
    fn default() -> Self {
        Self {
            speed: 1.0,
            smoothness: 0.5,
            start_early: false,
            motion_blur: 1.0,
            frame_rate: 60.0,
        }
    }
}

impl ZoomCameraSettings {
    /// The settings the camera actually runs with. Non-finite values fall back
    /// to the defaults instead of poisoning the spring.
    pub(super) fn clamped(self) -> Self {
        let defaults = Self::default();
        Self {
            speed: if self.speed.is_finite() {
                self.speed.clamp(0.25, 3.0)
            } else {
                defaults.speed
            },
            smoothness: if self.smoothness.is_finite() {
                self.smoothness.clamp(0.0, 1.0)
            } else {
                defaults.smoothness
            },
            start_early: self.start_early,
            motion_blur: if self.motion_blur.is_finite() {
                self.motion_blur.clamp(0.0, 1.0)
            } else {
                defaults.motion_blur
            },
            frame_rate: if self.frame_rate.is_finite() && self.frame_rate > 0.0 {
                self.frame_rate.clamp(1.0, 240.0)
            } else {
                defaults.frame_rate
            },
        }
    }

    /// The follow spring these knobs describe, on the same scale as the shared
    /// editor: stiffness `125·speed²`, damping `(4 + 16·smoothness)·speed`.
    pub(super) fn spring(self) -> CameraSpring {
        let settings = self.clamped();
        CameraSpring {
            stiffness: 125.0 * settings.speed * settings.speed,
            damping: (4.0 + 16.0 * settings.smoothness) * settings.speed,
            mass: 1.5,
        }
    }

    /// How long before a clip the camera starts moving when the lead-in is on.
    pub(super) fn lead_in_seconds(self) -> f64 {
        if self.start_early {
            0.35
        } else {
            0.0
        }
    }
}

const MAX_CAMERA_SCALE: f64 = 5.0;
const EASE_SECONDS: f64 = 0.6;
/// Auto zooms at most this far apart share one continuous move.
const MORPH_GAP_SECONDS: f64 = 0.5;

pub(super) type Camera = (f64, (f64, f64));
pub(super) type ViewRect = (f64, f64, f64, f64);

/// How composition time maps onto recorded source time. The Windows timeline
/// can trim the head, cut pieces apart, and hold the last frame, so a zoom
/// window in composition time is not automatically the same footage window.
/// Each entry is `(composition_start, composition_end, source_start, source_end)`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TimeMapping {
    segments: Vec<(f64, f64, f64, f64)>,
}

impl Default for TimeMapping {
    fn default() -> Self {
        Self {
            segments: vec![(0.0, f64::INFINITY, 0.0, f64::INFINITY)],
        }
    }
}

impl TimeMapping {
    pub(super) fn new(segments: Vec<(f64, f64, f64, f64)>) -> Self {
        Self {
            segments: segments
                .into_iter()
                .filter(|&(comp_start, comp_end, source_start, source_end)| {
                    comp_start.is_finite()
                        && comp_end.is_finite()
                        && source_start.is_finite()
                        && source_end.is_finite()
                        && comp_end > comp_start
                        && source_end >= source_start
                })
                .collect(),
        }
    }

    /// Every `(composition_start, composition_end, source_start, source_end)`
    /// footage window a clip's span covers. The identity mapping yields the
    /// clip itself.
    fn windows(&self, clip: ZoomClip) -> impl Iterator<Item = (f64, f64, f64, f64)> + '_ {
        self.segments
            .iter()
            .filter_map(move |&(comp_start, comp_end, source_start, source_end)| {
                let start = clip.start.max(comp_start);
                let end = clip.end.min(comp_end);
                if end <= start {
                    return None;
                }
                let source_start = source_start + (start - comp_start);
                let source_end = (source_start + (end - start)).min(source_end);
                Some((start, end, source_start, source_end))
            })
    }
}

#[derive(Clone, Copy)]
pub(super) struct CameraSpring {
    pub(super) stiffness: f64,
    pub(super) damping: f64,
    pub(super) mass: f64,
}

#[derive(Clone, Copy)]
struct CameraMotion {
    rect: [f64; 4],
    velocity: [f64; 4],
    target: [f64; 4],
    precision: [f64; 4],
}

impl CameraMotion {
    fn parked(rect: [f64; 4]) -> Self {
        Self {
            rect,
            velocity: [0.0; 4],
            target: rect,
            precision: [1.0; 4],
        }
    }

    fn advance(self, target: [f64; 4], spring: CameraSpring, seconds: f64) -> Self {
        let mut motion = self;
        for (axis, &goal) in target.iter().enumerate() {
            if goal != motion.target[axis] {
                motion.precision[axis] =
                    studied_precision(motion.rect[axis], goal, SCREEN_SPRING_PRECISION);
                motion.target[axis] = goal;
            }
            (motion.rect[axis], motion.velocity[axis]) = studied_advance(
                motion.rect[axis],
                motion.velocity[axis],
                goal,
                spring,
                motion.precision[axis],
                seconds * 1000.0,
            );
        }
        motion
    }
}

struct CameraZoom {
    clip: ZoomClip,
    /// One entry per movement group: the group's first composition time and
    /// its dwell-weighted centre in encoded pixels. Empty when the clip's
    /// footage window has no pointer samples, which keeps the static fallback.
    groups: Vec<(f64, (f64, f64))>,
    /// A later auto zoom starts within the morph gap, so this clip holds its
    /// framing through the gap instead of returning to the full frame.
    holds_for_next: bool,
}

struct ZoomCameraPlan {
    frame: (f64, f64),
    clips: Vec<ZoomClip>,
    zooms: Vec<CameraZoom>,
    settings: ZoomCameraSettings,
    start_time: Option<f64>,
    frames: Vec<CameraMotion>,
}

#[derive(Default)]
pub(super) struct ZoomCameraCache {
    clips: Option<Vec<ZoomClip>>,
    frame: Option<(f64, f64)>,
    mapping: Option<TimeMapping>,
    settings: Option<ZoomCameraSettings>,
    plan: Option<ZoomCameraPlan>,
}

impl ZoomCameraCache {
    pub(super) fn clear(&mut self) {
        self.clips = None;
        self.frame = None;
        self.mapping = None;
        self.settings = None;
        self.plan = None;
    }

    pub(super) fn evaluate(
        &mut self,
        clips: &[ZoomClip],
        frame: (f64, f64),
        mapping: &TimeMapping,
        settings: ZoomCameraSettings,
        time: f64,
        sidecar: impl FnOnce() -> Option<PointerSidecar>,
    ) -> Camera {
        let settings = settings.clamped();
        if self.clips.as_deref() != Some(clips)
            || self.frame != Some(frame)
            || self.mapping.as_ref() != Some(mapping)
            || self.settings != Some(settings)
            || self.plan.is_none()
        {
            let sidecar = sidecar();
            self.plan = Some(ZoomCameraPlan::new(
                clips,
                sidecar.as_ref(),
                frame,
                mapping,
                settings,
            ));
            self.clips = Some(clips.to_vec());
            self.frame = Some(frame);
            self.mapping = Some(mapping.clone());
            self.settings = Some(settings);
        }
        self.plan
            .as_mut()
            .map(|plan| plan.evaluate(time))
            .unwrap_or_else(|| static_camera_at(clips, time, frame))
    }
}

impl ZoomCameraPlan {
    fn new(
        clips: &[ZoomClip],
        sidecar: Option<&PointerSidecar>,
        frame: (f64, f64),
        mapping: &TimeMapping,
        settings: ZoomCameraSettings,
    ) -> Self {
        let mut clips = clips.to_vec();
        clips.sort_by(|left, right| left.start.total_cmp(&right.start));
        let events = sidecar
            .map(|sidecar| {
                let map = |x: f64, y: f64| sidecar.map_to_video(x, y, frame.0, frame.1);
                let mut events: Vec<PointerEvent> = Vec::new();
                for sample in &sidecar.pointer {
                    let (x, y) = map(sample.x, sample.y);
                    events.push(PointerEvent {
                        t: sample.t,
                        x,
                        y,
                        kind: PointerKind::Move,
                    });
                }
                for click in &sidecar.clicks {
                    let (x, y) = map(click.x, click.y);
                    events.push(PointerEvent {
                        t: click.t,
                        x,
                        y,
                        kind: PointerKind::Down,
                    });
                }
                for press in &sidecar.presses {
                    if !press.up.is_finite() || press.up < press.down {
                        continue;
                    }
                    if let Some((x, y)) = release_position(sidecar, press, frame.0, frame.1) {
                        events.push(PointerEvent {
                            t: press.up,
                            x,
                            y,
                            kind: PointerKind::Up,
                        });
                    }
                }
                events.retain(|e| e.t.is_finite() && e.x.is_finite() && e.y.is_finite());
                events.sort_by(|a, b| a.t.total_cmp(&b.t));
                // The studied camera reads the recording's pointer path after
                // its default shake filter (`B39:3274-3284`, `DgH:3254`).
                pointer_path::remove_shakes(&events, pointer_path::SHAKE_THRESHOLD)
            })
            .unwrap_or_default();
        let zooms = clips
            .iter()
            .copied()
            .filter(|clip| {
                !clip.hidden
                    && clip.start.is_finite()
                    && clip.end.is_finite()
                    && clip.end > clip.start
                    && clip.scale.is_finite()
            })
            .map(|clip| {
                let groups = if clip.mode == ZoomMode::Auto {
                    let points = follow_points(&events, mapping, clip);
                    group_centers(&movement_groups(
                        &points,
                        movement_group_budget(frame, clip.scale),
                    ))
                } else {
                    Vec::new()
                };
                CameraZoom {
                    clip,
                    groups,
                    holds_for_next: morphs_into_neighbour(&clips, clip),
                }
            })
            .collect::<Vec<_>>();
        let lead_in = settings.lead_in_seconds();
        let start_time = zooms
            .iter()
            .map(|zoom| (zoom.clip.start - lead_in).max(0.0))
            .min_by(f64::total_cmp);
        let frames = start_time
            .map(|_| vec![CameraMotion::parked(full_rect(frame))])
            .unwrap_or_default();
        Self {
            frame,
            clips,
            zooms,
            settings,
            start_time,
            frames,
        }
    }

    fn target_index_at(&self, time: f64) -> Option<usize> {
        if let Some(index) = self
            .zooms
            .iter()
            .rposition(|zoom| time >= zoom.clip.start && time <= zoom.clip.end)
        {
            return Some(index);
        }
        // With the lead-in on, the camera starts moving towards a clip before
        // that clip begins, so the move is already under way at its first frame.
        if self.settings.start_early {
            let lead_in = self.settings.lead_in_seconds();
            if let Some(index) = self.zooms.iter().position(|zoom| {
                !zoom.clip.instant && time >= zoom.clip.start - lead_in && time < zoom.clip.start
            }) {
                return Some(index);
            }
        }
        // A playhead in a morph gap holds the clip it just left.
        self.zooms
            .iter()
            .enumerate()
            .filter(|(_, zoom)| {
                zoom.holds_for_next
                    && time > zoom.clip.end
                    && time - zoom.clip.end <= MORPH_GAP_SECONDS
            })
            .max_by(|(_, left), (_, right)| left.clip.end.total_cmp(&right.clip.end))
            .map(|(index, _)| index)
    }

    fn target_rect(&self, index: usize, time: f64) -> [f64; 4] {
        let zoom = &self.zooms[index];
        // A lead-in sample sits before the clip; frame it where the clip
        // itself starts rather than reading a movement group that has not
        // begun yet.
        let time = time.max(zoom.clip.start);
        let center = if zoom.clip.mode == ZoomMode::Auto {
            group_center_at(&zoom.groups, time).unwrap_or(zoom.clip.center)
        } else {
            zoom.clip.center
        };
        focus_rect(zoom.clip.scale, center, self.frame)
    }

    fn advance(&self, motion: CameraMotion, from: f64, to: f64) -> CameraMotion {
        if to <= from {
            return motion;
        }
        let target = match self.target_index_at(to) {
            Some(index) => self.target_rect(index, to),
            None => full_rect(self.frame),
        };
        // An instant zoom swaps in a zero spring through its window, which
        // runs 50 ms past the clip (`B39:7908-7915`), so the camera snaps.
        if instant_blur_disabled(&self.clips, to) {
            return CameraMotion::parked(target);
        }
        motion.advance(target, self.settings.spring(), to - from)
    }

    fn evaluate(&mut self, time: f64) -> Camera {
        if !valid_frame(self.frame) || !time.is_finite() {
            return static_camera_at(&self.clips, time, self.frame);
        }
        let time = time.max(0.0);
        if let Some(index) = self.target_index_at(time) {
            if self.zooms[index].clip.instant {
                return frame_for_rect(self.target_rect(index, time), self.frame);
            }
        }
        let Some(start_time) = self.start_time else {
            return static_camera_at(&self.clips, time, self.frame);
        };
        if time < start_time {
            return static_camera_at(&self.clips, time, self.frame);
        }

        let step = 1.0 / self.settings.frame_rate;
        let frame_index = ((time - start_time) / step).floor() as usize;
        while self.frames.len() <= frame_index {
            let from = start_time + (self.frames.len() - 1) as f64 * step;
            let motion = self.advance(
                *self
                    .frames
                    .last()
                    .expect("camera plan starts with one frame"),
                from,
                from + step,
            );
            self.frames.push(motion);
        }
        let from = start_time + frame_index as f64 * step;
        let motion = self.advance(self.frames[frame_index], from, time);
        frame_for_rect(motion.rect, self.frame)
    }
}

pub(super) fn instant_blur_disabled(clips: &[ZoomClip], time: f64) -> bool {
    clips
        .iter()
        .any(|clip| !clip.hidden && clip.instant && time >= clip.start && time <= clip.end + 0.05)
}

pub(super) fn static_camera_at(clips: &[ZoomClip], time: f64, frame: (f64, f64)) -> Camera {
    let frame_center = (frame.0 / 2.0, frame.1 / 2.0);
    if !time.is_finite() || !valid_frame(frame) {
        return (1.0, frame_center);
    }
    let Some(index) = clips.iter().position(|clip| {
        !clip.hidden
            && clip.start.is_finite()
            && clip.end.is_finite()
            && clip.end > clip.start
            && clip.scale.is_finite()
            && time >= clip.start
            && time <= clip.end
    }) else {
        // Hold the clip just left across a morph gap so the next auto zoom
        // continues from its framing instead of flashing the full frame.
        return match gap_hold_predecessor(clips, time) {
            Some(previous) => (clips[previous].scale.max(1.0), clips[previous].center),
            None => (1.0, frame_center),
        };
    };

    let clip = &clips[index];
    let to_scale = clip.scale.max(1.0);
    // An instant clip has no ramp at either end: it is at its framing on its
    // first frame and back to the full frame on the frame after its last.
    if clip.instant {
        return (to_scale, clip.center);
    }
    let ease = EASE_SECONDS.min((clip.end - clip.start) / 2.0);
    if ease <= f64::EPSILON {
        return (to_scale, clip.center);
    }
    if time < clip.start + ease {
        let progress = smoothstep((time - clip.start) / ease);
        let (from_scale, from_center) = match morph_predecessor(clips, index) {
            Some(previous) => (clips[previous].scale.max(1.0), clips[previous].center),
            None => (1.0, clip.center),
        };
        return (
            from_scale + (to_scale - from_scale) * progress,
            (
                from_center.0 + (clip.center.0 - from_center.0) * progress,
                from_center.1 + (clip.center.1 - from_center.1) * progress,
            ),
        );
    }
    (to_scale, clip.center)
}

pub(super) fn view_rect(scale: f64, center: (f64, f64), frame: (f64, f64)) -> ViewRect {
    let (frame_width, frame_height) = frame;
    if !valid_frame(frame) {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    let width = (frame_width / scale).clamp(0.0, frame_width);
    let height = (frame_height / scale).clamp(0.0, frame_height);
    let center_x = if center.0.is_finite() {
        center.0
    } else {
        frame_width / 2.0
    };
    let center_y = if center.1.is_finite() {
        center.1
    } else {
        frame_height / 2.0
    };
    let x = (center_x - width / 2.0).clamp(0.0, (frame_width - width).max(0.0));
    let y = (center_y - height / 2.0).clamp(0.0, (frame_height - height).max(0.0));
    (x, y, width, height)
}

/// Mirrors the shared editor's `zoom_camera_transform` for the video Picture.
pub(super) fn camera_transform(
    view: ViewRect,
    src_w: f64,
    src_h: f64,
    clip_w: f64,
    clip_h: f64,
) -> (f64, f64, f64, f64) {
    let (view_x, view_y, view_w, view_h) = view;
    (
        -view_x / view_w.max(1.0) * clip_w,
        -view_y / view_h.max(1.0) * clip_h,
        src_w / view_w.max(1.0),
        src_h / view_h.max(1.0),
    )
}

/// Mirrors the shared editor's `source_to_zoomed_point` for overlay points.
pub(super) fn source_to_stage(
    point: (f64, f64),
    view: ViewRect,
    widget_w: f64,
    widget_h: f64,
) -> (f64, f64) {
    let (view_x, view_y, view_w, view_h) = view;
    (
        ((point.0 - view_x) / view_w.max(1.0)) * widget_w,
        ((point.1 - view_y) / view_h.max(1.0)) * widget_h,
    )
}

/// The outlined region a Manual zoom at `scale` will frame, mapped into the
/// rectangle the video occupies inside the preview widget. `view` is the source
/// region on screen, which is the whole frame while the outline is up.
pub(super) fn placement_rect(
    scale: f64,
    center: (f64, f64),
    view: ViewRect,
    video: [f64; 4],
) -> [f64; 4] {
    let scale = if scale.is_finite() {
        scale.max(1.0)
    } else {
        1.0
    };
    let width = video[2];
    let height = video[3];
    let rect_w = width / scale;
    let rect_h = height / scale;
    let center_x = if center.0.is_finite() {
        video[0] + ((center.0 - view.0) / view.2.max(1.0)) * width
    } else {
        video[0] + width / 2.0
    };
    let center_y = if center.1.is_finite() {
        video[1] + ((center.1 - view.1) / view.3.max(1.0)) * height
    } else {
        video[1] + height / 2.0
    };
    let x = if rect_w >= width {
        video[0] + (width - rect_w) / 2.0
    } else {
        (center_x - rect_w / 2.0).clamp(video[0], video[0] + width - rect_w)
    };
    let y = if rect_h >= height {
        video[1] + (height - rect_h) / 2.0
    } else {
        (center_y - rect_h / 2.0).clamp(video[1], video[1] + height - rect_h)
    };
    [x, y, rect_w, rect_h]
}

/// A point in the preview widget as a source pixel, through the view the
/// preview shows and the rectangle the video occupies. This is the inverse of
/// [`source_to_stage`] for the focus drag, which places a point rather than
/// projecting one.
pub(super) fn stage_point_to_source(
    point: (f64, f64),
    view: ViewRect,
    video: [f64; 4],
) -> (f64, f64) {
    (
        view.0 + ((point.0 - video[0]) / video[2].max(1.0)) * view.2,
        view.1 + ((point.1 - video[1]) / video[3].max(1.0)) * view.3,
    )
}

fn camera_rect(camera: Camera, frame: (f64, f64)) -> [f64; 4] {
    if !valid_frame(frame) {
        return full_rect(frame);
    }
    let scale = camera.0.clamp(1.0, MAX_CAMERA_SCALE);
    let width = frame.0 * scale;
    let height = frame.1 * scale;
    let center_x = if camera.1 .0.is_finite() {
        camera.1 .0
    } else {
        frame.0 / 2.0
    };
    let center_y = if camera.1 .1.is_finite() {
        camera.1 .1
    } else {
        frame.1 / 2.0
    };
    [
        (frame.0 / 2.0 - center_x * scale).clamp(frame.0 - width, 0.0),
        (frame.1 / 2.0 - center_y * scale).clamp(frame.1 - height, 0.0),
        width,
        height,
    ]
}

fn focus_rect(scale: f64, center: (f64, f64), frame: (f64, f64)) -> [f64; 4] {
    let scale = if scale.is_finite() {
        scale.clamp(1.0, MAX_CAMERA_SCALE)
    } else {
        1.0
    };
    camera_rect((scale, center), frame)
}

fn full_rect(frame: (f64, f64)) -> [f64; 4] {
    [0.0, 0.0, frame.0.max(0.0), frame.1.max(0.0)]
}

fn frame_for_rect(rect: [f64; 4], frame: (f64, f64)) -> Camera {
    if !valid_frame(frame) {
        return (1.0, (0.0, 0.0));
    }
    let width = rect[2].clamp(frame.0, frame.0 * MAX_CAMERA_SCALE);
    let height = width * frame.1 / frame.0;
    let x = rect[0].clamp(frame.0 - width, 0.0);
    let y = rect[1].clamp(frame.1 - height, 0.0);
    let scale = width / frame.0;
    (
        scale,
        ((frame.0 / 2.0 - x) / scale, (frame.1 / 2.0 - y) / scale),
    )
}

/// The closest earlier auto zoom that `index` morphs from: it must end
/// within the morph gap, and both clips must be auto and animated. A hidden
/// clip never contributes, and neither does a manual or an instant one.
fn morph_predecessor(clips: &[ZoomClip], index: usize) -> Option<usize> {
    let clip = &clips[index];
    if clip.mode != ZoomMode::Auto || clip.instant {
        return None;
    }
    clips
        .iter()
        .enumerate()
        .filter(|(other, previous)| {
            *other != index
                && previous.mode == ZoomMode::Auto
                && !previous.instant
                && !previous.hidden
                && previous.end <= clip.start
                && clip.start - previous.end <= MORPH_GAP_SECONDS
        })
        .max_by(|(_, left), (_, right)| left.end.total_cmp(&right.end))
        .map(|(previous, _)| previous)
}

/// True when a later auto zoom morphs from `clip`, so it holds its framing
/// through the gap instead of returning to the full frame.
fn morphs_into_neighbour(clips: &[ZoomClip], clip: ZoomClip) -> bool {
    clip.mode == ZoomMode::Auto
        && !clip.instant
        && !clip.hidden
        && clips.iter().any(|next| {
            *next != clip
                && next.mode == ZoomMode::Auto
                && !next.instant
                && !next.hidden
                && next.start >= clip.end
                && next.start - clip.end <= MORPH_GAP_SECONDS
        })
}

/// The closest auto zoom whose framing is held while `time` sits in a gap.
fn gap_hold_predecessor(clips: &[ZoomClip], time: f64) -> Option<usize> {
    clips
        .iter()
        .enumerate()
        .filter(|(_, clip)| {
            clip.mode == ZoomMode::Auto
                && !clip.instant
                && !clip.hidden
                && clip.end <= time
                && time - clip.end <= MORPH_GAP_SECONDS
        })
        .max_by(|(_, left), (_, right)| left.end.total_cmp(&right.end))
        .map(|(previous, _)| previous)
        .filter(|&previous| morphs_into_neighbour(clips, clips[previous]))
}

/// The pointer sample the studied camera uses when a clip window has none of
/// its own: the first sample at or after the window start within 500 ms,
/// otherwise the nearest sample.
fn nearest_follow_sample(samples: &[(f64, f64, f64)], start: f64) -> Option<(f64, f64, f64)> {
    const LOOKAHEAD_SECONDS: f64 = 0.5;
    samples
        .iter()
        .filter(|&&(time, x, y)| time.is_finite() && x.is_finite() && y.is_finite())
        .filter(|&&(time, _, _)| time >= start && time - start <= LOOKAHEAD_SECONDS)
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .or_else(|| {
            samples
                .iter()
                .filter(|&&(time, x, y)| time.is_finite() && x.is_finite() && y.is_finite())
                .min_by(|left, right| (left.0 - start).abs().total_cmp(&(right.0 - start).abs()))
        })
        .copied()
}

/// The pointer samples one clip's follow camera reads, as
/// `(composition_time, x, y)`. Moves closer than the studied 10 px to the
/// last kept event are dropped, but presses and releases are always kept,
/// and the last sample before each footage window is carried in at the
/// window's first instant so the first movement group starts where the
/// pointer already was. When the clip has no sample before or inside its
/// windows, the studied nearest-sample fallback keeps the camera following
/// instead of freezing on the stored centre.
fn follow_points(
    samples: &[PointerEvent],
    mapping: &TimeMapping,
    clip: ZoomClip,
) -> Vec<(f64, f64, f64)> {
    let mut points: Vec<PointerEvent> = Vec::new();
    let mut first_window = None;
    for (start, _, source_start, source_end) in mapping.windows(clip) {
        first_window.get_or_insert((start, source_start));
        let index = samples.partition_point(|e| e.t < source_start);
        if let Some(before) = index.checked_sub(1).and_then(|before| samples.get(before)) {
            points.push(PointerEvent {
                t: start - 0.001,
                kind: PointerKind::Move,
                ..*before
            });
        }
        points.extend(
            samples[index..]
                .iter()
                .take_while(|e| e.t <= source_end)
                .map(|e| PointerEvent {
                    t: start + (e.t - source_start),
                    ..*e
                }),
        );
    }
    if points.is_empty() {
        if let Some((start, source_start)) = first_window {
            let tuples = &samples.iter().map(|e| (e.t, e.x, e.y)).collect::<Vec<_>>();
            if let Some((time, x, y)) = nearest_follow_sample(tuples, source_start) {
                points.push(PointerEvent {
                    t: start + (time - source_start),
                    x,
                    y,
                    kind: PointerKind::Move,
                });
            }
        }
    }
    points.sort_by(|left, right| left.t.total_cmp(&right.t));
    // The studied focus path keeps clicks and drops moves closer than 10 px
    // to the last kept event (`B39:2804-2826`, `B39:3333`).
    pointer_path::decimate(&points, pointer_path::DECIMATE_DISTANCE)
        .into_iter()
        .map(|e| (e.t, e.x, e.y))
        .collect()
}

/// Half the zoomed view: the movement-group budget at this scale. A tighter
/// zoom groups more finely, a wider one lets the pointer roam further before
/// splitting.
fn movement_group_budget(frame: (f64, f64), scale: f64) -> (f64, f64) {
    let scale = scale.max(1.0);
    (
        (frame.0.max(1.0) / scale * 0.5).max(1.0),
        (frame.1.max(1.0) / scale * 0.5).max(1.0),
    )
}

/// Split time-ordered pointer samples into movement groups. A group keeps
/// accepting samples while its bounding box (including the candidate) fits
/// inside `max_distance`; the first sample that would burst the box opens a
/// new group.
fn movement_groups(
    points: &[(f64, f64, f64)],
    max_distance: (f64, f64),
) -> Vec<Vec<(f64, f64, f64)>> {
    let mut groups: Vec<Vec<(f64, f64, f64)>> = Vec::new();
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(time, x, y) in points {
        if !time.is_finite() || !x.is_finite() || !y.is_finite() {
            continue;
        }
        let fits = groups.last().is_some_and(|_| {
            let next_min = (min.0.min(x), min.1.min(y));
            let next_max = (max.0.max(x), max.1.max(y));
            next_max.0 - next_min.0 <= max_distance.0.max(0.0)
                && next_max.1 - next_min.1 <= max_distance.1.max(0.0)
        });
        if fits {
            min = (min.0.min(x), min.1.min(y));
            max = (max.0.max(x), max.1.max(y));
        } else {
            groups.push(Vec::new());
            min = (x, y);
            max = (x, y);
        }
        if let Some(group) = groups.last_mut() {
            group.push((time, x, y));
        }
    }
    groups
}

/// Each movement group's first time and dwell-weighted centre. The centre
/// does not depend on the query time — only which group is active does.
fn group_centers(groups: &[Vec<(f64, f64, f64)>]) -> Vec<(f64, (f64, f64))> {
    groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let first_time = group.first()?.0;
            let end_time = groups
                .get(index + 1)
                .and_then(|next| next.first().map(|first| first.0));
            Some((first_time, time_weighted_center(group, end_time)?))
        })
        .collect()
}

/// Dwell-weighted centre: each sample holds until the next one (or `end_time`,
/// or a 100 ms grace for a trailing sample), so a pause outweighs a fly-by.
fn time_weighted_center(points: &[(f64, f64, f64)], end_time: Option<f64>) -> Option<(f64, f64)> {
    let mut weight_sum = 0.0;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for (index, &(time, x, y)) in points.iter().enumerate() {
        let next_time = points
            .get(index + 1)
            .map(|next| next.0)
            .filter(|next| next.is_finite())
            .or(end_time.filter(|end| end.is_finite()))
            .unwrap_or(time + 0.1);
        let weight = (next_time - time).max(0.0);
        if weight <= 0.0 {
            continue;
        }
        weight_sum += weight;
        x_sum += x * weight;
        y_sum += y * weight;
    }
    (weight_sum > 0.0).then(|| (x_sum / weight_sum, y_sum / weight_sum))
}

/// Centre of the movement group active at `time`: the latest group whose
/// first sample starts at or before it. Falls back to the first group, and to
/// `None` without groups.
fn group_center_at(centers: &[(f64, (f64, f64))], time: f64) -> Option<(f64, f64)> {
    if centers.is_empty() || !time.is_finite() {
        return None;
    }
    let mut active = 0;
    for (index, (first_time, _)) in centers.iter().enumerate() {
        if *first_time <= time {
            active = index;
        } else {
            break;
        }
    }
    Some(centers[active].1)
}

/// The rest tolerance the studied spring sets whenever its target changes:
/// the move's size times the precision setting, never under one move unit.
fn studied_precision(value: f64, target: f64, precision: f64) -> f64 {
    if value == target {
        return 1.0;
    }
    (value - target).abs().max(1.0) * precision
}

/// One studied spring advance (`CpX:314-350`): the overspeed damping is chosen
/// once per call, then the spring integrates in whole milliseconds. The last
/// step uses the studied fractional rule exactly.
fn studied_advance(
    mut value: f64,
    mut velocity: f64,
    target: f64,
    spring: CameraSpring,
    precision: f64,
    millis: f64,
) -> (f64, f64) {
    if millis.is_nan()
        || millis <= 0.0
        || !value.is_finite()
        || !velocity.is_finite()
        || !target.is_finite()
    {
        return (value, velocity);
    }
    let stiffness = spring.stiffness.max(f64::EPSILON);
    let mass = spring.mass.max(f64::EPSILON);
    let mut damping = spring.damping.max(0.0);
    let displacement = target - value;
    if velocity * displacement > 0.0 {
        let natural = (stiffness / mass).sqrt();
        if velocity.abs() > displacement.abs() * natural {
            damping = damping.max(2.0 * (mass * stiffness).sqrt());
        }
    }
    let steps = millis.ceil() as u64;
    for step in 1..=steps {
        let step = step as f64;
        let dt = if step > millis {
            (step - millis) / 1000.0
        } else {
            0.001
        };
        let acceleration = (-(value - target) * stiffness - velocity * damping) / mass;
        let next_velocity = velocity + acceleration * dt;
        let next_value = value + next_velocity * dt;
        if next_velocity.abs() < precision && (next_value - target).abs() < precision {
            return (target, 0.0);
        }
        velocity = next_velocity;
        value = next_value;
    }
    (value, velocity)
}

pub(super) fn spring_step(
    value: f64,
    velocity: f64,
    target: f64,
    spring: CameraSpring,
    seconds: f64,
) -> (f64, f64) {
    let seconds = seconds.max(0.0);
    if seconds <= 0.0 || !value.is_finite() || !velocity.is_finite() || !target.is_finite() {
        return (value, velocity);
    }
    let stiffness = spring.stiffness.max(f64::EPSILON);
    let mass = spring.mass.max(f64::EPSILON);
    let mut damping = spring.damping.max(0.0);
    let displacement = target - value;
    if velocity * displacement > 0.0 {
        let natural = (stiffness / mass).sqrt();
        if velocity.abs() > displacement.abs() * natural {
            damping = damping.max(2.0 * (mass * stiffness).sqrt());
        }
    }
    let acceleration = (-(value - target) * stiffness - velocity * damping) / mass;
    let velocity = velocity + acceleration * seconds;
    let value = value + velocity * seconds;
    const REST_DISTANCE: f64 = 0.01;
    const REST_VELOCITY: f64 = 0.01;
    if (value - target).abs() < REST_DISTANCE && velocity.abs() < REST_VELOCITY {
        (target, 0.0)
    } else {
        (value, velocity)
    }
}

fn smoothstep(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn valid_frame(frame: (f64, f64)) -> bool {
    frame.0.is_finite() && frame.1.is_finite() && frame.0 > 0.0 && frame.1 > 0.0
}

#[cfg(test)]
mod tests {
    use super::{
        camera_transform, nearest_follow_sample, placement_rect, source_to_stage, spring_step,
        stage_point_to_source, static_camera_at, studied_advance, studied_precision, view_rect,
        Camera, CameraMotion, TimeMapping, ZoomCameraCache, ZoomCameraSettings,
        CAMERA_FOLLOW_SPRING, SCREEN_SPRING_PRECISION,
    };
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };
    use crate::video_editor::timeline::{ZoomClip, ZoomMode};

    fn auto_clip(start: f64, end: f64, scale: f64, center: (f64, f64)) -> ZoomClip {
        ZoomClip {
            start,
            end,
            scale,
            center,
            hidden: false,
            mode: ZoomMode::Auto,
            instant: false,
            anchor: None,
        }
    }

    fn manual_clip(start: f64, end: f64, scale: f64, center: (f64, f64)) -> ZoomClip {
        ZoomClip {
            mode: ZoomMode::Manual,
            ..auto_clip(start, end, scale, center)
        }
    }

    fn sidecar(frame: (u32, u32), points: &[(f64, f64, f64)]) -> PointerSidecar {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: frame.0,
                h: frame.1,
            },
        );
        sidecar
            .pointer
            .extend(points.iter().map(|&(time, x, y)| PointerSample {
                t: time,
                x,
                y,
                kind: CursorKind::Default,
                cursor_id: None,
            }));
        sidecar
    }

    fn evaluate(
        clips: &[ZoomClip],
        sidecar: &PointerSidecar,
        frame: (f64, f64),
        time: f64,
    ) -> Camera {
        let mut cache = ZoomCameraCache::default();
        cache.evaluate(
            clips,
            frame,
            &TimeMapping::default(),
            ZoomCameraSettings::default(),
            time,
            || Some(sidecar.clone()),
        )
    }

    #[test]
    fn camera_springs_in_holds_and_springs_out_without_pointer_samples() {
        let clips = [auto_clip(1.0, 4.0, 3.0, (320.0, 240.0))];
        let frame = (640.0, 480.0);
        let sidecar = sidecar((640, 480), &[]);
        assert!(evaluate(&clips, &sidecar, frame, 1.0).0 < 1.01);
        let entering = evaluate(&clips, &sidecar, frame, 1.3).0;
        assert!(
            entering > 1.5 && entering < 3.2,
            "entry rides the spring, got {entering}"
        );
        for time in [2.5, 3.7, 4.0] {
            let held = evaluate(&clips, &sidecar, frame, time).0;
            assert!((held - 3.0).abs() < 0.01, "time={time} held={held}");
        }
        let leaving = evaluate(&clips, &sidecar, frame, 4.05).0;
        assert!(
            leaving > 1.5 && leaving < 3.0,
            "exit rides the spring, got {leaving}"
        );
        assert!(evaluate(&clips, &sidecar, frame, 5.5).0 < 1.01);
    }

    #[test]
    fn the_live_fallback_prefers_a_lookahead_sample() {
        let samples = [(0.8, 10.0, 10.0), (1.2, 20.0, 20.0), (5.0, 30.0, 30.0)];
        assert_eq!(
            nearest_follow_sample(&samples, 1.0),
            Some((1.2, 20.0, 20.0))
        );
    }

    #[test]
    fn the_follow_camera_ignores_a_lone_pointer_spike() {
        let mut points: Vec<(f64, f64, f64)> = (0..=30)
            .map(|i| (1.0 + i as f64 * 0.1, 900.0, 500.0))
            .collect();
        // One far sample, as from a cursor warp, then straight back.
        points.insert(11, (2.05, 50.0, 50.0));
        let sidecar = sidecar((1280, 720), &points);
        let clips = [auto_clip(1.0, 4.0, 2.0, (640.0, 360.0))];
        let frame = (1280.0, 720.0);
        for step in 0..=12 {
            let time = 2.0 + step as f64 * 0.05;
            let (_, center) = evaluate(&clips, &sidecar, frame, time);
            assert!(
                (center.0 - 900.0).abs() < 1.0 && (center.1 - 500.0).abs() < 1.0,
                "time={time} center={center:?}"
            );
        }
    }

    #[test]
    fn a_clip_before_the_pointer_starts_follows_the_nearest_sample() {
        let clips = [auto_clip(1.0, 2.0, 3.0, (320.0, 240.0))];
        // The pointer appears long after this clip ends. The studied camera
        // still falls back to it, so the clip follows instead of freezing on
        // the stored centre.
        let sidecar = sidecar((640, 480), &[(6.0, 600.0, 300.0)]);
        let (scale, center) = evaluate(&clips, &sidecar, (640.0, 480.0), 1.5);
        assert!(scale > 1.5, "the clip must still zoom: {scale}");
        assert!(
            center.0 > 450.0,
            "the camera must follow the sample: {center:?}"
        );
    }

    #[test]
    fn the_spring_rests_at_the_studied_threshold() {
        let (value, velocity) = spring_step(99.995, 0.004, 100.0, CAMERA_FOLLOW_SPRING, 0.001);
        assert_eq!(value, 100.0);
        assert_eq!(velocity, 0.0);
    }

    #[test]
    fn the_camera_runs_the_studied_integrator_once_per_frame() {
        let motion = CameraMotion::parked([0.0; 4]).advance([100.0; 4], CAMERA_FOLLOW_SPRING, 0.1);
        let precision = studied_precision(0.0, 100.0, SCREEN_SPRING_PRECISION);
        let (value, _) = studied_advance(0.0, 0.0, 100.0, CAMERA_FOLLOW_SPRING, precision, 100.0);
        assert!((motion.rect[0] - value).abs() < 1e-9);
    }

    #[test]
    fn the_camera_spring_matches_the_studied_integrator_frame_by_frame() {
        // Reference values come from running the studied build's own spring
        // functions (`CpX:283-392`) per rendered frame: a 600 px move from
        // rest at 60, 30, and 24 fps, and an overspeed start that triggers the
        // damping clamp.
        let run = |start: CameraMotion, goal: f64, fps: f64, frames: usize| {
            let mut motion = start;
            let mut values = Vec::new();
            for _ in 0..frames {
                motion = motion.advance([goal; 4], CAMERA_FOLLOW_SPRING, 1.0 / fps);
                values.push(motion.rect[0]);
            }
            values
        };
        let check = |values: &[f64], expected: &[(usize, f64)]| {
            for &(frame, value) in expected {
                assert!(
                    (values[frame] - value).abs() < 1e-6,
                    "frame {frame}: {} vs {value}",
                    values[frame]
                );
            }
        };
        let rest = CameraMotion::parked([0.0; 4]);
        check(
            &run(rest, 600.0, 60.0, 90),
            &[
                (0, 6.77845978490818),
                (1, 25.109368517395612),
                (7, 279.8052349666634),
                (8, 327.48356247411374),
                (9, 368.0416739380785),
                (85, 600.0),
            ],
        );
        check(
            &run(rest, 600.0, 30.0, 45),
            &[(0, 26.57250330819989), (4, 382.62526235836464), (40, 600.0)],
        );
        check(
            &run(rest, 600.0, 24.0, 36),
            &[(0, 38.91758891664947), (4, 440.3020284892068), (31, 600.0)],
        );
        let fast = CameraMotion {
            velocity: [3000.0; 4],
            ..rest
        };
        check(
            &run(fast, 100.0, 60.0, 6),
            &[
                (0, 42.84784030863177),
                (1, 75.66619104852342),
                (5, 151.72445540251965),
            ],
        );
    }

    #[test]
    fn the_studied_precision_scales_with_the_move() {
        assert_eq!(studied_precision(5.0, 5.0, 0.002), 1.0);
        assert!((studied_precision(0.0, 600.0, 0.002) - 1.2).abs() < 1e-12);
        assert!((studied_precision(0.0, 0.3, 0.002) - 0.002).abs() < 1e-12);
    }

    #[test]
    fn instant_zoom_disables_the_trail_through_its_extended_window() {
        let mut clip = auto_clip(1.0, 2.0, 2.0, (320.0, 240.0));
        clip.instant = true;
        assert!(!super::instant_blur_disabled(&[clip], 0.99));
        assert!(super::instant_blur_disabled(&[clip], 1.0));
        assert!(super::instant_blur_disabled(&[clip], 2.049));
        assert!(!super::instant_blur_disabled(&[clip], 2.051));
        clip.hidden = true;
        assert!(!super::instant_blur_disabled(&[clip], 1.5));
    }

    #[test]
    fn an_auto_zoom_springs_back_after_its_last_frame() {
        let clips = [auto_clip(1.0, 2.0, 3.0, (320.0, 240.0))];
        let sidecar = sidecar((640, 480), &[(1.0, 500.0, 240.0), (1.9, 500.0, 240.0)]);
        assert!(
            evaluate(&clips, &sidecar, (640.0, 480.0), 2.0).0 > 2.5,
            "the clip must still be zoomed on its last frame"
        );
        let just_after = evaluate(&clips, &sidecar, (640.0, 480.0), 2.05).0;
        assert!(
            just_after > 1.5 && just_after < 3.0,
            "the exit must travel on the spring, got {just_after}"
        );
        assert!(
            evaluate(&clips, &sidecar, (640.0, 480.0), 2.8).0 < 1.05,
            "the spring must finish returning to the full frame"
        );
    }

    #[test]
    fn a_short_clip_springs_in_and_does_not_cut_at_its_end() {
        let clips = [auto_clip(1.0, 1.8, 3.0, (320.0, 240.0))];
        let sidecar = sidecar((640, 480), &[]);
        let frame = (640.0, 480.0);
        let entering = evaluate(&clips, &sidecar, frame, 1.2).0;
        assert!(entering > 1.0 && entering < 3.2, "got {entering}");
        let last = evaluate(&clips, &sidecar, frame, 1.8).0;
        assert!(last > 2.5, "still zoomed on its last frame, got {last}");
        let after = evaluate(&clips, &sidecar, frame, 1.81).0;
        assert!(
            after > 2.0,
            "the end springs back instead of cutting, got {after}"
        );
    }

    #[test]
    fn hidden_clips_do_not_move_the_camera() {
        let mut hidden = auto_clip(1.0, 4.0, 3.0, (50.0, 70.0));
        hidden.hidden = true;
        let sidecar = sidecar((640, 480), &[]);
        assert_eq!(
            evaluate(&[hidden], &sidecar, (640.0, 480.0), 2.0),
            (1.0, (320.0, 240.0))
        );
    }

    #[test]
    fn pointer_follow_settles_on_the_recorded_position() {
        let clips = [auto_clip(0.5, 4.0, 2.0, (320.0, 240.0))];
        let sidecar = sidecar(
            (640, 480),
            &[
                (0.5, 400.0, 240.0),
                (1.0, 400.0, 240.0),
                (3.0, 400.0, 240.0),
            ],
        );
        let (scale, center) = evaluate(&clips, &sidecar, (640.0, 480.0), 2.5);
        assert!((scale - 2.0).abs() < 0.03, "scale={scale}");
        assert!((center.0 - 400.0).abs() < 10.0, "center={center:?}");
        assert!((center.1 - 240.0).abs() < 10.0, "center={center:?}");
    }

    #[test]
    fn pointer_coordinates_are_mapped_to_encoded_pixels() {
        let clips = [auto_clip(0.5, 4.0, 2.0, (320.0, 180.0))];
        let mut sidecar = sidecar((1000, 500), &[(0.5, 750.0, 250.0), (3.0, 750.0, 250.0)]);
        sidecar.region.w = 1000;
        sidecar.region.h = 500;
        let (scale, center) = evaluate(&clips, &sidecar, (640.0, 360.0), 2.5);
        assert!((scale - 2.0).abs() < 0.03, "scale={scale}");
        assert!((center.0 - 480.0).abs() < 10.0, "center={center:?}");
        assert!((center.1 - 180.0).abs() < 10.0, "center={center:?}");
    }

    #[test]
    fn pointer_chase_never_exposes_pixels_past_any_frame_edge() {
        let frame = (640.0, 480.0);
        for (x, y, right_edge, bottom_edge) in [
            (0.0, 0.0, false, false),
            (640.0, 0.0, true, false),
            (0.0, 480.0, false, true),
            (640.0, 480.0, true, true),
        ] {
            let clips = [auto_clip(0.5, 4.0, 2.5, (320.0, 240.0))];
            let sidecar = sidecar((640, 480), &[(0.5, x, y), (3.0, x, y)]);
            let (scale, center) = evaluate(&clips, &sidecar, frame, 2.0);
            let (left, top, width, height) = view_rect(scale, center, frame);
            assert!(
                left >= 0.0 && top >= 0.0,
                "view={left},{top},{width},{height}"
            );
            assert!(
                left + width <= frame.0 + 1e-9,
                "view={left},{top},{width},{height}"
            );
            assert!(
                top + height <= frame.1 + 1e-9,
                "view={left},{top},{width},{height}"
            );
            if right_edge {
                assert!((left + width - frame.0).abs() < 1e-8);
            }
            if bottom_edge {
                assert!((top + height - frame.1).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn cached_frames_make_out_of_order_evaluations_deterministic() {
        let clips = [auto_clip(0.5, 4.0, 2.2, (320.0, 240.0))];
        let sidecar = sidecar(
            (640, 480),
            &[
                (0.5, 430.0, 210.0),
                (1.2, 500.0, 250.0),
                (2.0, 550.0, 200.0),
                (3.0, 400.0, 320.0),
            ],
        );
        let times = [2.5, 1.25, 3.4, 0.75, 1.25];
        let mut cache = ZoomCameraCache::default();
        let actual: Vec<_> = times
            .iter()
            .map(|&time| {
                cache.evaluate(
                    &clips,
                    (640.0, 480.0),
                    &TimeMapping::default(),
                    ZoomCameraSettings::default(),
                    time,
                    || Some(sidecar.clone()),
                )
            })
            .collect();
        for (time, result) in times.into_iter().zip(actual) {
            assert_eq!(
                result,
                evaluate(&clips, &sidecar, (640.0, 480.0), time),
                "time={time}"
            );
        }
    }

    #[test]
    fn clips_without_pointer_samples_keep_the_stored_centre_even_with_clicks() {
        let clips = [auto_clip(1.0, 4.0, 3.0, (100.0, 300.0))];
        let mut sidecar = sidecar((640, 480), &[]);
        sidecar.clicks.push(ClickSample {
            t: 2.0,
            x: 100.0,
            y: 300.0,
            button: 1,
            cursor_id: None,
        });
        let frame = (640.0, 480.0);
        let expected = super::frame_for_rect(super::focus_rect(3.0, (100.0, 300.0), frame), frame);
        let actual = evaluate(&clips, &sidecar, frame, 3.5);
        assert!(
            (actual.0 - expected.0).abs() < 0.01,
            "{actual:?} vs {expected:?}"
        );
        assert!(
            (actual.1 .0 - expected.1 .0).abs() < 0.5,
            "{actual:?} vs {expected:?}"
        );
        assert!(
            (actual.1 .1 - expected.1 .1).abs() < 0.5,
            "{actual:?} vs {expected:?}"
        );
    }

    #[test]
    fn a_trimmed_head_follows_the_footage_under_the_clip_window() {
        let clips = [auto_clip(0.0, 2.0, 2.0, (320.0, 240.0))];
        let sidecar = sidecar((640, 480), &[(4.0, 520.0, 240.0), (5.5, 520.0, 240.0)]);
        let mapping = TimeMapping::new(vec![(0.0, 2.0, 3.5, 5.5)]);
        let mut cache = ZoomCameraCache::default();
        let (_, center) = cache.evaluate(
            &clips,
            (640.0, 480.0),
            &mapping,
            ZoomCameraSettings::default(),
            1.2,
            || Some(sidecar.clone()),
        );
        assert!((center.0 - 480.0).abs() < 10.0, "center={center:?}");
    }

    #[test]
    fn cache_rebuilds_when_clips_change_or_a_document_is_cleared() {
        let frame = (640.0, 480.0);
        let clips = [auto_clip(0.5, 4.0, 2.0, (320.0, 240.0))];
        let first_sidecar = sidecar((640, 480), &[(0.5, 500.0, 240.0), (3.0, 500.0, 240.0)]);
        let second_sidecar = sidecar((640, 480), &[(0.5, 120.0, 240.0), (3.0, 120.0, 240.0)]);
        let mut cache = ZoomCameraCache::default();
        let identity = TimeMapping::default();
        let first = cache.evaluate(
            &clips,
            frame,
            &identity,
            ZoomCameraSettings::default(),
            2.0,
            || Some(first_sidecar.clone()),
        );
        let changed_clip = [auto_clip(0.5, 4.0, 3.0, (320.0, 240.0))];
        let changed = cache.evaluate(
            &changed_clip,
            frame,
            &identity,
            ZoomCameraSettings::default(),
            2.0,
            || Some(first_sidecar.clone()),
        );
        assert!(changed.0 > first.0 + 0.5);
        cache.clear();
        let changed_document = cache.evaluate(
            &clips,
            frame,
            &identity,
            ZoomCameraSettings::default(),
            2.0,
            || Some(second_sidecar.clone()),
        );
        assert!(changed_document.1 .0 < first.1 .0 - 100.0);
    }

    #[test]
    fn view_rect_clamps_to_each_frame_edge() {
        let frame = (640.0, 480.0);
        assert_eq!(
            view_rect(2.0, (-100.0, 240.0), frame),
            (0.0, 120.0, 320.0, 240.0)
        );
        assert_eq!(
            view_rect(2.0, (1000.0, 240.0), frame),
            (320.0, 120.0, 320.0, 240.0)
        );
        assert_eq!(
            view_rect(2.0, (320.0, -100.0), frame),
            (160.0, 0.0, 320.0, 240.0)
        );
        assert_eq!(
            view_rect(2.0, (320.0, 1000.0), frame),
            (160.0, 240.0, 320.0, 240.0)
        );
    }

    #[test]
    fn view_center_maps_to_widget_center_and_transform_matches() {
        let frame = (640.0, 480.0);
        let view = view_rect(2.0, (320.0, 240.0), frame);
        assert_eq!(
            source_to_stage((320.0, 240.0), view, 640.0, 480.0),
            (320.0, 240.0)
        );
        assert_eq!(
            camera_transform(view, 640.0, 480.0, 640.0, 480.0),
            (-320.0, -240.0, 2.0, 2.0)
        );
    }

    #[test]
    fn auto_zooms_within_the_morph_gap_share_one_continuous_framing() {
        let frame = (640.0, 480.0);
        let clips = [
            auto_clip(0.0, 1.5, 2.0, (400.0, 240.0)),
            auto_clip(1.9, 3.5, 3.0, (300.0, 240.0)),
        ];
        assert!(
            (static_camera_at(&clips, 1.0, frame).0 - 2.0).abs() < 1e-9,
            "the first clip holds its framing into the gap"
        );
        let gap = static_camera_at(&clips, 1.7, frame);
        assert!((gap.0 - 2.0).abs() < 1e-9, "gap framing: {gap:?}");
        assert_eq!(gap.1, (400.0, 240.0));
        let entry = static_camera_at(&clips, 1.9, frame);
        assert!(
            (entry.0 - 2.0).abs() < 1e-9,
            "no full frame at the next clip: {entry:?}"
        );
        assert_eq!(entry.1, (400.0, 240.0));
        assert_eq!(static_camera_at(&clips, 2.75, frame), (3.0, (300.0, 240.0)));

        let mut previous = static_camera_at(&clips, 1.4, frame);
        for step in 1..=80 {
            let time = 1.4 + f64::from(step) * 0.01;
            let current = static_camera_at(&clips, time, frame);
            assert!(
                current.0 > 1.9 || time >= 2.5,
                "the framing dropped to full between the clips at {time}: {current:?}"
            );
            assert!(
                (current.1 .0 - previous.1 .0).abs() < 15.0,
                "centre jumped at {time}: {previous:?} -> {current:?}"
            );
            previous = current;
        }
    }

    #[test]
    fn auto_zooms_beyond_the_morph_gap_do_not_morph() {
        let frame = (640.0, 480.0);
        let clips = [
            auto_clip(0.0, 1.5, 2.0, (400.0, 240.0)),
            auto_clip(2.3, 4.5, 3.0, (300.0, 240.0)),
        ];
        assert_eq!(static_camera_at(&clips, 1.5, frame).0, 2.0);
        assert_eq!(static_camera_at(&clips, 1.51, frame), (1.0, (320.0, 240.0)));
        assert_eq!(static_camera_at(&clips, 1.9, frame), (1.0, (320.0, 240.0)));
        assert_eq!(static_camera_at(&clips, 2.3, frame), (1.0, (300.0, 240.0)));
        assert_eq!(static_camera_at(&clips, 3.2, frame), (3.0, (300.0, 240.0)));
    }

    #[test]
    fn the_follow_camera_holds_through_the_morph_gap() {
        let frame = (640.0, 480.0);
        let clips = [
            auto_clip(0.5, 2.0, 2.0, (320.0, 240.0)),
            auto_clip(2.4, 4.0, 3.0, (320.0, 240.0)),
        ];
        let sidecar = sidecar(
            (640, 480),
            &[
                (0.5, 400.0, 240.0),
                (2.0, 400.0, 240.0),
                (2.4, 300.0, 240.0),
                (4.0, 300.0, 240.0),
            ],
        );
        let in_gap = evaluate(&clips, &sidecar, frame, 2.2);
        assert!(
            in_gap.0 > 1.8,
            "gap must not fall back to full frame: {in_gap:?}"
        );
        let after_entry = evaluate(&clips, &sidecar, frame, 2.45);
        assert!(
            after_entry.0 > 1.8,
            "the next clip must continue from the held framing: {after_entry:?}"
        );
    }

    #[test]
    fn a_manual_clip_never_morphs_in_either_direction() {
        let frame = (640.0, 480.0);
        let auto_then_manual = [
            auto_clip(0.0, 1.5, 2.0, (400.0, 240.0)),
            manual_clip(1.9, 3.5, 3.0, (300.0, 240.0)),
        ];
        assert_eq!(
            static_camera_at(&auto_then_manual, 1.9, frame),
            (1.0, (300.0, 240.0))
        );
        assert!(
            (static_camera_at(&auto_then_manual, 1.4, frame).0 - 2.0).abs() < 0.2,
            "the auto clip holds its own framing to its end: {:?}",
            static_camera_at(&auto_then_manual, 1.4, frame)
        );
        assert_eq!(
            static_camera_at(&auto_then_manual, 1.6, frame),
            (1.0, (320.0, 240.0)),
            "a manual neighbour must not hold the auto clip past its end"
        );

        let manual_then_auto = [
            manual_clip(0.0, 1.5, 2.0, (400.0, 240.0)),
            auto_clip(1.9, 3.5, 3.0, (300.0, 240.0)),
        ];
        assert_eq!(
            static_camera_at(&manual_then_auto, 1.9, frame),
            (1.0, (300.0, 240.0))
        );
        assert_eq!(
            static_camera_at(&manual_then_auto, 1.7, frame),
            (1.0, (320.0, 240.0))
        );
    }

    #[test]
    fn the_dwell_centre_holds_where_the_pointer_lingered() {
        let clips = [auto_clip(0.5, 4.5, 2.0, (320.0, 240.0))];
        let mut points = Vec::new();
        let mut time = 0.5;
        while time < 2.5 {
            points.push((time, 200.0, 240.0));
            time += 0.1;
        }
        points.push((2.6, 210.0, 240.0));
        points.push((2.7, 330.0, 240.0));
        while time < 3.5 {
            points.push((time, 330.0, 240.0));
            time += 0.1;
        }
        let sidecar = sidecar((640, 480), &points);
        let (scale, center) = evaluate(&clips, &sidecar, (640.0, 480.0), 3.3);
        assert!((scale - 2.0).abs() < 0.03, "scale={scale}");
        assert!(
            center.0 < 225.0,
            "the dwell centre must sit near the linger point, not the sweep: {center:?}"
        );
        let sweep_midpoint = (200.0 + 330.0) / 2.0;
        assert!(
            (center.0 - sweep_midpoint).abs() > (center.0 - 200.0).abs(),
            "camera settled mid-sweep: {center:?}"
        );
    }

    #[test]
    fn the_focus_outline_shows_the_region_a_manual_zoom_will_frame() {
        let video = [0.0, 0.0, 640.0, 480.0];
        assert_eq!(
            placement_rect(2.0, (320.0, 240.0), (0.0, 0.0, 640.0, 480.0), video),
            [160.0, 120.0, 320.0, 240.0]
        );
        // A corner focus point keeps the outline inside the picture.
        assert_eq!(
            placement_rect(2.0, (0.0, 0.0), (0.0, 0.0, 640.0, 480.0), video),
            [0.0, 0.0, 320.0, 240.0]
        );
        // At 1x the outline is the whole picture.
        assert_eq!(
            placement_rect(1.0, (400.0, 300.0), (0.0, 0.0, 640.0, 480.0), video),
            [0.0, 0.0, 640.0, 480.0]
        );
    }

    #[test]
    fn the_focus_outline_maps_through_a_letterboxed_video_rect() {
        // A portrait video inside a landscape widget: 270x480 centred.
        let video = [185.0, 0.0, 270.0, 480.0];
        let view = (0.0, 0.0, 270.0, 480.0);
        let (x, y) = stage_point_to_source((185.0 + 135.0, 240.0), view, video);
        assert!((x - 135.0).abs() < 1e-9, "x={x}");
        assert!((y - 240.0).abs() < 1e-9, "y={y}");
        assert_eq!(
            placement_rect(2.0, (135.0, 240.0), view, video),
            [252.5, 120.0, 135.0, 240.0]
        );
    }

    #[test]
    fn an_instant_zoom_snaps_with_no_ramp_at_either_end() {
        let mut clip = auto_clip(1.0, 4.0, 3.0, (320.0, 240.0));
        clip.instant = true;
        let sidecar = sidecar((640, 480), &[]);
        assert_eq!(
            evaluate(&[clip], &sidecar, (640.0, 480.0), 1.0),
            (3.0, (320.0, 240.0))
        );
        assert_eq!(
            evaluate(&[clip], &sidecar, (640.0, 480.0), 2.5),
            (3.0, (320.0, 240.0))
        );
        assert_eq!(
            evaluate(&[clip], &sidecar, (640.0, 480.0), 4.1),
            (1.0, (320.0, 240.0))
        );
    }

    #[test]
    fn an_instant_zoom_does_not_chase_the_pointer_on_the_spring() {
        let frame = (640.0, 480.0);
        let points = &[(1.0, 500.0, 300.0), (3.0, 500.0, 300.0)];
        let mut instant = auto_clip(1.0, 4.0, 2.0, (320.0, 240.0));
        instant.instant = true;
        let eased = auto_clip(1.0, 4.0, 2.0, (320.0, 240.0));
        let instant_sidecar = sidecar((640, 480), points);
        let eased_sidecar = sidecar((640, 480), points);
        let snapped = evaluate(&[instant], &instant_sidecar, frame, 1.1);
        let travelling = evaluate(&[eased], &eased_sidecar, frame, 1.1);
        assert!(
            (snapped.0 - 2.0).abs() < 1e-9,
            "an instant zoom is at its scale on the first frame: {snapped:?}"
        );
        assert!(
            travelling.0 < 1.9,
            "an animated zoom is still travelling there: {travelling:?}"
        );
    }

    #[test]
    fn the_camera_spring_follows_its_speed_and_smoothness() {
        let frame = (640.0, 480.0);
        let clips = [auto_clip(1.0, 4.0, 2.0, (320.0, 240.0))];
        let sidecar = sidecar((640, 480), &[(1.0, 500.0, 300.0), (3.0, 500.0, 300.0)]);
        let settle = |settings: ZoomCameraSettings| {
            let mut cache = ZoomCameraCache::default();
            cache
                .evaluate(
                    &clips,
                    frame,
                    &TimeMapping::default(),
                    settings,
                    1.25,
                    || Some(sidecar.clone()),
                )
                .0
        };
        let fast = settle(ZoomCameraSettings {
            speed: 3.0,
            ..ZoomCameraSettings::default()
        });
        let slow = settle(ZoomCameraSettings {
            speed: 0.5,
            ..ZoomCameraSettings::default()
        });
        assert!(
            fast > slow + 0.05,
            "a faster camera must be closer to its target by the same instant: fast={fast} slow={slow}"
        );
    }

    #[test]
    fn the_early_lead_in_starts_the_move_before_the_clip() {
        let frame = (640.0, 480.0);
        let clips = [auto_clip(2.0, 4.0, 2.0, (320.0, 240.0))];
        let sidecar = sidecar((640, 480), &[(2.0, 500.0, 240.0), (3.5, 500.0, 240.0)]);
        let at = |settings: ZoomCameraSettings| {
            let mut cache = ZoomCameraCache::default();
            cache
                .evaluate(
                    &clips,
                    frame,
                    &TimeMapping::default(),
                    settings,
                    1.8,
                    || Some(sidecar.clone()),
                )
                .0
        };
        assert_eq!(at(ZoomCameraSettings::default()), 1.0);
        let early = at(ZoomCameraSettings {
            start_early: true,
            ..ZoomCameraSettings::default()
        });
        assert!(
            early > 1.02,
            "the lead-in must have moved the camera before the clip starts: {early}"
        );
    }
}
