//! The webcam layer over a recording.
//!
//! The Windows recorder writes the camera to its own track and never into the
//! footage, so the layer is rebuilt here: the camera's frames decoded on
//! demand, placed in the frame, and drawn with a corner radius and a mirror.
//! The placement starts from what the recording captured — the sidecar's
//! fractional rect and shape — and the editor's panel can change it afterwards,
//! which the project file remembers.
//!
//! The same layer carries the five camera layouts: the placement bubble, a
//! fullscreen camera, a split screen, the screen alone, and a bottom-aligned
//! cutout of the person over the screen. A layout is chosen by cue in
//! composition time, so trimming or reordering the footage moves the picture
//! under a cue but never moves the cue on its own clock. Transitions between
//! layouts are smoothstepped and evaluated from the state on screen at the
//! cue, which keeps rapid cuts continuous and a seek deterministic.
//!
//! The preview and the export share the geometry in this file, which is what
//! keeps what the editor shows and what it writes the same picture.
//!
//! The camera's clock is not the desktop's — a busy machine makes the recorder
//! drop camera frames without touching the screen's timeline — so a video
//! instant is mapped onto the track by its recorded length. The track freezes
//! on its last frame past the end, the way a still camera would.

use crate::recording::editor::sidecar::{PointerSidecar, WebcamTrack};
use gtk4::cairo::{Context, Format, ImageSurface};
use gtk4::glib;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::cutout::CutoutProcessor;

/// How close two requests have to be before the second one is the same frame.
const REQUEST_STEP_SECONDS: f64 = 1.0 / 60.0;
/// How long the still preview waits for a decode before repainting.
const STILL_REPAINT_DELAY: Duration = Duration::from_millis(120);
/// The preview decodes at this width; the bubble is never larger on screen.
const PREVIEW_WIDTH: u32 = 720;
/// A portrait camera decodes taller than it is wide, up to this edge.
const PREVIEW_MAX_HEIGHT: u32 = 1280;
/// The smallest bubble the panel can make, as a fraction of the frame width.
pub(super) const MIN_SIZE: f64 = 0.06;
/// The largest bubble that still reads as an overlay.
pub(super) const MAX_SIZE: f64 = 0.9;
/// The shortest studied silence the camera can hide in.
pub(super) const MIN_SILENCE_SECONDS: f64 = 2.0;
/// The longest silence the camera can hide in.
pub(super) const MAX_SILENCE_SECONDS: f64 = 15.0;
/// The studied length a freshly switched-on hide uses.
pub(super) const DEFAULT_SILENCE_SECONDS: f64 = 5.0;
/// The studied overlay size range: the camera's longest side as a fraction
/// of the frame's shortest side.
pub(super) const MIN_OVERLAY_SIZE_RATIO: f64 = 0.1;
pub(super) const MAX_OVERLAY_SIZE_RATIO: f64 = 0.75;
/// The studied overlay aspect choices, width over height.
pub(super) const OVERLAY_ASPECTS: [(&str, f64); 5] = [
    ("16:9", 16.0 / 9.0),
    ("4:3", 4.0 / 3.0),
    ("1:1", 1.0),
    ("3:4", 3.0 / 4.0),
    ("9:16", 9.0 / 16.0),
];

/// The studied overlay sizing: a square of `size_ratio` times the frame's
/// shortest side, with the camera's aspect (width over height) fitted inside
/// it. Returns the camera's width and height as fractions of the frame.
pub(super) fn overlay_size_hw(
    frame_aspect: f64,
    camera_aspect: f64,
    size_ratio: f64,
) -> (f64, f64) {
    let frame = if frame_aspect.is_finite() && frame_aspect > 0.0 {
        frame_aspect
    } else {
        16.0 / 9.0
    };
    let camera = if camera_aspect.is_finite() && camera_aspect > 0.0 {
        camera_aspect
    } else {
        1.0
    };
    let ratio = if size_ratio.is_finite() {
        size_ratio.max(0.0)
    } else {
        0.0
    };
    // The shortest side over the width, and over the height.
    let short_over_width = 1.0 / frame.max(1.0);
    let short_over_height = frame.min(1.0);
    (
        ratio * short_over_width * camera.min(1.0),
        ratio * short_over_height * (1.0 / camera).min(1.0),
    )
}

/// How long a layout transition takes when the project has not said.
const DEFAULT_TRANSITION_SECONDS: f64 = 0.45;
/// The longest transition a project may ask for.
const MAX_TRANSITION_SECONDS: f64 = 3.0;
/// The narrowest share of the split the screen may take.
pub(super) const MIN_SPLIT_RATIO: f64 = 0.1;
/// The widest share of the split the screen may take.
pub(super) const MAX_SPLIT_RATIO: f64 = 0.9;
/// The smallest share of the frame's shortest side the cutout may take.
pub(super) const MIN_CUTOUT_SIZE: f64 = 0.2;
/// The largest share of the frame's shortest side the cutout may take.
pub(super) const MAX_CUTOUT_SIZE: f64 = 1.0;
/// The furthest the person's edge may be pulled in, in camera pixels.
pub(super) const MAX_CUTOUT_INSET: f64 = 8.0;

/// The studied camera size while the screen is zoomed.
pub(super) const ZOOMED_CAMERA_SCALE: f64 = 0.8;
/// The smallest studied size during a zoom.
pub(super) const MIN_ZOOMED_CAMERA_SCALE: f64 = 0.25;
/// The studied split-screen camera while zoomed: an overlay at this size,
/// inset from the frame edges by this fraction of its shortest side (the
/// studied 0.5 padding setting resolved against the default stage padding),
/// with its aspect held inside the studied 9:16..16:9 range.
const SPLIT_ZOOMED_CAMERA_SIZE: f64 = 0.5;
const SPLIT_ZOOMED_CAMERA_INSET: f64 = 0.0625;
const SPLIT_ZOOMED_MIN_ASPECT: f64 = 9.0 / 16.0;
const SPLIT_ZOOMED_MAX_ASPECT: f64 = 16.0 / 9.0;
/// The studied spring that moves the camera between its sizes, as
/// (stiffness, damping, mass), run with the studied clamp.
const CAMERA_SPRING: (f64, f64, f64) = (125.0, 12.0, 1.5);
const CAMERA_SPRING_PRECISION: f64 = 0.002;
/// How much history the spring replays before an instant; it has settled
/// well under its precision within it.
const CAMERA_SPRING_HISTORY_SECONDS: f64 = 2.0;

/// A rectangle in whichever units its caller works in: fractions of the frame
/// for layout math, pixels for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// The whole frame, in normalized layout units.
    pub(super) fn full() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    pub(super) fn is_full(self) -> bool {
        self.x.abs() <= 1e-6
            && self.y.abs() <= 1e-6
            && (self.width - 1.0).abs() <= 1e-6
            && (self.height - 1.0).abs() <= 1e-6
    }

    fn lerp(self, other: Self, t: f64) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            width: self.width + (other.width - self.width) * t,
            height: self.height + (other.height - self.height) * t,
        }
    }

    /// The rect inside `video`, in the same units as `video`.
    fn inside(self, video: Rect) -> Rect {
        Rect {
            x: video.x + self.x * video.width,
            y: video.y + self.y * video.height,
            width: self.width * video.width,
            height: self.height * video.height,
        }
    }
}

/// Where the camera sits in the frame and what shape it is cut to.
///
/// Fractions of the frame, so the layer renders the same at any output size.
/// `radius` is a fraction of the shorter side: zero is a rectangle, one is an
/// ellipse inscribed in the rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LayerSettings {
    pub enabled: bool,
    /// The camera fills the frame instead of sitting in a bubble.
    pub fullscreen: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub mirror: bool,
    /// Manual camera sync, in seconds: positive shows the camera later.
    pub delay_seconds: f64,
    /// The camera's size while the screen is zoomed, as a fraction of its size; 1 turns the studied shrink off.
    pub zoomed_scale: f64,
    /// Hide the camera while the audio is silent for at least this many seconds; None is off.
    pub hide_during_silence: Option<f64>,
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            fullscreen: false,
            x: 0.7,
            y: 0.55,
            w: 0.2,
            h: 0.25,
            radius: 0.12,
            mirror: false,
            delay_seconds: 0.0,
            zoomed_scale: ZOOMED_CAMERA_SCALE,
            hide_during_silence: None,
        }
    }
}

impl LayerSettings {
    /// Start from what the recording captured, so the editor opens on the
    /// bubble the recording showed.
    pub fn from_track(track: &WebcamTrack) -> Self {
        let (x, y, w, h) = track.rect();
        Self {
            enabled: true,
            fullscreen: false,
            x,
            y,
            w,
            h,
            radius: radius_for_shape(track.shape),
            mirror: track.flip,
            delay_seconds: 0.0,
            zoomed_scale: ZOOMED_CAMERA_SCALE,
            hide_during_silence: None,
        }
    }

    /// Every value pulled back inside the frame, so a hand-edited project
    /// cannot place the layer off the picture.
    pub fn clamped(mut self) -> Self {
        self.w = self.w.clamp(MIN_SIZE, MAX_SIZE);
        self.h = self.h.clamp(MIN_SIZE, MAX_SIZE);
        self.x = self.x.clamp(0.0, 1.0 - self.w);
        self.y = self.y.clamp(0.0, 1.0 - self.h);
        self.radius = self.radius.clamp(0.0, 1.0);
        self.delay_seconds = if self.delay_seconds.is_finite() {
            self.delay_seconds
                .clamp(-MAX_CAMERA_DELAY_SECONDS, MAX_CAMERA_DELAY_SECONDS)
        } else {
            0.0
        };
        self.zoomed_scale = if self.zoomed_scale.is_finite() {
            self.zoomed_scale.clamp(MIN_ZOOMED_CAMERA_SCALE, 1.0)
        } else {
            ZOOMED_CAMERA_SCALE
        };
        self.hide_during_silence = self
            .hide_during_silence
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(MIN_SILENCE_SECONDS, MAX_SILENCE_SECONDS));
        self
    }

    /// The layer inside a video rectangle, in the same units as the rectangle.
    pub fn rect_in(&self, video: Rect) -> Rect {
        if self.fullscreen {
            return video;
        }
        Rect {
            x: video.x + self.x * video.width,
            y: video.y + self.y * video.height,
            width: self.w * video.width,
            height: self.h * video.height,
        }
    }

    /// The corner radius in pixels for a rect.
    pub fn radius_px(&self, rect: Rect) -> f64 {
        if self.fullscreen {
            return 0.0;
        }
        self.radius * rect.width.min(rect.height) / 2.0
    }

    /// Where the layer sits in the travel its size leaves: `(0, 0)` against the
    /// top-left edges, `(1, 1)` against the bottom-right ones, and half way in
    /// between.
    pub(super) fn overlay_alignment(&self) -> (f64, f64) {
        let sane = self.clamped();
        (
            travel_fraction(sane.x, 1.0 - sane.w),
            travel_fraction(sane.y, 1.0 - sane.h),
        )
    }

    /// Put the layer at a fraction of the travel its size leaves, so an anchor
    /// keeps the same edge or centre it was clicked for.
    pub(super) fn set_overlay_alignment(&mut self, x: f64, y: f64) {
        *self = self.clamped();
        self.x = travel_offset(x, 1.0 - self.w);
        self.y = travel_offset(y, 1.0 - self.h);
    }

    /// Resize the layer to `width` and `width * height_over_width`, scaling
    /// both sides together when the shape's proportions would leave the size
    /// bounds, and leave it at the alignment it had.
    ///
    /// The height over width is a pixel ratio — a shape's own height over its
    /// width — so a resize keeps the bubble's proportions in the frame's
    /// pixels rather than in normalized fractions.
    pub(super) fn resize_overlay(&mut self, width: f64, height_over_width: f64) {
        let sane = self.clamped();
        let (alignment_x, alignment_y) = sane.overlay_alignment();
        let aspect = if height_over_width.is_finite() && height_over_width > 0.0 {
            height_over_width.clamp(MIN_SIZE / MAX_SIZE, MAX_SIZE / MIN_SIZE)
        } else {
            1.0
        };
        let mut width = if width.is_finite() {
            width.clamp(MIN_SIZE, MAX_SIZE)
        } else {
            sane.w
        };
        let mut height = width * aspect;
        if height > MAX_SIZE {
            height = MAX_SIZE;
            width = height / aspect;
        }
        if height < MIN_SIZE {
            height = MIN_SIZE;
            width = height / aspect;
        }
        self.w = width;
        self.h = height;
        self.x = travel_offset(alignment_x, 1.0 - width);
        self.y = travel_offset(alignment_y, 1.0 - height);
    }

    /// The bubble's width over height in the frame's pixels.
    pub(super) fn pixel_aspect(&self, frame_aspect: f64) -> f64 {
        let frame = if frame_aspect.is_finite() && frame_aspect > 0.0 {
            frame_aspect
        } else {
            16.0 / 9.0
        };
        if self.w > 0.0 && self.h > 0.0 && self.w.is_finite() && self.h.is_finite() {
            self.w * frame / self.h
        } else {
            1.0
        }
    }

    /// The bubble's longest side as a fraction of the frame's shortest side.
    pub(super) fn overlay_size_ratio(&self, frame_aspect: f64) -> f64 {
        let frame = if frame_aspect.is_finite() && frame_aspect > 0.0 {
            frame_aspect
        } else {
            16.0 / 9.0
        };
        // Both sides in units of the frame height.
        let longest = (self.w * frame).max(self.h);
        let shortest = frame.min(1.0);
        if longest.is_finite() && longest > 0.0 {
            longest / shortest
        } else {
            0.0
        }
    }

    /// Resize to a studied size ratio at the bubble's current aspect, keeping
    /// its alignment.
    pub(super) fn set_overlay_size(&mut self, size_ratio: f64, frame_aspect: f64) {
        let aspect = self.pixel_aspect(frame_aspect);
        let (w, h) = overlay_size_hw(frame_aspect, aspect, size_ratio);
        if w > 0.0 && h > 0.0 {
            self.resize_overlay(w, h / w);
        }
    }

    /// Refit the bubble to an aspect (width over height in pixels) at the
    /// size it has, keeping its alignment.
    pub(super) fn set_overlay_aspect(&mut self, camera_aspect: f64, frame_aspect: f64) {
        let ratio = self.overlay_size_ratio(frame_aspect);
        let (w, h) = overlay_size_hw(frame_aspect, camera_aspect, ratio);
        if w > 0.0 && h > 0.0 {
            self.resize_overlay(w, h / w);
        }
    }

    /// The layer at `factor` of its size, still at the alignment it had, so
    /// an anchored corner stays where it is.
    pub(super) fn scaled_in_place(self, factor: f64) -> Self {
        let sane = self.clamped();
        if !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return sane;
        }
        let (alignment_x, alignment_y) = sane.overlay_alignment();
        let mut scaled = sane;
        scaled.w = sane.w * factor;
        scaled.h = sane.h * factor;
        scaled.x = travel_offset(alignment_x, 1.0 - scaled.w);
        scaled.y = travel_offset(alignment_y, 1.0 - scaled.h);
        scaled
    }
}

/// How far along its travel a layer sits, as a 0..=1 fraction. A layer with no
/// travel, or unreadable values, reads as centred.
fn travel_fraction(offset: f64, travel: f64) -> f64 {
    if !offset.is_finite() || !travel.is_finite() || travel.abs() <= 1e-9 {
        return 0.5;
    }
    (offset / travel).clamp(0.0, 1.0)
}

/// The offset that puts a layer at `alignment` of its travel.
fn travel_offset(alignment: f64, travel: f64) -> f64 {
    let alignment = if alignment.is_finite() {
        alignment.clamp(0.0, 1.0)
    } else {
        0.5
    };
    alignment * travel.max(0.0)
}

/// One of the five ways a recording can frame its camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CameraLayout {
    /// The camera's bubble over the screen, placed by the layer settings.
    #[default]
    CameraOverlay,
    /// The camera alone, filling the frame.
    FullscreenCamera,
    /// Screen and camera side by side.
    SplitScreen,
    /// The screen alone, with no camera pixels.
    ScreenOnly,
    /// The recorded person, cut out of the camera and set on the screen.
    CameraCutout,
}

/// A layout change at an instant of composition time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct LayoutCue {
    /// Composition seconds, where the composition starts at zero.
    pub at: f64,
    pub layout: CameraLayout,
    /// The bubble's alignment for this change, overriding the shared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
    /// The cutout's position across the frame for this change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutout_x: Option<f64>,
}

/// Every layout the composition uses, and how it moves between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct LayoutSettings {
    pub default_layout: CameraLayout,
    pub cues: Vec<LayoutCue>,
    /// Any non-zero value moves the change on the studied layout spring; zero
    /// cuts.
    pub transition_seconds: f64,
    /// The screen's share of the split's long axis, in 0.1..=0.9.
    pub split_ratio: f64,
    /// Sizes the split the studied way instead of by `split_ratio`: the
    /// screen keeps its shape at full height when the camera column that
    /// leaves is wide enough. On by default; older projects read it as on.
    pub split_auto: bool,
    /// Puts the screen on the far side of the split.
    pub split_flip: bool,
    /// The studied Fill option: false fills the stage edge to edge, true
    /// leaves the Background padding between the screen and the camera and
    /// rounds both panes.
    pub split_padded: bool,
    /// Where the cutout sits across the frame's width, in 0..=1: zero holds
    /// the person against the left edge, one against the right, and half
    /// centres them. It applies to every cutout in the composition.
    pub cutout_position_x: f64,
    /// The cutout's size: a square of this fraction of the frame's shortest
    /// side, the camera fitted inside it. The studied range is 0.2..=1.
    pub cutout_size: f64,
    /// How soft the person's edge is, 0 hard to 1 softest.
    pub cutout_softness: f64,
    /// How far the person's edge is pulled in, in camera pixels.
    pub cutout_inset: f64,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            default_layout: CameraLayout::CameraOverlay,
            cues: Vec::new(),
            transition_seconds: DEFAULT_TRANSITION_SECONDS,
            split_ratio: 0.5,
            split_auto: true,
            split_flip: false,
            split_padded: false,
            cutout_position_x: 0.5,
            cutout_size: 0.5,
            cutout_softness: 0.2,
            cutout_inset: 0.0,
        }
    }
}

impl LayoutSettings {
    /// Every value pulled back into a range the renderer can draw: cues finite,
    /// non-negative, ordered and one per instant; the transition and split
    /// share inside their bounds, and the cutout placement across the frame.
    pub fn clamped(mut self) -> Self {
        self.transition_seconds = if self.transition_seconds.is_finite() {
            self.transition_seconds.clamp(0.0, MAX_TRANSITION_SECONDS)
        } else {
            DEFAULT_TRANSITION_SECONDS
        };
        self.split_ratio = if self.split_ratio.is_finite() {
            self.split_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
        } else {
            LayoutSettings::default().split_ratio
        };
        self.cutout_position_x = if self.cutout_position_x.is_finite() {
            self.cutout_position_x.clamp(0.0, 1.0)
        } else {
            LayoutSettings::default().cutout_position_x
        };
        self.cutout_size = if self.cutout_size.is_finite() {
            self.cutout_size.clamp(MIN_CUTOUT_SIZE, MAX_CUTOUT_SIZE)
        } else {
            LayoutSettings::default().cutout_size
        };
        self.cutout_softness = if self.cutout_softness.is_finite() {
            self.cutout_softness.clamp(0.0, 1.0)
        } else {
            LayoutSettings::default().cutout_softness
        };
        self.cutout_inset = if self.cutout_inset.is_finite() {
            self.cutout_inset.clamp(0.0, MAX_CUTOUT_INSET)
        } else {
            LayoutSettings::default().cutout_inset
        };

        let mut cues: Vec<LayoutCue> = self
            .cues
            .into_iter()
            .filter(|cue| cue.at.is_finite() && cue.at >= 0.0)
            .map(|mut cue| {
                cue.position = cue.position.and_then(|[x, y]| {
                    if x.is_finite() && y.is_finite() {
                        Some([x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)])
                    } else {
                        None
                    }
                });
                cue.cutout_x = cue.cutout_x.and_then(|x| {
                    if x.is_finite() {
                        Some(x.clamp(0.0, 1.0))
                    } else {
                        None
                    }
                });
                cue
            })
            .collect();
        cues.sort_by(|left, right| left.at.total_cmp(&right.at));
        let mut deduped: Vec<LayoutCue> = Vec::with_capacity(cues.len());
        for cue in cues {
            match deduped.last_mut() {
                Some(last) if (last.at - cue.at).abs() <= 1e-9 => *last = cue,
                _ => deduped.push(cue),
            }
        }
        self.cues = deduped;
        self
    }

    /// The layout the composition is heading to at this instant: the newest
    /// cue at or before it, else the default. During a transition this is its
    /// target, not the blended picture on screen.
    pub fn layout_at(&self, seconds: f64) -> CameraLayout {
        if !seconds.is_finite() {
            return self.default_layout;
        }
        self.cues
            .iter()
            .filter(|cue| cue.at.is_finite() && cue.at >= 0.0 && cue.at <= seconds)
            .max_by(|left, right| left.at.total_cmp(&right.at))
            .map(|cue| cue.layout)
            .unwrap_or(self.default_layout)
    }

    /// Whether any instant of the composition needs the person cut out.
    pub fn uses_cutout(&self) -> bool {
        self.default_layout == CameraLayout::CameraCutout
            || self
                .cues
                .iter()
                .any(|cue| cue.layout == CameraLayout::CameraCutout)
    }

    /// Whether any instant shows the camera at all. A composition that never
    /// leaves the screen alone has no camera pixels to bake.
    pub fn uses_camera(&self) -> bool {
        self.default_layout != CameraLayout::ScreenOnly
            || self
                .cues
                .iter()
                .any(|cue| cue.layout != CameraLayout::ScreenOnly)
    }

    /// The picture at `seconds`: layout, blended camera rect and opacity, and
    /// how much of the person's matte is showing.
    ///
    /// Cues are walked in order and each transition starts from the state the
    /// previous one had reached at that instant, so a cue during a transition
    /// continues from what is actually on screen instead of jumping to the
    /// older cue's target. No state is remembered between calls: a seek to a
    /// time and playback reaching it produce the same picture.
    pub(super) fn frame_at(
        &self,
        seconds: f64,
        overlay: LayerSettings,
        video_aspect: f64,
        camera_aspect: f64,
    ) -> LayoutFrame {
        self.frame_at_zoomed(seconds, overlay, video_aspect, camera_aspect, 0.0)
    }

    /// [`Self::frame_at`] while the screen's zoom has shrunk the camera by
    /// `zoom_shrink` (0..1, see [`zoom_shrink_at`]).
    pub(super) fn frame_at_zoomed(
        &self,
        seconds: f64,
        overlay: LayerSettings,
        video_aspect: f64,
        camera_aspect: f64,
        zoom_shrink: f64,
    ) -> LayoutFrame {
        self.frame_at_staged(
            seconds,
            overlay,
            StageShape::plain(video_aspect),
            camera_aspect,
            zoom_shrink,
        )
    }

    /// [`Self::frame_at_zoomed`] on `stage`, whose aspect may differ from the
    /// screen's. Rects are fractions of the stage.
    pub(super) fn frame_at_staged(
        &self,
        seconds: f64,
        overlay: LayerSettings,
        stage: StageShape,
        camera_aspect: f64,
        zoom_shrink: f64,
    ) -> LayoutFrame {
        let geometry = Geometry {
            overlay: overlay.clamped(),
            video_aspect: sane_aspect(stage.aspect),
            screen_aspect: sane_aspect(stage.screen_aspect),
            camera_aspect: sane_aspect(camera_aspect),
            split_ratio: self.split_ratio,
            split_auto: self.split_auto,
            split_flip: self.split_flip,
            split_gap: if self.split_padded {
                finite_non_negative(stage.split_gap)
            } else {
                0.0
            },
            pane_radius: if self.split_padded {
                finite_non_negative(stage.pane_radius)
            } else {
                0.0
            },
            cutout_position_x: self.cutout_position_x,
            cutout_size: self.cutout_size,
            zoom_shrink: if zoom_shrink.is_finite() {
                zoom_shrink.clamp(0.0, 1.0)
            } else {
                0.0
            },
        };
        // A layer switched off paints the screen alone at every instant. The
        // recorded geometry and the layout timeline stay as they are, so
        // switching it back on restores the picture that was there.
        if !geometry.overlay.enabled {
            return canonical(CameraLayout::ScreenOnly, geometry);
        }
        let default = canonical(self.default_layout, geometry);
        if !seconds.is_finite() {
            return default;
        }

        let mut from = default;
        let mut to = default;
        let mut started: Option<f64> = None;
        for (at, target) in self.targets(geometry, default) {
            if at > seconds {
                break;
            }
            if target == to {
                continue;
            }
            let state_at_cue = match started {
                Some(start) => transition(from, to, progress(start, at, self.transition_seconds)),
                None => to,
            };
            from = state_at_cue;
            to = target;
            started = Some(at);
        }
        match started {
            Some(start) => transition(from, to, progress(start, seconds, self.transition_seconds)),
            None => default,
        }
    }

    /// The resting frame each change aims at, in time order.
    ///
    /// Studied: under a fullscreen camera the screen keeps its neighbour's
    /// layout, the one before the change until the midpoint to the next
    /// change and the next one after it, so it never moves while the camera
    /// covers it and is already in place when the camera leaves. The midpoint
    /// is a target of its own, which keeps every move continuous.
    fn targets(&self, geometry: Geometry, default: LayoutFrame) -> Vec<(f64, LayoutFrame)> {
        let mut cues: Vec<&LayoutCue> = self
            .cues
            .iter()
            .filter(|cue| cue.at.is_finite() && cue.at >= 0.0)
            .collect();
        cues.sort_by(|a, b| a.at.total_cmp(&b.at));
        let mut targets = Vec::with_capacity(cues.len() * 2);
        let mut previous_screen = default.screen;
        for (index, cue) in cues.iter().enumerate() {
            let mut target = canonical(cue.layout, cue_geometry(geometry, cue));
            if cue.layout == CameraLayout::FullscreenCamera {
                target.screen = previous_screen;
                targets.push((cue.at, target));
                if let Some(next) = cues.get(index + 1) {
                    if next.layout != CameraLayout::FullscreenCamera {
                        let mut handed_on = target;
                        handed_on.screen =
                            canonical(next.layout, cue_geometry(geometry, next)).screen;
                        targets.push(((cue.at + next.at) / 2.0, handed_on));
                    }
                }
            } else {
                targets.push((cue.at, target));
            }
            if let Some((_, last)) = targets.last() {
                previous_screen = last.screen;
            }
        }
        targets
    }

    /// Whether the camera contributes a pixel at this instant.
    pub(super) fn camera_visible_at(&self, seconds: f64, overlay: LayerSettings) -> bool {
        self.frame_at(seconds, overlay, 16.0 / 9.0, 16.0 / 9.0)
            .camera_alpha
            > 1e-4
    }
}

/// The layout state a frame is drawn from, in fractions of the video rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LayoutFrame {
    /// Where the screen footage sits. Anything outside it is background, and
    /// the pane is filled by cover-cropping the footage whenever the two
    /// aspects differ.
    pub screen: Rect,
    /// Where the camera sits, whether or not it is currently visible.
    pub camera: Rect,
    /// How opaque the camera layer is, for fading into and out of screen only.
    pub camera_alpha: f64,
    /// The camera's corner radius, as a fraction of its shorter side.
    pub camera_radius: f64,
    /// The screen pane's corner radius, as a fraction of half its shorter
    /// side, like `camera_radius`. Only the padded split rounds it.
    pub screen_radius: f64,
    /// How much of the person's matte blends into the camera, 0..=1.
    pub cutout: f64,
    pub mirror: bool,
}

#[derive(Debug, Clone, Copy)]
struct Geometry {
    overlay: LayerSettings,
    video_aspect: f64,
    /// The screen recording's own aspect. `video_aspect` is the stage's,
    /// which is wider when the stage is widened for the split.
    screen_aspect: f64,
    camera_aspect: f64,
    split_ratio: f64,
    split_auto: bool,
    split_flip: bool,
    /// The padded split's gap, in stage heights; 0 unless padded.
    split_gap: f64,
    /// The padded split's pane radius, in stage heights; 0 unless padded.
    pane_radius: f64,
    cutout_position_x: f64,
    cutout_size: f64,
    zoom_shrink: f64,
}

fn sane_aspect(aspect: f64) -> f64 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    }
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// The geometry a layout rests at, before any transition.
fn canonical(layout: CameraLayout, geometry: Geometry) -> LayoutFrame {
    let screen = contained_screen(geometry.video_aspect, geometry.screen_aspect);
    let full = Rect::full();
    match layout {
        CameraLayout::CameraOverlay => {
            let camera = if geometry.overlay.fullscreen {
                full
            } else {
                geometry
                    .overlay
                    .scaled_in_place(zoomed_scale(
                        geometry.zoom_shrink,
                        geometry.overlay.zoomed_scale,
                    ))
                    .rect_in(full)
            };
            LayoutFrame {
                screen,
                camera,
                camera_alpha: if geometry.overlay.enabled { 1.0 } else { 0.0 },
                camera_radius: if geometry.overlay.fullscreen {
                    0.0
                } else {
                    geometry.overlay.radius
                },
                screen_radius: 0.0,
                cutout: 0.0,
                mirror: geometry.overlay.mirror,
            }
        }
        CameraLayout::FullscreenCamera => LayoutFrame {
            screen,
            camera: full,
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: geometry.overlay.mirror,
        },
        CameraLayout::SplitScreen => {
            let (screen, camera) = split_rects(geometry);
            let rest = LayoutFrame {
                screen,
                camera,
                camera_alpha: 1.0,
                camera_radius: pane_radius(geometry, camera),
                screen_radius: pane_radius(geometry, screen),
                cutout: 0.0,
                mirror: geometry.overlay.mirror,
            };
            if geometry.zoom_shrink <= 0.0 {
                rest
            } else {
                interpolate(
                    rest,
                    split_zoomed(geometry, geometry.overlay.radius),
                    geometry.zoom_shrink,
                )
            }
        }
        CameraLayout::ScreenOnly => LayoutFrame {
            screen,
            camera: geometry.overlay.rect_in(full),
            camera_alpha: 0.0,
            camera_radius: geometry.overlay.radius,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: geometry.overlay.mirror,
        },
        CameraLayout::CameraCutout => LayoutFrame {
            screen,
            camera: cutout_rect(geometry),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 1.0,
            mirror: geometry.overlay.mirror,
        },
    }
}

/// The padded split's corner radius for `pane`, as a fraction of half its
/// shorter side.
fn pane_radius(geometry: Geometry, pane: Rect) -> f64 {
    if geometry.pane_radius <= 0.0 {
        return 0.0;
    }
    let shorter = (pane.width * geometry.video_aspect).min(pane.height);
    if shorter <= 0.0 {
        return 0.0;
    }
    (geometry.pane_radius / (shorter / 2.0)).min(1.0)
}

/// The studied automatic split, as the screen's share of the frame's width.
/// `stage_aspect` is the frame's width over height and `screen_aspect` the
/// recording's. A stacked split halves the frame.
pub(super) fn auto_split_screen_ratio(stage_aspect: f64, screen_aspect: f64) -> f64 {
    let stage = sane_aspect(stage_aspect);
    if stage < 1.0 {
        return 0.5;
    }
    // Camera column width, in units of the frame's height.
    let rest = stage - sane_aspect(screen_aspect);
    let camera = if rest > 0.4 { rest } else { 0.5 };
    (1.0 - camera / stage).clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
}

/// The camera column the studied auto stage adds beside the screen, as a
/// width over the stage's inner height.
const AUTO_STAGE_CAMERA_ASPECT: f64 = 9.0 / 16.0;
/// The widest auto stage the studied build allows.
const AUTO_STAGE_MAX_ASPECT: f64 = 2.4;

/// The studied automatic stage aspect for a composition whose default layout
/// is the split: a 16:9 stage when a side camera column between 0.4 and 16/9
/// of the inner height fits beside the screen, otherwise the screen plus a
/// 9:16 camera column (and `padding` around and between them), 1.0..=2.4.
pub(super) fn auto_split_stage_aspect(screen_aspect: f64, padding: f64) -> f64 {
    let screen = sane_aspect(screen_aspect);
    let padding = if padding.is_finite() {
        padding.clamp(0.0, 0.25)
    } else {
        0.0
    };
    let inset = padding * 1080.0;
    let inner = 1080.0 - inset * 2.0;
    let column = (1920.0 - inset * 3.0 - screen * inner) / inner;
    if (0.4..=16.0 / 9.0).contains(&column) {
        return 16.0 / 9.0;
    }
    ((padding * 3.0 + screen + AUTO_STAGE_CAMERA_ASPECT) / (1.0 + padding * 2.0))
        .clamp(1.0, AUTO_STAGE_MAX_ASPECT)
}

/// [`auto_split_stage_aspect`] for ApexShot's stage, whose Background padding
/// sits outside it: the padded split's `gap` (in stage heights) joins the
/// screen and the 9:16 camera column. A zero gap is exactly the studied
/// unpadded resolver.
pub(super) fn auto_split_stage_with_gap(screen_aspect: f64, gap: f64) -> f64 {
    let screen = sane_aspect(screen_aspect);
    let gap = finite_non_negative(gap);
    let column = 16.0 / 9.0 - screen - gap;
    if (0.4..=16.0 / 9.0).contains(&column) {
        return 16.0 / 9.0;
    }
    (screen + gap + AUTO_STAGE_CAMERA_ASPECT).clamp(1.0, AUTO_STAGE_MAX_ASPECT)
}

/// The stage a composition's layouts are placed on. Lengths are in units of
/// the stage's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct StageShape {
    /// The stage's width over height.
    pub aspect: f64,
    /// The screen recording's width over height.
    pub screen_aspect: f64,
    /// The padded split's gap between its panes.
    pub split_gap: f64,
    /// The padded split's pane corner radius.
    pub pane_radius: f64,
}

impl StageShape {
    /// A stage with the screen's own aspect and no padding.
    pub(super) fn plain(aspect: f64) -> Self {
        Self {
            aspect,
            screen_aspect: aspect,
            split_gap: 0.0,
            pane_radius: 0.0,
        }
    }
}

/// Where the whole screen sits on a stage of another aspect: fitted inside it
/// and centred, the studied rest place for every layout but the split. The
/// whole stage when the two aspects match.
pub(super) fn contained_screen(stage_aspect: f64, screen_aspect: f64) -> Rect {
    let stage = sane_aspect(stage_aspect);
    let screen = sane_aspect(screen_aspect);
    if (stage - screen).abs() <= 1e-9 {
        return Rect::full();
    }
    if screen < stage {
        let width = screen / stage;
        Rect {
            x: (1.0 - width) / 2.0,
            y: 0.0,
            width,
            height: 1.0,
        }
    } else {
        let height = stage / screen;
        Rect {
            x: 0.0,
            y: (1.0 - height) / 2.0,
            width: 1.0,
            height,
        }
    }
}

/// Screen and camera rects for the split.
///
/// A landscape frame splits side by side and a portrait frame splits top and
/// bottom, and the screen takes its whole share: full height beside the camera
/// in a landscape frame, full width above or below it in a portrait one. The
/// screen's own picture is wider or taller than that pane, so it fills it by
/// [`screen_cover_rect`] — cover-cropped, never stretched or letterboxed —
/// while the camera fills its share from the raw camera aspect.
fn split_rects(geometry: Geometry) -> (Rect, Rect) {
    if geometry.split_gap > 0.0 {
        return padded_split_rects(geometry);
    }
    let ratio = if geometry.split_auto {
        auto_split_screen_ratio(geometry.video_aspect, geometry.screen_aspect)
    } else {
        geometry.split_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
    };
    if geometry.video_aspect >= 1.0 {
        let screen_width = ratio;
        let camera = Rect {
            x: if geometry.split_flip {
                0.0
            } else {
                screen_width
            },
            y: 0.0,
            width: 1.0 - screen_width,
            height: 1.0,
        };
        let screen = Rect {
            x: if geometry.split_flip {
                1.0 - screen_width
            } else {
                0.0
            },
            y: 0.0,
            width: screen_width,
            height: 1.0,
        };
        (screen, camera)
    } else {
        let screen_height = ratio;
        let camera = Rect {
            x: 0.0,
            y: if geometry.split_flip {
                0.0
            } else {
                screen_height
            },
            width: 1.0,
            height: 1.0 - screen_height,
        };
        let screen = Rect {
            x: 0.0,
            y: if geometry.split_flip {
                1.0 - screen_height
            } else {
                0.0
            },
            width: 1.0,
            height: screen_height,
        };
        (screen, camera)
    }
}

/// The studied screen pane may give up at most this share of its height,
/// when padded, rather than be cropped.
const SPLIT_PADDED_MAX_REDUCTION: f64 = 0.1;

/// [`split_rects`] with the padded split's gap between the panes. In a
/// landscape stage the screen pane gives up to a tenth of its height
/// instead of being cropped, the studied way.
fn padded_split_rects(geometry: Geometry) -> (Rect, Rect) {
    let stage = geometry.video_aspect;
    let ratio = geometry.split_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
    if stage >= 1.0 {
        // In units of the stage's height.
        let gap = geometry.split_gap.min(stage * 0.5);
        let usable = stage - gap;
        let camera = if geometry.split_auto {
            let rest = usable - geometry.screen_aspect;
            if rest > 0.4 {
                rest
            } else {
                0.5
            }
        } else {
            usable * (1.0 - ratio)
        }
        .clamp(
            usable * (1.0 - MAX_SPLIT_RATIO),
            usable * (1.0 - MIN_SPLIT_RATIO),
        );
        let zone = usable - camera;
        let (screen_x, camera_x) = if geometry.split_flip {
            (camera + gap, 0.0)
        } else {
            (0.0, zone + gap)
        };
        let height = if geometry.screen_aspect > zone {
            (zone / geometry.screen_aspect).clamp(1.0 - SPLIT_PADDED_MAX_REDUCTION, 1.0)
        } else {
            1.0
        };
        (
            Rect {
                x: screen_x / stage,
                y: (1.0 - height) / 2.0,
                width: zone / stage,
                height,
            },
            Rect {
                x: camera_x / stage,
                y: 0.0,
                width: camera / stage,
                height: 1.0,
            },
        )
    } else {
        let gap = geometry.split_gap.min(0.5);
        let usable = 1.0 - gap;
        let share = if geometry.split_auto { 0.5 } else { ratio };
        let zone = usable * share;
        let camera = usable - zone;
        let (screen_y, camera_y) = if geometry.split_flip {
            (camera + gap, 0.0)
        } else {
            (0.0, zone + gap)
        };
        (
            Rect {
                x: 0.0,
                y: screen_y,
                width: 1.0,
                height: zone,
            },
            Rect {
                x: 0.0,
                y: camera_y,
                width: 1.0,
                height: camera,
            },
        )
    }
}

/// The split layout while the screen is zoomed: the screen takes the whole
/// frame and the camera floats as a studied overlay in the corner opposite
/// the screen's pane: the bottom corner for a side-by-side split, and the
/// far edge's right corner for a stacked one.
fn split_zoomed(geometry: Geometry, overlay_radius: f64) -> LayoutFrame {
    let (_, pane) = split_rects(geometry);
    let frame = geometry.video_aspect;
    let pane_aspect = if pane.height > 0.0 {
        pane.width * frame / pane.height
    } else {
        1.0
    };
    let aspect = pane_aspect.clamp(SPLIT_ZOOMED_MIN_ASPECT, SPLIT_ZOOMED_MAX_ASPECT);
    let (width, height) = overlay_size_hw(frame, aspect, SPLIT_ZOOMED_CAMERA_SIZE);
    let inset_x = SPLIT_ZOOMED_CAMERA_INSET / frame.max(1.0);
    let inset_y = SPLIT_ZOOMED_CAMERA_INSET * frame.min(1.0);
    let (align_x, align_y) = if frame >= 1.0 {
        // Side by side: the screen's opposite side, bottom corner.
        (if geometry.split_flip { 0.0 } else { 1.0 }, 1.0)
    } else {
        // Stacked: the screen's opposite edge, right corner.
        (1.0, if geometry.split_flip { 0.0 } else { 1.0 })
    };
    LayoutFrame {
        screen: contained_screen(geometry.video_aspect, geometry.screen_aspect),
        camera: Rect {
            x: inset_x + align_x * (1.0 - 2.0 * inset_x - width).max(0.0),
            y: inset_y + align_y * (1.0 - 2.0 * inset_y - height).max(0.0),
            width,
            height,
        },
        camera_alpha: 1.0,
        camera_radius: overlay_radius,
        screen_radius: 0.0,
        cutout: 0.0,
        mirror: geometry.overlay.mirror,
    }
}

/// The rect of the whole screen, uniformly scaled until it covers a screen
/// pane, and centred on it.
///
/// A split's pane rarely has the footage's aspect — a landscape split gives it
/// a tall share of a wide frame — so covering it takes a scale set by the
/// pane's longer side. The picture then fills the pane and the overflow is
/// cropped evenly off both sides instead of leaving a letterbox or stretching
/// a face. The result is in the same normalized units as `screen` and may
/// reach outside the frame on the axis that is cropped.
///
/// A destination that already carries the source's aspect is its own cover:
/// the whole frame, or the contained rect a proportional placement gives, maps
/// to itself, so a picture that already fits is shown whole and is never
/// cropped.
pub(super) fn screen_cover_rect(screen: Rect) -> Rect {
    let x = if screen.x.is_finite() { screen.x } else { 0.0 };
    let y = if screen.y.is_finite() { screen.y } else { 0.0 };
    let width = if screen.width.is_finite() && screen.width > 0.0 {
        screen.width
    } else {
        1.0
    };
    let height = if screen.height.is_finite() && screen.height > 0.0 {
        screen.height
    } else {
        1.0
    };
    let side = width.max(height);
    Rect {
        x: x + (width - side) / 2.0,
        y: y + (height - side) / 2.0,
        width: side,
        height: side,
    }
}

/// [`screen_cover_rect`] on a stage of `stage_aspect` for a screen of
/// `screen_aspect`: the rect, in stage fractions, the whole screen is drawn
/// into so it covers `screen` with its own aspect, centred on it.
pub(super) fn screen_cover_rect_in(screen: Rect, stage_aspect: f64, screen_aspect: f64) -> Rect {
    let stage = sane_aspect(stage_aspect);
    let source = sane_aspect(screen_aspect);
    let x = if screen.x.is_finite() { screen.x } else { 0.0 };
    let y = if screen.y.is_finite() { screen.y } else { 0.0 };
    let width = if screen.width.is_finite() && screen.width > 0.0 {
        screen.width
    } else {
        1.0
    };
    let height = if screen.height.is_finite() && screen.height > 0.0 {
        screen.height
    } else {
        1.0
    };
    // In units of the stage's height.
    let scale = (width * stage / source).max(height);
    let draw_width = source * scale / stage;
    Rect {
        x: x + (width - draw_width) / 2.0,
        y: y + (height - scale) / 2.0,
        width: draw_width,
        height: scale,
    }
}

/// The geometry a change rests at: the shared one, with this change's camera
/// alignment and cutout position merged over it.
fn cue_geometry(geometry: Geometry, cue: &LayoutCue) -> Geometry {
    let mut geometry = geometry;
    if let Some([x, y]) = cue.position {
        geometry.overlay.set_overlay_alignment(x, y);
    }
    if let Some(x) = cue.cutout_x {
        geometry.cutout_position_x = x;
    }
    geometry
}

/// The bottom-aligned camera of the cutout, sized from the raw camera aspect
/// and placed across the frame by the project's cutout position.
fn cutout_rect(geometry: Geometry) -> Rect {
    let (width, height) = overlay_size_hw(
        geometry.video_aspect,
        geometry.camera_aspect,
        geometry.cutout_size,
    );
    let factor = zoomed_scale(geometry.zoom_shrink, geometry.overlay.zoomed_scale);
    let width = (width * factor).min(1.0);
    let height = (height * factor).min(1.0);
    let position = if geometry.cutout_position_x.is_finite() {
        geometry.cutout_position_x.clamp(0.0, 1.0)
    } else {
        0.5
    };
    Rect {
        x: (1.0 - width) * position,
        y: 1.0 - height,
        width,
        height,
    }
}

/// Whether the studied rule shrinks the camera at `time`: inside an enabled
/// zoom clip, or inside the lead-in before a clip that is not instant.
fn zoom_shrinks_camera_at(clips: &[super::timeline::ZoomClip], time: f64, lead_in: f64) -> bool {
    clips.iter().filter(|clip| !clip.hidden).any(|clip| {
        (time >= clip.start && time <= clip.end)
            || (lead_in > 0.0 && !clip.instant && time >= clip.start - lead_in && time < clip.start)
    })
}

/// How far the camera has shrunk for the screen's zoom at `time`: 0 at its
/// normal size, 1 at [`ZOOMED_CAMERA_SCALE`]. The 0/1 target is followed by
/// the studied camera spring in 1 ms semi-implicit Euler steps, replayed
/// over the last two seconds so a seek lands where playback does.
pub(super) fn zoom_shrink_at(clips: &[super::timeline::ZoomClip], time: f64, lead_in: f64) -> f64 {
    if !time.is_finite() || clips.iter().all(|clip| clip.hidden) {
        return 0.0;
    }
    let target = |at: f64| -> f64 {
        if zoom_shrinks_camera_at(clips, at, lead_in) {
            1.0
        } else {
            0.0
        }
    };
    let (stiffness, damping, mass) = CAMERA_SPRING;
    let omega = (stiffness / mass).sqrt();
    let critical = 2.0 * (mass * stiffness).sqrt();
    let start = (time - CAMERA_SPRING_HISTORY_SECONDS).max(0.0);
    let mut value = target(start);
    let mut velocity = 0.0;
    let steps = ((time - start) * 1000.0).ceil() as usize;
    let mut previous = start;
    for index in 1..=steps {
        let at = (start + index as f64 / 1000.0).min(time);
        let dt = at - previous;
        previous = at;
        let goal = target(at);
        let gap = goal - value;
        let effective = if velocity * gap > 0.0 && velocity.abs() > gap.abs() * omega {
            damping.max(critical)
        } else {
            damping
        };
        velocity += (gap * stiffness - velocity * effective) / mass * dt;
        value += velocity * dt;
        if velocity.abs() < CAMERA_SPRING_PRECISION
            && (goal - value).abs() < CAMERA_SPRING_PRECISION
        {
            value = goal;
            velocity = 0.0;
        }
    }
    value.clamp(0.0, 1.0)
}

/// The size factor for a shrink amount.
fn zoomed_scale(shrink: f64, target: f64) -> f64 {
    let shrink = if shrink.is_finite() {
        shrink.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let target = if target.is_finite() {
        target.clamp(MIN_ZOOMED_CAMERA_SCALE, 1.0)
    } else {
        ZOOMED_CAMERA_SCALE
    };
    1.0 - (1.0 - target) * shrink
}

/// The studied hide spring (stiffness, damping, mass), run clamped.
const HIDE_SPRING: (f64, f64, f64) = (530.0, 40.0, 1.0);
/// The studied size of a hidden camera, about its centre.
const HIDDEN_CAMERA_SCALE: f64 = 0.9;

/// How hidden the camera is for silence at source time `t`: 0 shown, 1
/// hidden. The 0/1 target follows the studied hide spring, replayed over the
/// last second so a seek lands where playback does.
pub(super) fn silence_hide_at(ranges: &[(f64, f64)], t: f64, min_seconds: Option<f64>) -> f64 {
    let Some(min_seconds) = min_seconds else {
        return 0.0;
    };
    if !t.is_finite() || ranges.is_empty() {
        return 0.0;
    }
    let target = |at: f64| -> f64 {
        if super::silence::silent_at(ranges, at, min_seconds) {
            1.0
        } else {
            0.0
        }
    };
    let (stiffness, damping, mass) = HIDE_SPRING;
    let omega = (stiffness / mass).sqrt();
    let critical = 2.0 * (mass * stiffness).sqrt();
    let start = (t - 1.0).max(0.0);
    let mut value = target(start);
    let mut velocity = 0.0;
    let steps = ((t - start) * 1000.0).ceil() as usize;
    let mut previous = start;
    for index in 1..=steps {
        let at = (start + index as f64 / 1000.0).min(t);
        let dt = at - previous;
        previous = at;
        let goal = target(at);
        let gap = goal - value;
        let effective = if velocity * gap > 0.0 && velocity.abs() > gap.abs() * omega {
            damping.max(critical)
        } else {
            damping
        };
        velocity += (gap * stiffness - velocity * effective) / mass * dt;
        value += velocity * dt;
        if velocity.abs() < CAMERA_SPRING_PRECISION
            && (goal - value).abs() < CAMERA_SPRING_PRECISION
        {
            value = goal;
            velocity = 0.0;
        }
    }
    value.clamp(0.0, 1.0)
}

/// A layout frame with the silence hide applied: the camera fades by `hide`
/// and shrinks toward 0.9 about its centre.
pub(super) fn hide_for_silence(mut frame: LayoutFrame, hide: f64) -> LayoutFrame {
    let hide = if hide.is_finite() {
        hide.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if hide <= 0.0 {
        return frame;
    }
    frame.camera_alpha *= 1.0 - hide;
    let factor = 1.0 - (1.0 - HIDDEN_CAMERA_SCALE) * hide;
    let (cx, cy) = (
        frame.camera.x + frame.camera.width / 2.0,
        frame.camera.y + frame.camera.height / 2.0,
    );
    frame.camera.width *= factor;
    frame.camera.height *= factor;
    frame.camera.x = cx - frame.camera.width / 2.0;
    frame.camera.y = cy - frame.camera.height / 2.0;
    frame
}

fn progress(start: f64, seconds: f64, duration: f64) -> f64 {
    // A zero duration is the project's "cut" setting.
    if !duration.is_finite() || duration <= 1e-9 {
        return 1.0;
    }
    let elapsed = seconds - start;
    if !elapsed.is_finite() || elapsed <= 0.0 {
        return 0.0;
    }
    // The project's transition length is the spring's speed: the default is
    // the studied spring itself, and a shorter one runs the same motion
    // faster, the way the zoom's speed scales its spring.
    spring_step_response(elapsed * DEFAULT_TRANSITION_SECONDS / duration)
}

/// The studied layout spring's step response from rest: how far a value has
/// travelled toward a new target `elapsed` seconds after the target changed,
/// in 1 ms semi-implicit Euler steps with the studied clamp, so it never
/// overshoots.
fn spring_step_response(elapsed: f64) -> f64 {
    let (stiffness, damping, mass) = CAMERA_SPRING;
    let omega = (stiffness / mass).sqrt();
    let critical = 2.0 * (mass * stiffness).sqrt();
    let steps = (elapsed * 1000.0).ceil().min(5000.0) as usize;
    let mut value: f64 = 0.0;
    let mut velocity: f64 = 0.0;
    for index in 1..=steps {
        let dt = ((index as f64 / 1000.0).min(elapsed) - ((index - 1) as f64 / 1000.0)).max(0.0);
        let gap = 1.0 - value;
        let effective = if velocity * gap > 0.0 && velocity.abs() > gap.abs() * omega {
            damping.max(critical)
        } else {
            damping
        };
        velocity += (gap * stiffness - velocity * effective) / mass * dt;
        value += velocity * dt;
        if velocity.abs() < CAMERA_SPRING_PRECISION && gap.abs() < CAMERA_SPRING_PRECISION {
            return 1.0;
        }
    }
    if steps >= 5000 {
        1.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

fn interpolate(from: LayoutFrame, to: LayoutFrame, t: f64) -> LayoutFrame {
    LayoutFrame {
        screen: from.screen.lerp(to.screen, t),
        camera: from.camera.lerp(to.camera, t),
        camera_alpha: from.camera_alpha + (to.camera_alpha - from.camera_alpha) * t,
        camera_radius: from.camera_radius + (to.camera_radius - from.camera_radius) * t,
        screen_radius: from.screen_radius + (to.screen_radius - from.screen_radius) * t,
        cutout: from.cutout + (to.cutout - from.cutout) * t,
        mirror: to.mirror,
    }
}

/// A layout change between two resting frames. Like [`interpolate`], except
/// a change to or from a hidden camera (Screen only) is a cut for the camera:
/// the studied renderer snaps its rect and opacity rather than fading.
fn transition(from: LayoutFrame, to: LayoutFrame, t: f64) -> LayoutFrame {
    let mut frame = interpolate(from, to, t);
    let hidden = |frame: &LayoutFrame| frame.camera_alpha <= 1e-6;
    if t > 0.0 && (hidden(&from) || hidden(&to)) {
        frame.camera = to.camera;
        frame.camera_alpha = to.camera_alpha;
        frame.camera_radius = to.camera_radius;
        frame.cutout = to.cutout;
    }
    frame
}

/// The corner radius a shape index starts at, as a fraction of half the shorter side.
/// 0.30 is the studied 0.15 of the shorter side; a circle is an ellipse.
pub(super) fn radius_for_shape(shape: u8) -> f64 {
    match shape {
        0 => 1.0,
        _ => 0.30,
    }
}

/// The height over width a shape index gives the bubble.
pub(super) fn aspect_for_shape(shape: u8) -> f64 {
    match shape {
        0 | 1 => 1.0,
        2 => 0.75,
        _ => 1.3,
    }
}

/// Add the layer's outline to the context's current path.
pub(super) fn add_shape_path(context: &Context, rect: Rect, radius: f64) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0);
    if radius <= 0.01 {
        context.new_path();
        context.rectangle(rect.x, rect.y, rect.width, rect.height);
        return;
    }
    if (radius - rect.width.min(rect.height) / 2.0).abs() < 0.5 {
        // A full radius is an ellipse; drawing it as one keeps a wide bubble
        // round instead of letting the corner curves overlap.
        let _ = context.save();
        context.translate(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        context.scale(rect.width / 2.0, rect.height / 2.0);
        context.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
        let _ = context.restore();
        return;
    }
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    context.new_path();
    context.move_to(x + radius, y);
    context.line_to(x + w - radius, y);
    context.curve_to(x + w, y, x + w, y, x + w, y + radius);
    context.line_to(x + w, y + h - radius);
    context.curve_to(x + w, y + h, x + w, y + h, x + w - radius, y + h);
    context.line_to(x + radius, y + h);
    context.curve_to(x, y + h, x, y + h, x, y + h - radius);
    context.line_to(x, y + radius);
    context.curve_to(x, y, x, y, x + radius, y);
    context.close_path();
}

/// Draw the layer over the current context, stretched to the rect.
///
/// This is the legacy bubble path, kept for the single-layer tests; the layout
/// renderer draws through [`draw_camera_cover`] so faces keep the camera's own
/// aspect in every layout.
pub(super) fn draw_bubble(
    context: &Context,
    rect: Rect,
    radius: f64,
    flip: bool,
    image: &ImageSurface,
) {
    let (image_width, image_height) = (f64::from(image.width()), f64::from(image.height()));
    if image_width <= 0.0 || image_height <= 0.0 {
        return;
    }
    let _ = context.save();
    add_shape_path(context, rect, radius);
    if radius > 0.01 {
        context.clip_preserve();
    } else {
        context.clip();
    }
    {
        let _ = context.save();
        context.translate(rect.x, rect.y);
        if flip {
            context.translate(rect.width, 0.0);
            context.scale(-1.0, 1.0);
        }
        context.scale(rect.width / image_width, rect.height / image_height);
        let _ = context.set_source_surface(image, 0.0, 0.0);
        let _ = context.paint();
        let _ = context.restore();
    }
    if radius > 0.01 {
        // The outline follows the cut edge, so a bubble on light footage still
        // reads as a layer instead of a hole.
        context.set_line_width(1.5);
        context.set_source_rgba(1.0, 1.0, 1.0, 40.0 / 255.0);
        let _ = context.stroke();
    }
    let _ = context.restore();
}

/// Draw a camera frame over the rect the way a camera fills a viewfinder:
/// scaled up until it covers the rect, anchored to the frame's top edge and
/// centred across, then cropped by the rect.
///
/// A wide rect crops away the bottom of a square or portrait frame instead of
/// cutting into the headroom the camera framed above the subject. The preview
/// and the export both draw through here, so they frame the same picture.
fn draw_camera_cover(
    context: &Context,
    rect: Rect,
    radius: f64,
    flip: bool,
    alpha: f64,
    image: &ImageSurface,
) {
    let (image_width, image_height) = (f64::from(image.width()), f64::from(image.height()));
    if image_width <= 0.0 || image_height <= 0.0 || alpha <= 0.0 {
        return;
    }
    let scale = (rect.width / image_width).max(rect.height / image_height);
    let drawn_width = image_width * scale;
    let offset_x = (rect.width - drawn_width) / 2.0;
    let _ = context.save();
    add_shape_path(context, rect, radius);
    context.clip();
    let _ = context.save();
    context.translate(rect.x + offset_x, rect.y);
    if flip {
        context.translate(drawn_width, 0.0);
        context.scale(-1.0, 1.0);
    }
    context.scale(scale, scale);
    let _ = context.set_source_surface(image, 0.0, 0.0);
    let _ = context.paint_with_alpha(alpha.clamp(0.0, 1.0));
    let _ = context.restore();
    let _ = context.restore();
}

struct Job {
    path: PathBuf,
    seconds: f64,
    /// The document that asked for this frame. A decode that finishes after
    /// another document loaded must not publish into the new one.
    generation: u64,
    /// The decode size, at the camera's own aspect so every layout covers it
    /// without stretching a face.
    size: (u32, u32),
    /// Whether the decoded frame also needs its person matte.
    cutout: bool,
    /// The person matte's edge controls, as (softness, inset in this decode's
    /// pixels).
    matte: (f64, f64),
}

struct Frame {
    width: u32,
    height: u32,
    stamp: u64,
    pixels: Vec<u8>,
    /// Full-frame person alpha, when the document uses the cutout.
    mask: Option<Vec<f32>>,
}

enum CameraError {
    Decode(String),
    Cutout(String),
}

impl CameraError {
    fn message(&self) -> String {
        match self {
            Self::Decode(message) | Self::Cutout(message) => message.clone(),
        }
    }
}

struct Shared {
    job: Mutex<Option<Job>>,
    frame: Mutex<Option<Frame>>,
    stamp: Mutex<u64>,
    error: Mutex<Option<CameraError>>,
    /// The silent spans of the recording's audio: None while the analysis
    /// runs, Some(None) for a recording without audio.
    #[allow(clippy::type_complexity)]
    silence: Mutex<Option<Option<Vec<(f64, f64)>>>>,
    /// Bumped by every document load; a worker only publishes frames from the
    /// generation it was asked for.
    generation: Mutex<u64>,
}

struct Track {
    path: PathBuf,
    duration: f64,
    /// Seconds into the video where the track's first frame sits.
    start: f64,
    video_seconds: f64,
    /// The last instant a seek can land on and still find a decoded frame.
    last_frame: f64,
    /// The camera's own frame size, from the sidecar.
    camera_size: (u32, u32),
}

fn last_camera_seconds(duration: f64, fps: Option<f64>) -> f64 {
    if duration <= 0.0 || !duration.is_finite() {
        return 0.0;
    }
    let gap = fps
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .map(|fps| 1.5 / fps)
        .unwrap_or_else(|| (duration * 0.02).clamp(0.04, 1.0));
    (duration - gap).max(0.0)
}

/// The largest manual camera delay, either way: the studied adjustment range.
pub(super) const MAX_CAMERA_DELAY_SECONDS: f64 = 0.2;

/// Where a video instant lands in the camera track.
///
/// The track's first frame sits `start` seconds into the video (the gap the
/// recorder measured), and from there it runs on its own clock. `rate` is
/// track seconds per video second after that gap: 1 for a recording that
/// stored its gap, whose track ends wherever the camera stopped, and the old
/// length ratio for a sidecar written before the gap existed, which stores
/// none. `delay` is the user's adjustment; a positive delay samples the
/// camera later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CameraClock {
    pub start: f64,
    pub rate: f64,
    pub delay: f64,
}

impl CameraClock {
    pub fn new(start: f64, track_seconds: f64, video_seconds: f64, delay: f64) -> Self {
        let start = if start.is_finite() {
            start.max(0.0)
        } else {
            0.0
        };
        let rate = if start > 0.0 {
            1.0
        } else if video_seconds.is_finite()
            && track_seconds.is_finite()
            && video_seconds > start
            && track_seconds > 0.0
        {
            (track_seconds / (video_seconds - start)).clamp(0.01, 100.0)
        } else {
            1.0
        };
        let delay = if delay.is_finite() {
            delay.clamp(-MAX_CAMERA_DELAY_SECONDS, MAX_CAMERA_DELAY_SECONDS)
        } else {
            0.0
        };
        Self { start, rate, delay }
    }

    /// The track instant, negative while the camera has not started yet.
    fn raw(&self, video_seconds: f64) -> f64 {
        (video_seconds - self.start) * self.rate + self.delay
    }

    /// The track instant for a video instant; the first frame before the
    /// camera starts.
    pub fn track_at(&self, video_seconds: f64) -> f64 {
        self.raw(video_seconds).max(0.0)
    }

    /// Video seconds from `video_seconds` until the camera's first frame,
    /// which is held over that lead.
    pub fn lead_at(&self, video_seconds: f64) -> f64 {
        (-self.raw(video_seconds)).max(0.0) / self.rate
    }

    /// The timestamp multiplier that puts track frames on the video's clock.
    pub fn time_scale(&self) -> f64 {
        1.0 / self.rate
    }
}

struct State {
    shared: Arc<Shared>,
    track: RefCell<Option<Track>>,
    /// What the recording captured, and what the editor currently shows.
    seeded: RefCell<Option<LayerSettings>>,
    layer: RefCell<Option<LayerSettings>>,
    layouts: RefCell<LayoutSettings>,
    seeded_layouts: RefCell<LayoutSettings>,
    /// Set once the panel edits layouts directly; until then the legacy
    /// enabled/fullscreen switches keep the default layout in step.
    layouts_custom: Cell<bool>,
    composition_seconds: Cell<f64>,
    /// The preview stage's aspect when it is wider than the screen, else 0.
    stage_aspect: Cell<f64>,
    split_gap: Cell<f64>,
    pane_radius: Cell<f64>,
    zoom_shrink: Cell<f64>,
    /// The source time of the frame on screen, for the silence hide.
    source_seconds: Cell<f64>,
    /// The video document this layer was loaded for, for its audio.
    source_path: RefCell<Option<PathBuf>>,
    cache: RefCell<Option<(u64, ImageSurface)>>,
    cutout_cache: RefCell<Option<(u64, f64, ImageSurface)>>,
    requested: Cell<f64>,
    requested_cutout: Cell<bool>,
    requested_matte: Cell<(f64, f64)>,
    repaint: Rc<dyn Fn()>,
}

/// The editor's camera layer: what to draw, from which track, and when.
#[derive(Clone)]
pub(super) struct WebcamModel {
    state: Rc<State>,
}

impl WebcamModel {
    pub fn new(repaint: Rc<dyn Fn()>) -> Self {
        let shared = Arc::new(Shared {
            job: Mutex::new(None),
            frame: Mutex::new(None),
            stamp: Mutex::new(0),
            error: Mutex::new(None),
            silence: Mutex::new(None),
            generation: Mutex::new(0),
        });
        let worker = shared.clone();
        thread::spawn(move || decode_loop(worker));
        Self {
            state: Rc::new(State {
                shared,
                track: RefCell::new(None),
                seeded: RefCell::new(None),
                layer: RefCell::new(None),
                layouts: RefCell::new(LayoutSettings::default()),
                seeded_layouts: RefCell::new(LayoutSettings::default()),
                layouts_custom: Cell::new(false),
                composition_seconds: Cell::new(0.0),
                stage_aspect: Cell::new(0.0),
                split_gap: Cell::new(0.0),
                pane_radius: Cell::new(0.0),
                zoom_shrink: Cell::new(0.0),
                source_seconds: Cell::new(0.0),
                source_path: RefCell::new(None),
                cache: RefCell::new(None),
                cutout_cache: RefCell::new(None),
                requested: Cell::new(-1.0),
                requested_cutout: Cell::new(false),
                requested_matte: Cell::new((0.0, 0.0)),
                repaint,
            }),
        }
    }

    /// Point the layer at a document. A recording without a camera, or whose
    /// track has gone missing, has no layer at all; the layouts fall back to
    /// the default with no cues.
    pub fn load(&self, video: Option<&Path>, video_seconds: f64) {
        let mut generation = self.state.shared.generation.lock().unwrap();
        *generation += 1;
        *self.state.track.borrow_mut() = None;
        *self.state.seeded.borrow_mut() = None;
        *self.state.layer.borrow_mut() = None;
        *self.state.layouts.borrow_mut() = LayoutSettings::default();
        *self.state.seeded_layouts.borrow_mut() = LayoutSettings::default();
        self.state.layouts_custom.set(false);
        self.state.composition_seconds.set(0.0);
        self.state.zoom_shrink.set(0.0);
        self.state.source_seconds.set(0.0);
        *self.state.source_path.borrow_mut() = None;
        *self.state.cache.borrow_mut() = None;
        *self.state.cutout_cache.borrow_mut() = None;
        self.state.requested.set(-1.0);
        self.state.requested_cutout.set(false);
        self.state.requested_matte.set((0.0, 0.0));
        *self.state.shared.frame.lock().unwrap() = None;
        *self.state.shared.job.lock().unwrap() = None;
        *self.state.shared.error.lock().unwrap() = None;
        *self.state.shared.silence.lock().unwrap() = None;
        drop(generation);

        let Some(video) = video else { return };
        let Some(sidecar) = PointerSidecar::load_next_to_video(video) else {
            return;
        };
        if sidecar.screen_file.is_some() && sidecar.editable_video_path(video).is_none() {
            return;
        }
        let configured = sidecar.webcam.clone();
        let Some(track) = configured.clone().filter(WebcamTrack::is_valid) else {
            if configured.is_some() {
                *self.state.shared.error.lock().unwrap() = Some(CameraError::Decode(
                    "the camera layer in this recording describes no usable track".to_string(),
                ));
            }
            return;
        };
        let Some(path) = sidecar.webcam_path(video) else {
            *self.state.shared.error.lock().unwrap() = Some(CameraError::Decode(format!(
                "the camera recording is missing: {}",
                track.file
            )));
            return;
        };
        let video_seconds = if video_seconds > 0.0 {
            video_seconds
        } else {
            crate::recording::editor::ffmpeg::probe_duration(video).unwrap_or(0.0)
        };
        let seeded = LayerSettings::from_track(&track);
        let seeded_layouts = LayoutSettings {
            default_layout: layout_from_legacy(&seeded),
            ..LayoutSettings::default()
        };
        *self.state.seeded.borrow_mut() = Some(seeded);
        *self.state.layer.borrow_mut() = Some(seeded);
        *self.state.layouts.borrow_mut() = seeded_layouts.clone();
        *self.state.seeded_layouts.borrow_mut() = seeded_layouts;
        let last_frame = last_camera_seconds(
            track.duration,
            crate::recording::editor::ffmpeg::probe_frame_rate(&path),
        );
        *self.state.track.borrow_mut() = Some(Track {
            path,
            duration: track.duration,
            start: track.start,
            video_seconds,
            last_frame,
            camera_size: (track.width, track.height),
        });
        *self.state.source_path.borrow_mut() = Some(video.to_path_buf());
        *self.state.shared.silence.lock().unwrap() = None;
        {
            let shared = self.state.shared.clone();
            let generation = *shared.generation.lock().unwrap();
            let source = video.to_path_buf();
            thread::spawn(move || {
                let ranges = super::silence::silent_ranges(&source);
                if *shared.generation.lock().unwrap() == generation {
                    *shared.silence.lock().unwrap() = Some(ranges);
                }
            });
        }
    }

    pub fn has_track(&self) -> bool {
        self.state.track.borrow().is_some()
    }

    /// The layer the panel edits, or `None` for a recording without a camera.
    pub fn settings(&self) -> Option<LayerSettings> {
        *self.state.layer.borrow()
    }

    /// The layouts the panel edits, with the defaults when there is no camera.
    pub fn layouts(&self) -> LayoutSettings {
        self.state.layouts.borrow().clone()
    }

    /// Set the layer and repaint. The first call on a fresh document moves the
    /// layer away from what the recording captured, which is what the project
    /// file records.
    pub fn set_settings(&self, settings: LayerSettings) {
        if !self.has_track() {
            return;
        }
        let clamped = settings.clamped();
        if self.settings() == Some(clamped) {
            return;
        }
        *self.state.layer.borrow_mut() = Some(clamped);
        if !self.state.layouts_custom.get() {
            let mut layouts = self.state.layouts.borrow().clone();
            let default_layout = layout_from_legacy(&clamped);
            if layouts.default_layout != default_layout {
                layouts.default_layout = default_layout;
                *self.state.layouts.borrow_mut() = layouts;
            }
        }
        self.state.requested.set(-1.0);
        (self.state.repaint)();
    }

    /// Replace every cue and layout setting. Values are clamped here, so the
    /// renderer never has to defend itself against a hand-edited project.
    pub fn set_layouts(&self, layouts: LayoutSettings) {
        if !self.has_track() {
            return;
        }
        let clamped = layouts.clamped();
        self.state.layouts_custom.set(true);
        if *self.state.layouts.borrow() == clamped {
            return;
        }
        *self.state.layouts.borrow_mut() = clamped;
        self.state.requested.set(-1.0);
        (self.state.repaint)();
    }

    /// The composition clock the cues run on.
    pub fn set_composition_time(&self, seconds: f64) {
        if !seconds.is_finite() {
            return;
        }
        self.state.composition_seconds.set(seconds.max(0.0));
    }

    /// The preview stage's aspect, or 0 for a stage with the screen's own.
    pub fn set_stage_aspect(&self, aspect: f64) {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            0.0
        };
        if (self.state.stage_aspect.get() - aspect).abs() < 1e-12 {
            return;
        }
        self.state.stage_aspect.set(aspect);
        (self.state.repaint)();
    }

    /// The padded split's gap and pane radius, in stage heights.
    pub fn set_split_padding(&self, gap: f64, radius: f64) {
        let gap = finite_non_negative(gap);
        let radius = finite_non_negative(radius);
        if (self.state.split_gap.get() - gap).abs() < 1e-12
            && (self.state.pane_radius.get() - radius).abs() < 1e-12
        {
            return;
        }
        self.state.split_gap.set(gap);
        self.state.pane_radius.set(radius);
        (self.state.repaint)();
    }

    /// Whether the studied auto stage widens for this composition: a camera
    /// that is on, whose default layout is the split.
    pub fn widens_stage(&self) -> bool {
        self.has_track()
            && self.settings().is_some_and(|settings| settings.enabled)
            && self.state.layouts.borrow().default_layout == CameraLayout::SplitScreen
    }

    /// How far the screen's zoom has shrunk the camera at the composition
    /// instant, from [`zoom_shrink_at`].
    pub fn set_zoom_shrink(&self, shrink: f64) {
        self.state.zoom_shrink.set(if shrink.is_finite() {
            shrink.clamp(0.0, 1.0)
        } else {
            0.0
        });
    }

    /// Put the layer back where the recording captured it.
    pub fn reset(&self) {
        let Some(seeded) = *self.state.seeded.borrow() else {
            return;
        };
        let seeded_layouts = self.state.seeded_layouts.borrow().clone();
        *self.state.layer.borrow_mut() = Some(seeded);
        *self.state.layouts.borrow_mut() = seeded_layouts;
        self.state.layouts_custom.set(false);
        self.state.requested.set(-1.0);
        (self.state.repaint)();
    }

    /// Whether the layer or the layouts have been moved away from the
    /// recording they came from.
    pub fn edited(&self) -> bool {
        self.state.seeded.borrow().as_ref() != self.state.layer.borrow().as_ref()
            || *self.state.seeded_layouts.borrow() != *self.state.layouts.borrow()
    }

    /// The picture at the current composition instant, in fractions of the
    /// video rect. `source_size` is the recording's own frame size, which sets
    /// the video's aspect.
    pub fn layout_frame(&self, source_size: Option<(u32, u32)>) -> Option<LayoutFrame> {
        let track = self.state.track.borrow();
        let track = track.as_ref()?;
        let settings = self.settings()?;
        let video_aspect = source_size
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| f64::from(width) / f64::from(height))
            .unwrap_or(16.0 / 9.0);
        let camera_aspect = track.camera_size.0 as f64 / f64::from(track.camera_size.1.max(1));
        let stage = match self.state.stage_aspect.get() {
            aspect if aspect > 0.0 => aspect,
            _ => video_aspect,
        };
        let frame = self.state.layouts.borrow().frame_at_staged(
            self.state.composition_seconds.get(),
            settings,
            StageShape {
                aspect: stage,
                screen_aspect: video_aspect,
                split_gap: self.state.split_gap.get(),
                pane_radius: self.state.pane_radius.get(),
            },
            camera_aspect,
            self.state.zoom_shrink.get(),
        );
        Some(hide_for_silence(
            frame,
            silence_hide_at(
                &self.silence_ranges(),
                self.state.source_seconds.get(),
                settings.hide_during_silence,
            ),
        ))
    }

    /// Whether the recording's audio could be analysed: `None` while the
    /// analysis runs, `Some(false)` for a recording with no audio.
    #[allow(dead_code)]
    pub fn has_audio(&self) -> Option<bool> {
        self.state
            .shared
            .silence
            .lock()
            .unwrap()
            .as_ref()
            .map(|ranges| ranges.is_some())
    }

    fn silence_ranges(&self) -> Vec<(f64, f64)> {
        self.state
            .shared
            .silence
            .lock()
            .unwrap()
            .clone()
            .flatten()
            .unwrap_or_default()
    }

    /// The last failure from the camera decode or the person matte, for the UI
    /// to surface.
    pub fn error(&self) -> Option<String> {
        self.state
            .shared
            .error
            .lock()
            .unwrap()
            .as_ref()
            .map(CameraError::message)
    }

    /// The layer the export should bake, with the camera's own length so the
    /// frames can be mapped onto the video's clock.
    pub fn export(&self) -> Option<WebcamExport> {
        let track = self.state.track.borrow();
        let track = track.as_ref()?;
        let settings = self.settings()?;
        let layouts = self.layouts();
        // A switched-off layer exports nothing, so the file comes out with the
        // screen alone and no camera track beside it.
        if !layouts.uses_camera() || !settings.enabled {
            return None;
        }
        let silence = if settings.hide_during_silence.is_some() {
            self.state
                .shared
                .silence
                .lock()
                .unwrap()
                .clone()
                .flatten()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Some(WebcamExport {
            path: track.path.clone(),
            settings,
            layouts,
            split_gap: self.state.split_gap.get(),
            pane_radius: self.state.pane_radius.get(),
            track_seconds: track.duration,
            start: track.start,
            video_seconds: track.video_seconds,
            last_frame: track.last_frame,
            source_size: track.camera_size,
            silence,
        })
    }

    /// Ask for the frame the playhead needs. The decode runs on a worker, and
    /// the still preview repaints once it lands; playback repaints anyway.
    pub fn request(&self, source_seconds: f64) {
        self.state.source_seconds.set(source_seconds);
        let Some(target) = self.target_seconds(source_seconds) else {
            return;
        };
        let Some(settings) = self.settings() else {
            return;
        };
        if !self
            .state
            .layouts
            .borrow()
            .camera_visible_at(self.state.composition_seconds.get(), settings)
        {
            self.state.requested.set(-1.0);
            return;
        }
        let cutout = self
            .layout_frame(None)
            .is_some_and(|frame| frame.cutout > 1e-3);
        let (path, camera_size) = {
            let track = self.state.track.borrow();
            let Some(track) = track.as_ref() else {
                return;
            };
            (track.path.clone(), track.camera_size)
        };
        let job_width = preview_size(camera_size).0;
        let matte = {
            let layouts = self.state.layouts.borrow();
            (
                layouts.cutout_softness,
                layouts.cutout_inset * f64::from(job_width) / f64::from(camera_size.0.max(1)),
            )
        };
        if (self.state.requested.get() - target).abs() < REQUEST_STEP_SECONDS
            && self.state.requested_cutout.get() == cutout
            && self.state.requested_matte.get() == matte
        {
            return;
        }
        self.state.requested.set(target);
        self.state.requested_cutout.set(cutout);
        self.state.requested_matte.set(matte);

        let generation = *self.state.shared.generation.lock().unwrap();
        *self.state.shared.job.lock().unwrap() = Some(Job {
            path,
            seconds: target,
            generation,
            size: preview_size(camera_size),
            cutout,
            matte,
        });

        let repaint = self.state.repaint.clone();
        glib::timeout_add_local_once(STILL_REPAINT_DELAY, move || repaint());
    }

    /// Draw the layout's camera over the picture.
    ///
    /// The bubble keeps its place in the frame while the footage zooms
    /// underneath it: the layer belongs to the composition, not to the
    /// footage. The screen's own transform — the split's pane and the cover
    /// rect that fills it — is the stage's job, because its clip and cursor
    /// share it.
    pub fn draw(
        &self,
        context: &Context,
        allocation_width: i32,
        allocation_height: i32,
        source_size: Option<(u32, u32)>,
        video: Option<Rect>,
    ) {
        let Some((width, height)) = source_size else {
            return;
        };
        let Some(frame) = self.layout_frame(source_size) else {
            return;
        };
        if frame.camera_alpha <= 0.0 {
            return;
        }
        let video = match video {
            Some(video) => video,
            None => {
                let Some(video) = super::cursor::video_rect_for(
                    f64::from(allocation_width),
                    f64::from(allocation_height),
                    f64::from(width),
                    f64::from(height),
                ) else {
                    return;
                };
                Rect {
                    x: video.x,
                    y: video.y,
                    width: video.width,
                    height: video.height,
                }
            }
        };
        let rect = frame.camera.inside(video);
        if rect.width < 1.0 || rect.height < 1.0 {
            return;
        }
        let Some(image) = self.camera_surface(frame.cutout) else {
            return;
        };
        let radius = frame.camera_radius * rect.width.min(rect.height) / 2.0;
        draw_camera_cover(
            context,
            rect,
            radius,
            frame.mirror,
            frame.camera_alpha,
            &image,
        );
    }

    fn target_seconds(&self, source_seconds: f64) -> Option<f64> {
        let delay = self
            .settings()
            .map(|settings| settings.delay_seconds)
            .unwrap_or(0.0);
        let track = self.state.track.borrow();
        let track = track.as_ref()?;
        let clock = CameraClock::new(track.start, track.duration, track.video_seconds, delay);
        Some(clock.track_at(source_seconds).min(track.last_frame))
    }

    /// The newest decoded frame as a surface, rebuilt only when a new frame
    /// lands. cairo surfaces belong to the thread that draws them, so the
    /// worker hands over pixels and the surface is built here.
    fn current_surface(&self) -> Option<ImageSurface> {
        let guard = self.state.shared.frame.lock().unwrap();
        let frame = guard.as_ref()?;
        if let Some((stamp, surface)) = self.state.cache.borrow().as_ref() {
            if *stamp == frame.stamp {
                return Some(surface.clone());
            }
        }
        let stride = frame.width as usize * 4;
        let surface = ImageSurface::create_for_data(
            frame.pixels.clone(),
            Format::ARgb32,
            frame.width as i32,
            frame.height as i32,
            stride as i32,
        )
        .ok()?;
        *self.state.cache.borrow_mut() = Some((frame.stamp, surface.clone()));
        Some(surface)
    }

    /// The frame with the person's matte blended in, when the cutout is
    /// visible. The pixels are premultiplied because cairo expects an
    /// `ARgb32` surface to be; the blend is cached per frame and cutout
    /// fraction so a transition rebuilds it, not every repaint.
    fn camera_surface(&self, cutout: f64) -> Option<ImageSurface> {
        let cutout = cutout.clamp(0.0, 1.0);
        if cutout <= 1e-3 {
            return self.current_surface();
        }
        let guard = self.state.shared.frame.lock().unwrap();
        let frame = guard.as_ref()?;
        let mask = frame.mask.as_deref()?;
        let quantized = (cutout * 1000.0).round() / 1000.0;
        if let Some((stamp, cached, surface)) = self.state.cutout_cache.borrow().as_ref() {
            if *stamp == frame.stamp && (*cached - quantized).abs() < 1e-9 {
                return Some(surface.clone());
            }
        }
        let pixels = camera_premultiplied(
            &frame.pixels,
            (frame.width, frame.height),
            Some(mask),
            quantized,
        )
        .ok()?;
        let surface = ImageSurface::create_for_data(
            pixels,
            Format::ARgb32,
            frame.width as i32,
            frame.height as i32,
            frame.width as i32 * 4,
        )
        .ok()?;
        *self.state.cutout_cache.borrow_mut() = Some((frame.stamp, quantized, surface.clone()));
        Some(surface)
    }
}

/// The camera track an export should bake, in the units the exporter works in.
#[derive(Debug, Clone)]
pub(super) struct WebcamExport {
    pub path: PathBuf,
    pub settings: LayerSettings,
    pub layouts: LayoutSettings,
    /// The padded split's gap and pane radius, in stage heights.
    pub split_gap: f64,
    pub pane_radius: f64,
    pub track_seconds: f64,
    /// Seconds into the video where the track's first frame sits.
    pub start: f64,
    pub video_seconds: f64,
    /// The last instant a seek can land on and still find a decoded frame.
    pub last_frame: f64,
    /// The camera's own frame size, so the decode keeps its aspect.
    pub source_size: (u32, u32),
    /// The silent spans of the recording's audio, in source seconds.
    pub silence: Vec<(f64, f64)>,
}

impl WebcamExport {
    fn clock(&self) -> CameraClock {
        CameraClock::new(
            self.start,
            self.track_seconds,
            self.video_seconds,
            self.settings.delay_seconds,
        )
    }

    /// A video instant as a time in the track.
    pub fn track_at(&self, video_seconds: f64) -> f64 {
        self.clock().track_at(video_seconds)
    }

    /// The decode start for a video instant, capped at the camera's last real
    /// frame so a trim that opens near the end still finds footage.
    pub fn start_at(&self, video_seconds: f64) -> f64 {
        self.track_at(video_seconds).min(self.last_frame)
    }

    /// Video seconds the first camera frame is held for before the track
    /// starts playing, from a video instant.
    pub fn lead_at(&self, video_seconds: f64) -> f64 {
        self.clock().lead_at(video_seconds)
    }

    /// The timestamp multiplier that puts the track's frames on the video's
    /// clock.
    pub fn time_scale(&self) -> f64 {
        self.clock().time_scale()
    }
}

/// The layout a legacy layer starts from. Old project files only know the
/// enabled and fullscreen switches; a new document opens on the bubble the
/// recorder drew.
fn layout_from_legacy(settings: &LayerSettings) -> CameraLayout {
    if !settings.enabled {
        CameraLayout::ScreenOnly
    } else if settings.fullscreen {
        CameraLayout::FullscreenCamera
    } else {
        CameraLayout::CameraOverlay
    }
}

/// The person matte with the studied edge controls applied: `inset` pulls
/// the edge in by that many pixels (a min filter), and `softness` sets how
/// wide the edge's ramp is around the matte's midpoint (0 hard, 1 as soft as
/// the matte itself).
pub(super) fn refine_matte(mask: &mut [f32], size: (u32, u32), softness: f64, inset: f64) {
    let (width, height) = (size.0 as usize, size.1 as usize);
    if width == 0 || height == 0 || mask.len() != width * height {
        return;
    }
    let radius = if inset.is_finite() {
        inset.max(0.0).round() as usize
    } else {
        0
    };
    if radius > 0 {
        // Separable erosion: rows, then columns.
        let mut scratch = mask.to_vec();
        for y in 0..height {
            let row = &mask[y * width..(y + 1) * width];
            for x in 0..width {
                let lo = x.saturating_sub(radius);
                let hi = (x + radius).min(width - 1);
                scratch[y * width + x] = row[lo..=hi].iter().copied().fold(f32::INFINITY, f32::min);
            }
        }
        for x in 0..width {
            for y in 0..height {
                let lo = y.saturating_sub(radius);
                let hi = (y + radius).min(height - 1);
                let mut value = f32::INFINITY;
                for yy in lo..=hi {
                    value = value.min(scratch[yy * width + x]);
                }
                mask[y * width + x] = value;
            }
        }
    }
    let softness = if softness.is_finite() {
        softness.clamp(0.0, 1.0)
    } else {
        0.2
    };
    let half = (0.5 * softness).max(0.01) as f32;
    let (lo, hi) = (0.5 - half, 0.5 + half);
    for value in mask.iter_mut() {
        let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
        *value = t * t * (3.0 - 2.0 * t);
    }
}

/// Multiply a raw camera frame's alpha by the blended person matte and
/// premultiply its colour, ready for cairo's `ARgb32`.
fn camera_premultiplied(
    pixels: &[u8],
    size: (u32, u32),
    mask: Option<&[f32]>,
    cutout: f64,
) -> Result<Vec<u8>, String> {
    let expected = size.0 as usize * size.1 as usize * 4;
    if pixels.len() != expected {
        return Err("the camera frame does not match its size".to_string());
    }
    let cutout = cutout.clamp(0.0, 1.0);
    let mask = match mask {
        Some(mask) => {
            let count = size.0 as usize * size.1 as usize;
            if mask.len() != count {
                return Err("the camera matte does not match the frame".to_string());
            }
            Some(mask)
        }
        None if cutout > 1e-3 => {
            return Err("the camera cutout has no person matte".to_string());
        }
        None => None,
    };
    let mut output = pixels.to_vec();
    for (index, pixel) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let person = mask.map(|mask| mask[index].clamp(0.0, 1.0)).unwrap_or(0.0);
        let alpha = (1.0 - cutout) + cutout * f64::from(person);
        pixel[0] = (f64::from(pixel[0]) * alpha).round() as u8;
        pixel[1] = (f64::from(pixel[1]) * alpha).round() as u8;
        pixel[2] = (f64::from(pixel[2]) * alpha).round() as u8;
        pixel[3] = (alpha * 255.0).round() as u8;
    }
    Ok(output)
}

/// Run `write` only while the shared generation still belongs to `job`, with
/// the generation lock held so a load cannot clear and a worker cannot publish
/// in the same breath.
fn with_current_generation(shared: &Shared, job: &Job, write: impl FnOnce()) -> bool {
    let generation = shared.generation.lock().unwrap();
    if *generation != job.generation {
        return false;
    }
    write();
    true
}

fn publish_frame(
    shared: &Shared,
    job: &Job,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    mask: Option<Vec<f32>>,
) -> bool {
    with_current_generation(shared, job, || {
        if job.cutout && mask.is_some() {
            let mut error = shared.error.lock().unwrap();
            if error.is_some() {
                *error = None;
            }
        }
        let stamp = {
            let mut stamp = shared.stamp.lock().unwrap();
            *stamp += 1;
            *stamp
        };
        *shared.frame.lock().unwrap() = Some(Frame {
            width,
            height,
            stamp,
            pixels,
            mask,
        });
    })
}

fn decode_loop(shared: Arc<Shared>) {
    let mut processor: Option<CutoutProcessor> = None;
    let mut matte_failed = false;
    let mut generation = 0;
    loop {
        let job = shared.job.lock().unwrap().take();
        let Some(job) = job else {
            thread::sleep(Duration::from_millis(8));
            continue;
        };
        if job.generation != generation {
            generation = job.generation;
            matte_failed = false;
        }
        let Some((pixels, width, height)) = decode_frame(&job.path, job.seconds, job.size) else {
            with_current_generation(shared.as_ref(), &job, || {
                let mut error = shared.error.lock().unwrap();
                if error.is_none() {
                    *error = Some(CameraError::Decode(
                        "the camera frame could not be decoded".to_string(),
                    ));
                }
            });
            continue;
        };
        with_current_generation(shared.as_ref(), &job, || {
            let mut error = shared.error.lock().unwrap();
            if matches!(error.as_ref(), Some(CameraError::Decode(_))) {
                *error = None;
            }
        });
        let mask = if job.cutout && !matte_failed {
            if processor.is_none() {
                match CutoutProcessor::new() {
                    Ok(created) => processor = Some(created),
                    Err(error) => {
                        matte_failed = true;
                        with_current_generation(shared.as_ref(), &job, || {
                            *shared.error.lock().unwrap() = Some(CameraError::Cutout(format!(
                                "the camera cutout could not start: {error}"
                            )));
                        });
                    }
                };
            }
            match processor.as_mut() {
                Some(processor) => match processor.matte(&pixels, (width, height)) {
                    Ok(mut mask) => {
                        refine_matte(&mut mask, (width, height), job.matte.0, job.matte.1);
                        Some(mask)
                    }
                    Err(error) => {
                        with_current_generation(shared.as_ref(), &job, || {
                            *shared.error.lock().unwrap() = Some(CameraError::Cutout(format!(
                                "the camera cutout failed: {error}"
                            )));
                        });
                        None
                    }
                },
                None => None,
            }
        } else {
            None
        };
        publish_frame(shared.as_ref(), &job, pixels, width, height, mask);
    }
}

fn decode_frame(path: &Path, seconds: f64, size: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    let (width, height) = size;
    let frame_len = width as usize * height as usize * 4;
    let mut child = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        .args(["-ss", &format!("{seconds:.6}")])
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1"])
        .args(["-vf", &format!("scale={width}:{height},setsar=1")])
        .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let mut buffer = vec![0u8; frame_len];
    let read = stdout.read_exact(&mut buffer).is_ok();
    let _ = child.kill();
    let _ = child.wait();
    read.then_some((buffer, width, height))
}

/// The preview decode size, at the camera's own aspect. A portrait camera is
/// bounded by its height rather than squeezed into a landscape frame.
fn preview_size(camera_size: (u32, u32)) -> (u32, u32) {
    let aspect = if camera_size.0 > 0 && camera_size.1 > 0 {
        f64::from(camera_size.0) / f64::from(camera_size.1)
    } else {
        16.0 / 9.0
    };
    let mut width = PREVIEW_WIDTH;
    let mut height = (f64::from(width) / aspect).round().max(2.0) as u32;
    if height > PREVIEW_MAX_HEIGHT {
        height = PREVIEW_MAX_HEIGHT;
        width = (f64::from(height) * aspect).round().max(2.0) as u32;
    }
    (width, height)
}

/// The export decode size: the camera's aspect, capped at the output frame so
/// the cover crop can never need more pixels than the composition has.
pub(super) fn camera_decode_size(camera: (u32, u32), output: (u32, u32)) -> (u32, u32) {
    if camera.0 == 0 || camera.1 == 0 || output.0 == 0 || output.1 == 0 {
        return (output.0.max(2), output.1.max(2));
    }
    let scale = (f64::from(output.0) / f64::from(camera.0))
        .min(f64::from(output.1) / f64::from(camera.1))
        .min(1.0);
    let width = (f64::from(camera.0) * scale).round().max(2.0) as u32;
    let height = (f64::from(camera.1) * scale).round().max(2.0) as u32;
    (width, height)
}

/// A sequential reader for the export: frames are read in lock step with the
/// video's, so the two timelines cannot drift apart inside a span.
pub(super) struct TrackReader {
    child: Child,
    stdout: ChildStdout,
    size: (u32, u32),
    buffer: Vec<u8>,
    last: Vec<u8>,
    finished: bool,
}

impl TrackReader {
    /// Open the track over one kept span.
    ///
    /// `time_scale` is the track's clock against the video's, so a camera that
    /// recorded faster or slower than the screen is resampled onto the output
    /// timeline instead of playing at 1x and drifting.
    /// `lead_seconds` holds the first frame before the track starts, for a span
    /// that opens before the camera did.
    pub fn open(
        path: &Path,
        start_seconds: f64,
        duration_seconds: f64,
        fps: f64,
        size: (u32, u32),
        time_scale: f64,
        lead_seconds: f64,
    ) -> Option<Self> {
        let frame_len = size.0 as usize * size.1 as usize * 4;
        let frames = (duration_seconds.max(0.0) * fps).ceil().max(1.0) as u64;
        let time_scale = if time_scale.is_finite() && time_scale > 0.0 {
            time_scale
        } else {
            1.0
        };
        let lead_seconds = if lead_seconds.is_finite() {
            lead_seconds.max(0.0)
        } else {
            0.0
        };
        let mut filter = format!(
            "scale={}:{},setsar=1,setpts=(PTS-STARTPTS)*{time_scale:.9},fps={fps}",
            size.0, size.1
        );
        if lead_seconds * fps >= 0.5 {
            filter.push_str(&format!(
                ",tpad=start_mode=clone:start_duration={lead_seconds:.6}"
            ));
        }
        let mut child = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-ss", &format!("{start_seconds:.6}")])
            .arg("-i")
            .arg(path)
            .args(["-vf", &filter])
            .args(["-frames:v", &frames.to_string()])
            .args(["-f", "rawvideo", "-pix_fmt", "bgra", "pipe:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        Some(Self {
            child,
            stdout,
            size,
            buffer: vec![0u8; frame_len],
            last: Vec::new(),
            finished: false,
        })
    }

    /// The next frame, or the last one again when the track has run out, which
    /// is what a camera that stopped early should look like.
    pub fn next_frame(&mut self) -> Option<(&[u8], (u32, u32))> {
        if !self.finished {
            if self.stdout.read_exact(&mut self.buffer).is_ok() {
                self.last.clone_from(&self.buffer);
                return Some((&self.last, self.size));
            }
            self.finished = true;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        (!self.last.is_empty()).then_some((self.last.as_slice(), self.size))
    }
}

impl Drop for TrackReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Draw the layer over a composed RGBA frame.
///
/// This is the single-layer helper the editor started with; the layout
/// renderer is [`composite_layout_into`]. It stays for the saved single-layer
/// behaviour and its tests.
///
/// The layer holds the frame's own pixels under the camera, so the antialiased
/// edge blends into the footage and the result can be copied back whole. Only
/// the layer's box is ever touched, so the cost follows the camera and not the
/// frame size.
#[allow(dead_code)]
pub(super) fn composite_into(
    pixels: &mut [u8],
    size: (u32, u32),
    settings: LayerSettings,
    frame: &[u8],
    frame_size: (u32, u32),
) {
    if size.0 == 0 || size.1 == 0 || frame_size.0 == 0 || frame_size.1 == 0 {
        return;
    }
    let rect = settings.rect_in(Rect {
        x: 0.0,
        y: 0.0,
        width: f64::from(size.0),
        height: f64::from(size.1),
    });
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = (rect.x + rect.width).round().clamp(0.0, f64::from(size.0)) as u32;
    let y1 = (rect.y + rect.height).round().clamp(0.0, f64::from(size.1)) as u32;
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width < 2 || height < 2 {
        return;
    }

    let Ok(mut layer) = ImageSurface::create(Format::ARgb32, width as i32, height as i32) else {
        return;
    };
    let frame_stride = size.0 as usize * 4;
    let stride = layer.stride() as usize;
    {
        let Ok(mut data) = layer.data() else {
            return;
        };
        for y in 0..height as usize {
            for x in 0..width as usize {
                let source = (y0 as usize + y) * frame_stride + (x0 as usize + x) * 4;
                let target = y * stride + x * 4;
                let pixel = &pixels[source..source + 4];
                data[target] = pixel[2];
                data[target + 1] = pixel[1];
                data[target + 2] = pixel[0];
                data[target + 3] = 255;
            }
        }
    }

    let Ok(image) = ImageSurface::create_for_data(
        frame.to_vec(),
        Format::ARgb32,
        frame_size.0 as i32,
        frame_size.1 as i32,
        frame_size.0 as i32 * 4,
    ) else {
        return;
    };
    {
        let Ok(context) = Context::new(&layer) else {
            return;
        };
        draw_bubble(
            &context,
            Rect {
                x: 0.0,
                y: 0.0,
                width: f64::from(width),
                height: f64::from(height),
            },
            settings.radius_px(Rect {
                x: 0.0,
                y: 0.0,
                width: f64::from(width),
                height: f64::from(height),
            }),
            settings.mirror,
            &image,
        );
    }
    layer.mark_dirty();
    {
        let Ok(data) = layer.data() else {
            return;
        };
        for y in 0..height as usize {
            for x in 0..width as usize {
                let target = (y0 as usize + y) * frame_stride + (x0 as usize + x) * 4;
                let source = y * stride + x * 4;
                pixels[target] = data[source + 2];
                pixels[target + 1] = data[source + 1];
                pixels[target + 2] = data[source];
                pixels[target + 3] = 255;
            }
        }
    }
}

/// Composite one layout frame's screen and camera onto an RGBA buffer.
///
/// The screen is cover-resampled into the split's pane first, which is how the
/// footage genuinely fills its share instead of being masked over; the cursor
/// and the ripple are already inside those pixels. The camera then covers its
/// own rect from the raw camera aspect, with the person's matte blended in when
/// a cutout is on screen.
pub(super) fn composite_layout_into(
    pixels: &mut [u8],
    size: (u32, u32),
    frame: LayoutFrame,
    camera: &[u8],
    camera_size: (u32, u32),
    mask: Option<&[f32]>,
) -> Result<(), String> {
    if size.0 == 0 || size.1 == 0 {
        return Ok(());
    }
    shrink_screen(pixels, size, frame.screen);
    if frame.camera_alpha <= 0.0 {
        return Ok(());
    }
    if camera_size.0 == 0 || camera_size.1 == 0 {
        return Err("the camera frame has no size".to_string());
    }
    let (x0, y0, x1, y1) = pixel_rect(frame.camera, size);
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width < 2 || height < 2 {
        return Ok(());
    }
    let camera_image = camera_surface(camera, camera_size, mask, frame.cutout)?;
    let mut layer = ImageSurface::create(Format::ARgb32, width as i32, height as i32)
        .map_err(|error| format!("could not allocate the camera layer: {error}"))?;
    {
        let context = Context::new(&layer)
            .map_err(|error| format!("could not draw the camera layer: {error}"))?;
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(width),
            height: f64::from(height),
        };
        let radius = frame.camera_radius * f64::from(width.min(height)) / 2.0;
        draw_camera_cover(&context, rect, radius, frame.mirror, 1.0, &camera_image);
    }
    layer.mark_dirty();
    blend_layer(
        pixels,
        size,
        (x0, y0, x1, y1),
        &mut layer,
        frame.camera_alpha,
    );
    Ok(())
}

/// The studied camera blur strength: the motion blur amount scaled to the
/// frame rate, and nothing while the camera is hidden.
pub(super) fn camera_blur_strength(amount: f64, fps: f64, camera_alpha: f64) -> f64 {
    if !amount.is_finite()
        || !fps.is_finite()
        || fps <= 0.0
        || camera_alpha.is_nan()
        || camera_alpha <= 0.0
    {
        return 0.0;
    }
    amount.clamp(0.0, 1.0) * fps / 60.0
}

/// How many layouts to average along a camera trail from `before` to `now`
/// on a frame of `size` pixels: one when neither the camera nor the screen
/// moved a pixel, otherwise one per 4 pixels of the longest move, 2..=16.
pub(super) fn trail_sample_count(now: LayoutFrame, before: LayoutFrame, size: (u32, u32)) -> usize {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let edges = |r: Rect| [r.x * w, r.y * h, (r.x + r.width) * w, (r.y + r.height) * h];
    let mut longest: f64 = 0.0;
    for (a, b) in [(now.camera, before.camera), (now.screen, before.screen)] {
        for (p, q) in edges(a).into_iter().zip(edges(b)) {
            let d = (p - q).abs();
            if d.is_finite() {
                longest = longest.max(d);
            }
        }
    }
    if longest < 1.0 {
        return 1;
    }
    ((longest / 4.0).ceil() as usize + 1).clamp(2, 16)
}

/// Composite the camera along its trail: every layout in `frames` is drawn
/// over its own copy of the frame and the copies are averaged in squared
/// light. One layout is exactly [`composite_layout_into`].
pub(super) fn composite_layout_trail_into(
    pixels: &mut [u8],
    size: (u32, u32),
    frames: &[LayoutFrame],
    camera: &[u8],
    camera_size: (u32, u32),
    mask: Option<&[f32]>,
) -> Result<(), String> {
    match frames {
        [] => return Ok(()),
        [frame] => return composite_layout_into(pixels, size, *frame, camera, camera_size, mask),
        _ => {}
    }
    let base = pixels.to_vec();
    let mut scratch = vec![0u8; pixels.len()];
    let mut sum = vec![0.0f32; pixels.len()];
    for frame in frames {
        scratch.copy_from_slice(&base);
        composite_layout_into(&mut scratch, size, *frame, camera, camera_size, mask)?;
        for (s, v) in sum.iter_mut().zip(&scratch) {
            let v = f32::from(*v);
            *s += v * v;
        }
    }
    let n = frames.len() as f32;
    for (p, s) in pixels.iter_mut().zip(&sum) {
        *p = (s / n).sqrt().round().clamp(0.0, 255.0) as u8;
    }
    Ok(())
}

/// [`composite_layout_trail_into`] for a stage wider than the screen: each
/// layout places the whole screen on the stage first, then the camera.
#[allow(clippy::too_many_arguments)]
pub(super) fn composite_stage_trail_into(
    stage: &mut [u8],
    stage_size: (u32, u32),
    frames: &[LayoutFrame],
    screen: &[u8],
    screen_size: (u32, u32),
    background: Option<&[u8]>,
    camera: &[u8],
    camera_size: (u32, u32),
    mask: Option<&[f32]>,
) -> Result<(), String> {
    let draw = |target: &mut [u8], frame: LayoutFrame| {
        place_screen_into(
            target,
            stage_size,
            screen,
            screen_size,
            frame.screen,
            background,
            frame.screen_radius,
        );
        composite_layout_into(
            target,
            stage_size,
            LayoutFrame {
                screen: Rect::full(),
                ..frame
            },
            camera,
            camera_size,
            mask,
        )
    };
    match frames {
        [] => return Ok(()),
        [frame] => return draw(stage, *frame),
        _ => {}
    }
    let mut scratch = vec![0u8; stage.len()];
    let mut sum = vec![0.0f32; stage.len()];
    for frame in frames {
        draw(&mut scratch, *frame)?;
        for (s, v) in sum.iter_mut().zip(&scratch) {
            let v = f32::from(*v);
            *s += v * v;
        }
    }
    let n = frames.len() as f32;
    for (p, s) in stage.iter_mut().zip(&sum) {
        *p = (s / n).sqrt().round().clamp(0.0, 255.0) as u8;
    }
    Ok(())
}

fn camera_surface(
    camera: &[u8],
    camera_size: (u32, u32),
    mask: Option<&[f32]>,
    cutout: f64,
) -> Result<ImageSurface, String> {
    let pixels = if cutout > 1e-3 {
        camera_premultiplied(camera, camera_size, mask, cutout)?
    } else {
        camera.to_vec()
    };
    ImageSurface::create_for_data(
        pixels,
        Format::ARgb32,
        camera_size.0 as i32,
        camera_size.1 as i32,
        camera_size.0 as i32 * 4,
    )
    .map_err(|error| format!("could not allocate the camera frame: {error}"))
}

/// Blend a premultiplied camera layer into the frame at `alpha`. Cairo stores
/// BGRA and the export frame is RGBA, so each channel moves as it blends.
fn blend_layer(
    pixels: &mut [u8],
    size: (u32, u32),
    rect: (u32, u32, u32, u32),
    layer: &mut ImageSurface,
    alpha: f64,
) {
    let (x0, y0, x1, y1) = rect;
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    let stride = layer.stride() as usize;
    let frame_stride = size.0 as usize * 4;
    let alpha = alpha.clamp(0.0, 1.0);
    let Ok(data) = layer.data() else {
        return;
    };
    for y in 0..height as usize {
        let source_row = y * stride;
        let frame_row = (y0 as usize + y) * frame_stride + x0 as usize * 4;
        for x in 0..width as usize {
            let source = &data[source_row + x * 4..source_row + x * 4 + 4];
            let source_alpha = f64::from(source[3]) / 255.0 * alpha;
            let inverse = 1.0 - source_alpha;
            let target = &mut pixels[frame_row + x * 4..frame_row + x * 4 + 4];
            target[0] =
                (f64::from(source[2]) * alpha + f64::from(target[0]) * inverse).round() as u8;
            target[1] =
                (f64::from(source[1]) * alpha + f64::from(target[1]) * inverse).round() as u8;
            target[2] =
                (f64::from(source[0]) * alpha + f64::from(target[2]) * inverse).round() as u8;
            target[3] = 255;
        }
    }
}

/// Resample the screen into the layout's pane, filling the rest black.
///
/// The pane is written at `screen`'s bounds while the pixels come from the
/// cover rect, so a pane without the footage's aspect shows the middle of the
/// picture instead of shrinking the whole of it into a letterbox. A full rect
/// is left untouched, which is every layout but the split.
fn shrink_screen(pixels: &mut [u8], size: (u32, u32), screen: Rect) {
    if screen.is_full() {
        return;
    }
    let (x0, y0, x1, y1) = pixel_rect(screen, size);
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width == 0 || height == 0 {
        return;
    }
    if x0 == 0 && y0 == 0 && x1 == size.0 && y1 == size.1 {
        return;
    }
    let scaled = resample_rgba(pixels, size, screen_cover_rect(screen), (x0, y0, x1, y1));
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }
    let frame_stride = size.0 as usize * 4;
    let row_bytes = width as usize * 4;
    for y in 0..height as usize {
        let source = &scaled[y * row_bytes..(y + 1) * row_bytes];
        let target = (y0 as usize + y) * frame_stride + x0 as usize * 4;
        pixels[target..target + row_bytes].copy_from_slice(source);
    }
}

/// Draw the whole screen (`screen`, `screen_size` pixels) onto a stage of
/// `stage_size` pixels so it covers `pane`, and fill the rest of the stage
/// with `background` (RGBA at `stage_size`) or black. `radius` rounds the
/// pane's corners, as a fraction of half its shorter side.
pub(super) fn place_screen_into(
    stage: &mut [u8],
    stage_size: (u32, u32),
    screen: &[u8],
    screen_size: (u32, u32),
    pane: Rect,
    background: Option<&[u8]>,
    radius: f64,
) {
    let fill_from = background.filter(|background| background.len() == stage.len());
    match fill_from {
        Some(background) => stage.copy_from_slice(background),
        None => {
            for pixel in stage.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    if stage_size.0 == 0 || stage_size.1 == 0 || screen_size.0 == 0 || screen_size.1 == 0 {
        return;
    }
    let (x0, y0, x1, y1) = pixel_rect(pane, stage_size);
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width == 0 || height == 0 {
        return;
    }
    let cover = screen_cover_rect_in(
        pane,
        f64::from(stage_size.0) / f64::from(stage_size.1),
        f64::from(screen_size.0) / f64::from(screen_size.1),
    );
    let scaled = resample_rgba_from(screen, screen_size, stage_size, cover, (x0, y0, x1, y1));
    let stride = stage_size.0 as usize * 4;
    let row_bytes = width as usize * 4;
    for y in 0..height as usize {
        let source = &scaled[y * row_bytes..(y + 1) * row_bytes];
        let target = (y0 as usize + y) * stride + x0 as usize * 4;
        stage[target..target + row_bytes].copy_from_slice(source);
    }
    if radius > 0.0 {
        let r = radius.min(1.0) * f64::from(width.min(height)) / 2.0;
        if r > 0.0 {
            let reach = r.ceil().min(f64::from(width)).min(f64::from(height)) as u32;
            let mut round = |x: u32, y: u32, cx: f64, cy: f64, left: bool, top: bool| {
                let px = f64::from(x) + 0.5;
                let py = f64::from(y) + 0.5;
                let outside_x = if left { px < cx } else { px > cx };
                let outside_y = if top { py < cy } else { py > cy };
                if !(outside_x && outside_y) {
                    return;
                }
                let d = ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt();
                let coverage = (r - d + 0.5).clamp(0.0, 1.0);
                if coverage >= 1.0 {
                    return;
                }
                let index = (y0 as usize + y as usize) * stride + (x0 as usize + x as usize) * 4;
                let mut fill = [0u8; 3];
                if let Some(background) = fill_from {
                    fill.copy_from_slice(&background[index..index + 3]);
                }
                for channel in 0..3 {
                    let pane = f64::from(stage[index + channel]);
                    let fill = f64::from(fill[channel]);
                    stage[index + channel] = (fill + (pane - fill) * coverage).round() as u8;
                }
                stage[index + 3] = 255;
            };
            for y in 0..reach {
                for x in 0..reach {
                    round(x, y, r, r, true, true);
                    round(width - 1 - x, y, f64::from(width) - r, r, false, true);
                    round(x, height - 1 - y, r, f64::from(height) - r, true, false);
                    round(
                        width - 1 - x,
                        height - 1 - y,
                        f64::from(width) - r,
                        f64::from(height) - r,
                        false,
                        false,
                    );
                }
            }
        }
    }
}

/// Bilinear RGBA resize of the source region `cover` into `destination`.
///
/// `cover` is in normalized frame units and may reach outside the frame on the
/// axis it crops; each destination pixel reads the point of the frame it sits
/// on, mapped through the cover rect onto the source. `destination` is in
/// pixels of the same frame, so the output is exactly the pane's size.
fn resample_rgba(
    pixels: &[u8],
    size: (u32, u32),
    cover: Rect,
    destination: (u32, u32, u32, u32),
) -> Vec<u8> {
    resample_rgba_from(pixels, size, size, cover, destination)
}

/// [`resample_rgba`] reading a source of `source_size` pixels while `cover`
/// and `destination` are measured on a frame of `frame` pixels.
fn resample_rgba_from(
    pixels: &[u8],
    source_size: (u32, u32),
    frame: (u32, u32),
    cover: Rect,
    destination: (u32, u32, u32, u32),
) -> Vec<u8> {
    let (x0, y0, x1, y1) = destination;
    let out_width = x1.saturating_sub(x0) as usize;
    let out_height = y1.saturating_sub(y0) as usize;
    let mut output = vec![0u8; out_width * out_height * 4];
    if out_width == 0
        || out_height == 0
        || source_size.0 == 0
        || source_size.1 == 0
        || frame.0 == 0
        || frame.1 == 0
    {
        return output;
    }
    let source_width = source_size.0 as usize;
    let source_height = source_size.1 as usize;
    let frame_width = f64::from(frame.0);
    let frame_height = f64::from(frame.1);
    let start_x = (x0 as f64 + 0.5) / frame_width;
    let start_y = (y0 as f64 + 0.5) / frame_height;
    let base_x = (start_x - cover.x) / cover.width;
    let base_y = (start_y - cover.y) / cover.height;
    let step_x = 1.0 / (frame_width * cover.width);
    let step_y = 1.0 / (frame_height * cover.height);
    output
        .par_chunks_mut(out_width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let normalized_y = base_y + y as f64 * step_y;
            let source_y = normalized_y * f64::from(source_size.1) - 0.5;
            let clamped_y = source_y.clamp(0.0, source_height.saturating_sub(1) as f64);
            let y0_index = clamped_y as usize;
            let y1_index = (y0_index + 1).min(source_height.saturating_sub(1));
            let fy = clamped_y - y0_index as f64;
            for x in 0..out_width {
                let normalized_x = base_x + x as f64 * step_x;
                let source_x = normalized_x * f64::from(source_size.0) - 0.5;
                let clamped_x = source_x.clamp(0.0, source_width.saturating_sub(1) as f64);
                let x0_index = clamped_x as usize;
                let x1_index = (x0_index + 1).min(source_width.saturating_sub(1));
                let fx = clamped_x - x0_index as f64;
                let top_left = (y0_index * source_width + x0_index) * 4;
                let top_right = (y0_index * source_width + x1_index) * 4;
                let bottom_left = (y1_index * source_width + x0_index) * 4;
                let bottom_right = (y1_index * source_width + x1_index) * 4;
                let target = x * 4;
                for channel in 0..4 {
                    let top = f64::from(pixels[top_left + channel]) * (1.0 - fx)
                        + f64::from(pixels[top_right + channel]) * fx;
                    let bottom = f64::from(pixels[bottom_left + channel]) * (1.0 - fx)
                        + f64::from(pixels[bottom_right + channel]) * fx;
                    row[target + channel] =
                        (top * (1.0 - fy) + bottom * fy + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        });
    output
}

fn pixel_rect(rect: Rect, size: (u32, u32)) -> (u32, u32, u32, u32) {
    let width = f64::from(size.0);
    let height = f64::from(size.1);
    let x0 = (rect.x * width).round().clamp(0.0, width) as u32;
    let y0 = (rect.y * height).round().clamp(0.0, height) as u32;
    let x1 = ((rect.x + rect.width) * width).round().clamp(0.0, width) as u32;
    let y1 = ((rect.y + rect.height) * height).round().clamp(0.0, height) as u32;
    (x0, y0, x1.max(x0), y1.max(y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_editor::timeline::{ZoomClip, ZoomMode};

    fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Vec<u8> {
        bgra.iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect()
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = (y as usize * width as usize + x as usize) * 4;
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }

    const RED: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [255, 0, 0, 255];

    fn camera_with(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&colour(x, y));
            }
        }
        pixels
    }

    fn layer(x: f64, y: f64, w: f64, h: f64, radius: f64) -> LayerSettings {
        LayerSettings {
            x,
            y,
            w,
            h,
            radius,
            ..LayerSettings::default()
        }
    }

    fn cue(at: f64, layout: CameraLayout) -> LayoutCue {
        LayoutCue {
            at,
            layout,
            position: None,
            cutout_x: None,
        }
    }

    fn settings(cues: Vec<LayoutCue>, transition: f64) -> LayoutSettings {
        LayoutSettings {
            cues,
            transition_seconds: transition,
            ..LayoutSettings::default()
        }
    }

    fn resting(settings: &LayoutSettings, seconds: f64) -> LayoutFrame {
        settings.clone().clamped().frame_at(
            seconds,
            LayerSettings::default(),
            16.0 / 9.0,
            16.0 / 9.0,
        )
    }

    #[test]
    fn a_new_layout_track_opens_on_the_bubble() {
        let settings = LayoutSettings::default();
        assert_eq!(settings.default_layout, CameraLayout::CameraOverlay);
        assert!(settings.cues.is_empty());
        assert!((settings.transition_seconds - 0.45).abs() < 1e-9);
        assert!((settings.split_ratio - 0.5).abs() < 1e-9);
        assert!(!settings.split_flip);
        assert!(!settings.uses_cutout());
        assert!(settings.uses_camera());
    }

    #[test]
    fn a_switched_off_layer_paints_the_screen_alone() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::FullscreenCamera),
                cue(2.0, CameraLayout::CameraCutout),
            ],
            0.45,
        );
        assert!(resting(&settings, 1.5).camera_alpha > 0.9, "on paints it");

        let off = LayerSettings {
            enabled: false,
            ..LayerSettings::default()
        };
        for seconds in [0.0, 1.0, 1.5, 2.5] {
            let frame = settings
                .clone()
                .clamped()
                .frame_at(seconds, off, 16.0 / 9.0, 16.0 / 9.0);
            assert_eq!(
                frame.camera_alpha, 0.0,
                "no camera pixels at {seconds} while switched off"
            );
            assert_eq!(frame.cutout, 0.0, "and no matte at {seconds}");
        }
    }

    #[test]
    fn a_change_can_carry_its_own_camera_position() {
        let frame = 16.0 / 9.0;
        let mut layer = LayerSettings::default();
        layer.set_overlay_alignment(1.0, 1.0);
        let layouts = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            cues: vec![
                LayoutCue {
                    at: 2.0,
                    layout: CameraLayout::CameraOverlay,
                    position: Some([0.0, 0.0]),
                    cutout_x: None,
                },
                LayoutCue {
                    at: 6.0,
                    layout: CameraLayout::CameraOverlay,
                    position: None,
                    cutout_x: None,
                },
            ],
            ..LayoutSettings::default()
        };
        let shared = layouts.frame_at(1.0, layer, frame, 1.0).camera;
        assert!(
            (shared.x + shared.width - 1.0).abs() < 1e-9,
            "the default sits bottom-right"
        );
        let own = layouts.frame_at(5.0, layer, frame, 1.0).camera;
        assert!(
            own.x.abs() < 1e-9 && own.y.abs() < 1e-9,
            "the change sits top-left: {own:?}"
        );
        let back = layouts.frame_at(9.0, layer, frame, 1.0).camera;
        assert!(
            (back.x + back.width - 1.0).abs() < 1e-9,
            "a change without a position uses the shared one"
        );
    }

    #[test]
    fn a_change_without_a_position_reads_as_none() {
        let cue: LayoutCue =
            serde_json::from_str(r#"{"at":1.0,"layout":"camera_overlay"}"#).unwrap();
        assert_eq!(cue.position, None);
        assert_eq!(cue.cutout_x, None);
    }

    #[test]
    fn hand_edited_cues_are_ordered_deduped_and_finite() {
        let settings = LayoutSettings {
            cues: vec![
                cue(3.0, CameraLayout::SplitScreen),
                cue(-1.0, CameraLayout::FullscreenCamera),
                cue(1.0, CameraLayout::ScreenOnly),
                cue(1.0, CameraLayout::CameraCutout),
                cue(f64::NAN, CameraLayout::FullscreenCamera),
                cue(2.0, CameraLayout::FullscreenCamera),
            ],
            transition_seconds: f64::INFINITY,
            split_ratio: 4.0,
            ..LayoutSettings::default()
        }
        .clamped();
        assert!((settings.transition_seconds - 0.45).abs() < 1e-9);
        assert!((settings.split_ratio - MAX_SPLIT_RATIO).abs() < 1e-9);
        assert_eq!(
            settings.cues,
            vec![
                cue(1.0, CameraLayout::CameraCutout),
                cue(2.0, CameraLayout::FullscreenCamera),
                cue(3.0, CameraLayout::SplitScreen),
            ]
        );
        assert!(settings.uses_cutout());
        assert!(settings.uses_camera());

        let split = LayoutSettings {
            default_layout: CameraLayout::ScreenOnly,
            split_ratio: -2.0,
            ..LayoutSettings::default()
        }
        .clamped();
        assert_eq!(split.split_ratio, MIN_SPLIT_RATIO);
        assert!(!split.uses_camera());
    }

    #[test]
    fn the_target_layout_is_the_newest_cue_in_the_past() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::FullscreenCamera),
                cue(2.0, CameraLayout::ScreenOnly),
            ],
            0.45,
        );
        assert_eq!(settings.layout_at(-1.0), CameraLayout::CameraOverlay);
        assert_eq!(settings.layout_at(0.5), CameraLayout::CameraOverlay);
        assert_eq!(settings.layout_at(1.0), CameraLayout::FullscreenCamera);
        assert_eq!(settings.layout_at(1.9), CameraLayout::FullscreenCamera);
        assert_eq!(settings.layout_at(2.0), CameraLayout::ScreenOnly);
        assert_eq!(settings.layout_at(f64::NAN), CameraLayout::CameraOverlay);
    }

    #[test]
    fn a_split_gives_the_screen_its_whole_share_of_the_frame() {
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            ..LayoutSettings::default()
        };
        let frame = resting(&split, 0.0);
        assert!((frame.screen.x - 0.0).abs() < 1e-9);
        assert!((frame.screen.y - 0.0).abs() < 1e-9);
        assert!((frame.screen.width - 0.5).abs() < 1e-9);
        assert!((frame.screen.height - 1.0).abs() < 1e-9);
        assert!((frame.camera.x - 0.5).abs() < 1e-9);
        assert!((frame.camera.width - 0.5).abs() < 1e-9);
        assert!((frame.camera.height - 1.0).abs() < 1e-9);
        assert_eq!(frame.camera_alpha, 1.0);

        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            split_flip: true,
            ..LayoutSettings::default()
        };
        let flipped = resting(&split, 0.0);
        assert!(flipped.camera.x < 1e-9);
        assert!((flipped.screen.x - 0.5).abs() < 1e-9);
        assert!((flipped.screen.width - 0.5).abs() < 1e-9);
        assert!((flipped.screen.height - 1.0).abs() < 1e-9);

        let portrait = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            ..LayoutSettings::default()
        }
        .frame_at(0.0, LayerSettings::default(), 9.0 / 16.0, 4.0 / 3.0);
        assert!((portrait.screen.x - 0.0).abs() < 1e-9);
        assert!((portrait.screen.y - 0.0).abs() < 1e-9);
        assert!((portrait.screen.width - 1.0).abs() < 1e-9);
        assert!((portrait.screen.height - 0.5).abs() < 1e-9);
        assert!((portrait.camera.y - 0.5).abs() < 1e-9);
        assert!((portrait.camera.width - 1.0).abs() < 1e-9);
        assert!((portrait.camera.height - 0.5).abs() < 1e-9);

        let wide_share = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            split_ratio: 0.3,
            ..LayoutSettings::default()
        };
        let frame = resting(&wide_share, 0.0);
        assert!((frame.screen.width - 0.3).abs() < 1e-9);
        assert!((frame.camera.x - 0.3).abs() < 1e-9);
        assert!((frame.camera.width - 0.7).abs() < 1e-9);
        assert!((frame.screen.height - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_automatic_split_keeps_the_screen_shape_when_the_camera_column_fits() {
        // A 16:10 recording in its own frame leaves no column: the camera gets
        // half the height's width and the screen keeps 1 - 0.5/1.6.
        assert!((auto_split_screen_ratio(1.6, 1.6) - 0.6875).abs() < 1e-9);
        assert!((auto_split_screen_ratio(4.0 / 3.0, 4.0 / 3.0) - 0.625).abs() < 1e-9);
        // A frame widened for the camera: the screen keeps exactly its shape.
        let wide = 1.6 + 0.5625;
        assert!((auto_split_screen_ratio(wide, 1.6) - 1.6 / wide).abs() < 1e-9);
        // A column no wider than 0.4 of the height falls back to half.
        assert!((auto_split_screen_ratio(2.0, 1.6) - (1.0 - 0.5 / 2.0)).abs() < 1e-9);
        // Stacked splits halve the frame.
        assert_eq!(auto_split_screen_ratio(9.0 / 16.0, 9.0 / 16.0), 0.5);
        // The layout uses it unless the project turned it off.
        let layer = LayerSettings::default();
        let auto = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            ..LayoutSettings::default()
        };
        assert!((auto.frame_at(0.0, layer, 1.6, 1.0).screen.width - 0.6875).abs() < 1e-9);
        let fixed = LayoutSettings {
            split_auto: false,
            ..auto.clone()
        };
        assert!((fixed.frame_at(0.0, layer, 1.6, 1.0).screen.width - 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_cover_rect_of_a_screen_pane_keeps_the_sources_aspect() {
        assert_eq!(screen_cover_rect(Rect::full()), Rect::full());

        let contained = Rect {
            x: 0.0,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        };
        assert_eq!(screen_cover_rect(contained), contained);
        let portrait_contained = Rect {
            x: 0.25,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        };
        assert_eq!(screen_cover_rect(portrait_contained), portrait_contained);

        let wide = screen_cover_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        });
        assert!((wide.x + 0.25).abs() < 1e-9);
        assert!(wide.y.abs() < 1e-9);
        assert!((wide.width - 1.0).abs() < 1e-9);
        assert!((wide.height - 1.0).abs() < 1e-9);

        let flipped = screen_cover_rect(Rect {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        });
        assert!((flipped.x - 0.25).abs() < 1e-9);
        assert!((flipped.width - 1.0).abs() < 1e-9);

        let tall = screen_cover_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 0.5,
        });
        assert!(tall.x.abs() < 1e-9);
        assert!((tall.y + 0.25).abs() < 1e-9);
        assert!((tall.width - 1.0).abs() < 1e-9);
        assert!((tall.height - 1.0).abs() < 1e-9);

        let guarded = screen_cover_rect(Rect {
            x: f64::NAN,
            y: 0.0,
            width: 0.0,
            height: f64::INFINITY,
        });
        assert!((guarded.x - 0.0).abs() < 1e-9);
        assert!((guarded.y - 0.0).abs() < 1e-9);
        assert!((guarded.width - 1.0).abs() < 1e-9);
        assert!((guarded.height - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_fullscreen_camera_covers_while_screen_only_leaves_none() {
        let fullscreen = LayoutSettings {
            default_layout: CameraLayout::FullscreenCamera,
            ..LayoutSettings::default()
        };
        let frame = resting(&fullscreen, 0.0);
        assert_eq!(frame.camera, Rect::full());
        assert_eq!(frame.camera_alpha, 1.0);
        assert_eq!(frame.camera_radius, 0.0);

        let screen = LayoutSettings {
            default_layout: CameraLayout::ScreenOnly,
            ..LayoutSettings::default()
        };
        let frame = resting(&screen, 0.0);
        assert_eq!(frame.camera_alpha, 0.0);
        assert_eq!(frame.screen, Rect::full());
    }

    #[test]
    fn transitions_move_between_the_resting_rects() {
        let settings = LayoutSettings {
            split_auto: false,
            ..settings(
                vec![
                    cue(1.0, CameraLayout::SplitScreen),
                    cue(2.0, CameraLayout::FullscreenCamera),
                ],
                1.0,
            )
        };
        let before = resting(&settings, 1.0);
        let middle = resting(&settings, 1.5);
        let end = resting(&settings, 5.0);
        let p = progress(1.0, 1.5, 1.0);
        assert!(p > 0.0 && p < 1.0, "half way through the move: {p}");
        assert!((before.screen.width - 1.0).abs() < 1e-9);
        assert!((middle.screen.width - (1.0 - 0.5 * p)).abs() < 1e-9);
        assert!(
            (middle.screen.height - 1.0).abs() < 1e-9,
            "the growing split pane keeps the frame's full height"
        );
        assert!((middle.camera.y - 0.55 * (1.0 - p)).abs() < 1e-9);
        // Under the fullscreen camera the screen keeps the split's pane.
        assert!((end.screen.width - 0.5).abs() < 1e-9);
        assert!(end.camera.is_full());
        assert!((middle.camera_alpha - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_layout_change_moves_on_the_studied_spring() {
        assert_eq!(progress(1.0, 1.0, 0.45), 0.0);
        assert_eq!(progress(1.0, 1.5, 0.0), 1.0, "a zero duration cuts");
        let mut previous = 0.0;
        let mut t = 0.0;
        while t <= 3.0 {
            let value = progress(0.0, t, 0.45);
            assert!(
                value >= previous - 1e-12 && value <= 1.0,
                "monotonic and clamped at {t}: {value}"
            );
            previous = value;
            t += 0.01;
        }
        // The studied spring is about 0.31 of the way after 0.1 s and has
        // settled within ~1.2 s.
        assert!(
            progress(0.0, 0.1, 0.45) < 0.35,
            "{}",
            progress(0.0, 0.1, 0.45)
        );
        assert!(
            progress(0.0, 0.5, 0.45) > 0.6,
            "{}",
            progress(0.0, 0.5, 0.45)
        );
        assert!(
            progress(0.0, 1.5, 0.45) > 0.99,
            "{}",
            progress(0.0, 1.5, 0.45)
        );
    }

    #[test]
    fn screen_only_cuts_the_camera_both_ways() {
        let layer = LayerSettings::default();
        let to_screen = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            cues: vec![LayoutCue {
                at: 1.0,
                layout: CameraLayout::ScreenOnly,
                position: None,
                cutout_x: None,
            }],
            ..LayoutSettings::default()
        };
        assert_eq!(
            to_screen
                .frame_at(1.01, layer, 16.0 / 9.0, 1.0)
                .camera_alpha,
            0.0
        );
        let from_screen = LayoutSettings {
            default_layout: CameraLayout::ScreenOnly,
            cues: vec![LayoutCue {
                at: 1.0,
                layout: CameraLayout::CameraOverlay,
                position: None,
                cutout_x: None,
            }],
            ..LayoutSettings::default()
        };
        let frame = from_screen.frame_at(1.01, layer, 16.0 / 9.0, 1.0);
        let rest = from_screen.frame_at(5.0, layer, 16.0 / 9.0, 1.0);
        assert_eq!(frame.camera_alpha, 1.0);
        assert_eq!(frame.camera, rest.camera, "the camera appears in place");
    }

    #[test]
    fn a_fullscreen_camera_holds_the_screen_and_switches_it_at_the_midpoint() {
        let layer = LayerSettings::default();
        let layouts = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            cues: vec![
                LayoutCue {
                    at: 2.0,
                    layout: CameraLayout::FullscreenCamera,
                    position: None,
                    cutout_x: None,
                },
                LayoutCue {
                    at: 6.0,
                    layout: CameraLayout::CameraOverlay,
                    position: None,
                    cutout_x: None,
                },
            ],
            ..LayoutSettings::default()
        };
        let aspect = 16.0 / 9.0;
        let split_screen = layouts.frame_at(1.0, layer, aspect, 1.0).screen;
        assert!(!split_screen.is_full());
        // Under the camera, before the midpoint (4.0): the split's screen.
        assert_eq!(
            layouts.frame_at(2.1, layer, aspect, 1.0).screen,
            split_screen
        );
        assert_eq!(
            layouts.frame_at(3.9, layer, aspect, 1.0).screen,
            split_screen
        );
        // From the midpoint (4.0) the screen springs, still under the camera,
        // to the overlay's screen (the whole frame) and settles there before
        // the camera leaves.
        let handing_on = layouts.frame_at(4.1, layer, aspect, 1.0).screen;
        assert!(handing_on.width > split_screen.width && handing_on.width < 1.0);
        assert!(layouts.frame_at(5.9, layer, aspect, 1.0).screen.is_full());
        // Leaving the fullscreen change, the screen is already in place.
        assert!(layouts.frame_at(6.05, layer, aspect, 1.0).screen.is_full());
    }

    #[test]
    fn a_cue_during_a_transition_continues_from_the_state_on_screen() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::SplitScreen),
                cue(1.2, CameraLayout::FullscreenCamera),
            ],
            1.0,
        );
        let before = resting(&settings, 1.2 - 1e-6);
        let at = resting(&settings, 1.2);
        let after = resting(&settings, 1.25);
        for (left, right) in [
            (before.screen.x, at.screen.x),
            (before.screen.width, at.screen.width),
            (before.camera.x, at.camera.x),
            (before.camera.width, at.camera.width),
        ] {
            assert!((left - right).abs() < 1e-4, "the picture jumped at the cue");
        }
        assert!(after.camera.width > at.camera.width);
        assert!(after.camera.width < 1.0);
    }

    #[test]
    fn a_redundant_cue_does_not_extend_the_transition() {
        let without = settings(vec![cue(1.0, CameraLayout::FullscreenCamera)], 1.0);
        let with = settings(
            vec![
                cue(1.0, CameraLayout::FullscreenCamera),
                cue(1.3, CameraLayout::FullscreenCamera),
            ],
            1.0,
        );
        assert_eq!(resting(&without, 1.5), resting(&with, 1.5));
        assert_eq!(resting(&without, 2.0), resting(&with, 2.0));
        // A spring settles rather than landing at a set time; once settled
        // the camera fills the frame.
        assert!((resting(&with, 4.5).camera.width - 1.0).abs() < 1e-9);
    }

    #[test]
    fn seeking_to_the_same_instant_draws_the_same_frame() {
        let settings = settings(
            vec![
                cue(0.5, CameraLayout::SplitScreen),
                cue(1.0, CameraLayout::CameraCutout),
                cue(1.5, CameraLayout::ScreenOnly),
            ],
            0.45,
        );
        let first = resting(&settings, 1.1);
        let _ = resting(&settings, 0.2);
        let _ = resting(&settings, 2.0);
        let second = resting(&settings, 1.1);
        assert_eq!(first, second);
    }

    #[test]
    fn a_cutout_sits_on_the_bottom_edge() {
        let settings = LayoutSettings {
            default_layout: CameraLayout::CameraCutout,
            ..LayoutSettings::default()
        };
        let wide = resting(&settings, 0.0);
        assert!((wide.camera.y + wide.camera.height - 1.0).abs() < 1e-9);
        assert!((wide.camera.height - 0.28125).abs() < 1e-9);
        assert!((wide.camera.x + wide.camera.width / 2.0 - 0.5).abs() < 1e-9);
        assert_eq!(wide.cutout, 1.0);
        assert!(settings.uses_cutout());

        let portrait = settings.frame_at(0.0, LayerSettings::default(), 16.0 / 9.0, 9.0 / 16.0);
        assert!(portrait.camera.width < wide.camera.width);
        assert!((portrait.camera.y + portrait.camera.height - 1.0).abs() < 1e-9);
        assert!(
            (portrait.camera.width / portrait.camera.height * 16.0 / 9.0 - 9.0 / 16.0).abs() < 1e-9
        );
    }

    #[test]
    fn the_cutout_takes_the_studied_size_on_the_bottom_edge() {
        let frame = 16.0 / 9.0;
        let layer = LayerSettings::default();
        let layouts = LayoutSettings {
            default_layout: CameraLayout::CameraCutout,
            ..LayoutSettings::default()
        };
        let rest = layouts.frame_at(0.0, layer, frame, 1.0).camera;
        // Size 0.5 of the 1080 shortest side: a square camera 540 px tall.
        assert!((rest.height - 0.5).abs() < 1e-9, "{}", rest.height);
        assert!(
            (rest.y + rest.height - 1.0).abs() < 1e-9,
            "stands on the bottom"
        );
        let big = LayoutSettings {
            cutout_size: 1.0,
            ..layouts.clone()
        }
        .frame_at(0.0, layer, frame, 1.0)
        .camera;
        assert!((big.height - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_cutout_transition_blends_the_matte_in_and_out() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::CameraCutout),
                cue(2.0, CameraLayout::CameraOverlay),
            ],
            1.0,
        );
        let into = resting(&settings, 1.5);
        assert!((into.cutout - progress(1.0, 1.5, 1.0)).abs() < 1e-9);
        assert!((into.camera_alpha - 1.0).abs() < 1e-9);
        assert!(into.camera.y > 0.0);
        let out = resting(&settings, 2.5);
        let at_change = progress(1.0, 2.0, 1.0);
        let expected = at_change * (1.0 - progress(2.0, 2.5, 1.0));
        assert!(
            (out.cutout - expected).abs() < 1e-9,
            "{} vs {expected}",
            out.cutout
        );
    }

    #[test]
    fn a_cutout_can_sit_anywhere_across_the_bottom_edge() {
        let cutout = |position: f64| {
            LayoutSettings {
                default_layout: CameraLayout::CameraCutout,
                cutout_position_x: position,
                ..LayoutSettings::default()
            }
            .frame_at(0.0, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0)
        };
        let centre = cutout(0.5);
        assert!((centre.camera.x + centre.camera.width / 2.0 - 0.5).abs() < 1e-9);
        assert!((centre.camera.y + centre.camera.height - 1.0).abs() < 1e-9);

        let left = cutout(0.0);
        assert!(left.camera.x.abs() < 1e-9);
        assert!((left.camera.width - centre.camera.width).abs() < 1e-9);
        assert!((left.camera.height - centre.camera.height).abs() < 1e-9);
        assert!((left.camera.y - centre.camera.y).abs() < 1e-9);

        let right = cutout(1.0);
        assert!((right.camera.x + right.camera.width - 1.0).abs() < 1e-9);
        assert!((right.camera.width - centre.camera.width).abs() < 1e-9);

        assert!(cutout(-2.0).camera.x.abs() < 1e-9);
        assert!((cutout(4.0).camera.x + centre.camera.width - 1.0).abs() < 1e-9);
        assert_eq!(cutout(f64::NAN).camera, centre.camera);

        let clamped = LayoutSettings {
            default_layout: CameraLayout::CameraCutout,
            cutout_position_x: f64::INFINITY,
            ..LayoutSettings::default()
        }
        .clamped();
        assert!((clamped.cutout_position_x - 0.5).abs() < 1e-9);
        let clamped = LayoutSettings {
            cutout_position_x: 7.0,
            ..LayoutSettings::default()
        }
        .clamped();
        assert!((clamped.cutout_position_x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_export_follows_the_cutouts_placement() {
        let size = (320, 180);
        let camera = solid(160, 90, [0, 255, 0, 255]);
        let mask = vec![1.0f32; 160 * 90];
        let cutout = |position: f64| {
            LayoutSettings {
                default_layout: CameraLayout::CameraCutout,
                cutout_position_x: position,
                ..LayoutSettings::default()
            }
            .frame_at(0.0, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0)
        };
        let mut left = solid(size.0, size.1, [10, 20, 30, 255]);
        composite_layout_into(
            &mut left,
            size,
            cutout(0.0),
            &camera,
            (160, 90),
            Some(&mask),
        )
        .unwrap();
        let mut right = solid(size.0, size.1, [10, 20, 30, 255]);
        composite_layout_into(
            &mut right,
            size,
            cutout(1.0),
            &camera,
            (160, 90),
            Some(&mask),
        )
        .unwrap();
        assert!(
            pixel(&left, size.0, 8, 170)[1] > 200,
            "the left placement covers the left edge"
        );
        assert_eq!(pixel(&right, size.0, 8, 170), [10, 20, 30, 255]);
        assert!(
            pixel(&right, size.0, 311, 170)[1] > 200,
            "the right placement covers the right edge"
        );
        assert_eq!(pixel(&left, size.0, 311, 170), [10, 20, 30, 255]);
        assert!(
            pixel(&left, size.0, 45, 170)[1] > 200 && pixel(&right, size.0, 275, 170)[1] > 200,
            "each placement covers the middle of its own band"
        );
    }

    #[test]
    fn the_cutout_blends_the_matte_into_premultiplied_camera_pixels() {
        let camera = solid(2, 1, [40, 80, 120, 255]);
        let mask = vec![0.0, 1.0];
        let half = camera_premultiplied(&camera, (2, 1), Some(&mask), 0.5).unwrap();
        assert_eq!(pixel(&half, 2, 0, 0), [20, 40, 60, 128]);
        assert_eq!(pixel(&half, 2, 1, 0), [40, 80, 120, 255]);

        let cut = camera_premultiplied(&camera, (2, 1), Some(&mask), 1.0).unwrap();
        assert_eq!(pixel(&cut, 2, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&cut, 2, 1, 0), [40, 80, 120, 255]);

        assert!(camera_premultiplied(&camera, (2, 1), None, 1.0).is_err());
        assert!(camera_premultiplied(&camera, (2, 1), Some(&[1.0]), 1.0).is_err());
    }

    #[test]
    fn the_matte_edge_follows_softness_and_inset() {
        // A 1-px-tall ramp from 0 to 1 across 11 pixels.
        let mut hard: Vec<f32> = (0..11).map(|i| i as f32 / 10.0).collect();
        refine_matte(&mut hard, (11, 1), 0.0, 0.0);
        assert!(
            hard[3] < 0.01 && hard[7] > 0.99,
            "softness 0 is a hard edge: {hard:?}"
        );
        let mut soft: Vec<f32> = (0..11).map(|i| i as f32 / 10.0).collect();
        refine_matte(&mut soft, (11, 1), 1.0, 0.0);
        assert!(
            soft[3] > 0.05 && soft[3] < 0.5,
            "softness 1 keeps a ramp: {soft:?}"
        );
        let mut person = vec![0.0f32, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0];
        refine_matte(&mut person, (9, 1), 0.0, 1.0);
        assert_eq!(person[2], 0.0, "inset pulls the edge in by one pixel");
        assert_eq!(person[4], 1.0);
    }

    #[test]
    fn a_decode_from_a_replaced_document_cannot_publish() {
        use std::sync::Mutex;

        let shared = Shared {
            job: Mutex::new(None),
            frame: Mutex::new(Some(Frame {
                width: 2,
                height: 2,
                stamp: 7,
                pixels: vec![1, 2, 3, 255],
                mask: None,
            })),
            stamp: Mutex::new(7),
            error: Mutex::new(Some(CameraError::Cutout("the matte failed".to_string()))),
            silence: Mutex::new(None),
            generation: Mutex::new(2),
        };
        let old = Job {
            path: PathBuf::from("old.mp4"),
            seconds: 0.0,
            generation: 1,
            size: (2, 2),
            cutout: true,
            matte: (0.2, 0.0),
        };
        assert!(!publish_frame(
            &shared,
            &old,
            vec![9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255],
            2,
            2,
            Some(vec![1.0; 4]),
        ));
        {
            let frame = shared.frame.lock().unwrap();
            let frame = frame.as_ref().unwrap();
            assert_eq!((frame.stamp, frame.width, frame.pixels[0]), (7, 2, 1));
            assert!(frame.mask.is_none());
        }
        match shared.error.lock().unwrap().as_ref() {
            Some(CameraError::Cutout(message)) => assert_eq!(message, "the matte failed"),
            _ => panic!("the stale decode should leave the error alone"),
        }

        let current = Job {
            generation: 2,
            ..old
        };
        assert!(publish_frame(
            &shared,
            &current,
            vec![9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255, 9, 9, 9, 255],
            2,
            2,
            Some(vec![1.0; 4]),
        ));
        {
            let frame = shared.frame.lock().unwrap();
            let frame = frame.as_ref().unwrap();
            assert_eq!((frame.stamp, frame.pixels[0]), (8, 9));
            assert!(frame.mask.is_some());
        }
        assert!(shared.error.lock().unwrap().is_none());
    }

    #[test]
    fn the_camera_cover_crops_instead_of_stretching() {
        let mut camera = Vec::new();
        for _ in 0..32 {
            for x in 0..32 {
                camera.extend_from_slice(if x < 16 {
                    &[0, 0, 255, 255]
                } else {
                    &[0, 255, 0, 255]
                });
            }
        }
        let mut frame = solid(200, 100, [0, 0, 0, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect::full(),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        composite_layout_into(&mut frame, (200, 100), layout, &camera, (32, 32), None).unwrap();
        let left = pixel(&frame, 200, 20, 50);
        let right = pixel(&frame, 200, 180, 50);
        assert!(
            left[0] > 200 && left[1] < 40,
            "left should be red: {left:?}"
        );
        assert!(
            right[1] > 200 && right[0] < 40,
            "right should be green: {right:?}"
        );
    }

    #[test]
    fn a_square_camera_keeps_its_top_edge_in_a_wide_layer() {
        let camera = camera_with(32, 32, |_, y| if y < 4 { RED } else { GREEN });
        let mut frame = solid(200, 100, [0, 0, 0, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect::full(),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        composite_layout_into(&mut frame, (200, 100), layout, &camera, (32, 32), None).unwrap();
        let top = pixel(&frame, 200, 100, 2);
        assert!(
            top[0] > 200 && top[1] < 40,
            "the layer starts at the camera's top edge, red: {top:?}"
        );
        let band = pixel(&frame, 200, 100, 50);
        assert!(
            band[1] > 200 && band[0] < 40,
            "the pixels below the top band are the camera's own, green: {band:?}"
        );
        let bottom = pixel(&frame, 200, 100, 98);
        assert!(
            bottom[1] > 200,
            "the crop still fills the layer's bottom edge, green: {bottom:?}"
        );
    }

    #[test]
    fn a_portrait_camera_keeps_its_top_edge_in_a_wide_layer() {
        let camera = camera_with(24, 48, |_, y| if y < 8 { RED } else { GREEN });
        let mut frame = solid(200, 100, [0, 0, 0, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect::full(),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        composite_layout_into(&mut frame, (200, 100), layout, &camera, (24, 48), None).unwrap();
        let top = pixel(&frame, 200, 100, 4);
        assert!(
            top[0] > 200 && top[1] < 40,
            "the layer starts at the portrait frame's top edge, red: {top:?}"
        );
        let lower = pixel(&frame, 200, 100, 90);
        assert!(
            lower[1] > 200 && lower[0] < 40,
            "the layer keeps covering below the top band, green: {lower:?}"
        );
    }

    #[test]
    fn a_mirrored_wide_layer_keeps_the_top_edge_and_flips_the_sides() {
        let camera = camera_with(32, 32, |x, y| match (x < 16, y < 16) {
            (true, true) => RED,
            (false, true) => GREEN,
            (true, false) => BLUE,
            (false, false) => [255, 255, 255, 255],
        });
        let size = (200, 100);
        let mut frame = solid(size.0, size.1, [0, 0, 0, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect::full(),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: true,
        };
        composite_layout_into(&mut frame, size, layout, &camera, (32, 32), None).unwrap();
        let left = pixel(&frame, size.0, size.0 / 10, size.1 / 2);
        assert!(
            left[1] > 200 && left[2] < 40,
            "the mirror shows the camera's top-right quadrant on the left, green: {left:?}"
        );
        let right = pixel(&frame, size.0, size.0 * 9 / 10, size.1 / 2);
        assert!(
            right[0] > 200 && right[1] < 40,
            "the mirror shows the camera's top-left quadrant on the right, red: {right:?}"
        );
        let lower = pixel(&frame, size.0, size.0 / 10, size.1 * 3 / 4);
        assert!(
            lower[1] > 200 && lower[2] < 40,
            "the mirrored layer still stops at the camera's top half, green: {lower:?}"
        );
    }

    #[test]
    fn the_preview_starts_the_camera_at_its_top_edge_too() {
        let camera = camera_with(32, 32, |_, y| if y < 4 { RED } else { GREEN });
        let image = ImageSurface::create_for_data(camera, Format::ARgb32, 32, 32, 32 * 4).unwrap();
        let mut surface = ImageSurface::create(Format::ARgb32, 200, 100).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            draw_camera_cover(
                &context,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                },
                0.0,
                false,
                1.0,
                &image,
            );
        }
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let at = |x: usize, y: usize| {
            let offset = y * stride + x * 4;
            [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]
        };
        let top = at(100, 2);
        assert!(
            top[2] > 200 && top[1] < 40,
            "the preview starts at the camera's top edge, red: {top:?}"
        );
        let lower = at(100, 90);
        assert!(
            lower[1] > 200 && lower[2] < 40,
            "the preview still fills the bottom of its rect, green: {lower:?}"
        );
    }

    #[test]
    fn a_tall_layer_keeps_the_cameras_horizontal_centre() {
        let camera = camera_with(
            128,
            32,
            |x, _| if (56..72).contains(&x) { RED } else { GREEN },
        );
        let mut frame = solid(100, 100, [0, 0, 0, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect::full(),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        composite_layout_into(&mut frame, (100, 100), layout, &camera, (128, 32), None).unwrap();
        let centre = pixel(&frame, 100, 50, 50);
        assert!(
            centre[0] > 200 && centre[1] < 40,
            "the layer keeps the middle of a wide camera, red: {centre:?}"
        );
        let left = pixel(&frame, 100, 2, 50);
        assert!(
            left[1] > 200 && left[0] < 40,
            "the crop drops the camera's left margin, green: {left:?}"
        );
        let right = pixel(&frame, 100, 97, 50);
        assert!(
            right[1] > 200 && right[0] < 40,
            "the crop drops the camera's right margin, green: {right:?}"
        );
    }

    #[test]
    fn a_screen_pane_keeps_its_pixels_inside_its_bounds() {
        let mut frame = solid(200, 100, [10, 20, 30, 255]);
        let layout = LayoutFrame {
            screen: Rect {
                x: 0.0,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            camera: Rect {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        let camera = solid(32, 32, [0, 255, 0, 255]);
        composite_layout_into(&mut frame, (200, 100), layout, &camera, (32, 32), None).unwrap();
        assert_eq!(
            pixel(&frame, 200, 50, 50),
            [10, 20, 30, 255],
            "the screen keeps its pixels inside its new rect"
        );
        assert_eq!(
            pixel(&frame, 200, 50, 5),
            [0, 0, 0, 255],
            "outside the pane is background"
        );
        assert!(
            pixel(&frame, 200, 150, 50)[1] > 200,
            "the camera owns its half"
        );
    }

    #[test]
    fn a_split_screen_keeps_every_quadrant_from_both_sides() {
        let size = (160, 90);
        let mut screen = Vec::with_capacity(160 * 90 * 4);
        for y in 0..90 {
            for x in 0..160 {
                screen.extend_from_slice(match (x < 80, y < 45) {
                    (true, true) => &[255, 0, 0, 255],
                    (false, true) => &[0, 255, 0, 255],
                    (true, false) => &[0, 0, 255, 255],
                    (false, false) => &[255, 255, 255, 255],
                });
            }
        }
        for flip in [false, true] {
            let layout = LayoutSettings {
                default_layout: CameraLayout::SplitScreen,
                split_flip: flip,
                ..LayoutSettings::default()
            };
            let frame = layout.frame_at(0.0, LayerSettings::default(), 160.0 / 90.0, 1.0);
            let (x0, y0, x1, y1) = pixel_rect(frame.screen, size);
            let mut pixels = screen.clone();
            shrink_screen(&mut pixels, size, frame.screen);
            let inside = |x: u32, y: u32| pixel(&pixels, 160, x, y);
            let top_left = inside(x0 + 4, y0 + 4);
            let top_right = inside(x1 - 5, y0 + 4);
            let bottom_left = inside(x0 + 4, y1 - 5);
            let bottom_right = inside(x1 - 5, y1 - 5);
            assert!(
                top_left[0] > 200 && top_left[1] < 50 && top_left[2] < 50,
                "the screen keeps its top-left corner, flip={flip}: {top_left:?}"
            );
            assert!(
                top_right[1] > 200 && top_right[0] < 50 && top_right[2] < 50,
                "the screen keeps its top-right corner, flip={flip}: {top_right:?}"
            );
            assert!(
                bottom_left[2] > 200 && bottom_left[0] < 50 && bottom_left[1] < 50,
                "the screen keeps its bottom-left corner, flip={flip}: {bottom_left:?}"
            );
            assert!(
                bottom_right[0] > 200 && bottom_right[1] > 200 && bottom_right[2] > 200,
                "the screen keeps its bottom-right corner, flip={flip}: {bottom_right:?}"
            );
            let middle = (x0 + x1) / 2;
            for y in [y0 + 1, y1 - 2] {
                let edge = inside(middle, y);
                assert_ne!(
                    edge,
                    [0, 0, 0, 255],
                    "the pane is covered to its edge at y={y}, flip={flip}"
                );
            }
            let outside_x = if flip { x0 - 2 } else { x1 + 2 };
            assert_eq!(
                inside(outside_x, y0 + 2),
                [0, 0, 0, 255],
                "the split's background stays outside the screen, flip={flip}"
            );
        }
    }

    #[test]
    fn a_split_pane_covers_its_share_with_the_middle_of_the_screen() {
        let size = (320, 180);
        let mut screen = Vec::with_capacity(320 * 180 * 4);
        for y in 0..180 {
            for x in 0..320 {
                screen.extend_from_slice(if y < 4 {
                    &RED
                } else if (80..240).contains(&x) {
                    &GREEN
                } else {
                    &BLUE
                });
            }
        }
        let layout = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            ..LayoutSettings::default()
        };
        let frame = layout.frame_at(0.0, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0);
        assert_eq!(pixel_rect(frame.screen, size), (0, 0, 160, 180));
        let mut pixels = screen.clone();
        shrink_screen(&mut pixels, size, frame.screen);
        let top = pixel(&pixels, size.0, 80, 1);
        assert_eq!(
            top, RED,
            "the pane starts at the screen's own top edge, not at its middle"
        );
        for y in [10, 90, 177] {
            for x in [2, 80, 157] {
                let shown = pixel(&pixels, size.0, x, y);
                assert!(
                    shown[1] > 200 && shown[0] < 40 && shown[2] < 40,
                    "the pane shows the covered middle of the screen at ({x}, {y}), green: {shown:?}"
                );
            }
        }
        assert_eq!(
            pixel(&pixels, size.0, 200, 90),
            [0, 0, 0, 255],
            "the camera's share is left for the camera"
        );
    }

    #[test]
    fn a_portrait_split_covers_the_top_pane_with_the_middle_of_the_screen() {
        let size = (180, 320);
        let mut screen = Vec::with_capacity(180 * 320 * 4);
        for y in 0..320 {
            for _ in 0..180 {
                screen.extend_from_slice(if (80..240).contains(&y) {
                    &GREEN
                } else {
                    &BLUE
                });
            }
        }
        let layout = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            ..LayoutSettings::default()
        };
        let frame = layout.frame_at(0.0, LayerSettings::default(), 9.0 / 16.0, 9.0 / 16.0);
        assert_eq!(pixel_rect(frame.screen, size), (0, 0, 180, 160));
        let mut pixels = screen.clone();
        shrink_screen(&mut pixels, size, frame.screen);
        for y in [2, 80, 157] {
            for x in [2, 90, 177] {
                let shown = pixel(&pixels, size.0, x, y);
                assert!(
                    shown[1] > 200 && shown[0] < 40 && shown[2] < 40,
                    "the pane shows the covered middle of the portrait screen at ({x}, {y}), green: {shown:?}"
                );
            }
        }
        assert_eq!(
            pixel(&pixels, size.0, 90, 300),
            [0, 0, 0, 255],
            "the camera's share is left for the camera"
        );
    }

    #[test]
    fn a_split_composite_fills_both_shares_without_a_letterbox() {
        let size = (320, 180);
        let camera = solid(64, 64, [0, 255, 0, 255]);
        let layout = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            ..LayoutSettings::default()
        };
        let frame = layout.frame_at(0.0, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0);
        let mut pixels = solid(size.0, size.1, [10, 20, 30, 255]);
        composite_layout_into(&mut pixels, size, frame, &camera, (64, 64), None).unwrap();
        for x in [1, 79, 158] {
            for y in [1, 90, 178] {
                assert_eq!(
                    pixel(&pixels, size.0, x, y),
                    [10, 20, 30, 255],
                    "the screen share is filled at ({x}, {y})"
                );
            }
        }
        for x in [161, 240, 318] {
            for y in [1, 90, 178] {
                let shown = pixel(&pixels, size.0, x, y);
                assert!(
                    shown[1] > 200,
                    "the camera share is filled at ({x}, {y}): {shown:?}"
                );
            }
        }
    }

    #[test]
    fn a_split_transition_composites_the_same_picture_every_time() {
        let size = (320, 180);
        let layout = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            cues: vec![cue(1.0, CameraLayout::SplitScreen)],
            transition_seconds: 1.0,
            ..LayoutSettings::default()
        }
        .clamped();
        let frame = layout.frame_at(1.5, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0);
        let screen = camera_with(320, 180, |x, _| if x < 160 { RED } else { GREEN });
        let camera = solid(64, 64, [0, 255, 0, 255]);
        let mut first = screen.clone();
        composite_layout_into(&mut first, size, frame, &camera, (64, 64), None).unwrap();
        let mut second = screen;
        composite_layout_into(&mut second, size, frame, &camera, (64, 64), None).unwrap();
        assert_eq!(first, second);
        let again = layout.frame_at(1.5, LayerSettings::default(), 16.0 / 9.0, 16.0 / 9.0);
        assert_eq!(frame, again);
    }

    #[test]
    fn screen_only_never_touches_a_camera_pixel() {
        let mut frame = solid(64, 64, [12, 34, 56, 255]);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect {
                x: 0.5,
                y: 0.5,
                width: 0.5,
                height: 0.5,
            },
            camera_alpha: 0.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        composite_layout_into(
            &mut frame,
            (64, 64),
            layout,
            &solid(32, 32, [0, 255, 0, 255]),
            (32, 32),
            None,
        )
        .unwrap();
        assert_eq!(pixel(&frame, 64, 40, 40), [12, 34, 56, 255]);
    }

    #[test]
    fn the_export_decode_keeps_the_raw_camera_aspect() {
        assert_eq!(camera_decode_size((1920, 1080), (1920, 1080)), (1920, 1080));
        assert_eq!(camera_decode_size((1080, 1920), (1920, 1080)), (608, 1080));
        assert_eq!(camera_decode_size((320, 240), (1920, 1080)), (320, 240));
        assert_eq!(camera_decode_size((0, 0), (320, 240)), (320, 240));
        let preview = preview_size((1080, 1920));
        assert_eq!(preview.1, 1280);
        assert_eq!(preview.0, 720);
        let preview = preview_size((320, 240));
        assert_eq!((preview.0, preview.1), (720, 540));
    }

    #[test]
    fn the_layer_sits_on_its_fractions_of_the_picture() {
        let video = Rect {
            x: 20.0,
            y: 40.0,
            width: 1000.0,
            height: 500.0,
        };
        let rect = layer(0.5, 0.25, 0.2, 0.3, 0.0).rect_in(video);
        assert!((rect.x - 520.0).abs() < 1e-9);
        assert!((rect.y - 165.0).abs() < 1e-9);
        assert!((rect.width - 200.0).abs() < 1e-9);
        assert!((rect.height - 150.0).abs() < 1e-9);
    }

    #[test]
    fn a_fullscreen_camera_is_the_whole_frame() {
        let video = Rect {
            x: 10.0,
            y: 10.0,
            width: 640.0,
            height: 360.0,
        };
        let mut settings = layer(0.5, 0.5, 0.2, 0.2, 0.0);
        settings.fullscreen = true;
        assert_eq!(settings.rect_in(video).width, 640.0);
        assert_eq!(settings.radius_px(video), 0.0);
    }

    #[test]
    fn a_hand_edited_layer_stays_inside_the_frame() {
        let settings = LayerSettings {
            x: 0.9,
            y: 1.4,
            w: 5.0,
            h: 0.0,
            radius: 4.0,
            ..LayerSettings::default()
        }
        .clamped();
        assert!((settings.x + settings.w - 1.0).abs() < 1e-9);
        assert!((settings.y + settings.h - 1.0).abs() < 1e-9);
        assert!((settings.w - MAX_SIZE).abs() < 1e-9);
        assert!((settings.h - MIN_SIZE).abs() < 1e-9);
        assert_eq!(settings.radius, 1.0);
    }

    #[test]
    fn an_overlay_alignment_reads_from_where_the_layer_sits() {
        let mut settings = layer(0.0, 0.0, 0.2, 0.25, 0.0);
        assert_eq!(settings.overlay_alignment(), (0.0, 0.0));

        settings = layer(0.8, 0.75, 0.2, 0.25, 0.0);
        assert_eq!(settings.overlay_alignment(), (1.0, 1.0));

        settings = layer(0.4, 0.375, 0.2, 0.25, 0.0);
        let (x, y) = settings.overlay_alignment();
        assert!((x - 0.5).abs() < 1e-9);
        assert!((y - 0.5).abs() < 1e-9);

        settings = layer(0.9, -0.4, 0.2, 0.25, 0.0);
        assert_eq!(settings.overlay_alignment(), (1.0, 0.0));

        settings = LayerSettings {
            w: 0.2,
            h: 0.25,
            x: f64::NAN,
            y: 0.75,
            ..LayerSettings::default()
        };
        let (x, y) = settings.overlay_alignment();
        assert!(x.is_finite() && (x - 0.5).abs() < 1e-9);
        assert!((y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn setting_an_overlay_alignment_puts_the_layer_on_that_anchor() {
        let mut settings = layer(0.7, 0.55, 0.2, 0.25, 0.0);
        settings.set_overlay_alignment(1.0, 1.0);
        assert!((settings.x - 0.8).abs() < 1e-9);
        assert!((settings.y - 0.75).abs() < 1e-9);
        assert_eq!(settings.w, 0.2);
        assert_eq!(settings.h, 0.25);

        settings.set_overlay_alignment(0.0, 0.0);
        assert!(settings.x.abs() < 1e-9);
        assert!(settings.y.abs() < 1e-9);

        settings.set_overlay_alignment(0.5, 0.5);
        assert!((settings.x - 0.4).abs() < 1e-9);
        assert!((settings.y - 0.375).abs() < 1e-9);
        let (x, y) = settings.overlay_alignment();
        assert!((x - 0.5).abs() < 1e-9);
        assert!((y - 0.5).abs() < 1e-9);

        settings.set_overlay_alignment(f64::NAN, 4.0);
        assert!((settings.x - 0.4).abs() < 1e-9);
        assert!((settings.y - 0.75).abs() < 1e-9);
    }

    #[test]
    fn a_resized_overlay_keeps_its_alignment_at_every_anchor() {
        let mut centred = layer(0.4, 0.375, 0.2, 0.25, 0.0);
        centred.resize_overlay(0.4, 0.625);
        assert!((centred.w - 0.4).abs() < 1e-9);
        assert!((centred.h - 0.25).abs() < 1e-9);
        assert!((centred.x - 0.3).abs() < 1e-9);
        assert!((centred.y - 0.375).abs() < 1e-9);

        let mut corner = layer(0.8, 0.75, 0.2, 0.25, 0.0);
        corner.resize_overlay(0.4, 0.625);
        assert!((corner.x + corner.w - 1.0).abs() < 1e-9);
        assert!((corner.y + corner.h - 1.0).abs() < 1e-9);

        let mut edge = layer(0.8, 0.0, 0.2, 0.25, 0.0);
        edge.resize_overlay(0.4, 0.625);
        assert!((edge.x + edge.w - 1.0).abs() < 1e-9);
        assert!(edge.y.abs() < 1e-9);

        let mut tall = layer(0.4, 0.375, 0.2, 0.25, 0.0);
        tall.resize_overlay(0.9, 1.6);
        assert!(tall.w <= MAX_SIZE + 1e-9 && tall.h <= MAX_SIZE + 1e-9);
        assert!(tall.w >= MIN_SIZE && tall.h >= MIN_SIZE);
        assert!((tall.h - tall.w * 1.6).abs() < 1e-9);
        assert!((tall.x - 0.5 * (1.0 - tall.w)).abs() < 1e-9);

        let mut small = layer(0.4, 0.375, 0.2, 0.25, 0.0);
        small.resize_overlay(MIN_SIZE, 0.1);
        assert!(small.w >= MIN_SIZE && small.h >= MIN_SIZE);
        assert!((small.w - small.h / 0.1).abs() < 1e-9);

        let mut guarded = layer(0.4, 0.375, 0.2, 0.25, 0.0);
        guarded.resize_overlay(f64::NAN, f64::NAN);
        assert!((guarded.w - 0.2).abs() < 1e-9);
        assert!((guarded.h - 0.2).abs() < 1e-9);
        assert!((guarded.x - 0.4).abs() < 1e-9);
    }

    #[test]
    fn the_overlay_size_is_the_longest_side_over_the_shortest_frame_side() {
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        assert!(close(
            overlay_size_hw(16.0 / 9.0, 16.0 / 9.0, 0.5),
            (0.28125, 0.28125)
        ));
        assert!(close(overlay_size_hw(16.0 / 9.0, 1.0, 0.5), (0.28125, 0.5)));
        assert!(close(
            overlay_size_hw(9.0 / 16.0, 16.0 / 9.0, 0.5),
            (0.5, 0.1582)
        ));
        for (frame, camera) in [
            (16.0 / 9.0, 16.0 / 9.0),
            (16.0 / 9.0, 1.0),
            (9.0 / 16.0, 16.0 / 9.0),
            (1.6, 0.75),
        ] {
            let (w, h) = overlay_size_hw(frame, camera, 0.5);
            let layer = LayerSettings {
                w,
                h,
                ..LayerSettings::default()
            };
            assert!((layer.overlay_size_ratio(frame) - 0.5).abs() < 1e-9);
            assert!((layer.pixel_aspect(frame) - camera).abs() < 1e-9);
        }
    }

    #[test]
    fn an_aspect_change_keeps_the_size_and_the_anchor() {
        let frame = 16.0 / 9.0;
        let mut layer = LayerSettings::default();
        layer.set_overlay_size(0.4, frame);
        layer.set_overlay_alignment(1.0, 1.0);
        layer.set_overlay_aspect(9.0 / 16.0, frame);
        assert!((layer.overlay_size_ratio(frame) - 0.4).abs() < 1e-9);
        assert!((layer.pixel_aspect(frame) - 9.0 / 16.0).abs() < 1e-9);
        assert!((layer.x + layer.w - 1.0).abs() < 1e-9);
        assert!((layer.y + layer.h - 1.0).abs() < 1e-9);
        layer.set_overlay_size(0.6, frame);
        assert!((layer.overlay_size_ratio(frame) - 0.6).abs() < 1e-9);
        assert!((layer.pixel_aspect(frame) - 9.0 / 16.0).abs() < 1e-9);
    }

    #[test]
    fn the_camera_shrinks_through_a_zoom_and_back() {
        let clip = |start: f64, end: f64| ZoomClip {
            start,
            end,
            scale: 2.0,
            center: (0.0, 0.0),
            hidden: false,
            mode: ZoomMode::Auto,
            instant: false,
            anchor: None,
        };
        let clips = vec![clip(3.0, 8.0)];
        assert_eq!(zoom_shrink_at(&[], 5.0, 0.0), 0.0);
        assert_eq!(zoom_shrink_at(&clips, 1.0, 0.0), 0.0);
        assert!(
            zoom_shrink_at(&clips, 3.0, 0.0) < 0.05,
            "the shrink eases in"
        );
        let mid = zoom_shrink_at(&clips, 3.2, 0.0);
        assert!(mid > 0.05 && mid < 1.0, "on its way: {mid}");
        assert!((zoom_shrink_at(&clips, 6.0, 0.0) - 1.0).abs() < 1e-9);
        assert!(zoom_shrink_at(&clips, 10.5, 0.0) < 1e-9);
        let mut t = 2.5;
        while t < 11.0 {
            let value = zoom_shrink_at(&clips, t, 0.0);
            assert!((0.0..=1.0).contains(&value), "clamped at {t}: {value}");
            t += 0.05;
        }
        // The lead-in starts the shrink before a gliding clip, never an instant one.
        assert!(zoom_shrink_at(&clips, 3.0, 0.35) > zoom_shrink_at(&clips, 3.0, 0.0));
        let instant = vec![ZoomClip {
            instant: true,
            ..clip(3.0, 8.0)
        }];
        assert_eq!(zoom_shrink_at(&instant, 2.9, 0.35), 0.0);
        let hidden = vec![ZoomClip {
            hidden: true,
            ..clip(3.0, 8.0)
        }];
        assert_eq!(zoom_shrink_at(&hidden, 6.0, 0.0), 0.0);
    }

    #[test]
    fn a_zoom_shrinks_the_overlay_about_its_anchor_and_the_cutout_on_its_edge() {
        let frame = 16.0 / 9.0;
        let mut layer = LayerSettings::default();
        layer.set_overlay_size(0.5, frame);
        layer.set_overlay_alignment(1.0, 1.0);
        let layouts = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            ..LayoutSettings::default()
        };
        let rest = layouts.frame_at_zoomed(0.0, layer, frame, 1.0, 0.0).camera;
        let full = layouts.frame_at_zoomed(0.0, layer, frame, 1.0, 1.0).camera;
        let half = layouts.frame_at_zoomed(0.0, layer, frame, 1.0, 0.5).camera;
        assert!((full.width - 0.8 * rest.width).abs() < 1e-9);
        assert!((full.height - 0.8 * rest.height).abs() < 1e-9);
        assert!((half.width - 0.9 * rest.width).abs() < 1e-9);
        assert!(
            (full.x + full.width - (rest.x + rest.width)).abs() < 1e-9,
            "right edge stays"
        );
        assert!(
            (full.y + full.height - (rest.y + rest.height)).abs() < 1e-9,
            "bottom edge stays"
        );

        let cutout = LayoutSettings {
            default_layout: CameraLayout::CameraCutout,
            ..LayoutSettings::default()
        };
        let rest = cutout.frame_at_zoomed(0.0, layer, frame, 1.0, 0.0).camera;
        let full = cutout.frame_at_zoomed(0.0, layer, frame, 1.0, 1.0).camera;
        assert!((full.height - 0.8 * rest.height).abs() < 1e-9);
        assert!(
            (full.y + full.height - 1.0).abs() < 1e-9,
            "the cutout stays on the bottom"
        );
    }

    #[test]
    fn the_layer_chooses_how_far_the_zoom_shrinks_it() {
        let frame = 16.0 / 9.0;
        let layouts = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            ..LayoutSettings::default()
        };
        let mut layer = LayerSettings::default();
        layer.set_overlay_size(0.5, frame);
        layer.set_overlay_alignment(1.0, 1.0);

        let mut off = layer;
        off.zoomed_scale = 1.0;
        assert_eq!(
            layouts.frame_at_zoomed(0.0, off, frame, 1.0, 1.0).camera,
            layouts.frame_at_zoomed(0.0, off, frame, 1.0, 0.0).camera,
            "a shrink of 1 keeps the camera at its rest size"
        );

        let mut half = layer;
        half.zoomed_scale = 0.5;
        let rest = layouts.frame_at_zoomed(0.0, half, frame, 1.0, 0.0).camera;
        let full = layouts.frame_at_zoomed(0.0, half, frame, 1.0, 1.0).camera;
        assert!((full.width - 0.5 * rest.width).abs() < 1e-9);
        assert!((full.height - 0.5 * rest.height).abs() < 1e-9);
    }

    #[test]
    fn a_zoomed_split_fills_the_frame_and_floats_the_camera_opposite_the_screen() {
        let frame = 4.0 / 3.0;
        let layer = LayerSettings {
            radius: 0.3,
            ..LayerSettings::default()
        };
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            ..LayoutSettings::default()
        };
        let rest = split.frame_at_zoomed(0.0, layer, frame, 1.0, 0.0);
        assert!(!rest.screen.is_full(), "at rest the screen keeps its pane");
        let zoomed = split.frame_at_zoomed(0.0, layer, frame, 1.0, 1.0);
        assert!(
            zoomed.screen.is_full(),
            "zoomed, the screen takes the frame"
        );
        // Default split 0.5: the camera pane is 160x240 in a 320x240 frame,
        // aspect 2:3. At size 0.5 the camera is 80x120 px, inset 15 px,
        // bottom-right: x 225..305, y 105..225.
        let px = |rect: Rect| {
            (
                rect.x * 320.0,
                rect.y * 240.0,
                rect.width * 320.0,
                rect.height * 240.0,
            )
        };
        let (x, y, w, h) = px(zoomed.camera);
        assert!(
            (w - 80.0).abs() < 1e-6 && (h - 120.0).abs() < 1e-6,
            "{w}x{h}"
        );
        assert!(
            (x - 225.0).abs() < 1e-6 && (y - 105.0).abs() < 1e-6,
            "{x},{y}"
        );
        assert!((zoomed.camera_radius - 0.3).abs() < 1e-9);
        let flipped = LayoutSettings {
            split_flip: true,
            ..split.clone()
        };
        let (x, _, _, _) = px(flipped.frame_at_zoomed(0.0, layer, frame, 1.0, 1.0).camera);
        assert!(
            (x - 15.0).abs() < 1e-6,
            "a flipped split floats the camera bottom-left: {x}"
        );
        let half = split.frame_at_zoomed(0.0, layer, frame, 1.0, 0.5);
        assert!(half.screen.width > rest.screen.width && half.screen.width < 1.0);
    }

    #[test]
    fn the_track_pixels_land_inside_the_layer_and_nowhere_else() {
        let mut frame = solid(400, 400, [30, 20, 10, 255]);
        let camera = solid(100, 100, [0, 255, 0, 255]);
        composite_into(
            &mut frame,
            (400, 400),
            layer(0.6, 0.6, 0.3, 0.3, 0.0),
            &camera,
            (100, 100),
        );
        let inside = pixel(&frame, 400, 300, 300);
        assert!(inside[1] > 200, "camera pixel expected, got {inside:?}");
        assert!(inside[0] < 40);
        assert_eq!(pixel(&frame, 400, 10, 10), [30, 20, 10, 255]);
        assert_eq!(pixel(&frame, 400, 380, 60), [30, 20, 10, 255]);
    }

    #[test]
    fn a_round_layer_leaves_the_corners_of_its_box_alone() {
        let mut frame = solid(200, 200, [10, 10, 10, 255]);
        let camera = solid(64, 64, [0, 255, 0, 255]);
        composite_into(
            &mut frame,
            (200, 200),
            layer(0.25, 0.25, 0.5, 0.5, 1.0),
            &camera,
            (64, 64),
        );
        assert_eq!(pixel(&frame, 200, 52, 52), [10, 10, 10, 255]);
        assert!(pixel(&frame, 200, 100, 100)[1] > 200);
    }

    #[test]
    fn the_preview_draws_the_layer_through_its_shape() {
        let mut surface = ImageSurface::create(Format::ARgb32, 200, 200).unwrap();
        let camera = ImageSurface::create_for_data(
            solid(32, 32, [0, 0, 255, 255]),
            Format::ARgb32,
            32,
            32,
            32 * 4,
        )
        .unwrap();
        {
            let context = Context::new(&surface).unwrap();
            draw_bubble(
                &context,
                Rect {
                    x: 50.0,
                    y: 50.0,
                    width: 100.0,
                    height: 100.0,
                },
                50.0,
                false,
                &camera,
            );
        }
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let alpha_at = |x: usize, y: usize| data[y * stride + x * 4 + 3];
        assert_eq!(alpha_at(52, 52), 0, "the circle's corner stays empty");
        assert!(alpha_at(100, 100) > 200, "the middle is painted");
    }

    #[test]
    fn a_mirrored_layer_flips_only_the_picture() {
        let mut camera = Vec::new();
        for _ in 0..16 {
            for x in 0..32 {
                let colour = if x < 16 {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                };
                camera.extend_from_slice(&colour);
            }
        }
        let mut frame = solid(100, 100, [0, 0, 0, 255]);
        let mut mirrored = layer(0.0, 0.0, 1.0, 1.0, 0.0);
        mirrored.mirror = true;
        composite_into(&mut frame, (100, 100), mirrored, &camera, (32, 16));
        let left = pixel(&frame, 100, 10, 50);
        assert!(
            left[0] < 40 && left[2] > 200,
            "left side should now be the camera's right half, blue: {left:?}"
        );
    }

    #[test]
    fn a_long_silence_hides_the_camera_and_a_short_one_does_not() {
        assert_eq!(silence_hide_at(&[(0.0, 20.0)], 10.0, Some(5.0)), 1.0);
        assert_eq!(silence_hide_at(&[(0.0, 20.0)], 10.0, None), 0.0);
        let springing = silence_hide_at(&[(2.0, 20.0)], 2.55, Some(5.0));
        assert!(
            springing > 0.0 && springing < 1.0,
            "the hide springs in after the margin: {springing}"
        );
        assert_eq!(silence_hide_at(&[(2.0, 20.0)], 4.0, Some(5.0)), 1.0);
    }

    #[test]
    fn a_hidden_camera_fades_out_and_shrinks_about_its_centre() {
        let frame = LayoutFrame {
            screen: Rect::full(),
            camera: Rect {
                x: 0.6,
                y: 0.55,
                width: 0.2,
                height: 0.25,
            },
            camera_alpha: 1.0,
            camera_radius: 0.1,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        let hidden = hide_for_silence(frame, 1.0);
        assert_eq!(hidden.camera_alpha, 0.0);
        assert!((hidden.camera.width - 0.18).abs() < 1e-9);
        assert!((hidden.camera.height - 0.225).abs() < 1e-9);
        assert!((hidden.camera.x + hidden.camera.width / 2.0 - 0.7).abs() < 1e-9);
        assert!((hidden.camera.y + hidden.camera.height / 2.0 - 0.675).abs() < 1e-9);
        assert_eq!(hide_for_silence(frame, 0.0), frame);
    }

    #[test]
    fn a_shape_starts_at_the_radius_the_recorder_drew() {
        assert_eq!(radius_for_shape(0), 1.0);
        assert_eq!(radius_for_shape(1), 0.30);
        assert_eq!(radius_for_shape(2), 0.30);
        assert_eq!(aspect_for_shape(0), aspect_for_shape(1));
        assert!(aspect_for_shape(3) > aspect_for_shape(2));
    }

    #[test]
    fn the_last_camera_seek_decodes_the_tracks_final_frame() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-camera-last-frame-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("camera.mp4");
        let status = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args([
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=32x32:r=30:d=0.4",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&path)
            .status();
        if !matches!(status, Ok(status) if status.success()) {
            eprintln!("skipping the camera last-frame test: ffmpeg is unavailable");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let duration = crate::recording::editor::ffmpeg::probe_duration(&path)
            .expect("the clip has a duration");
        let fps = crate::recording::editor::ffmpeg::probe_frame_rate(&path)
            .expect("the clip has a frame rate");
        let last = last_camera_seconds(duration, Some(fps));
        assert!(last < duration);
        assert!(
            decode_frame(&path, last, (32, 32)).is_some(),
            "a seek at the last camera frame still decodes"
        );
        let export = WebcamExport {
            path: path.clone(),
            settings: LayerSettings::default(),
            layouts: LayoutSettings::default(),
            split_gap: 0.0,
            pane_radius: 0.0,
            track_seconds: duration,
            start: 0.0,
            video_seconds: duration,
            last_frame: last,
            source_size: (32, 32),
            silence: Vec::new(),
        };
        assert!((export.start_at(duration) - last).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_export_maps_video_time_onto_the_tracks_own_clock() {
        let export = WebcamExport {
            path: PathBuf::new(),
            settings: LayerSettings::default(),
            layouts: LayoutSettings::default(),
            split_gap: 0.0,
            pane_radius: 0.0,
            track_seconds: 8.0,
            start: 0.0,
            video_seconds: 10.0,
            last_frame: 7.95,
            source_size: (1280, 720),
            silence: Vec::new(),
        };
        assert!((export.track_at(1.0) - 0.8).abs() < 1e-9);
        assert!((export.time_scale() - 1.25).abs() < 1e-9);
        let unknown = WebcamExport {
            track_seconds: 0.0,
            ..export
        };
        assert!((unknown.track_at(3.6) - 3.6).abs() < 1e-9);
        assert!((unknown.time_scale() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_recorded_gap_plays_the_camera_one_to_one() {
        // A 9.6 s track that started 0.4 s into a 10 s video.
        let clock = CameraClock::new(0.4, 9.6, 10.0, 0.0);
        assert!((clock.rate - 1.0).abs() < 1e-9);
        assert_eq!(clock.track_at(0.0), 0.0);
        assert!((clock.lead_at(0.0) - 0.4).abs() < 1e-9);
        assert!((clock.track_at(1.0) - 0.6).abs() < 1e-9);
        assert_eq!(clock.lead_at(1.0), 0.0);
        assert!((clock.track_at(10.0) - 9.6).abs() < 1e-9);
        assert!((clock.time_scale() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_camera_delay_shifts_the_track() {
        let later = CameraClock::new(0.4, 9.6, 10.0, 0.1);
        assert!((later.track_at(1.0) - 0.7).abs() < 1e-9);
        let earlier = CameraClock::new(0.4, 9.6, 10.0, -0.2);
        assert!((earlier.track_at(1.0) - 0.4).abs() < 1e-9);
        assert!((earlier.lead_at(0.0) - 0.6).abs() < 1e-9);
        // The adjustment is clamped to the slider's range.
        let wild = CameraClock::new(0.0, 10.0, 10.0, 5.0);
        assert!((wild.delay - MAX_CAMERA_DELAY_SECONDS).abs() < 1e-9);
    }

    #[test]
    fn a_sidecar_without_a_gap_keeps_the_length_ratio() {
        let clock = CameraClock::new(0.0, 8.0, 10.0, 0.0);
        assert!((clock.track_at(1.0) - 0.8).abs() < 1e-9);
        assert!((clock.time_scale() - 1.25).abs() < 1e-9);
        assert_eq!(clock.lead_at(1.0), 0.0);
    }

    #[test]
    fn a_stored_gap_keeps_the_camera_at_its_own_speed() {
        // A measured gap leaves the track ending where the camera stopped,
        // which need not be the video's end; the track must not be stretched.
        let clock = CameraClock::new(1.375, 29.433, 31.0, 0.0);
        assert_eq!(clock.rate, 1.0);
        assert!((clock.track_at(11.375) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn the_camera_blur_follows_the_studied_strength() {
        assert!((camera_blur_strength(1.0, 60.0, 1.0) - 1.0).abs() < 1e-9);
        assert!((camera_blur_strength(0.5, 30.0, 1.0) - 0.25).abs() < 1e-9);
        assert_eq!(camera_blur_strength(1.0, 60.0, 0.0), 0.0);
        assert_eq!(camera_blur_strength(0.0, 60.0, 1.0), 0.0);
    }

    #[test]
    fn a_still_camera_trail_is_the_plain_composite() {
        let size = (200, 100);
        let camera = camera_with(32, 32, |_, _| GREEN);
        let layout = LayoutFrame {
            screen: Rect::full(),
            camera: Rect {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.5,
            },
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        assert_eq!(trail_sample_count(layout, layout, size), 1);
        let mut plain = solid(size.0, size.1, [0, 0, 0, 255]);
        composite_layout_into(&mut plain, size, layout, &camera, (32, 32), None).unwrap();
        let mut trail = solid(size.0, size.1, [0, 0, 0, 255]);
        composite_layout_trail_into(&mut trail, size, &[layout], &camera, (32, 32), None).unwrap();
        assert_eq!(plain, trail);
    }

    #[test]
    fn a_moving_camera_trail_smears_its_edge() {
        let size = (200, 100);
        let camera = camera_with(32, 32, |_, _| GREEN);
        let at = |x: f64| LayoutFrame {
            screen: Rect::full(),
            camera: Rect {
                x,
                y: 0.2,
                width: 0.3,
                height: 0.5,
            },
            camera_alpha: 1.0,
            camera_radius: 0.0,
            screen_radius: 0.0,
            cutout: 0.0,
            mirror: false,
        };
        let before = at(0.1);
        let now = at(0.3);
        assert_eq!(trail_sample_count(now, before, size), 11);
        let frames: Vec<LayoutFrame> = (0..11)
            .map(|k| at(0.1 + 0.2 * f64::from(k) / 10.0))
            .collect();
        let mut frame = solid(size.0, size.1, [0, 0, 0, 255]);
        composite_layout_trail_into(&mut frame, size, &frames, &camera, (32, 32), None).unwrap();
        let first = pixel(&frame, 200, 25, 50)[1];
        assert!(
            first > 20 && first < 235,
            "the first samples alone soften the edge: {first}"
        );
        let every = pixel(&frame, 200, 70, 50)[1];
        assert!(every > 235, "the samples all cover the middle: {every}");
        let never = pixel(&frame, 200, 5, 50)[1];
        assert!(never < 5, "the trail never reaches the left edge: {never}");
    }

    #[test]
    fn the_auto_stage_follows_the_studied_resolver() {
        assert!((auto_split_stage_aspect(1.6, 0.0) - 2.1625).abs() < 1e-9);
        assert!(
            (auto_split_stage_aspect(16.0 / 9.0, 0.0) - (16.0 / 9.0 + 9.0 / 16.0)).abs() < 1e-9
        );
        assert!((auto_split_stage_aspect(1.0, 0.0) - 16.0 / 9.0).abs() < 1e-9);
        assert!((auto_split_stage_aspect(4.0 / 3.0, 0.0) - 16.0 / 9.0).abs() < 1e-9);
        assert!((auto_split_stage_aspect(3.0, 0.0) - 2.4).abs() < 1e-9);
        assert!((auto_split_stage_aspect(1.6, 0.1) - (0.3 + 1.6 + 0.5625) / 1.2).abs() < 1e-9);
    }

    #[test]
    fn a_screen_is_contained_on_a_wider_stage() {
        assert_eq!(contained_screen(1.6, 1.6), Rect::full());
        let contained = contained_screen(2.1625, 1.6);
        let width = 1.6 / 2.1625;
        assert!((contained.width - width).abs() < 1e-9);
        assert!((contained.x - (1.0 - width) / 2.0).abs() < 1e-9);
        assert!(contained.y.abs() < 1e-9);
        assert!((contained.height - 1.0).abs() < 1e-9);
        let tall = contained_screen(1.0, 2.0);
        assert!((tall.width - 1.0).abs() < 1e-9);
        assert!((tall.height - 0.5).abs() < 1e-9);
        assert!((tall.y - 0.25).abs() < 1e-9);
    }

    #[test]
    fn the_widened_split_keeps_the_screen_uncropped() {
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: true,
            split_flip: false,
            ..LayoutSettings::default()
        };
        let layer = LayerSettings {
            enabled: true,
            ..LayerSettings::default()
        };
        let frame = split.frame_at_staged(
            0.0,
            layer,
            StageShape {
                aspect: 2.1625,
                screen_aspect: 1.6,
                split_gap: 0.0,
                pane_radius: 0.0,
            },
            1.0,
            0.0,
        );
        let width = 1.6 / 2.1625;
        assert!(frame.screen.x.abs() < 1e-9);
        assert!((frame.screen.height - 1.0).abs() < 1e-9);
        assert!((frame.screen.width - width).abs() < 1e-9);
        assert!((frame.camera.x - frame.screen.width).abs() < 1e-9);
        assert!((frame.camera.width - (1.0 - frame.screen.width)).abs() < 1e-9);
        let cover = screen_cover_rect_in(frame.screen, 2.1625, 1.6);
        assert!((cover.x - frame.screen.x).abs() < 1e-9);
        assert!((cover.y - frame.screen.y).abs() < 1e-9);
        assert!((cover.width - frame.screen.width).abs() < 1e-9);
        assert!((cover.height - frame.screen.height).abs() < 1e-9);
    }

    #[test]
    fn other_layouts_contain_the_screen_on_a_widened_stage() {
        let layout = LayoutSettings {
            default_layout: CameraLayout::CameraOverlay,
            ..LayoutSettings::default()
        };
        let frame = layout.frame_at_staged(
            0.0,
            LayerSettings::default(),
            StageShape {
                aspect: 2.1625,
                screen_aspect: 1.6,
                split_gap: 0.0,
                pane_radius: 0.0,
            },
            1.0,
            0.0,
        );
        let contained = contained_screen(2.1625, 1.6);
        assert!((frame.screen.x - contained.x).abs() < 1e-9);
        assert!((frame.screen.y - contained.y).abs() < 1e-9);
        assert!((frame.screen.width - contained.width).abs() < 1e-9);
        assert!((frame.screen.height - contained.height).abs() < 1e-9);
    }

    #[test]
    fn the_gapped_auto_stage_matches_the_studied_one_without_a_gap() {
        for screen in [1.0, 4.0 / 3.0, 1.6, 16.0 / 9.0, 3.0] {
            let gapped = auto_split_stage_with_gap(screen, 0.0);
            let studied = auto_split_stage_aspect(screen, 0.0);
            assert!(
                (gapped - studied).abs() < 1e-9,
                "{screen}: {gapped} vs {studied}"
            );
        }
        assert!((auto_split_stage_with_gap(1.6, 0.1) - (1.6 + 0.1 + 0.5625)).abs() < 1e-9);
    }

    #[test]
    fn a_padded_split_leaves_a_gap_and_keeps_the_screen_shape() {
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: true,
            split_flip: false,
            split_padded: true,
            ..LayoutSettings::default()
        };
        let stage = StageShape {
            aspect: 2.2625,
            screen_aspect: 1.6,
            split_gap: 0.1,
            pane_radius: 0.05,
        };
        let layer = LayerSettings {
            enabled: true,
            ..LayerSettings::default()
        };
        let frame = split.frame_at_staged(0.0, layer, stage, 1.0, 0.0);
        assert!(frame.screen.x.abs() < 1e-9);
        assert!((frame.screen.width - 1.6 / 2.2625).abs() < 1e-9);
        assert!((frame.screen.height - 1.0).abs() < 1e-9);
        assert!((frame.camera.x - 1.7 / 2.2625).abs() < 1e-9);
        assert!((frame.camera.width - 0.5625 / 2.2625).abs() < 1e-9);
        let gap = frame.camera.x - (frame.screen.x + frame.screen.width);
        assert!((gap - 0.1 / 2.2625).abs() < 1e-9);
        assert!((frame.screen_radius - 0.05 / 0.5).abs() < 1e-9);
        assert!((frame.camera_radius - 0.05 / (0.5625 / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn padding_off_keeps_the_fullscreen_split() {
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: true,
            split_flip: false,
            split_padded: false,
            ..LayoutSettings::default()
        };
        let layer = LayerSettings {
            enabled: true,
            ..LayerSettings::default()
        };
        let padded = StageShape {
            aspect: 2.2625,
            screen_aspect: 1.6,
            split_gap: 0.1,
            pane_radius: 0.05,
        };
        let plain = StageShape {
            aspect: 2.2625,
            screen_aspect: 1.6,
            split_gap: 0.0,
            pane_radius: 0.0,
        };
        let frame = split.frame_at_staged(0.0, layer, padded, 1.0, 0.0);
        assert_eq!(frame, split.frame_at_staged(0.0, layer, plain, 1.0, 0.0));
        assert!((frame.screen.width - (1.0 - frame.camera.width)).abs() < 1e-9);
        assert_eq!(frame.screen_radius, 0.0);
        assert_eq!(frame.camera_radius, 0.0);
    }

    #[test]
    fn a_padded_screen_gives_up_a_tenth_before_it_crops() {
        let split = LayoutSettings {
            default_layout: CameraLayout::SplitScreen,
            split_auto: false,
            split_ratio: 0.6,
            split_padded: true,
            ..LayoutSettings::default()
        };
        let stage = StageShape {
            aspect: 16.0 / 9.0,
            screen_aspect: 16.0 / 9.0,
            split_gap: 0.05,
            pane_radius: 0.0,
        };
        let layer = LayerSettings {
            enabled: true,
            ..LayerSettings::default()
        };
        let frame = split.frame_at_staged(0.0, layer, stage, 1.0, 0.0);
        let zone = (16.0 / 9.0 - 0.05) * 0.6;
        assert!((frame.screen.height - 0.9).abs() < 1e-9);
        assert!((frame.screen.y - 0.05).abs() < 1e-9);
        assert!((frame.screen.width - zone / (16.0 / 9.0)).abs() < 1e-9);
    }

    #[test]
    fn the_two_aspect_cover_matches_the_one_aspect_cover() {
        for screen in [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            Rect {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            },
            Rect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            Rect {
                x: 0.0,
                y: 0.5,
                width: 1.0,
                height: 0.5,
            },
        ] {
            let one = screen_cover_rect(screen);
            let two = screen_cover_rect_in(screen, 1.6, 1.6);
            assert!((two.x - one.x).abs() < 1e-9);
            assert!((two.y - one.y).abs() < 1e-9);
            assert!((two.width - one.width).abs() < 1e-9);
            assert!((two.height - one.height).abs() < 1e-9);
        }
    }

    #[test]
    fn a_screen_placed_on_a_wider_stage_shows_the_background_beside_it() {
        let screen = solid(20, 20, [255, 0, 0, 255]);
        let background = solid(40, 20, [0, 255, 0, 255]);
        let mut stage = vec![0u8; 40 * 20 * 4];
        place_screen_into(
            &mut stage,
            (40, 20),
            &screen,
            (20, 20),
            contained_screen(2.0, 1.0),
            Some(&background),
            0.0,
        );
        assert_eq!(pixel(&stage, 40, 20, 10), [255, 0, 0, 255]);
        assert_eq!(pixel(&stage, 40, 2, 10), [0, 255, 0, 255]);
        assert_eq!(pixel(&stage, 40, 37, 10), [0, 255, 0, 255]);

        let mut empty = vec![0u8; 40 * 20 * 4];
        place_screen_into(
            &mut empty,
            (40, 20),
            &screen,
            (20, 20),
            contained_screen(2.0, 1.0),
            None,
            0.0,
        );
        assert_eq!(pixel(&empty, 40, 2, 10), [0, 0, 0, 255]);
    }

    #[test]
    fn a_rounded_pane_shows_the_background_in_its_corner() {
        let screen = solid(20, 20, [255, 0, 0, 255]);
        let background = solid(40, 20, [0, 255, 0, 255]);
        let mut stage = vec![0u8; 40 * 20 * 4];
        place_screen_into(
            &mut stage,
            (40, 20),
            &screen,
            (20, 20),
            contained_screen(2.0, 1.0),
            Some(&background),
            1.0,
        );
        let corner = pixel(&stage, 40, 10, 0);
        assert!(
            corner[1] > corner[0],
            "the rounded corner shows the background: {corner:?}"
        );
        assert_eq!(pixel(&stage, 40, 20, 10), [255, 0, 0, 255]);
    }
}
