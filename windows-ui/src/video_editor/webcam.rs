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
/// How long a layout transition takes when the project has not said.
const DEFAULT_TRANSITION_SECONDS: f64 = 0.45;
/// The longest transition a project may ask for.
const MAX_TRANSITION_SECONDS: f64 = 3.0;
/// The narrowest share of the split the screen may take.
pub(super) const MIN_SPLIT_RATIO: f64 = 0.1;
/// The widest share of the split the screen may take.
pub(super) const MAX_SPLIT_RATIO: f64 = 0.9;
/// How much of the frame's height the cutout's camera covers.
const CUTOUT_HEIGHT_RATIO: f64 = 0.9;

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
}

/// Every layout the composition uses, and how it moves between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct LayoutSettings {
    pub default_layout: CameraLayout,
    pub cues: Vec<LayoutCue>,
    pub transition_seconds: f64,
    /// The screen's share of the split's long axis, in 0.1..=0.9.
    pub split_ratio: f64,
    /// Puts the screen on the far side of the split.
    pub split_flip: bool,
    /// Where the cutout sits across the frame's width, in 0..=1: zero holds
    /// the person against the left edge, one against the right, and half
    /// centres them. It applies to every cutout in the composition.
    pub cutout_position_x: f64,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            default_layout: CameraLayout::CameraOverlay,
            cues: Vec::new(),
            transition_seconds: DEFAULT_TRANSITION_SECONDS,
            split_ratio: 0.5,
            split_flip: false,
            cutout_position_x: 0.5,
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

        let mut cues: Vec<LayoutCue> = self
            .cues
            .into_iter()
            .filter(|cue| cue.at.is_finite() && cue.at >= 0.0)
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
        let geometry = Geometry {
            overlay: overlay.clamped(),
            video_aspect: sane_aspect(video_aspect),
            camera_aspect: sane_aspect(camera_aspect),
            split_ratio: self.split_ratio,
            split_flip: self.split_flip,
            cutout_position_x: self.cutout_position_x,
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
        for cue in &self.cues {
            if !cue.at.is_finite() || cue.at < 0.0 || cue.at > seconds {
                continue;
            }
            let target = canonical(cue.layout, geometry);
            if target == to {
                continue;
            }
            let state_at_cue = match started {
                Some(start) => {
                    interpolate(from, to, progress(start, cue.at, self.transition_seconds))
                }
                None => to,
            };
            from = state_at_cue;
            to = target;
            started = Some(cue.at);
        }
        match started {
            Some(start) => interpolate(from, to, progress(start, seconds, self.transition_seconds)),
            None => default,
        }
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
    /// How much of the person's matte blends into the camera, 0..=1.
    pub cutout: f64,
    pub mirror: bool,
}

#[derive(Debug, Clone, Copy)]
struct Geometry {
    overlay: LayerSettings,
    video_aspect: f64,
    camera_aspect: f64,
    split_ratio: f64,
    split_flip: bool,
    cutout_position_x: f64,
}

fn sane_aspect(aspect: f64) -> f64 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    }
}

/// The geometry a layout rests at, before any transition.
fn canonical(layout: CameraLayout, geometry: Geometry) -> LayoutFrame {
    let full = Rect::full();
    match layout {
        CameraLayout::CameraOverlay => {
            let camera = if geometry.overlay.fullscreen {
                full
            } else {
                geometry.overlay.rect_in(full)
            };
            LayoutFrame {
                screen: full,
                camera,
                camera_alpha: if geometry.overlay.enabled { 1.0 } else { 0.0 },
                camera_radius: if geometry.overlay.fullscreen {
                    0.0
                } else {
                    geometry.overlay.radius
                },
                cutout: 0.0,
                mirror: geometry.overlay.mirror,
            }
        }
        CameraLayout::FullscreenCamera => LayoutFrame {
            screen: full,
            camera: full,
            camera_alpha: 1.0,
            camera_radius: 0.0,
            cutout: 0.0,
            mirror: geometry.overlay.mirror,
        },
        CameraLayout::SplitScreen => {
            let (screen, camera) = split_rects(geometry);
            LayoutFrame {
                screen,
                camera,
                camera_alpha: 1.0,
                camera_radius: 0.0,
                cutout: 0.0,
                mirror: geometry.overlay.mirror,
            }
        }
        CameraLayout::ScreenOnly => LayoutFrame {
            screen: full,
            camera: geometry.overlay.rect_in(full),
            camera_alpha: 0.0,
            camera_radius: geometry.overlay.radius,
            cutout: 0.0,
            mirror: geometry.overlay.mirror,
        },
        CameraLayout::CameraCutout => LayoutFrame {
            screen: full,
            camera: cutout_rect(geometry),
            camera_alpha: 1.0,
            camera_radius: 0.0,
            cutout: 1.0,
            mirror: geometry.overlay.mirror,
        },
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
    let ratio = geometry.split_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
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

/// The bottom-aligned camera of the cutout, sized from the raw camera aspect
/// and placed across the frame by the project's cutout position.
fn cutout_rect(geometry: Geometry) -> Rect {
    let height = CUTOUT_HEIGHT_RATIO;
    let width = (height * geometry.camera_aspect / geometry.video_aspect).min(1.0);
    let height = if width < 1.0 {
        height
    } else {
        geometry.video_aspect / geometry.camera_aspect
    };
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

fn progress(start: f64, seconds: f64, duration: f64) -> f64 {
    if !duration.is_finite() || duration <= 1e-9 {
        return 1.0;
    }
    let t = ((seconds - start) / duration).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn interpolate(from: LayoutFrame, to: LayoutFrame, t: f64) -> LayoutFrame {
    LayoutFrame {
        screen: from.screen.lerp(to.screen, t),
        camera: from.camera.lerp(to.camera, t),
        camera_alpha: from.camera_alpha + (to.camera_alpha - from.camera_alpha) * t,
        camera_radius: from.camera_radius + (to.camera_radius - from.camera_radius) * t,
        cutout: from.cutout + (to.cutout - from.cutout) * t,
        mirror: to.mirror,
    }
}

/// The corner radius a shape index starts at, as a fraction of the shorter side.
pub(super) fn radius_for_shape(shape: u8) -> f64 {
    match shape {
        0 => 1.0,
        1 => 0.08,
        _ => 0.12,
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
        let _ = context.clip();
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
    /// Bumped by every document load; a worker only publishes frames from the
    /// generation it was asked for.
    generation: Mutex<u64>,
}

struct Track {
    path: PathBuf,
    duration: f64,
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
    cache: RefCell<Option<(u64, ImageSurface)>>,
    cutout_cache: RefCell<Option<(u64, f64, ImageSurface)>>,
    requested: Cell<f64>,
    requested_cutout: Cell<bool>,
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
                cache: RefCell::new(None),
                cutout_cache: RefCell::new(None),
                requested: Cell::new(-1.0),
                requested_cutout: Cell::new(false),
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
        *self.state.cache.borrow_mut() = None;
        *self.state.cutout_cache.borrow_mut() = None;
        self.state.requested.set(-1.0);
        self.state.requested_cutout.set(false);
        *self.state.shared.frame.lock().unwrap() = None;
        *self.state.shared.job.lock().unwrap() = None;
        *self.state.shared.error.lock().unwrap() = None;
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
            video_seconds,
            last_frame,
            camera_size: (track.width, track.height),
        });
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
        Some(self.state.layouts.borrow().frame_at(
            self.state.composition_seconds.get(),
            settings,
            video_aspect,
            camera_aspect,
        ))
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
        Some(WebcamExport {
            path: track.path.clone(),
            settings,
            layouts,
            track_seconds: track.duration,
            video_seconds: track.video_seconds,
            last_frame: track.last_frame,
            source_size: track.camera_size,
        })
    }

    /// Ask for the frame the playhead needs. The decode runs on a worker, and
    /// the still preview repaints once it lands; playback repaints anyway.
    pub fn request(&self, source_seconds: f64) {
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
        if (self.state.requested.get() - target).abs() < REQUEST_STEP_SECONDS
            && self.state.requested_cutout.get() == cutout
        {
            return;
        }
        self.state.requested.set(target);
        self.state.requested_cutout.set(cutout);

        let (path, camera_size) = {
            let track = self.state.track.borrow();
            let Some(track) = track.as_ref() else {
                return;
            };
            (track.path.clone(), track.camera_size)
        };
        let generation = *self.state.shared.generation.lock().unwrap();
        *self.state.shared.job.lock().unwrap() = Some(Job {
            path,
            seconds: target,
            generation,
            size: preview_size(camera_size),
            cutout,
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
        let track = self.state.track.borrow();
        let track = track.as_ref()?;
        let target = if track.video_seconds > 0.0 && track.duration > 0.0 {
            (source_seconds * (track.duration / track.video_seconds)).max(0.0)
        } else {
            source_seconds.max(0.0)
        };
        Some(target.min(track.last_frame))
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
    pub track_seconds: f64,
    pub video_seconds: f64,
    /// The last instant a seek can land on and still find a decoded frame.
    pub last_frame: f64,
    /// The camera's own frame size, so the decode keeps its aspect.
    pub source_size: (u32, u32),
}

impl WebcamExport {
    /// A video instant as a time in the track.
    pub fn track_at(&self, video_seconds: f64) -> f64 {
        if self.video_seconds > 0.0 && self.track_seconds > 0.0 {
            return (video_seconds * (self.track_seconds / self.video_seconds)).max(0.0);
        }
        video_seconds.max(0.0)
    }

    /// The decode start for a video instant, capped at the camera's last real
    /// frame so a trim that opens near the end still finds footage.
    pub fn start_at(&self, video_seconds: f64) -> f64 {
        self.track_at(video_seconds).min(self.last_frame)
    }

    /// The timestamp multiplier that puts the track's frames on the video's
    /// clock: a track that recorded less than the video is slowed by the
    /// reciprocal of its length ratio, so a video instant still asks for the
    /// same camera instant.
    pub fn time_scale(&self) -> f64 {
        if self.video_seconds > 0.0 && self.track_seconds > 0.0 {
            (self.video_seconds / self.track_seconds).clamp(0.01, 100.0)
        } else {
            1.0
        }
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
                    Ok(mask) => Some(mask),
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
    pub fn open(
        path: &Path,
        start_seconds: f64,
        duration_seconds: f64,
        fps: f64,
        size: (u32, u32),
        time_scale: f64,
    ) -> Option<Self> {
        let frame_len = size.0 as usize * size.1 as usize * 4;
        let frames = (duration_seconds.max(0.0) * fps).ceil().max(1.0) as u64;
        let time_scale = if time_scale.is_finite() && time_scale > 0.0 {
            time_scale
        } else {
            1.0
        };
        let mut child = Command::new(crate::recording::editor::ffmpeg::ffmpeg_executable())
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-ss", &format!("{start_seconds:.6}")])
            .arg("-i")
            .arg(path)
            .args([
                "-vf",
                &format!(
                    "scale={}:{},setsar=1,setpts=(PTS-STARTPTS)*{time_scale:.9},fps={fps}",
                    size.0, size.1
                ),
            ])
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
    let (x0, y0, x1, y1) = destination;
    let out_width = x1.saturating_sub(x0) as usize;
    let out_height = y1.saturating_sub(y0) as usize;
    let mut output = vec![0u8; out_width * out_height * 4];
    if out_width == 0 || out_height == 0 || size.0 == 0 || size.1 == 0 {
        return output;
    }
    let source_width = size.0 as usize;
    let source_height = size.1 as usize;
    let frame_width = f64::from(size.0);
    let frame_height = f64::from(size.1);
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
            let source_y = normalized_y * frame_height - 0.5;
            let clamped_y = source_y.clamp(0.0, source_height.saturating_sub(1) as f64);
            let y0_index = clamped_y as usize;
            let y1_index = (y0_index + 1).min(source_height.saturating_sub(1));
            let fy = clamped_y - y0_index as f64;
            for x in 0..out_width {
                let normalized_x = base_x + x as f64 * step_x;
                let source_x = normalized_x * frame_width - 0.5;
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
        LayoutCue { at, layout }
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
    fn transitions_smoothstep_between_the_resting_rects() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::SplitScreen),
                cue(2.0, CameraLayout::FullscreenCamera),
            ],
            1.0,
        );
        let before = resting(&settings, 1.0);
        let middle = resting(&settings, 1.5);
        let end = resting(&settings, 3.0);
        assert!((before.screen.width - 1.0).abs() < 1e-9);
        assert!((middle.screen.width - 0.75).abs() < 1e-9);
        assert!(
            (middle.screen.height - 1.0).abs() < 1e-9,
            "the growing split pane keeps the frame's full height"
        );
        assert!((middle.camera.y - 0.275).abs() < 1e-9);
        assert!((end.screen.width - 1.0).abs() < 1e-9);
        assert!((middle.camera_alpha - 1.0).abs() < 1e-9);
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
        assert!((resting(&with, 2.0).camera.width - 1.0).abs() < 1e-9);
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
        assert!((wide.camera.height - CUTOUT_HEIGHT_RATIO).abs() < 1e-9);
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
    fn a_cutout_transition_blends_the_matte_in_and_out() {
        let settings = settings(
            vec![
                cue(1.0, CameraLayout::CameraCutout),
                cue(2.0, CameraLayout::CameraOverlay),
            ],
            1.0,
        );
        let into = resting(&settings, 1.5);
        assert!((into.cutout - 0.5).abs() < 1e-9);
        assert!((into.camera_alpha - 1.0).abs() < 1e-9);
        assert!(into.camera.y > 0.0);
        let out = resting(&settings, 2.5);
        assert!((out.cutout - 0.5).abs() < 1e-9);
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
            pixel(&left, size.0, 8, 90)[1] > 200,
            "the left placement covers the left edge"
        );
        assert_eq!(pixel(&right, size.0, 8, 90), [10, 20, 30, 255]);
        assert!(
            pixel(&right, size.0, 311, 90)[1] > 200,
            "the right placement covers the right edge"
        );
        assert_eq!(pixel(&left, size.0, 311, 90), [10, 20, 30, 255]);
        assert!(
            pixel(&left, size.0, 160, 90)[1] > 200 && pixel(&right, size.0, 160, 90)[1] > 200,
            "both placements still cover the middle of the frame"
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
            generation: Mutex::new(2),
        };
        let old = Job {
            path: PathBuf::from("old.mp4"),
            seconds: 0.0,
            generation: 1,
            size: (2, 2),
            cutout: true,
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
    fn a_shape_starts_at_the_radius_the_recorder_drew() {
        assert_eq!(radius_for_shape(0), 1.0);
        assert!(radius_for_shape(1) < radius_for_shape(2));
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
            track_seconds: duration,
            video_seconds: duration,
            last_frame: last,
            source_size: (32, 32),
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
            track_seconds: 8.0,
            video_seconds: 10.0,
            last_frame: 7.95,
            source_size: (1280, 720),
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
}
