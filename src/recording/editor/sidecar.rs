use super::cursor_path::SpringConfig;
use super::cursor_planner::{self, PlannedPath, PlannerPoint, SyncPoint};
use super::cursor_shake::{self, ShakeEvent, ShakeEventKind, DEFAULT_REMOVE_SHAKE_THRESHOLD};
use super::model::CursorRotationMode;
use super::recording_id::{
    recording_directory, recording_id_of, recordings_directory, RECORDING_SIDECAR_NAME,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const SIDECAR_VERSION: u32 = 2;
pub const MAX_POINTER_SAMPLES: usize = 8_000;
pub const WINDOW_CURSOR_STALE_SECONDS: f64 = 0.2;
pub const MAX_CLICKS: usize = 500;
pub const MAX_PRESSES: usize = 500;
pub const CLICK_PULSE_WINDOW_SECONDS: f64 = 0.08;
/// Visible lifetime of the studied ripple effect, in seconds.
pub const CLICK_RIPPLE_WINDOW_SECONDS: f64 =
    crate::recording::editor::click_effect::RIPPLE_VISIBLE_DURATION_MS / 1000.0;
/// The studied recorder keeps at most this many recent clicks alive at once.
pub const MAX_SIMULTANEOUS_CLICKS: usize = 3;

/// The studied cursor velocity profile: tilt follows horizontal speed, clamped
/// to a modest angle and calmed for the first half second. These are the
/// studied defaults (`velocity` mode, `rotateOnXMovementRatio` 0.5).
pub const ROTATE_ON_X_MOVEMENT_RATIO: f64 = 0.5;
pub const BASE_CURSOR_ROTATION_RAD: f64 = 0.0;
const ROTATION_STEP_MS: f64 = 16.0;
const ROTATION_TILT_DEG_PER_VELOCITY: f64 = 12.0;
const ROTATION_MAX_TILT_DEG: f64 = 20.0;
const ROTATION_VELOCITY_SMOOTHING_MS: f64 = 120.0;
const ROTATION_STARTUP_CALM_MS: f64 = 500.0;
const ROTATION_ENTRY_TELEPORT_SPEED: f64 = 15.0;
const ROTATION_EMIT_EPSILON: f64 = 0.0015;

/// The studied directional profile: the cursor turns toward its travel
/// heading, measured from the arrow art's own neutral heading, with bounded
/// turn rate and a short angular damping.
const DIRECTIONAL_ALIGN_RATE_PER_PX: f64 = 0.00005;
const DIRECTIONAL_SPEED_SATURATION_PX_PER_MS: f64 = 3.0;
const DIRECTIONAL_ANGULAR_DAMPING_MS: f64 = 100.0;
const DIRECTIONAL_MAX_TURN_RATE_DEG_PER_MS: f64 = 0.4;
const DIRECTIONAL_MAX_FROM_NEUTRAL_DEG: f64 = 135.0;
const DIRECTIONAL_MIN_TRAVEL_PX: f64 = 4.0;
const DIRECTIONAL_NEUTRAL_HEADING_DEG: f64 = -112.5;

#[derive(Default)]
struct RotationEmitter {
    keyframes: Vec<(f64, f64)>,
    last: f64,
    pending: Option<(f64, f64)>,
}

impl RotationEmitter {
    fn emit(&mut self, time_ms: f64, radians: f64, force: bool) {
        if !force && (radians - self.last).abs() < ROTATION_EMIT_EPSILON {
            self.pending = Some((time_ms, radians));
            return;
        }
        if let Some((pending_time, pending_radians)) = self.pending.take() {
            if pending_time < time_ms {
                self.keyframes.push((pending_time, pending_radians));
            }
        }
        if let Some(last) = self.keyframes.last_mut() {
            if (last.0 - time_ms).abs() < 1e-9 {
                last.1 = radians;
                self.last = radians;
                return;
            }
        }
        self.keyframes.push((time_ms, radians));
        self.last = radians;
    }
}

fn tilt_for_velocity(velocity_px_per_ms: f64, ratio: f64) -> f64 {
    (velocity_px_per_ms * ROTATION_TILT_DEG_PER_VELOCITY * ratio)
        .clamp(-ROTATION_MAX_TILT_DEG, ROTATION_MAX_TILT_DEG)
        .to_radians()
}

fn simulate_rotation(
    velocity: &mut f64,
    emitter: &mut RotationEmitter,
    ratio: f64,
    start_ms: f64,
    span_ms: f64,
    target: f64,
) {
    let steps = (span_ms / ROTATION_STEP_MS).ceil().max(1.0);
    let step = span_ms / steps;
    let alpha = 1.0 - (-step / ROTATION_VELOCITY_SMOOTHING_MS).exp();
    for index in 1..=(steps as usize) {
        *velocity += (target - *velocity) * alpha;
        emitter.emit(
            start_ms + index as f64 * step,
            tilt_for_velocity(*velocity, ratio),
            false,
        );
    }
}

/// Whether the pointer entered the footage fast enough to be a teleport: the
/// studied profile skips the first two samples' velocity when it does.
fn entry_is_fast(samples: &[PointerSample]) -> bool {
    if samples.len() < 2 {
        return false;
    }
    let first = &samples[0];
    let second = &samples[1];
    let dt = (second.t - first.t) * 1000.0;
    dt > 0.0 && (second.x - first.x).hypot(second.y - first.y) / dt > ROTATION_ENTRY_TELEPORT_SPEED
}

/// Horizontal speed at the sample `start`, looking forward within the
/// smoothing window. Mirrors the studied `velocitySmoothingMs` scan.
fn horizontal_velocity(samples: &[PointerSample], start: usize) -> f64 {
    let Some(previous) = start.checked_sub(1).and_then(|index| samples.get(index)) else {
        return 0.0;
    };
    for index in start..samples.len() {
        let sample = &samples[index];
        let dt = (sample.t - previous.t) * 1000.0;
        if dt >= ROTATION_VELOCITY_SMOOTHING_MS || index == samples.len() - 1 {
            return if dt > 0.0 {
                (sample.x - previous.x) / dt
            } else {
                0.0
            };
        }
    }
    0.0
}

fn interpolate_rotation(keyframes: &[(f64, f64)], time_ms: f64) -> f64 {
    let Some((_, first_radians)) = keyframes.first().copied() else {
        return 0.0;
    };
    let after = keyframes.partition_point(|(time, _)| *time <= time_ms);
    if after == 0 {
        return first_radians;
    }
    let (from_time, from_radians) = keyframes[after - 1];
    let Some((to_time, to_radians)) = keyframes.get(after).copied() else {
        return from_radians;
    };
    let span = to_time - from_time;
    let amount = if span > 0.0 {
        ((time_ms - from_time) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    from_radians + (to_radians - from_radians) * amount
}

/// Hold the studied startup window flat at the rotation it reaches by then,
/// instead of easing out of the first frame's spike.
fn calm_startup(keyframes: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if keyframes.len() < 2 {
        return keyframes;
    }
    let calm_at = keyframes[0].0 + ROTATION_STARTUP_CALM_MS;
    let calm = interpolate_rotation(&keyframes, calm_at);
    let mut result = vec![(keyframes[0].0, calm), (calm_at, calm)];
    result.extend(keyframes.into_iter().filter(|(time, _)| *time > calm_at));
    result
}

fn velocity_rotation_keyframes(samples: &[PointerSample], ratio: f64) -> Vec<(f64, f64)> {
    let teleport = if entry_is_fast(samples) { 2 } else { 1 };
    let mut emitter = RotationEmitter::default();
    let mut velocity = horizontal_velocity(samples, teleport);
    let first = &samples[0];

    emitter.emit(first.t * 1000.0, tilt_for_velocity(velocity, ratio), true);
    let mut previous = first;
    for (index, sample) in samples.iter().enumerate().skip(1) {
        let dt = (sample.t - previous.t) * 1000.0;
        if dt > 0.0 {
            let target = if index < teleport {
                0.0
            } else {
                (sample.x - previous.x) / dt
            };
            simulate_rotation(
                &mut velocity,
                &mut emitter,
                ratio,
                previous.t * 1000.0,
                dt,
                target,
            );
        }
        previous = sample;
    }
    let tail = ROTATION_VELOCITY_SMOOTHING_MS * 6.0;
    simulate_rotation(
        &mut velocity,
        &mut emitter,
        ratio,
        previous.t * 1000.0,
        tail,
        0.0,
    );
    emitter.emit(previous.t * 1000.0 + tail, 0.0, true);
    calm_startup(emitter.keyframes)
}

fn directional_rotation_keyframes(samples: &[PointerSample]) -> Vec<(f64, f64)> {
    let max_from_neutral = DIRECTIONAL_MAX_FROM_NEUTRAL_DEG.to_radians();
    let teleport = if entry_is_fast(samples) { 2 } else { 1 };
    let mut emitter = RotationEmitter::default();
    let mut rotation = heading_for(samples, teleport);
    let mut angular_velocity = 0.0;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_ms = 0.0;

    emitter.emit(samples[0].t * 1000.0, rotation, true);
    let mut previous = &samples[0];
    for (index, sample) in samples.iter().enumerate().skip(1) {
        let dt = (sample.t - previous.t) * 1000.0;
        if dt > 0.0 {
            if index < teleport {
                step_directional(
                    &mut rotation,
                    &mut angular_velocity,
                    &mut emitter,
                    previous.t * 1000.0,
                    dt,
                    None,
                    0.0,
                    max_from_neutral,
                );
            } else {
                sum_x += sample.x - previous.x;
                sum_y += sample.y - previous.y;
                sum_ms += dt;
                let travel = sum_x.hypot(sum_y);
                if travel >= DIRECTIONAL_MIN_TRAVEL_PX {
                    let heading = sum_y.atan2(sum_x);
                    step_directional(
                        &mut rotation,
                        &mut angular_velocity,
                        &mut emitter,
                        previous.t * 1000.0,
                        dt,
                        Some(heading),
                        travel / sum_ms,
                        max_from_neutral,
                    );
                    sum_x = 0.0;
                    sum_y = 0.0;
                    sum_ms = 0.0;
                } else {
                    step_directional(
                        &mut rotation,
                        &mut angular_velocity,
                        &mut emitter,
                        previous.t * 1000.0,
                        dt,
                        None,
                        0.0,
                        max_from_neutral,
                    );
                }
            }
        }
        previous = sample;
    }
    let tail = DIRECTIONAL_ANGULAR_DAMPING_MS * 8.0;
    step_directional(
        &mut rotation,
        &mut angular_velocity,
        &mut emitter,
        previous.t * 1000.0,
        tail,
        None,
        0.0,
        max_from_neutral,
    );
    emitter.emit(previous.t * 1000.0 + tail, rotation, true);
    calm_startup(emitter.keyframes)
}

#[allow(clippy::too_many_arguments)]
fn step_directional(
    rotation: &mut f64,
    angular_velocity: &mut f64,
    emitter: &mut RotationEmitter,
    start_ms: f64,
    span_ms: f64,
    heading: Option<f64>,
    travel_speed: f64,
    max_from_neutral: f64,
) {
    let neutral = DIRECTIONAL_NEUTRAL_HEADING_DEG.to_radians();
    let max_turn_rate = DIRECTIONAL_MAX_TURN_RATE_DEG_PER_MS.to_radians();
    let steps = (span_ms / ROTATION_STEP_MS).ceil().max(1.0);
    let step = span_ms / steps;
    let damping = (-step / DIRECTIONAL_ANGULAR_DAMPING_MS).exp();
    for index in 1..=(steps as usize) {
        if let Some(heading) = heading {
            let delta = heading - (neutral + *rotation);
            let speed = travel_speed.min(DIRECTIONAL_SPEED_SATURATION_PX_PER_MS);
            *angular_velocity += DIRECTIONAL_ALIGN_RATE_PER_PX * speed * delta.sin() * step;
        }
        *angular_velocity *= damping;
        *angular_velocity = angular_velocity.clamp(-max_turn_rate, max_turn_rate);
        *rotation += *angular_velocity * step;
        if rotation.abs() > max_from_neutral {
            *rotation = rotation.clamp(-max_from_neutral, max_from_neutral);
            *angular_velocity = 0.0;
        }
        emitter.emit(start_ms + index as f64 * step, *rotation, false);
    }
}

/// The studied opening heading: the first travel direction at or after
/// `start`, relative to the arrow art's neutral heading.
fn heading_for(samples: &[PointerSample], start: usize) -> f64 {
    let neutral = DIRECTIONAL_NEUTRAL_HEADING_DEG.to_radians();
    let max_from_neutral = DIRECTIONAL_MAX_FROM_NEUTRAL_DEG.to_radians();
    let Some(previous) = start.checked_sub(1).and_then(|index| samples.get(index)) else {
        return 0.0;
    };
    for sample in samples.iter().skip(start) {
        let dx = sample.x - previous.x;
        let dy = sample.y - previous.y;
        if dx.hypot(dy) >= DIRECTIONAL_MIN_TRAVEL_PX {
            return normalize_radians(dy.atan2(dx) - neutral)
                .clamp(-max_from_neutral, max_from_neutral);
        }
    }
    0.0
}

fn normalize_radians(angle: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    ((angle + PI) % TAU + TAU) % TAU - PI
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointerDataSource {
    #[default]
    Recorded,
    InferredFromVideo,
    WindowMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorKind {
    #[default]
    Default,
    Hand,
    Text,
    Crosshair,
    Move,
    Wait,
    #[serde(rename = "resize_h", alias = "resize")]
    ResizeH,
    #[serde(rename = "resize_v")]
    ResizeV,
    #[serde(rename = "resize_diag")]
    ResizeDiag,
}

impl CursorKind {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "hand" => Self::Hand,
            "text" => Self::Text,
            "crosshair" => Self::Crosshair,
            "move" | "fleur" | "all-scroll" => Self::Move,
            "wait" | "watch" | "busy" => Self::Wait,
            "resize" | "resize_h" | "ew-resize" | "col-resize" => Self::ResizeH,
            "resize_v" | "ns-resize" | "row-resize" => Self::ResizeV,
            "resize_diag" | "nwse-resize" | "nesw-resize" => Self::ResizeDiag,
            _ => Self::Default,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Hand => "hand",
            Self::Text => "text",
            Self::Crosshair => "crosshair",
            Self::Move => "move",
            Self::Wait => "wait",
            Self::ResizeH => "resize_h",
            Self::ResizeV => "resize_v",
            Self::ResizeDiag => "resize_diag",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl CaptureRegion {
    pub fn from_capture(x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>) -> Self {
        match (x, y, w, h) {
            (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => Self { x, y, w, h },
            _ => Self {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        }
    }

    pub fn is_area(self) -> bool {
        self.w > 0 && self.h > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMotion {
    pub smooth: f64,
    pub hide_idle: bool,
    pub idle_ms: f64,
    pub speed: f64,
    pub rotation_mode: CursorRotationMode,
    pub rotation_ratio: f64,
    pub base_rotation_deg: f64,
}

impl Default for CursorMotion {
    fn default() -> Self {
        Self {
            smooth: 0.0,
            hide_idle: false,
            idle_ms: 800.0,
            speed: 1.0,
            rotation_mode: CursorRotationMode::Velocity,
            rotation_ratio: ROTATE_ON_X_MOVEMENT_RATIO,
            base_rotation_deg: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CursorFrame {
    pub x: f64,
    pub y: f64,
    pub kind: CursorKind,
    pub alpha: f64,
    /// Tilt in radians about the pointer, from the studied velocity profile.
    pub rotation: f64,
    /// Catalog id of the recorded cursor image, when this frame has one.
    pub cursor_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub kind: CursorKind,
    /// Catalog id of the recorded cursor image shown at this sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
}

/// A recorded change in whether the selected window has a visible cursor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CursorVisibilitySample {
    /// Event time on the recording clock, in seconds.
    pub t: f64,
    /// Whether a fresh cursor position is visible in the selected window.
    pub visible: bool,
    /// Preserve a cursor through a media-clock hold inserted for a pause.
    #[serde(default)]
    pub hold_until_next: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClickSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub button: i32,
    /// Catalog id of the recorded cursor image at the click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
}

/// One mouse button held from `down` to `up` on the recording clock, in
/// seconds. `dragged` is set when the pointer moved past a small threshold
/// before the button came up, so a drag reads differently from a plain click.
///
/// Recorded from button state only: button identity and timing, never a key,
/// a keycode, or a typed character.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PressSample {
    pub down: f64,
    pub up: f64,
    pub button: i32,
    #[serde(default)]
    pub dragged: bool,
}

impl PressSample {
    /// Whether `t` falls inside this press interval. The interval is
    /// half-open, so a press is not held at the exact instant it comes up.
    pub fn contains(&self, t: f64) -> bool {
        t >= self.down && t < self.up
    }
}

/// One wheel event on the recording clock. `delta` keeps the sign the native
/// hook reports; `horizontal` separates a side wheel from a vertical one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScrollSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub delta: i32,
    #[serde(default)]
    pub horizontal: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
}

/// One key down or up on the recording clock. `kind` is `key_down` or
/// `key_up`; `key` is the virtual-key code and `scan_code` the hardware scan
/// code. Capture is opt-in, so an older sidecar simply has no key stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeySample {
    pub t: f64,
    #[serde(rename = "type")]
    pub kind: String,
    pub key: i32,
    #[serde(default)]
    pub scan_code: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<String>,
}

/// A cursor shape captured beside the pointer samples. `file` is relative to
/// the sidecar's `cursor_images` folder; the hotspot is in cursor pixels, so
/// the editor draws the true pointer instead of guessing from `CursorKind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorAsset {
    pub id: String,
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub hotspot_x: i32,
    pub hotspot_y: i32,
    #[serde(default)]
    pub kind: CursorKind,
}

/// A captured cursor PNG resolved from the sidecar catalog, with the hotspot
/// the image must be drawn against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedCursorImage {
    pub path: PathBuf,
    pub hotspot_x: i32,
    pub hotspot_y: i32,
}

impl CursorAsset {
    /// The hotspot in cursor pixels.
    pub fn hotspot(&self) -> (i32, i32) {
        (self.hotspot_x, self.hotspot_y)
    }
}

/// A catalog name must be a bare file or folder name. Cursor images live
/// beside the sidecar, so a separator, a drive, or an alternate data stream
/// would let a hand-edited sidecar point somewhere else.
fn is_bare_catalog_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':' | '\0'))
}

/// The camera track recorded beside a video.
///
/// `file` names a file in the pointer sidecar's own directory, and the fractions
/// place the bubble in the recorded region: they hold at any output resolution,
/// which is what lets the editor re-render the layer instead of replaying a
/// burn-in. `duration` is the track's own length: the camera's clock is not the
/// desktop's, so the editor scales video time by it to find the same instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WebcamTrack {
    pub file: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub duration: f64,
    /// Seconds from the start of the video to the first frame of the track.
    /// The camera process starts after the screen process, so this is usually
    /// the startup gap the editor has to hold or skip.
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
    /// 0 circle, 1 square, 2 rectangle, 3 vertical: the shared shape table.
    #[serde(default)]
    pub shape: u8,
    #[serde(default)]
    pub flip: bool,
}

impl WebcamTrack {
    /// Whether the block describes a track the editor can draw.
    pub fn is_valid(&self) -> bool {
        !self.file.is_empty() && self.width > 0 && self.height > 0 && self.w > 0.0 && self.h > 0.0
    }

    /// The bubble's fractional rect, clamped so a hand-edited sidecar cannot
    /// place the layer outside the footage.
    pub fn rect(&self) -> (f64, f64, f64, f64) {
        let x = self.x.clamp(0.0, 1.0);
        let y = self.y.clamp(0.0, 1.0);
        let w = self.w.clamp(0.0, 1.0 - x);
        let h = self.h.clamp(0.0, 1.0 - y);
        (x, y, w, h)
    }

    /// A video instant as a time in the track. A track of unknown length is
    /// played one to one, which is what a camera that kept up records anyway.
    pub fn track_seconds(&self, video_seconds: f64, video_duration: f64) -> f64 {
        let start = self.start.max(0.0);
        if video_duration > start && self.duration > 0.0 {
            let available = video_duration - start;
            let time = (video_seconds - start).max(0.0);
            return time * (self.duration / available);
        }
        (video_seconds - start).max(0.0)
    }
}

/// The cursor path planned for one spring and speed, kept between frames.
#[derive(Debug, Default)]
struct CachedPlan {
    key: Option<u64>,
    path: Option<Arc<PlannedPath>>,
}

/// The output of [`PointerSidecar::shake_survivors`]: one flag per pointer
/// sample and per press end.
struct ShakeSurvivors {
    pointer: Vec<bool>,
    press_down: Vec<bool>,
    press_up: Vec<bool>,
}

/// Runtime-only cache. Two sidecars compare equal regardless of what each
/// planned, and a clone starts cold so a copied sidecar never shows another's
/// recording.
#[derive(Debug, Default)]
struct PlanCache(Mutex<Option<CachedPlan>>);

impl Clone for PlanCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for PlanCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// The pointer samples that survive the shake filter, for the unsmoothed
/// cursor. Runtime-only and cold on clone, like [`PlanCache`].
#[derive(Debug, Default)]
struct ShakeCache(Mutex<Option<(u64, Arc<Vec<PointerSample>>)>>);

impl Clone for ShakeCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for ShakeCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointerSidecar {
    pub version: u32,
    /// Monotonic origin of the recording. Newly recorded event times use the
    /// retained media clock, excluding pauses when the backend removes them.
    pub t0_monotonic_us: i64,
    pub region: CaptureRegion,
    pub pointer: Vec<PointerSample>,
    pub clicks: Vec<ClickSample>,
    /// Visibility changes recorded beside the pointer. Window metadata keeps
    /// its bounded/stale rules; screen and area recordings use the last sample.
    /// Old sidecars have no visibility data and stay always visible.
    #[serde(default)]
    pub visibility: Vec<CursorVisibilitySample>,
    /// Mouse press intervals. Absent in version-1 sidecars, which load as an
    /// empty list and are otherwise unchanged.
    #[serde(default)]
    pub presses: Vec<PressSample>,
    /// Wheel events. Absent when the platform recorder does not capture them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scrolls: Vec<ScrollSample>,
    /// Key events. Empty unless the recorder was asked to capture keystrokes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeySample>,
    #[serde(default)]
    pub source: PointerDataSource,
    /// Folder beside the sidecar holding the cursor PNG files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_images: Option<String>,
    /// Cursor shapes referenced by `cursor_id` on pointer and click samples.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cursors: Vec<CursorAsset>,
    /// The camera layer, absent for a recording made without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webcam: Option<WebcamTrack>,
    /// Clean screen footage retained beside a composited recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_file: Option<String>,
    /// Directory the sidecar was loaded from. Runtime-only: an export may read
    /// the hidden clean-screen file while the cursor catalog stays beside the
    /// original recording's sidecar.
    #[serde(skip)]
    pub sidecar_directory: Option<PathBuf>,
    /// Source instants the edit cuts at, ascending. Pushed in by the editor
    /// before it draws or exports; runtime-only, like the plan built from it.
    #[serde(skip)]
    cuts_seconds: Vec<f64>,
    /// The studied shake filter's threshold in pixels; zero turns it off.
    /// Pushed in by the editor from the cursor settings; runtime-only.
    #[serde(skip, default = "default_shake_threshold")]
    shake_threshold: f64,
    /// The samples the shake filter keeps, for the unsmoothed cursor.
    #[serde(skip)]
    shake_cache: ShakeCache,
    /// The planned cursor path for the current spring and speed.
    #[serde(skip)]
    plan_cache: PlanCache,
}

/// Moments the recorder marked, and where each pause ended, in source
/// seconds, from the recording manifest beside the pointer sidecar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingMarks {
    /// Moments the person marked with the recording-marker shortcut.
    pub markers: Vec<f64>,
    /// Where each session after the first begins: the recording resumed there.
    pub pauses: Vec<f64>,
}

impl RecordingMarks {
    /// Read the marks from a manifest's JSON text. Anything missing or
    /// malformed reads as no marks.
    pub fn parse(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Self::default();
        };
        let seconds = |value: &serde_json::Value| {
            value
                .as_f64()
                .filter(|ms| ms.is_finite() && *ms >= 0.0)
                .map(|ms| ms / 1000.0)
        };
        let mut markers: Vec<f64> = value
            .get("markers")
            .and_then(|markers| markers.as_array())
            .map(|markers| markers.iter().filter_map(seconds).collect())
            .unwrap_or_default();
        markers.sort_by(f64::total_cmp);
        let mut pauses: Vec<f64> = value
            .get("sessions")
            .and_then(|sessions| sessions.as_array())
            .map(|sessions| {
                sessions
                    .iter()
                    .skip(1)
                    .filter_map(|session| session.get("startMs").and_then(seconds))
                    .collect()
            })
            .unwrap_or_default();
        pauses.sort_by(f64::total_cmp);
        Self { markers, pauses }
    }

    /// The marks recorded beside `video_path`, or none.
    pub fn for_video(video_path: &Path) -> Self {
        let mut path = PointerSidecar::resolved_sidecar_path(video_path).into_os_string();
        path.push(".recording.json");
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }
}

impl PointerSidecar {
    pub fn new(t0_monotonic_us: i64, region: CaptureRegion) -> Self {
        Self {
            version: SIDECAR_VERSION,
            t0_monotonic_us,
            region,
            pointer: Vec::new(),
            clicks: Vec::new(),
            visibility: Vec::new(),
            presses: Vec::new(),
            scrolls: Vec::new(),
            keys: Vec::new(),
            source: PointerDataSource::Recorded,
            cursor_images: None,
            cursors: Vec::new(),
            webcam: None,
            screen_file: None,
            sidecar_directory: None,
            cuts_seconds: Vec::new(),
            shake_threshold: DEFAULT_REMOVE_SHAKE_THRESHOLD,
            shake_cache: ShakeCache::default(),
            plan_cache: PlanCache::default(),
        }
    }

    pub fn mark_inferred_from_video(&mut self) {
        self.source = PointerDataSource::InferredFromVideo;
    }

    pub fn can_render_cursor_overlay(&self) -> bool {
        matches!(
            self.source,
            PointerDataSource::Recorded | PointerDataSource::WindowMetadata
        ) && !self.pointer.is_empty()
    }

    /// Whether a mouse button is held at `t` (the interval is half-open, so a
    /// press is not held at the exact instant it comes up).
    pub fn is_pressed_at(&self, t: f64) -> bool {
        self.presses.iter().any(|press| press.contains(t))
    }

    /// Whether the press covering `t` moved far enough to be a drag.
    pub fn is_dragged_at(&self, t: f64) -> bool {
        self.presses
            .iter()
            .any(|press| press.dragged && press.contains(t))
    }

    pub fn sidecar_path(video_path: &Path) -> PathBuf {
        pointer_sidecars_directory().join(sidecar_file_name(video_path))
    }

    /// The sidecar inside the folder named by the video's recording ID, when
    /// the MP4 carries one. It may not exist yet.
    pub fn recording_sidecar_path(video_path: &Path) -> Option<PathBuf> {
        recording_id_of(video_path).map(|id| recording_directory(&id).join(RECORDING_SIDECAR_NAME))
    }

    /// Where this video's sidecar is read from: the recording-ID folder, then
    /// the path hash used before recordings carried an ID.
    pub fn resolved_sidecar_path(video_path: &Path) -> PathBuf {
        Self::recording_sidecar_path(video_path)
            .filter(|path| path.is_file())
            .unwrap_or_else(|| Self::sidecar_path(video_path))
    }

    pub fn legacy_sidecar_path(video_path: &Path) -> PathBuf {
        let parent = video_path.parent().unwrap_or_else(|| Path::new(""));
        let stem = video_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("recording");
        parent.join(format!("{stem}.apexshot.json"))
    }

    pub fn exists_next_to_video(video_path: &Path) -> bool {
        Self::resolved_sidecar_path(video_path).is_file()
            || Self::legacy_sidecar_path(video_path).is_file()
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        let mut sidecar: Self = serde_json::from_slice(&bytes)?;
        sidecar.sidecar_directory = path.parent().map(Path::to_path_buf);
        Ok(sidecar)
    }

    /// The directory that owns this sidecar's relative assets.
    fn asset_directory(&self, video_path: &Path) -> Option<PathBuf> {
        self.sidecar_directory.clone().or_else(|| {
            Self::resolved_sidecar_path(video_path)
                .parent()
                .map(Path::to_path_buf)
        })
    }

    /// The folder beside this sidecar's directory that holds the cursor PNGs.
    pub fn cursor_image_directory(&self, sidecar_directory: &Path) -> Option<PathBuf> {
        let folder = self.cursor_images.as_deref()?;
        if !is_bare_catalog_name(folder) {
            return None;
        }
        Some(sidecar_directory.join(folder))
    }

    /// The catalog entry for `cursor_id`.
    pub fn cursor_asset(&self, cursor_id: &str) -> Option<&CursorAsset> {
        self.cursors.iter().find(|asset| asset.id == cursor_id)
    }

    /// Resolve one catalog PNG inside a known sidecar directory. Both the
    /// folder and the file name must be bare names, and the file must exist.
    pub fn cursor_asset_path_in(
        &self,
        sidecar_directory: &Path,
        cursor_id: &str,
    ) -> Option<PathBuf> {
        let asset = self.cursor_asset(cursor_id)?;
        if !is_bare_catalog_name(&asset.file) {
            return None;
        }
        let path = self
            .cursor_image_directory(sidecar_directory)?
            .join(&asset.file);
        path.is_file().then_some(path)
    }

    /// Resolve one catalog PNG beside the sidecar recorded for `video_path`.
    pub fn cursor_asset_path(&self, video_path: &Path, cursor_id: &str) -> Option<PathBuf> {
        self.cursor_asset_path_in(&self.asset_directory(video_path)?, cursor_id)
    }

    /// The recorded cursor image for a pointer frame, when the catalog holds
    /// it and its PNG is on disk. A missing entry or file leaves the renderer
    /// free to fall back to the sample's `kind`.
    pub fn cursor_capture(
        &self,
        video_path: &Path,
        frame: &CursorFrame,
    ) -> Option<CapturedCursorImage> {
        let cursor_id = frame.cursor_id.as_deref()?;
        let asset = self.cursor_asset(cursor_id)?;
        let path = self.cursor_asset_path_in(&self.asset_directory(video_path)?, cursor_id)?;
        let (hotspot_x, hotspot_y) = asset.hotspot();
        Some(CapturedCursorImage {
            path,
            hotspot_x,
            hotspot_y,
        })
    }

    /// The camera track's file, resolved beside this sidecar. The recorder
    /// keeps the two together, so the pair is found from the video path alone.
    pub fn webcam_path(&self, video_path: &Path) -> Option<PathBuf> {
        let track = self.webcam.as_ref().filter(|track| track.is_valid())?;
        let directory = self.asset_directory(video_path)?;
        let path = directory.join(&track.file);
        path.is_file().then_some(path)
    }

    /// The clean footage to use when editing a video that already includes its camera.
    pub fn editable_video_path(&self, video_path: &Path) -> Option<PathBuf> {
        let file = self.screen_file.as_deref()?;
        let name = Path::new(file);
        if !matches!(
            name.components().next(),
            Some(std::path::Component::Normal(_))
        ) || name.components().count() != 1
            || self.webcam_path(video_path).is_none()
        {
            return None;
        }
        let path = video_path.parent()?.join(name);
        path.is_file().then_some(path)
    }

    pub fn load_next_to_video(video_path: &Path) -> Option<Self> {
        let path = Self::resolved_sidecar_path(video_path);
        if !path.is_file() {
            return Self::load_migrate_legacy_sidecar(video_path);
        }
        Self::load_logged(&path)
    }

    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn write_next_to_video(&self, video_path: &Path) -> anyhow::Result<PathBuf> {
        let path = Self::recording_sidecar_path(video_path)
            .unwrap_or_else(|| Self::sidecar_path(video_path));
        self.write(&path)?;
        Ok(path)
    }

    pub fn delete_next_to_video(video_path: &Path) {
        if let Some(tagged) = Self::recording_sidecar_path(video_path) {
            if let Some(folder) = tagged.parent() {
                if tagged.is_file() {
                    if let Ok(sidecar) = Self::load(&tagged) {
                        Self::remove_screen_file(&sidecar, video_path);
                    }
                }
                // Only ever a folder directly under the recordings root.
                if folder.parent() == Some(recordings_directory().as_path()) && folder.is_dir() {
                    if let Err(err) = std::fs::remove_dir_all(folder) {
                        eprintln!(
                            "[recording] failed to delete recording folder {}: {err}",
                            folder.display()
                        );
                    }
                }
            }
        }

        let current_path = Self::sidecar_path(video_path);
        if current_path.is_file() {
            if let Ok(sidecar) = Self::load(&current_path) {
                Self::remove_screen_file(&sidecar, video_path);
                let mut webcam_path = current_path.as_os_str().to_os_string();
                webcam_path.push(".webcam.mp4");
                let _ = std::fs::remove_file(webcam_path);
            }
            let mut manifest_path = current_path.as_os_str().to_os_string();
            manifest_path.push(".recording.json");
            let _ = std::fs::remove_file(manifest_path);
            remove_sidecar(&current_path);
        }

        let legacy_path = Self::legacy_sidecar_path(video_path);
        if legacy_path.is_file() {
            remove_sidecar(&legacy_path);
        }
    }

    /// Remove the clean-screen file a sidecar names beside the video.
    fn remove_screen_file(sidecar: &Self, video_path: &Path) {
        if let Some(file) = sidecar.screen_file.as_deref() {
            let name = Path::new(file);
            if file.starts_with(".apexshot-screen-")
                && matches!(
                    name.components().next(),
                    Some(std::path::Component::Normal(_))
                )
                && name.components().count() == 1
            {
                if let Some(parent) = video_path.parent() {
                    let _ = std::fs::remove_file(parent.join(name));
                }
            }
        }
    }

    pub fn subtract_region(&mut self) {
        if !self.region.is_area() {
            return;
        }
        let origin_x = self.region.x as f64;
        let origin_y = self.region.y as f64;
        for sample in &mut self.pointer {
            sample.x -= origin_x;
            sample.y -= origin_y;
        }
        for click in &mut self.clicks {
            click.x -= origin_x;
            click.y -= origin_y;
        }
    }

    fn load_logged(path: &Path) -> Option<Self> {
        match Self::load(path) {
            Ok(sidecar) => Some(sidecar),
            Err(err) => {
                eprintln!(
                    "[recording] failed to load pointer sidecar {}: {err}",
                    path.display()
                );
                None
            }
        }
    }

    fn load_migrate_legacy_sidecar(video_path: &Path) -> Option<Self> {
        let legacy_path = Self::legacy_sidecar_path(video_path);
        if !legacy_path.is_file() {
            return None;
        }

        let sidecar = Self::load_logged(&legacy_path)?;
        if sidecar.write(&Self::sidecar_path(video_path)).is_ok() {
            // The MP4 folder should contain media files only. Keep cursor data
            // in the private app-data directory and migrate old recordings too.
            remove_sidecar(&legacy_path);
        }
        Some(sidecar)
    }

    pub fn sample_at(&self, t: f64) -> Option<&PointerSample> {
        if self.pointer.is_empty() {
            return None;
        }
        let idx = self.pointer.partition_point(|sample| sample.t <= t);
        if idx == 0 {
            return self.pointer.first();
        }
        if idx >= self.pointer.len() {
            return self.pointer.last();
        }
        let prev = &self.pointer[idx - 1];
        let next = &self.pointer[idx];
        if (t - prev.t).abs() <= (next.t - t).abs() {
            Some(prev)
        } else {
            Some(next)
        }
    }

    pub fn presented_at(&self, t: f64, motion: CursorMotion) -> Option<CursorFrame> {
        if self.source != PointerDataSource::WindowMetadata && !self.recorded_cursor_visible_at(t) {
            return None;
        }
        let (_, _, kind, cursor_id) = self.interpolated_at_sample(t)?;
        let (x, y) = self.motion_position_at(t, motion.smooth, motion.speed)?;
        let alpha = if motion.hide_idle {
            self.idle_alpha(t, motion.idle_ms)
        } else {
            1.0
        };
        Some(CursorFrame {
            x,
            y,
            kind,
            alpha,
            rotation: self.cursor_rotation_at(
                t,
                motion.rotation_mode,
                motion.rotation_ratio,
                motion.base_rotation_deg,
            ),
            cursor_id,
        })
    }

    /// The studied cursor rotation at `t`. `Velocity` leans the cursor with
    /// horizontal speed; `Directional` points it along the travel heading.
    /// Both add `base_degrees`, clamp to their studied limits, and calm the
    /// first half second. Times are converted to the profile's milliseconds.
    pub fn cursor_rotation_at(
        &self,
        t: f64,
        mode: CursorRotationMode,
        ratio: f64,
        base_degrees: f64,
    ) -> f64 {
        let base = base_degrees.to_radians();
        if self.pointer.is_empty() {
            return base;
        }
        let keyframes = match mode {
            CursorRotationMode::Velocity => {
                if !ratio.is_finite() || ratio == 0.0 {
                    return base;
                }
                velocity_rotation_keyframes(&self.pointer, ratio)
            }
            CursorRotationMode::Directional => directional_rotation_keyframes(&self.pointer),
        };
        base + interpolate_rotation(&keyframes, t * 1000.0)
    }

    /// Whether a directly recorded cursor is visible at `t`. Window metadata
    /// has its own bounded/stale visibility path; recorded screen and area
    /// sidecars use the last recorded visibility transition.
    fn recorded_cursor_visible_at(&self, t: f64) -> bool {
        let index = self.visibility.partition_point(|sample| sample.t <= t);
        match index
            .checked_sub(1)
            .and_then(|index| self.visibility.get(index))
        {
            Some(sample) => sample.visible,
            None => true,
        }
    }

    /// Return a pointer frame in the encoded video's pixel coordinate space.
    ///
    /// Pointer tracking uses global logical Shell coordinates. They are
    /// translated to the capture rectangle when the sidecar is written, but
    /// PipeWire can encode that rectangle at a different scale (for example on
    /// a HiDPI monitor or after an output-size limit). Keeping this conversion
    /// next to the sidecar ensures the preview and export use the same pixels.
    pub fn presented_in_video_at(
        &self,
        t: f64,
        motion: CursorMotion,
        video_width: f64,
        video_height: f64,
    ) -> Option<CursorFrame> {
        let mut frame = self.presented_at(t, motion)?;
        (frame.x, frame.y) = self.map_to_video(frame.x, frame.y, video_width, video_height);
        Some(frame)
    }

    /// Cursor position after applying the configured smoothing. Camera tracking
    /// uses this same path so the viewport and rendered cursor never react to
    /// different pointer data.
    pub fn motion_position_at(&self, t: f64, smooth: f64, speed: f64) -> Option<(f64, f64)> {
        if self.source == PointerDataSource::WindowMetadata {
            let samples = self.window_pointer_segment_at(t)?;
            return if smooth <= 0.01 {
                Self::interpolated_window_in(samples, t).map(|(x, y, _, _)| (x, y))
            } else if samples.is_empty() {
                None
            } else {
                Some(self.smoothed_at(t, smooth, speed))
            };
        }
        if smooth <= 0.01 {
            // The planned path filters shake on its own; the raw cursor
            // reads the same filtered samples so both agree.
            if self.shake_threshold > 0.0 {
                let filtered = self.shake_filtered_pointer();
                return Self::interpolated_in(&filtered, t).map(|(x, y, _, _)| (x, y));
            }
            return Self::interpolated_in(&self.pointer, t).map(|(x, y, _, _)| (x, y));
        }
        if self.pointer.is_empty() {
            None
        } else {
            Some(self.smoothed_at(t, smooth, speed))
        }
    }

    /// The smoothed cursor position mapped into the encoded video's pixel
    /// space. Camera following and default focus use this so they evaluate the
    /// same point the cursor overlay draws, instead of the capture-local
    /// coordinates that only line up for unscaled full-screen recordings. See
    /// [`Self::presented_in_video_at`] for why area recordings need this.
    pub fn motion_position_in_video_at(
        &self,
        t: f64,
        smooth: f64,
        speed: f64,
        video_width: f64,
        video_height: f64,
    ) -> Option<(f64, f64)> {
        let (x, y) = self.motion_position_at(t, smooth, speed)?;
        Some(self.map_to_video(x, y, video_width, video_height))
    }

    /// The cursor's travelled position: the studied spring on the studied
    /// planned path, with the smoothing slider selecting its preset and the
    /// speed setting scaling it. See [`super::cursor_planner`].
    fn smoothed_at(&self, t: f64, smooth: f64, speed: f64) -> (f64, f64) {
        let Some(path) = self.planned_path(SpringConfig::for_smooth(smooth), speed) else {
            return (0.0, 0.0);
        };
        path.position_at(t * 1000.0)
    }

    /// Where the edit cuts the recording, in source seconds. The plan leads the
    /// cursor, but never across a cut, so it never travels toward footage the
    /// edit removed.
    pub fn set_cut_times(&mut self, cuts: &[f64]) {
        self.cuts_seconds = cuts.iter().copied().filter(|cut| cut.is_finite()).collect();
        self.cuts_seconds.sort_by(f64::total_cmp);
        self.cuts_seconds.dedup();
    }

    /// The studied shake filter's threshold, in pixels. Zero or a negative
    /// value turns it off. The plan is rebuilt for the new value.
    pub fn set_shake_threshold(&mut self, threshold: f64) {
        self.shake_threshold = if threshold.is_finite() {
            threshold.max(0.0)
        } else {
            0.0
        };
    }

    /// Which pointer samples and press ends survive the studied shake filter.
    /// Press ends are placed at the pointer's interpolated position at their
    /// instant, since the sidecar stores presses as times only.
    fn shake_survivors(&self) -> ShakeSurvivors {
        let mut survivors = ShakeSurvivors {
            pointer: vec![true; self.pointer.len()],
            press_down: vec![true; self.presses.len()],
            press_up: vec![true; self.presses.len()],
        };
        if self.shake_threshold <= 0.0 {
            return survivors;
        }
        enum Origin {
            Pointer(usize),
            Down(usize),
            Up(usize),
        }
        let mut events: Vec<(ShakeEvent, Origin)> = self
            .pointer
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                (
                    ShakeEvent {
                        time_ms: sample.t * 1000.0,
                        x: sample.x,
                        y: sample.y,
                        kind: ShakeEventKind::Move,
                    },
                    Origin::Pointer(index),
                )
            })
            .collect();
        for (index, press) in self.presses.iter().enumerate() {
            for (time, kind) in [
                (press.down, ShakeEventKind::Down),
                (press.up, ShakeEventKind::Up),
            ] {
                let Some((x, y, _, _)) = Self::interpolated_in(&self.pointer, time) else {
                    continue;
                };
                let origin = if kind == ShakeEventKind::Down {
                    Origin::Down(index)
                } else {
                    Origin::Up(index)
                };
                events.push((
                    ShakeEvent {
                        time_ms: time * 1000.0,
                        x,
                        y,
                        kind,
                    },
                    origin,
                ));
            }
        }
        // A stable sort keeps a move ahead of a press end at the same instant.
        events.sort_by(|left, right| left.0.time_ms.total_cmp(&right.0.time_ms));
        let stream: Vec<ShakeEvent> = events.iter().map(|(event, _)| *event).collect();
        let keep = cursor_shake::shake_keep_mask(&stream, self.shake_threshold);
        for ((_, origin), kept) in events.iter().zip(keep) {
            if kept {
                continue;
            }
            match origin {
                Origin::Pointer(index) => survivors.pointer[*index] = false,
                Origin::Down(index) => survivors.press_down[*index] = false,
                Origin::Up(index) => survivors.press_up[*index] = false,
            }
        }
        survivors
    }

    /// The pointer samples the shake filter keeps, built once per threshold
    /// and recording.
    fn shake_filtered_pointer(&self) -> Arc<Vec<PointerSample>> {
        let key = [
            self.shake_threshold.to_bits(),
            self.pointer.len() as u64,
            self.pointer
                .first()
                .map(|sample| sample.t.to_bits())
                .unwrap_or(0),
            self.pointer
                .last()
                .map(|sample| sample.t.to_bits())
                .unwrap_or(0),
        ]
        .iter()
        .fold(1469598103934665603_u64, |hash, value| {
            (hash ^ value).wrapping_mul(1099511628211)
        });
        if let Ok(cache) = self.shake_cache.0.lock() {
            if let Some((cached_key, samples)) = cache.as_ref() {
                if *cached_key == key {
                    return Arc::clone(samples);
                }
            }
        }
        let survivors = self.shake_survivors();
        let samples: Arc<Vec<PointerSample>> = Arc::new(
            self.pointer
                .iter()
                .zip(&survivors.pointer)
                .filter(|(_, kept)| **kept)
                .map(|(sample, _)| sample.clone())
                .collect(),
        );
        if let Ok(mut cache) = self.shake_cache.0.lock() {
            *cache = Some((key, Arc::clone(&samples)));
        }
        samples
    }

    /// Plan the cursor path once per spring and speed, then answer frames from
    /// it. The moments the plan must be exact are the recorded clicks and the
    /// cursor-image changes, matching the reference's sync points.
    fn planned_path(&self, spring: SpringConfig, speed: f64) -> Option<Arc<PlannedPath>> {
        let key = self.plan_key(spring, speed);
        if let Ok(cache) = self.plan_cache.0.lock() {
            if let Some(cached) = cache.as_ref() {
                if let (Some(cached_key), Some(path)) = (cached.key, cached.path.as_ref()) {
                    if cached_key == key {
                        return Some(Arc::clone(path));
                    }
                }
            }
        }
        let survivors = self.shake_survivors();
        let kept_pointer: Vec<&PointerSample> = self
            .pointer
            .iter()
            .zip(&survivors.pointer)
            .filter_map(|(sample, kept)| kept.then_some(sample))
            .collect();
        let points: Vec<PlannerPoint> = kept_pointer
            .iter()
            .map(|sample| PlannerPoint {
                time_ms: sample.t * 1000.0,
                x: sample.x,
                y: sample.y,
            })
            .collect();
        // A click whose press the filter dropped is no longer a moment the
        // plan must meet.
        let dropped_downs: Vec<f64> = self
            .presses
            .iter()
            .zip(&survivors.press_down)
            .filter_map(|(press, kept)| (!kept).then_some(press.down))
            .collect();
        let mut sync: Vec<SyncPoint> = self
            .clicks
            .iter()
            .filter(|click| {
                !dropped_downs
                    .iter()
                    .any(|down| (down - click.t).abs() < 0.0015)
            })
            .map(|click| SyncPoint {
                time_ms: click.t * 1000.0,
                tolerance: 2.0,
            })
            .collect();
        for pair in kept_pointer.windows(2) {
            if pair[0].cursor_id != pair[1].cursor_id {
                sync.push(SyncPoint {
                    time_ms: pair[1].t * 1000.0,
                    tolerance: 6.0,
                });
            }
        }
        // A held button is a sync point twice over: at the press and release
        // the studied planner must meet the pointer exactly, and while the
        // pointer is dragged every recorded move carries the same demand.
        for (index, press) in self.presses.iter().enumerate() {
            let down_kept = survivors.press_down[index];
            if down_kept {
                sync.push(SyncPoint {
                    time_ms: press.down * 1000.0,
                    tolerance: 2.0,
                });
            }
            if survivors.press_up[index] {
                sync.push(SyncPoint {
                    time_ms: press.up * 1000.0,
                    tolerance: 2.0,
                });
            }
            if press.dragged && down_kept {
                for sample in &kept_pointer {
                    if press.contains(sample.t) {
                        sync.push(SyncPoint {
                            time_ms: sample.t * 1000.0,
                            tolerance: 2.0,
                        });
                    }
                }
            }
        }
        let duration_ms = self
            .pointer
            .last()
            .map(|sample| sample.t * 1000.0 + 1000.0)
            .unwrap_or(0.0);
        let cuts_ms: Vec<f64> = self.cuts_seconds.iter().map(|cut| cut * 1000.0).collect();
        let planned = Arc::new(cursor_planner::plan(
            &points,
            &sync,
            spring,
            speed,
            duration_ms,
            &cuts_ms,
        ));
        if let Ok(mut cache) = self.plan_cache.0.lock() {
            *cache = Some(CachedPlan {
                key: Some(key),
                path: Some(Arc::clone(&planned)),
            });
        }
        Some(planned)
    }

    /// A cheap identity for a planned path: everything that changes the plan.
    fn plan_key(&self, spring: SpringConfig, speed: f64) -> u64 {
        let mut hash: u64 = 1469598103934665603;
        let mut values = vec![
            self.pointer.len() as u64,
            self.clicks.len() as u64,
            self.pointer
                .first()
                .map(|sample| sample.t.to_bits())
                .unwrap_or(0),
            self.pointer
                .last()
                .map(|sample| sample.t.to_bits())
                .unwrap_or(0),
            self.pointer
                .iter()
                .filter(|sample| sample.cursor_id.is_some())
                .count() as u64,
            self.presses.len() as u64,
            self.presses
                .last()
                .map(|press| press.up.to_bits())
                .unwrap_or(0),
            spring.stiffness.to_bits(),
            spring.damping.to_bits(),
            spring.mass.to_bits(),
            speed.to_bits(),
            self.shake_threshold.to_bits(),
        ];
        values.push(self.cuts_seconds.len() as u64);
        values.extend(self.cuts_seconds.iter().map(|cut| cut.to_bits()));
        for value in values.drain(..) {
            hash ^= value;
            hash = hash.wrapping_mul(1099511628211);
        }
        hash
    }

    fn idle_alpha(&self, t: f64, idle_ms: f64) -> f64 {
        let window = (idle_ms / 1000.0).clamp(0.12, 4.0);
        let Some((cx, cy, _)) = self.interpolated_at(t) else {
            return 1.0;
        };
        let mut last_move = self.pointer.first().map(|sample| sample.t).unwrap_or(0.0);
        for sample in &self.pointer {
            if sample.t > t {
                break;
            }
            if (sample.x - cx).hypot(sample.y - cy) > 8.0 {
                last_move = sample.t;
            }
        }
        let still = t - last_move;
        if still <= window {
            return 1.0;
        }
        (1.0 - (still - window) / 0.18).clamp(0.0, 1.0)
    }

    pub fn interpolated_at(&self, t: f64) -> Option<(f64, f64, CursorKind)> {
        self.interpolated_at_sample(t)
            .map(|(x, y, kind, _)| (x, y, kind))
    }

    fn interpolated_at_sample(&self, t: f64) -> Option<(f64, f64, CursorKind, Option<String>)> {
        if self.source == PointerDataSource::WindowMetadata {
            Self::interpolated_window_in(self.window_pointer_segment_at(t)?, t)
        } else {
            Self::interpolated_in(&self.pointer, t)
        }
    }

    fn interpolated_window_in(
        pointer: &[PointerSample],
        t: f64,
    ) -> Option<(f64, f64, CursorKind, Option<String>)> {
        let idx = pointer.partition_point(|sample| sample.t <= t);
        if idx > 0 && idx < pointer.len() {
            let prev = &pointer[idx - 1];
            let next = &pointer[idx];
            if next.t - prev.t > WINDOW_CURSOR_STALE_SECONDS {
                return Some((prev.x, prev.y, prev.kind, prev.cursor_id.clone()));
            }
        }
        Self::interpolated_in(pointer, t)
    }

    fn interpolated_in(
        pointer: &[PointerSample],
        t: f64,
    ) -> Option<(f64, f64, CursorKind, Option<String>)> {
        if pointer.is_empty() {
            return None;
        }
        let idx = pointer.partition_point(|sample| sample.t <= t);
        if idx == 0 {
            let first = &pointer[0];
            return Some((first.x, first.y, first.kind, first.cursor_id.clone()));
        }
        if idx >= pointer.len() {
            let last = pointer.last()?;
            return Some((last.x, last.y, last.kind, last.cursor_id.clone()));
        }
        let prev = &pointer[idx - 1];
        let next = &pointer[idx];
        let span = (next.t - prev.t).max(f64::EPSILON);
        let alpha = ((t - prev.t) / span).clamp(0.0, 1.0);
        Some((
            prev.x + (next.x - prev.x) * alpha,
            prev.y + (next.y - prev.y) * alpha,
            prev.kind,
            prev.cursor_id.clone(),
        ))
    }

    fn window_pointer_segment_at(&self, t: f64) -> Option<&[PointerSample]> {
        let visibility_index = self.visibility.partition_point(|sample| sample.t <= t);
        let visible_since = self.visibility.get(visibility_index.checked_sub(1)?)?;
        if !visible_since.visible {
            return None;
        }
        let visibility_after = &self.visibility[visibility_index..];
        let bounded = visibility_after.iter().any(|sample| !sample.visible);
        let pointer_index = self.pointer.partition_point(|sample| sample.t <= t);
        let latest_pointer = self.pointer.get(pointer_index.checked_sub(1)?)?;
        if latest_pointer.t < visible_since.t {
            return None;
        }
        let hold_until = visibility_after
            .iter()
            .find(|sample| sample.hold_until_next || !sample.visible);
        if t - latest_pointer.t > WINDOW_CURSOR_STALE_SECONDS
            && !bounded
            && !hold_until.is_some_and(|sample| sample.visible && sample.hold_until_next)
        {
            return None;
        }
        let start = self
            .pointer
            .partition_point(|sample| sample.t < visible_since.t);
        let hidden_at = visibility_after
            .iter()
            .find(|sample| !sample.visible)
            .map(|sample| sample.t)
            .unwrap_or(f64::INFINITY);
        let end = self.pointer.partition_point(|sample| sample.t < hidden_at);
        self.pointer.get(start..end)
    }

    pub(crate) fn compact_samples(&mut self) {
        compact_to_limit(&mut self.pointer, MAX_POINTER_SAMPLES);
        compact_to_limit(&mut self.clicks, MAX_CLICKS);
        compact_to_limit(&mut self.presses, MAX_PRESSES);
    }

    pub fn click_pulse_at(&self, t: f64) -> f64 {
        let mut peak: f64 = 0.0;
        for click in &self.clicks {
            let distance = (t - click.t).abs();
            if distance <= CLICK_PULSE_WINDOW_SECONDS {
                let amount = 1.0 - (distance / CLICK_PULSE_WINDOW_SECONDS);
                peak = peak.max(amount);
            }
        }
        1.0 + peak * 0.35
    }

    pub fn click_ripples_at(&self, t: f64, window: f64) -> Vec<(f64, f64, f64)> {
        let window = window.max(1e-6);
        let mut ripples = Vec::new();
        for click in &self.clicks {
            let age = t - click.t;
            // The studied effect drops a click once its age reaches the
            // visible duration, so the upper bound is exclusive.
            if (0.0..window).contains(&age) {
                let progress = (age / window).clamp(0.0, 1.0);
                ripples.push((click.x, click.y, progress));
            }
        }
        if ripples.len() > MAX_SIMULTANEOUS_CLICKS {
            let excess = ripples.len() - MAX_SIMULTANEOUS_CLICKS;
            ripples.drain(0..excess);
        }
        ripples
    }

    /// Return click-ripple positions in encoded-video pixels. See
    /// [`Self::presented_in_video_at`] for why area recordings need this.
    pub fn click_ripples_in_video_at(
        &self,
        t: f64,
        window: f64,
        video_width: f64,
        video_height: f64,
    ) -> Vec<(f64, f64, f64)> {
        self.click_ripples_at(t, window)
            .into_iter()
            .map(|(x, y, progress)| {
                let (x, y) = self.map_to_video(x, y, video_width, video_height);
                (x, y, progress)
            })
            .collect()
    }

    pub(crate) fn map_to_video(
        &self,
        x: f64,
        y: f64,
        video_width: f64,
        video_height: f64,
    ) -> (f64, f64) {
        if !self.region.is_area() || video_width <= 0.0 || video_height <= 0.0 {
            return (x, y);
        }
        (
            x * video_width / self.region.w as f64,
            y * video_height / self.region.h as f64,
        )
    }
}

fn compact_to_limit<T: Clone>(samples: &mut Vec<T>, limit: usize) {
    while samples.len() > limit {
        let last = samples.len() - 1;
        let mut compacted = Vec::with_capacity(last / 2 + 1);
        compacted.push(samples[0].clone());
        compacted.extend(
            samples
                .iter()
                .enumerate()
                .skip(2)
                .take(last.saturating_sub(2))
                .filter_map(|(index, sample)| (index % 2 == 0).then_some(sample.clone())),
        );
        compacted.push(samples[last].clone());
        *samples = compacted;
    }
}

fn default_shake_threshold() -> f64 {
    DEFAULT_REMOVE_SHAKE_THRESHOLD
}

fn pointer_sidecars_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("apexshot")
        .join("pointer-sidecars")
}

fn sidecar_file_name(video_path: &Path) -> PathBuf {
    let canonical = video_path
        .canonicalize()
        .unwrap_or_else(|_| video_path.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    PathBuf::from(format!("{:x}", hasher.finalize()))
}

fn remove_sidecar(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        eprintln!(
            "[recording] failed to delete pointer sidecar {}: {err}",
            path.display()
        );
    }
}

pub fn delete_recording_outputs(video_path: &Path) {
    PointerSidecar::delete_next_to_video(video_path);
    if video_path.exists() {
        let _ = std::fs::remove_file(video_path);
    }
}

#[cfg(test)]
mod recording_location_tests {
    use super::super::recording_id::{recording_directory, tagged_mp4_bytes, RECORDING_ID_PREFIX};
    use super::*;

    /// A unique, valid ID per test, so parallel tests never share a folder.
    fn unique_id(offset: u64) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        format!(
            "{:08x}-0000-4000-8000-{:012x}",
            std::process::id(),
            nanos.wrapping_add(offset) & 0xffff_ffff_ffff
        )
    }

    fn video_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-recording-location-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_tagged_video(path: &Path, id: &str) {
        let tagged = tagged_mp4_bytes(&format!("{RECORDING_ID_PREFIX}{id}"));
        std::fs::write(path, tagged).unwrap();
    }

    fn sidecar_with_pointer(x: f64, y: f64) -> PointerSidecar {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x,
            y,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar
    }

    #[test]
    fn a_tagged_video_reads_its_sidecar_from_the_recording_folder() {
        let id = unique_id(0x1000);
        let dir = video_dir("reads-from-folder");
        let video = dir.join("a.mp4");
        write_tagged_video(&video, &id);

        let sidecar = sidecar_with_pointer(1.0, 2.0);
        let written = sidecar.write_next_to_video(&video).unwrap();
        assert_eq!(written, recording_directory(&id).join("pointer.json"));

        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded.pointer, sidecar.pointer);
        assert_eq!(loaded.sidecar_directory, Some(recording_directory(&id)));
        assert!(PointerSidecar::exists_next_to_video(&video));

        let _ = std::fs::remove_dir_all(recording_directory(&id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_the_video_keeps_its_sidecar() {
        let id = unique_id(0x2000);
        let dir = video_dir("rename");
        let a = dir.join("a.mp4");
        let b = dir.join("b.mp4");
        write_tagged_video(&a, &id);
        let sidecar = sidecar_with_pointer(3.0, 4.0);
        sidecar.write_next_to_video(&a).unwrap();

        std::fs::rename(&a, &b).unwrap();

        let loaded = PointerSidecar::load_next_to_video(&b).unwrap();
        assert_eq!(loaded.pointer, sidecar.pointer);

        std::fs::write(
            recording_directory(&id).join("pointer.json.recording.json"),
            r#"{"markers":[1500]}"#,
        )
        .unwrap();
        assert_eq!(RecordingMarks::for_video(&b).markers, vec![1.5]);

        let camera = recording_directory(&id).join("pointer.json.webcam.mp4");
        std::fs::write(&camera, b"camera").unwrap();
        let mut with_webcam = sidecar.clone();
        with_webcam.webcam = Some(WebcamTrack {
            file: "pointer.json.webcam.mp4".to_string(),
            width: 640,
            height: 480,
            duration: 4.0,
            start: 0.0,
            x: 0.1,
            y: 0.1,
            w: 0.2,
            h: 0.2,
            shape: 0,
            flip: false,
        });
        with_webcam.write_next_to_video(&b).unwrap();
        let loaded = PointerSidecar::load_next_to_video(&b).unwrap();
        assert_eq!(loaded.webcam_path(&b), Some(camera));

        let _ = std::fs::remove_dir_all(recording_directory(&id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_recording_folder_wins_over_the_path_hash() {
        let id = unique_id(0x3000);
        let dir = video_dir("folder-wins");
        let video = dir.join("a.mp4");
        write_tagged_video(&video, &id);

        let hashed = sidecar_with_pointer(1.0, 0.0);
        hashed.write(&PointerSidecar::sidecar_path(&video)).unwrap();
        let tagged = sidecar_with_pointer(2.0, 0.0);
        tagged.write_next_to_video(&video).unwrap();

        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded.pointer[0].x, 2.0);

        std::fs::remove_dir_all(recording_directory(&id)).unwrap();
        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded.pointer[0].x, 1.0);

        PointerSidecar::delete_next_to_video(&video);
        assert!(!PointerSidecar::sidecar_path(&video).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_untagged_video_keeps_the_path_hash() {
        let dir = video_dir("untagged");
        let video = dir.join("a.mp4");
        std::fs::write(&video, b"not an mp4").unwrap();

        let sidecar = sidecar_with_pointer(5.0, 6.0);
        let written = sidecar.write_next_to_video(&video).unwrap();
        assert_eq!(written, PointerSidecar::sidecar_path(&video));
        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded.pointer, sidecar.pointer);

        PointerSidecar::delete_next_to_video(&video);
        assert!(!PointerSidecar::sidecar_path(&video).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_a_tagged_recording_removes_its_folder() {
        let id = unique_id(0x5000);
        let dir = video_dir("delete-folder");
        let video = dir.join("a.mp4");
        write_tagged_video(&video, &id);
        sidecar_with_pointer(0.0, 0.0).write_next_to_video(&video).unwrap();
        let cursors = recording_directory(&id).join("pointer.json.cursors");
        std::fs::create_dir_all(&cursors).unwrap();
        std::fs::write(cursors.join("x.png"), b"png").unwrap();

        delete_recording_outputs(&video);

        assert!(!recording_directory(&id).exists());
        assert!(!video.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_kinds_round_trip_through_strings_and_serde() {
        let kinds = [
            CursorKind::Default,
            CursorKind::Hand,
            CursorKind::Text,
            CursorKind::Crosshair,
            CursorKind::Move,
            CursorKind::Wait,
            CursorKind::ResizeH,
            CursorKind::ResizeV,
            CursorKind::ResizeDiag,
        ];

        for kind in kinds {
            let encoded = serde_json::to_string(&kind).unwrap();
            let decoded: CursorKind = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, kind);
            assert_eq!(CursorKind::parse(kind.as_str()), kind);
        }

        let legacy: PointerSample =
            serde_json::from_str(r#"{"t":0.0,"x":0.0,"y":0.0,"kind":"resize"}"#).unwrap();
        assert_eq!(legacy.kind, CursorKind::ResizeH);
    }

    #[test]
    fn cursor_kind_parser_accepts_cursor_aliases_case_insensitively() {
        for (value, expected) in [
            (" FLEUR ", CursorKind::Move),
            ("all-scroll", CursorKind::Move),
            ("WATCH", CursorKind::Wait),
            ("busy", CursorKind::Wait),
            ("ew-resize", CursorKind::ResizeH),
            ("col-resize", CursorKind::ResizeH),
            ("resize_v", CursorKind::ResizeV),
            ("ns-resize", CursorKind::ResizeV),
            ("row-resize", CursorKind::ResizeV),
            ("resize_diag", CursorKind::ResizeDiag),
            ("nwse-resize", CursorKind::ResizeDiag),
            ("nesw-resize", CursorKind::ResizeDiag),
        ] {
            assert_eq!(CursorKind::parse(value), expected, "{value}");
        }
    }

    #[test]
    fn legacy_json_defaults_to_recorded_pointer_data() {
        let json = r#"{
            "version": 1,
            "t0_monotonic_us": 0,
            "region": { "x": 0, "y": 0, "w": 1920, "h": 1080 },
            "pointer": [],
            "clicks": []
        }"#;
        let sidecar: PointerSidecar = serde_json::from_str(json).unwrap();
        assert_eq!(sidecar.source, PointerDataSource::Recorded);
        assert!(sidecar.visibility.is_empty());
        assert!(sidecar.presses.is_empty());
    }

    #[test]
    fn window_cursor_visibility_and_source_round_trip_in_the_sidecar() {
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 100,
                h: 80,
            },
        );
        sidecar.source = PointerDataSource::WindowMetadata;
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 20.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.visibility.push(CursorVisibilitySample {
            t: 0.0,
            visible: true,
            hold_until_next: false,
        });
        sidecar.visibility.push(CursorVisibilitySample {
            t: 1.0,
            visible: false,
            hold_until_next: false,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.5,
            x: 10.0,
            y: 20.0,
            button: 1,
            cursor_id: None,
        });

        let encoded = serde_json::to_vec(&sidecar).unwrap();
        let loaded: PointerSidecar = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(loaded, sidecar);
        assert!(loaded.can_render_cursor_overlay());
    }

    #[test]
    fn new_sidecars_are_version_two() {
        let sidecar = PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        assert_eq!(sidecar.version, 2);
    }

    #[test]
    fn recording_marks_read_markers_and_resumes_from_the_manifest() {
        let marks = RecordingMarks::parse(
            r#"{"sessions":[{"startMs":0,"endMs":800},{"startMs":800,"endMs":1800},
                {"startMs":1800,"endMs":2500}],"markers":[1500,400]}"#,
        );
        assert_eq!(marks.markers, vec![0.4, 1.5]);
        assert_eq!(marks.pauses, vec![0.8, 1.8]);
        assert_eq!(RecordingMarks::parse("not json"), RecordingMarks::default());
        assert_eq!(
            RecordingMarks::parse(r#"{"sessions":[{"startMs":0}]}"#),
            RecordingMarks::default()
        );
    }

    #[test]
    fn press_intervals_round_trip_and_report_press_and_drag() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.presses.push(PressSample {
            down: 1.0,
            up: 1.5,
            button: 1,
            dragged: true,
        });
        sidecar.presses.push(PressSample {
            down: 2.0,
            up: 2.2,
            button: 1,
            dragged: false,
        });

        let loaded: PointerSidecar =
            serde_json::from_str(&serde_json::to_string(&sidecar).unwrap()).unwrap();
        assert_eq!(loaded, sidecar);
        assert!(loaded.is_pressed_at(1.2));
        assert!(loaded.is_dragged_at(1.2));
        assert!(loaded.is_pressed_at(2.1));
        assert!(!loaded.is_dragged_at(2.1));
        // The interval is half-open: past the release it is no longer pressed.
        assert!(!loaded.is_pressed_at(1.5));
        assert!(!loaded.is_pressed_at(1.9));
    }

    #[test]
    fn inferred_pointer_data_cannot_render_a_second_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 20.0,
            y: 30.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        assert!(sidecar.can_render_cursor_overlay());

        sidecar.mark_inferred_from_video();
        assert!(!sidecar.can_render_cursor_overlay());
        let loaded: PointerSidecar =
            serde_json::from_str(&serde_json::to_string(&sidecar).unwrap()).unwrap();
        assert_eq!(loaded.source, PointerDataSource::InferredFromVideo);
        assert!(!loaded.can_render_cursor_overlay());
    }

    #[test]
    fn area_pointer_and_click_coordinates_scale_to_video_pixels() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 960,
                h: 540,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 480.0,
            y: 270.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.0,
            x: 480.0,
            y: 270.0,
            button: 1,
            cursor_id: None,
        });

        let frame = sidecar
            .presented_in_video_at(0.0, CursorMotion::default(), 1920.0, 1080.0)
            .unwrap();
        assert_eq!((frame.x, frame.y), (960.0, 540.0));

        let ripples = sidecar.click_ripples_in_video_at(0.0, 0.32, 1920.0, 1080.0);
        assert_eq!(ripples, vec![(960.0, 540.0, 0.0)]);
    }

    #[test]
    fn motion_position_in_video_scales_area_coordinates() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 960,
                h: 540,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 480.0,
            y: 270.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        assert_eq!(
            sidecar.motion_position_in_video_at(0.0, 0.0, 1.0, 1920.0, 1080.0),
            Some((960.0, 540.0))
        );
        // The raw capture-local accessor is intentionally left unscaled.
        assert_eq!(
            sidecar.motion_position_at(0.0, 0.0, 1.0),
            Some((480.0, 270.0))
        );
    }

    #[test]
    fn motion_position_in_video_is_identity_without_an_area_capture() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 250.0,
            y: 180.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        assert_eq!(
            sidecar.motion_position_in_video_at(0.0, 0.0, 1.0, 1920.0, 1080.0),
            Some((250.0, 180.0))
        );
    }

    #[test]
    fn negative_monitor_origin_translates_before_hidpi_scaling() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: -1280,
                y: -200,
                w: 1280,
                h: 800,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: -640.0,
            y: 200.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.0,
            x: -640.0,
            y: 200.0,
            button: 1,
            cursor_id: None,
        });

        sidecar.subtract_region();
        let frame = sidecar
            .presented_in_video_at(0.0, CursorMotion::default(), 2560.0, 1600.0)
            .unwrap();
        assert_eq!((frame.x, frame.y), (1280.0, 800.0));
        assert_eq!(
            sidecar.click_ripples_in_video_at(0.0, 0.32, 2560.0, 1600.0),
            vec![(1280.0, 800.0, 0.0)]
        );
    }

    #[test]
    fn sidecar_path_is_app_local_and_legacy_path_stays_local() {
        let video = PathBuf::from("/tmp/ApexShot Recording 2026-08-15 at 12-00-00.mp4");
        assert_eq!(
            PointerSidecar::sidecar_path(&video),
            pointer_sidecars_directory().join(sidecar_file_name(&video))
        );
        assert_ne!(
            PointerSidecar::sidecar_path(&video),
            PointerSidecar::legacy_sidecar_path(&video)
        );
    }

    #[test]
    fn roundtrip_preserves_clock_and_samples() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        let mut sidecar = PointerSidecar::new(
            1_700_000,
            CaptureRegion {
                x: 10,
                y: 20,
                w: 800,
                h: 600,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 100.0,
            y: 200.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.2,
            x: 100.0,
            y: 200.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.write_next_to_video(&video).unwrap();

        let mut loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        loaded.sidecar_directory = None;
        assert_eq!(loaded, sidecar);
        PointerSidecar::delete_next_to_video(&video);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_sidecar_file_fails_parse() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.apexshot.json");
        std::fs::write(&path, b"").unwrap();
        assert!(PointerSidecar::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_sidecar_is_migrated_out_of_media_folder() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "apexshot-sidecar-legacy-migration-{}-{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("legacy.mp4");
        let mut sidecar =
            PointerSidecar::new(42, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 5.0,
            y: 6.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar
            .write(&PointerSidecar::legacy_sidecar_path(&video))
            .unwrap();

        let mut loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        loaded.sidecar_directory = None;
        assert_eq!(loaded, sidecar);
        assert!(PointerSidecar::sidecar_path(&video).is_file());
        assert!(!PointerSidecar::legacy_sidecar_path(&video).exists());

        PointerSidecar::delete_next_to_video(&video);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn region_subtract_moves_desktop_coords_into_capture_space() {
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 200,
                y: 100,
                w: 640,
                h: 480,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 250.0,
            y: 180.0,
            kind: CursorKind::Hand,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.4,
            x: 200.0,
            y: 100.0,
            button: 1,
            cursor_id: None,
        });
        sidecar.subtract_region();
        assert_eq!(sidecar.pointer[0].x, 50.0);
        assert_eq!(sidecar.pointer[0].y, 80.0);
        assert_eq!(sidecar.clicks[0].x, 0.0);
        assert_eq!(sidecar.clicks[0].y, 0.0);
    }

    #[test]
    fn window_metadata_visibility_hides_outside_and_stale_cursor_samples() {
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 100,
                h: 80,
            },
        );
        sidecar.source = PointerDataSource::WindowMetadata;
        sidecar.pointer = vec![
            PointerSample {
                t: 0.0,
                x: 10.0,
                y: 15.0,
                kind: CursorKind::Default,
                cursor_id: None,
            },
            PointerSample {
                t: 0.1,
                x: 20.0,
                y: 25.0,
                kind: CursorKind::Default,
                cursor_id: None,
            },
            PointerSample {
                t: 0.3,
                x: 30.0,
                y: 35.0,
                kind: CursorKind::Default,
                cursor_id: None,
            },
        ];
        sidecar.visibility = vec![
            CursorVisibilitySample {
                t: 0.0,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 0.15,
                visible: false,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 0.3,
                visible: true,
                hold_until_next: false,
            },
        ];

        assert!(sidecar.can_render_cursor_overlay());
        assert!(sidecar.presented_at(0.1, CursorMotion::default()).is_some());
        assert!(sidecar.presented_at(0.2, CursorMotion::default()).is_none());
        assert!(sidecar.motion_position_at(0.2, 0.8, 1.0).is_none());
        assert!(sidecar
            .presented_at(0.35, CursorMotion::default())
            .is_some());
        assert!(sidecar
            .presented_at(0.51, CursorMotion::default())
            .is_none());
        assert!(sidecar
            .presented_in_video_at(0.2, CursorMotion::default(), 100.0, 80.0)
            .is_none());
    }

    #[test]
    fn bounded_window_visibility_holds_cursor_until_explicit_exit() {
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 100,
                h: 80,
            },
        );
        sidecar.source = PointerDataSource::WindowMetadata;
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 15.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.visibility = vec![
            CursorVisibilitySample {
                t: 0.0,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 2.0,
                visible: false,
                hold_until_next: false,
            },
        ];

        assert_eq!(
            sidecar.motion_position_at(1.0, 0.0, 1.0),
            Some((10.0, 15.0))
        );
        assert!(sidecar.motion_position_at(2.0, 0.0, 1.0).is_none());
    }

    #[test]
    fn window_cursor_holds_through_long_sample_gaps_without_interpolating() {
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 100,
                h: 80,
            },
        );
        sidecar.source = PointerDataSource::WindowMetadata;
        sidecar.pointer = vec![
            PointerSample {
                t: 0.0,
                x: 10.0,
                y: 15.0,
                kind: CursorKind::Default,
                cursor_id: None,
            },
            PointerSample {
                t: 1.0,
                x: 90.0,
                y: 75.0,
                kind: CursorKind::Default,
                cursor_id: None,
            },
        ];
        sidecar.visibility = vec![
            CursorVisibilitySample {
                t: 0.0,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 2.0,
                visible: false,
                hold_until_next: false,
            },
        ];

        assert_eq!(
            sidecar.motion_position_at(0.5, 0.0, 1.0),
            Some((10.0, 15.0))
        );
        assert_eq!(
            sidecar.motion_position_at(1.0, 0.0, 1.0),
            Some((90.0, 75.0))
        );
    }

    #[test]
    fn window_cursor_reentry_does_not_reuse_the_previous_visible_segment() {
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 100,
                h: 80,
            },
        );
        sidecar.source = PointerDataSource::WindowMetadata;
        sidecar.pointer = vec![PointerSample {
            t: 0.0,
            x: 10.0,
            y: 15.0,
            kind: CursorKind::Default,
            cursor_id: None,
        }];
        sidecar.visibility = vec![
            CursorVisibilitySample {
                t: 0.0,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 1.0,
                visible: false,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 1.05,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 2.0,
                visible: false,
                hold_until_next: false,
            },
        ];

        assert!(sidecar.motion_position_at(1.02, 0.0, 1.0).is_none());
        assert!(sidecar.motion_position_at(1.05, 0.0, 1.0).is_none());
    }

    #[test]
    fn sample_compaction_preserves_recording_end_and_first_position() {
        let mut sidecar =
            PointerSidecar::new(1, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer = (0..(MAX_POINTER_SAMPLES + 1))
            .map(|index| PointerSample {
                t: index as f64,
                x: index as f64,
                y: index as f64,
                kind: CursorKind::Default,
                cursor_id: None,
            })
            .collect();
        sidecar.compact_samples();

        assert!(sidecar.pointer.len() <= MAX_POINTER_SAMPLES);
        assert_eq!(sidecar.pointer.first().unwrap().x, 0.0);
        assert_eq!(
            sidecar.pointer.last().unwrap().x,
            MAX_POINTER_SAMPLES as f64
        );
    }

    #[test]
    fn fullscreen_region_does_not_subtract() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 250.0,
            y: 180.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.subtract_region();
        assert_eq!(sidecar.pointer[0].x, 250.0);
        assert_eq!(sidecar.pointer[0].y, 180.0);
    }

    #[test]
    fn presented_at_springs_behind_the_recorded_point() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.1,
            x: 200.0,
            y: 0.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        let raw = sidecar.interpolated_at(0.15).unwrap();
        let presented = sidecar
            .presented_at(
                0.15,
                CursorMotion {
                    smooth: 0.8,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!(presented.x < raw.0, "the spring should trail the point");
        assert!(presented.x > 0.0, "the spring should be moving");
        let settled = sidecar
            .presented_at(
                4.0,
                CursorMotion {
                    smooth: 0.8,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!((settled.x - 200.0).abs() < 0.01, "the spring should land");
        assert!((presented.alpha - 1.0).abs() < 1e-9);
    }

    #[test]
    fn presented_at_hides_idle_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.05,
            x: 12.0,
            y: 10.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 2.0,
            x: 12.0,
            y: 10.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        let shown = sidecar
            .presented_at(
                0.05,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 400.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let hidden = sidecar
            .presented_at(
                2.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 400.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!((shown.alpha - 1.0).abs() < 1e-9);
        assert!(hidden.alpha < 0.05);
    }

    #[test]
    fn idle_delay_controls_when_cursor_fades() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        let short_delay = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 200.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let long_delay = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    hide_idle: true,
                    idle_ms: 2000.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();

        assert!(short_delay.alpha < 0.05);
        assert!((long_delay.alpha - 1.0).abs() < 1e-9);
    }

    #[test]
    fn presented_at_speed_keeps_cursor_on_the_recorded_timeline() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 1.0,
            x: 100.0,
            y: 10.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 2.0,
            x: 200.0,
            y: 20.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 3.0,
            x: 300.0,
            y: 30.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 100.0,
            y: 10.0,
            button: 1,
            cursor_id: None,
        });
        let expected = sidecar.interpolated_at(1.0).unwrap();
        let presented = sidecar
            .presented_at(
                1.0,
                CursorMotion {
                    speed: 2.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        assert!((presented.x - expected.0).abs() < 1e-9);
        assert!((presented.y - expected.1).abs() < 1e-9);
        assert!((presented.x - 100.0).abs() < 1e-9);
        let ripples = sidecar.click_ripples_at(1.0, CLICK_RIPPLE_WINDOW_SECONDS);
        assert_eq!(ripples.len(), 1);
        assert!((presented.x - ripples[0].0).abs() < 1e-9);
        assert!((presented.y - ripples[0].1).abs() < 1e-9);
        assert!(sidecar.click_pulse_at(1.0) > 1.0);
    }

    #[test]
    fn speed_controls_how_quickly_the_spring_catches_up() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0..=40 {
            let t = index as f64 * 0.008;
            sidecar.pointer.push(PointerSample {
                t,
                x: index as f64 * 5.0,
                y: 0.0,
                kind: CursorKind::Default,
                cursor_id: None,
            });
        }
        let slow = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    smooth: 0.8,
                    speed: 0.5,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let fast = sidecar
            .presented_at(
                0.2,
                CursorMotion {
                    smooth: 0.8,
                    speed: 2.0,
                    ..CursorMotion::default()
                },
            )
            .unwrap();
        let raw = sidecar.interpolated_at(0.2).unwrap();

        assert!(fast.x > slow.x, "a faster spring should trail less");
        assert!(fast.x <= raw.0);
    }

    #[test]
    fn a_drag_keeps_the_cursor_on_the_recorded_path() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0_u32..=100 {
            let moving = index.saturating_sub(40) as f64;
            sidecar.pointer.push(PointerSample {
                t: index as f64 * 0.008,
                x: 100.0 + moving * 8.0,
                y: 200.0,
                kind: CursorKind::Default,
                cursor_id: None,
            });
        }
        sidecar.presses.push(PressSample {
            down: 0.32,
            up: 1.1,
            button: 1,
            dragged: true,
        });
        let motion = CursorMotion {
            smooth: 0.8,
            speed: 1.0,
            ..CursorMotion::default()
        };
        let raw = sidecar.interpolated_at(0.6).unwrap();
        let dragged = sidecar.presented_at(0.6, motion).unwrap();
        assert!(
            (dragged.x - raw.0).abs() <= 2.0 && (dragged.y - raw.1).abs() <= 2.0,
            "the dragged cursor should stay on the path, got {dragged:?} for {raw:?}"
        );
    }

    #[test]
    fn a_cut_holds_the_planned_cursor_back() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0_u32..=100 {
            sidecar.pointer.push(PointerSample {
                t: index as f64 * 0.008,
                x: index as f64 * 8.0,
                y: 0.0,
                kind: CursorKind::Default,
                cursor_id: None,
            });
        }
        sidecar.clicks.push(ClickSample {
            t: 0.4,
            x: 320.0,
            y: 0.0,
            button: 1,
            cursor_id: None,
        });
        let motion = CursorMotion {
            smooth: 0.8,
            speed: 1.0,
            ..CursorMotion::default()
        };
        let free = sidecar.presented_at(0.429, motion).unwrap().x;
        sidecar.set_cut_times(&[0.430]);
        let cut = sidecar.presented_at(0.429, motion).unwrap().x;
        assert!(
            free - cut > 10.0,
            "the cut should hold the cursor back: {cut} vs {free}"
        );
        let again = sidecar.presented_at(0.429, motion).unwrap().x;
        assert!(
            (again - cut).abs() < 1e-9,
            "the cached plan must stay cut-aware: {again} vs {cut}"
        );
    }

    #[test]
    fn the_shake_filter_removes_a_lone_spike_from_the_planned_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0_u32..=20 {
            let t = index as f64 * 0.05;
            let x = if index == 10 { 1500.0 } else { 100.0 };
            sidecar.pointer.push(PointerSample {
                t,
                x,
                y: 100.0,
                kind: CursorKind::Default,
                cursor_id: None,
            });
        }
        let motion = CursorMotion {
            smooth: 0.8,
            speed: 1.0,
            ..CursorMotion::default()
        };
        let peak = |sidecar: &PointerSidecar| {
            (40..=80)
                .map(|step| sidecar.presented_at(step as f64 * 0.01, motion).unwrap().x)
                .fold(f64::MIN, f64::max)
        };
        let filtered = peak(&sidecar);
        sidecar.set_shake_threshold(0.0);
        let unfiltered = peak(&sidecar);
        assert!(
            filtered < 150.0,
            "the spike should be filtered out, peaked at {filtered}"
        );
        assert!(
            unfiltered > filtered + 50.0,
            "without the filter the spike should pull the cursor: {unfiltered} vs {filtered}"
        );
    }

    #[test]
    fn the_shake_filter_also_removes_the_spike_without_smoothing() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0_u32..=20 {
            sidecar.pointer.push(PointerSample {
                t: index as f64 * 0.05,
                x: if index == 10 { 1500.0 } else { 100.0 },
                y: 100.0,
                kind: CursorKind::Default,
                cursor_id: None,
            });
        }
        let filtered = sidecar.motion_position_at(0.5, 0.0, 1.0).unwrap().0;
        sidecar.set_shake_threshold(0.0);
        let raw = sidecar.motion_position_at(0.5, 0.0, 1.0).unwrap().0;
        assert!(
            (filtered - 100.0).abs() < 1e-9,
            "filtered cursor at {filtered}"
        );
        assert!((raw - 1500.0).abs() < 1e-9, "unfiltered cursor at {raw}");
    }

    #[test]
    fn interpolated_sample_lerps_between_neighbors() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.pointer.push(PointerSample {
            t: 1.0,
            x: 100.0,
            y: 50.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        let (x, y, kind) = sidecar.interpolated_at(0.5).unwrap();
        assert!((x - 50.0).abs() < 1e-9);
        assert!((y - 25.0).abs() < 1e-9);
        assert_eq!(kind, CursorKind::Default);
    }

    #[test]
    fn click_pulse_peaks_at_click_time() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 0.0,
            y: 0.0,
            button: 1,
            cursor_id: None,
        });
        assert!((sidecar.click_pulse_at(1.0) - 1.35).abs() < 1e-9);
        assert!((sidecar.click_pulse_at(0.0) - 1.0).abs() < 1e-9);
        assert!(sidecar.click_pulse_at(1.04) > 1.0);
    }

    #[test]
    fn click_ripples_expand_after_click() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 40.0,
            y: 80.0,
            button: 1,
            cursor_id: None,
        });
        assert!(sidecar
            .click_ripples_at(0.5, CLICK_RIPPLE_WINDOW_SECONDS)
            .is_empty());
        let ripples = sidecar.click_ripples_at(1.08, CLICK_RIPPLE_WINDOW_SECONDS);
        assert_eq!(ripples.len(), 1);
        assert!((ripples[0].0 - 40.0).abs() < 1e-9);
        assert!(ripples[0].2 > 0.0 && ripples[0].2 < 1.0);
        assert!(
            sidecar
                .click_ripples_at(1.5, CLICK_RIPPLE_WINDOW_SECONDS)
                .len()
                == 1
        );
        assert!(sidecar
            .click_ripples_at(2.0, CLICK_RIPPLE_WINDOW_SECONDS)
            .is_empty());
    }

    #[test]
    fn click_ripples_honor_effect_window() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 40.0,
            y: 80.0,
            button: 1,
            cursor_id: None,
        });
        assert!(sidecar.click_ripples_at(1.4, 0.32).is_empty());
        let long = sidecar.click_ripples_at(1.4, 0.5);
        assert_eq!(long.len(), 1);
        assert!((long[0].2 - 0.8).abs() < 1e-9);
        let short = sidecar.click_ripples_at(1.16, 0.32);
        assert_eq!(short.len(), 1);
        assert!((short[0].2 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn click_ripples_drop_at_the_visible_duration_boundary() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 40.0,
            y: 80.0,
            button: 1,
            cursor_id: None,
        });
        // Age exactly equal to the visible duration is not drawn.
        assert!(sidecar
            .click_ripples_at(
                1.0 + CLICK_RIPPLE_WINDOW_SECONDS,
                CLICK_RIPPLE_WINDOW_SECONDS
            )
            .is_empty());
    }

    #[test]
    fn click_ripples_keep_only_the_three_most_recent() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        for index in 0..5 {
            sidecar.clicks.push(ClickSample {
                t: 1.0 + index as f64 * 0.01,
                x: index as f64,
                y: 0.0,
                button: 1,
                cursor_id: None,
            });
        }
        let ripples = sidecar.click_ripples_at(1.04, CLICK_RIPPLE_WINDOW_SECONDS);
        assert_eq!(ripples.len(), MAX_SIMULTANEOUS_CLICKS);
        assert_eq!(ripples[0].0, 2.0);
        assert_eq!(ripples[2].0, 4.0);
    }

    #[test]
    fn discard_deletes_video_and_sidecar() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-sidecar-discard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("take.mp4");
        std::fs::write(&video, b"mp4").unwrap();
        let sidecar = PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.write_next_to_video(&video).unwrap();
        assert!(PointerSidecar::sidecar_path(&video).exists());
        delete_recording_outputs(&video);
        assert!(!video.exists());
        assert!(!PointerSidecar::sidecar_path(&video).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod webcam_tests {
    use super::*;

    #[test]
    fn composited_recording_keeps_editable_screen_and_camera() {
        let dir =
            std::env::temp_dir().join(format!("apexshot-editable-window-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("recording.mp4");
        let screen = dir.join(".apexshot-screen-test.mp4");
        std::fs::write(&video, b"composited").unwrap();
        std::fs::write(&screen, b"screen").unwrap();
        let sidecar_path = PointerSidecar::sidecar_path(&video);
        std::fs::create_dir_all(sidecar_path.parent().unwrap()).unwrap();
        let mut track_name = sidecar_path.as_os_str().to_os_string();
        track_name.push(".webcam.mp4");
        let camera = PathBuf::from(track_name);
        std::fs::write(&camera, b"camera").unwrap();
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.webcam = Some(WebcamTrack {
            file: camera.file_name().unwrap().to_string_lossy().into_owned(),
            width: 640,
            height: 480,
            w: 0.2,
            h: 0.2,
            ..WebcamTrack::default()
        });
        sidecar.screen_file = Some(screen.file_name().unwrap().to_string_lossy().into_owned());
        sidecar.write_next_to_video(&video).unwrap();
        let loaded = PointerSidecar::load_next_to_video(&video).unwrap();
        assert_eq!(loaded.editable_video_path(&video), Some(screen.clone()));
        std::fs::remove_file(&camera).unwrap();
        assert!(loaded.editable_video_path(&video).is_none());
        std::fs::write(&camera, b"camera").unwrap();
        let mut invalid = loaded.clone();
        invalid.screen_file = Some("../outside.mp4".to_string());
        assert!(invalid.editable_video_path(&video).is_none());
        delete_recording_outputs(&video);
        assert!(!video.exists());
        assert!(!screen.exists());
        assert!(!camera.exists());
        assert!(!sidecar_path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    const TRACK_JSON: &str = r#"{
        "file": "4f2a.webcam.mp4",
        "width": 1280,
        "height": 720,
        "duration": 9.5,
        "start": 0.4,
        "x": 0.7,
        "y": 0.55,
        "w": 0.2,
        "h": 0.25,
        "shape": 3,
        "flip": true
    }"#;

    #[test]
    fn a_sidecar_without_a_webcam_block_has_no_layer() {
        let sidecar = PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        assert!(sidecar.webcam.is_none());
        let encoded = serde_json::to_vec(&sidecar).unwrap();
        let text = String::from_utf8(encoded).unwrap();
        assert!(!text.contains("webcam"));
    }

    #[test]
    fn a_webcam_block_loads_with_its_geometry() {
        let track: WebcamTrack = serde_json::from_str(TRACK_JSON).unwrap();
        assert_eq!(track.file, "4f2a.webcam.mp4");
        assert_eq!(track.width, 1280);
        assert_eq!(track.height, 720);
        assert_eq!(track.duration, 9.5);
        assert_eq!(track.start, 0.4);
        assert_eq!(track.shape, 3);
        assert!(track.flip);
        assert!(track.is_valid());
        assert_eq!(track.rect(), (0.7, 0.55, 0.2, 0.25));
    }

    #[test]
    fn a_webcam_block_without_pixels_or_fractions_is_not_drawn() {
        let missing = WebcamTrack {
            file: String::new(),
            ..WebcamTrack::default()
        };
        assert!(!missing.is_valid());
        let flat = WebcamTrack {
            file: "track.mp4".to_string(),
            width: 640,
            height: 480,
            w: 0.0,
            h: 0.2,
            ..WebcamTrack::default()
        };
        assert!(!flat.is_valid());
    }

    #[test]
    fn a_hand_edited_rect_stays_inside_the_footage() {
        let track = WebcamTrack {
            x: 0.9,
            y: 1.4,
            w: 0.5,
            h: 0.5,
            ..WebcamTrack::default()
        };
        let (x, y, w, h) = track.rect();
        assert!((x - 0.9).abs() < 1e-9);
        assert!((y - 1.0).abs() < 1e-9);
        assert!((w - 0.1).abs() < 1e-9);
        assert!((h - 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_video_instant_maps_onto_the_tracks_own_clock() {
        let track = WebcamTrack {
            duration: 8.0,
            ..WebcamTrack::default()
        };
        // The camera fell behind the screen by a fifth, so a video second is
        // four fifths of a second into the track.
        assert!((track.track_seconds(1.0, 10.0) - 0.8).abs() < 1e-9);
        assert!((track.track_seconds(12.5, 10.0) - 10.0).abs() < 1e-9);
        // An unknown length plays one to one rather than freezing the layer.
        let unknown = WebcamTrack::default();
        assert!((unknown.track_seconds(3.6, 10.0) - 3.6).abs() < 1e-9);
    }

    #[test]
    fn a_late_camera_waits_for_its_start_offset() {
        let track = WebcamTrack {
            duration: 8.0,
            start: 0.5,
            ..WebcamTrack::default()
        };
        assert_eq!(track.track_seconds(0.0, 10.0), 0.0);
        assert!((track.track_seconds(5.5, 10.0) - (5.0 * 8.0 / 9.5)).abs() < 1e-9);
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    #[test]
    fn a_sidecar_carries_the_cursor_catalog() {
        let json = r#"{
            "version": 2,
            "t0_monotonic_us": 100,
            "region": {"x": 0, "y": 0, "w": 320, "h": 240},
            "pointer": [],
            "clicks": [],
            "cursor_images": "recording.cursors",
            "cursors": [{
                "id": "70cb1326f7ca717b",
                "file": "70cb1326f7ca717b.png",
                "width": 32,
                "height": 32,
                "hotspot_x": 6,
                "hotspot_y": 0,
                "kind": "hand"
            }]
        }"#;
        let sidecar: PointerSidecar = serde_json::from_str(json).unwrap();
        assert_eq!(sidecar.cursor_images.as_deref(), Some("recording.cursors"));
        assert_eq!(sidecar.cursors.len(), 1);
        assert_eq!(sidecar.cursors[0].kind, CursorKind::Hand);
        assert_eq!(sidecar.cursors[0].hotspot_x, 6);
    }

    #[test]
    fn scrolls_and_keys_survive_a_round_trip() {
        let json = r#"{
            "version": 2,
            "t0_monotonic_us": 100,
            "region": {"x": 0, "y": 0, "w": 320, "h": 240},
            "pointer": [],
            "clicks": [],
            "scrolls": [{
                "t": 1.5,
                "x": 12.0,
                "y": 34.0,
                "delta": -120,
                "horizontal": false,
                "cursor_id": "70cb1326f7ca717b"
            }],
            "keys": [{
                "t": 2.0,
                "type": "key_down",
                "key": 75,
                "scan_code": 37,
                "modifiers": ["shift", "control"],
                "cursor_id": "70cb1326f7ca717b"
            }]
        }"#;
        let sidecar: PointerSidecar = serde_json::from_str(json).unwrap();
        assert_eq!(sidecar.scrolls.len(), 1);
        assert_eq!(sidecar.scrolls[0].delta, -120);
        assert_eq!(
            sidecar.scrolls[0].cursor_id.as_deref(),
            Some("70cb1326f7ca717b")
        );
        assert_eq!(sidecar.keys.len(), 1);
        assert_eq!(sidecar.keys[0].kind, "key_down");
        assert_eq!(sidecar.keys[0].key, 75);
        assert_eq!(sidecar.keys[0].modifiers, ["shift", "control"]);

        let encoded = serde_json::to_string(&sidecar).unwrap();
        let restored: PointerSidecar = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.scrolls, sidecar.scrolls);
        assert_eq!(restored.keys, sidecar.keys);
    }

    #[test]
    fn pointer_and_click_cursor_ids_round_trip() {
        let json = r#"{
            "version": 2,
            "t0_monotonic_us": 100,
            "region": {"x": 0, "y": 0, "w": 320, "h": 240},
            "pointer": [{
                "t": 1.0,
                "x": 12.0,
                "y": 34.0,
                "kind": "hand",
                "cursor_id": "85b8e6f7cad0464a"
            }],
            "clicks": [{
                "t": 1.1,
                "x": 12.0,
                "y": 34.0,
                "button": 1,
                "cursor_id": "85b8e6f7cad0464a"
            }]
        }"#;
        let sidecar: PointerSidecar = serde_json::from_str(json).unwrap();
        assert_eq!(
            sidecar.pointer[0].cursor_id.as_deref(),
            Some("85b8e6f7cad0464a")
        );
        assert_eq!(
            sidecar.clicks[0].cursor_id.as_deref(),
            Some("85b8e6f7cad0464a")
        );

        let encoded = serde_json::to_string(&sidecar).unwrap();
        let restored: PointerSidecar = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.pointer, sidecar.pointer);
        assert_eq!(restored.clicks, sidecar.clicks);
    }

    #[test]
    fn samples_without_a_cursor_id_omit_the_field() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 1.0,
            y: 2.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.0,
            x: 1.0,
            y: 2.0,
            button: 1,
            cursor_id: None,
        });
        let encoded = serde_json::to_string(&sidecar).unwrap();
        assert!(!encoded.contains("cursor_id"), "{encoded}");
    }

    #[test]
    fn a_presented_frame_carries_the_sample_cursor_id() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 20.0,
            kind: CursorKind::Hand,
            cursor_id: Some("hand-id".to_string()),
        });
        let frame = sidecar
            .presented_at(0.0, CursorMotion::default())
            .expect("the frame should render");
        assert_eq!(frame.kind, CursorKind::Hand);
        assert_eq!(frame.cursor_id.as_deref(), Some("hand-id"));
    }

    #[test]
    fn the_cursor_tilts_with_horizontal_velocity() {
        let rotation = |step: f64| {
            let mut sidecar =
                PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
            for index in 0..25 {
                let t = index as f64 * 0.016;
                sidecar.pointer.push(PointerSample {
                    t,
                    x: step * index as f64,
                    y: 100.0,
                    kind: CursorKind::Default,
                    cursor_id: None,
                });
            }
            sidecar.cursor_rotation_at(
                0.2,
                CursorRotationMode::Velocity,
                ROTATE_ON_X_MOVEMENT_RATIO,
                0.0,
            )
        };
        assert!(rotation(40.0) > 0.05, "rightward motion should lean right");
        assert!(rotation(-40.0) < -0.05, "leftward motion should lean left");
        assert!(rotation(0.0).abs() < 1e-9, "no motion should not tilt");
    }

    #[test]
    fn the_directional_mode_turns_toward_the_travel_heading() {
        let rotation = |dx: f64, dy: f64| {
            let mut sidecar =
                PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
            for index in 0..60 {
                let t = index as f64 * 0.016;
                sidecar.pointer.push(PointerSample {
                    t,
                    x: 100.0 + dx * index as f64,
                    y: 100.0 + dy * index as f64,
                    kind: CursorKind::Default,
                    cursor_id: None,
                });
            }
            sidecar.cursor_rotation_at(0.7, CursorRotationMode::Directional, 0.5, 0.0)
        };
        // Moving right is +112.5 degrees from the arrow's neutral heading.
        assert!(
            rotation(6.0, 0.0) > 1.5,
            "rightward travel should point right"
        );
        assert!(
            (-1.3..=-1.0).contains(&rotation(-6.0, 0.0)),
            "leftward travel should point left"
        );
        // Upward travel is close to the art's neutral heading.
        assert!(
            rotation(0.0, -6.0).abs() < 0.6,
            "upward travel should stay near neutral"
        );
    }

    #[test]
    fn recorded_visibility_hides_and_restores_the_cursor() {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 20.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.visibility = vec![
            CursorVisibilitySample {
                t: 0.0,
                visible: true,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 1.0,
                visible: false,
                hold_until_next: false,
            },
            CursorVisibilitySample {
                t: 2.0,
                visible: true,
                hold_until_next: false,
            },
        ];

        assert!(sidecar.presented_at(0.5, CursorMotion::default()).is_some());
        assert!(sidecar.presented_at(1.5, CursorMotion::default()).is_none());
        assert!(sidecar.presented_at(2.5, CursorMotion::default()).is_some());
    }

    #[test]
    fn a_loaded_sidecar_resolves_cursors_from_its_own_directory() {
        let root = std::env::temp_dir().join(format!(
            "apexshot-cursor-source-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let sidecar_path = root.join("sidecar.json");
        let cursor_folder = root.join("clip.cursors");
        std::fs::create_dir_all(&cursor_folder).unwrap();
        std::fs::write(cursor_folder.join("hand.png"), b"not a png yet").unwrap();

        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.cursor_images = Some("clip.cursors".to_string());
        sidecar.cursors.push(CursorAsset {
            id: "hand-id".to_string(),
            file: "hand.png".to_string(),
            width: 32,
            height: 32,
            hotspot_x: 6,
            hotspot_y: 0,
            kind: CursorKind::Hand,
        });
        sidecar.write(&sidecar_path).unwrap();

        let loaded = PointerSidecar::load(&sidecar_path).unwrap();
        let clean_screen = root.join("clean-screen.mp4");
        let frame = CursorFrame {
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Hand,
            alpha: 1.0,
            rotation: 0.0,
            cursor_id: Some("hand-id".to_string()),
        };
        assert_eq!(
            loaded.cursor_capture(&clean_screen, &frame).unwrap().path,
            cursor_folder.join("hand.png")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn catalog_sidecar(folder: &str, file: &str) -> PointerSidecar {
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.cursor_images = Some(folder.to_string());
        sidecar.cursors.push(CursorAsset {
            id: "hand-id".to_string(),
            file: file.to_string(),
            width: 32,
            height: 32,
            hotspot_x: 6,
            hotspot_y: 0,
            kind: CursorKind::Hand,
        });
        sidecar
    }

    #[test]
    fn cursor_catalog_names_must_be_bare_and_inside_the_sidecar_folder() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-cursor-catalog-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.join("clip.cursors");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("hand.png"), b"not a png yet").unwrap();

        let sidecar = catalog_sidecar("clip.cursors", "hand.png");
        assert!(sidecar.cursor_asset("hand-id").is_some());
        assert!(sidecar.cursor_asset("missing").is_none());
        assert_eq!(
            sidecar.cursor_asset_path_in(&dir, "hand-id"),
            Some(folder.join("hand.png"))
        );

        for bad in [
            "../outside",
            "nested/folder",
            "nested\\folder",
            "C:\\cursors",
            ".",
            "..",
            "",
        ] {
            let bad_sidecar = catalog_sidecar(bad, "hand.png");
            assert!(
                bad_sidecar.cursor_asset_path_in(&dir, "hand-id").is_none(),
                "folder {bad:?} must not resolve"
            );
        }

        let mut bad_file = catalog_sidecar("clip.cursors", "../hand.png");
        assert!(bad_file.cursor_asset_path_in(&dir, "hand-id").is_none());
        bad_file.cursors[0].file = "nested\\hand.png".to_string();
        assert!(bad_file.cursor_asset_path_in(&dir, "hand-id").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_catalog_entry_has_no_captured_cursor() {
        let sidecar = catalog_sidecar("clip.cursors", "hand.png");
        let frame = CursorFrame {
            x: 10.0,
            y: 20.0,
            kind: CursorKind::Hand,
            alpha: 1.0,
            rotation: 0.0,
            cursor_id: Some("missing".to_string()),
        };
        let video = std::env::temp_dir().join("apexshot-cursor-missing.mp4");
        assert!(sidecar.cursor_capture(&video, &frame).is_none());

        let no_id = CursorFrame {
            cursor_id: None,
            ..frame
        };
        assert!(sidecar.cursor_capture(&video, &no_id).is_none());
    }
}
