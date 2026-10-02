fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

/// Shortest span an effect clip (zoom or cursor-hide) may occupy. Matches the
/// minimum the timeline drags already enforce.
const MIN_EFFECT_CLIP_SECONDS: f64 = 0.2;

/// The longest an effect clip may run: the edited video's own length. The
/// program never plays past its last frame, so a block sitting in the empty
/// canvas beyond it could never do anything.
pub(crate) fn effect_clip_limit(state: &VideoEditState) -> f64 {
    state.composition_duration()
}

/// Clamp a requested clip span to the video's length, growing it to the
/// minimum where there is still room. `None` when even the minimum would not
/// fit — the caller must leave the clip unchanged rather than write a
/// degenerate one.
pub(crate) fn fit_effect_span(state: &VideoEditState, start: f64, end: f64) -> Option<(f64, f64)> {
    let limit = effect_clip_limit(state);
    let mut start = start.clamp(0.0, limit);
    let mut end = end.clamp(0.0, limit);
    if end < start {
        std::mem::swap(&mut start, &mut end);
    }
    if end - start < MIN_EFFECT_CLIP_SECONDS {
        end = (start + MIN_EFFECT_CLIP_SECONDS).min(limit);
        if end - start < MIN_EFFECT_CLIP_SECONDS {
            return None;
        }
    }
    Some((start, end))
}

/// Move an effect clip without changing its length: pull it back so its end
/// never passes the last frame.
pub(crate) fn fit_effect_move(state: &VideoEditState, start: f64, duration: f64) -> (f64, f64) {
    let limit = effect_clip_limit(state);
    let duration = duration
        .max(MIN_EFFECT_CLIP_SECONDS)
        .min(limit.max(MIN_EFFECT_CLIP_SECONDS));
    let start = start.max(0.0).min((limit - duration).max(0.0));
    (start, (start + duration).min(limit))
}

pub fn playhead_for_replay(playhead: f64, content_end: f64) -> f64 {
    if playhead >= content_end - 0.05 {
        0.0
    } else {
        playhead
    }
}

/// How early a freeze hold may start before the playhead reaches `footage_end`.
///
/// The final decodable frame's timestamp is one frame short of the container
/// duration, and the media can take a beat to report end, so without a lead the
/// playhead parks on that last frame until EOS lands — a visible hitch at the
/// handoff. One frame closes the gap without eating real footage. Falls back to
/// the transport's tick when the frame rate is unknown.
pub fn freeze_hold_lead(frame_rate: f64) -> f64 {
    if frame_rate.is_finite() && frame_rate > 0.0 {
        1.0 / frame_rate + 1e-3
    } else {
        0.05
    }
}

/// Whether playback should enter a freeze hold: a tail exists and the source is
/// exhausted. The source is exhausted when the media reports end, or the
/// playhead has come within `lead` of `footage_end` — the last decodable
/// frame's timestamp is usually one frame short of the container duration, so
/// the playhead alone need not ever reach the exact end before the source runs
/// out.
pub fn freeze_hold_active(
    freeze_tail: f64,
    media_done: bool,
    playhead: f64,
    footage_end: f64,
    lead: f64,
) -> bool {
    freeze_tail > 1e-9 && (media_done || playhead >= footage_end - lead.max(0.0) - 1e-9)
}

pub fn usable_media_timestamp_seconds(timestamp_us: i64, seeking: bool) -> Option<f64> {
    if seeking || timestamp_us < 0 {
        None
    } else {
        Some(timestamp_us as f64 / 1_000_000.0)
    }
}

pub fn snap_to_target(value: f64, target: f64, threshold: f64) -> f64 {
    if threshold >= 0.0 && (value - target).abs() <= threshold {
        target
    } else {
        value
    }
}

pub fn snap_range_to_target(start: f64, duration: f64, target: f64, threshold: f64) -> f64 {
    let duration = duration.max(0.0);
    let end = start + duration;
    let to_start = (target - start).abs();
    let to_end = (target - end).abs();
    let can_snap_start = to_start <= threshold;
    let aligned_start = target - duration;
    let can_snap_end = to_end <= threshold && aligned_start >= -1e-9;
    if can_snap_start && (!can_snap_end || to_start <= to_end) {
        target.max(0.0)
    } else if can_snap_end {
        aligned_start.max(0.0)
    } else {
        start
    }
}

/// Auto zooms at most this far apart morph into one another instead of
/// returning to the full frame between their clips.
const ZOOM_MORPH_GAP_SECONDS: f64 = 0.5;

pub fn eval_zoom(
    clips: &[ZoomClip],
    t: f64,
    frame_width: f64,
    frame_height: f64,
) -> (f64, (f64, f64)) {
    let frame_center = (frame_width / 2.0, frame_height / 2.0);
    let Some(index) = clips
        .iter()
        .position(|clip| !clip.hidden && t >= clip.start && t <= clip.end)
    else {
        // Hold the earlier framing across a morph gap so the next auto zoom
        // continues from it instead of flashing the full frame between them.
        return match zoom_gap_hold_predecessor(clips, t) {
            Some(previous) => (clips[previous].scale.max(1.0), clips[previous].center),
            None => (1.0, frame_center),
        };
    };
    let clip = &clips[index];
    let to_scale = clip.scale.max(1.0);
    // An instant zoom snaps: no eased scale ramp and no morph from a
    // neighbour. The studied lead-in returns no animation window for such
    // zooms, and the follow camera (see `eval_zoom_at`) snaps the same way,
    // so scale and position stay in step instead of one gliding while the
    // other jumps.
    if clip.instant {
        return (to_scale, clip.center);
    }
    let ease = (clip.ease_ms as f64 / 1000.0).clamp(0.0, clip.duration() / 2.0);
    if ease <= f64::EPSILON {
        return (to_scale, clip.center);
    }
    if t < clip.start + ease {
        let progress = clip.easing.apply(((t - clip.start) / ease).clamp(0.0, 1.0));
        // A zoom that morphs from a neighbour starts at the neighbour's
        // framing; a standalone zoom opens around its own focus point.
        let (from_scale, from_center) = match morph_predecessor(clips, index) {
            Some(previous) => (clips[previous].scale.max(1.0), clips[previous].center),
            None => (1.0, clip.center),
        };
        return (
            lerp(from_scale, to_scale, progress),
            (
                lerp(from_center.0, clip.center.0, progress),
                lerp(from_center.1, clip.center.1, progress),
            ),
        );
    }
    if t > clip.end - ease {
        // Hold the framing for a neighbour that morphs from this zoom;
        // alone, settle back to the full frame as before.
        if morphs_into_neighbour(clips, index) {
            return (to_scale, clip.center);
        }
        let progress = clip.easing.apply(((clip.end - t) / ease).clamp(0.0, 1.0));
        return (lerp(1.0, to_scale, progress), clip.center);
    }
    (to_scale, clip.center)
}

/// The closest earlier auto zoom that clip `index` can morph from.
///
/// A hidden clip never contributes, here or anywhere else: it is deliberately
/// out of the output, so it must not shape a visible transition. An instant
/// clip never morphs either: it snaps, so there is no lead-in to blend from.
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
                && !previous.hidden
                && !previous.instant
                && previous.end <= clip.start
                && clip.start - previous.end <= ZOOM_MORPH_GAP_SECONDS
        })
        .max_by(|(_, a), (_, b)| a.end.total_cmp(&b.end))
        .map(|(previous, _)| previous)
}

/// True when a later auto zoom morphs from clip `index`, so it must hold
/// its framing through its own ease-out instead of returning to full frame.
fn morphs_into_neighbour(clips: &[ZoomClip], index: usize) -> bool {
    let clip = &clips[index];
    clip.mode == ZoomMode::Auto
        && !clip.hidden
        && !clip.instant
        && clips.iter().enumerate().any(|(other, next)| {
            other != index
                && next.mode == ZoomMode::Auto
                && !next.hidden
                && !next.instant
                && next.start >= clip.end
                && next.start - clip.end <= ZOOM_MORPH_GAP_SECONDS
        })
}

/// The closest auto zoom whose framing is held while `t` sits in a gap.
///
/// This is the clip the camera has just left, so a caller evaluating the full
/// camera can keep following the pointer from that clip's evaluated endpoint
/// instead of snapping back to its stored center.
pub(crate) fn zoom_gap_hold_predecessor(clips: &[ZoomClip], t: f64) -> Option<usize> {
    morph_gap_predecessor(clips, t).filter(|&previous| morphs_into_neighbour(clips, previous))
}

/// The nearest earlier auto zoom to `t`, regardless of whether a later clip
/// actually morphs from it.
fn morph_gap_predecessor(clips: &[ZoomClip], t: f64) -> Option<usize> {
    clips
        .iter()
        .enumerate()
        .filter(|(_, clip)| {
            clip.mode == ZoomMode::Auto
                && !clip.hidden
                && clip.end <= t
                && t - clip.end <= ZOOM_MORPH_GAP_SECONDS
        })
        .max_by(|(_, a), (_, b)| a.end.total_cmp(&b.end))
        .map(|(previous, _)| previous)
}

/// Follow camera: a spring chase toward the movement-group centre.
///
/// The camera does not track the raw cursor and has no dead zone. Its target
/// is the centre of the pointer-movement group active at the current source
/// time, and a damped spring carries the framing toward that target. This is
/// the studied automatic-zoom camera: the movement-group centre chased on
/// the project's screen spring, with a per-zoom instant snap.
///
/// The studied screen is driven by one project-level spring
/// (`screenMovementSpring`, our [`CAMERA_FOLLOW_SPRING`]). An earlier port
/// also stiffened the camera near a click and while a mouse drag was
/// recorded, but in the studied app those `mouseMovementSpring` changes
/// smooth the *cursor sprite*, not the camera — the camera keeps its single
/// screen spring. That stiffness is deliberately not applied here.
///
/// The evaluator is pure: every call simulates from the clip's source start
/// to the evaluation time at the studied adaptive step, starting at the
/// clip's stored centre with zero velocity. There is no carried state, so a
/// random seek lands exactly where sequential playback would be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSpring {
    pub stiffness: f64,
    pub damping: f64,
    pub mass: f64,
}

/// Default follow spring, shared by the pointer and the screen in the
/// studied defaults.
pub const CAMERA_FOLLOW_SPRING: CameraSpring = CameraSpring {
    stiffness: 125.0,
    damping: 12.0,
    mass: 1.5,
};

/// Adaptive integration step in milliseconds, ported from the studied
/// integrator. It returns the 1000/60 ms frame unless the frame time times
/// the faster of the natural frequency and the damping rate exceeds half a
/// step, in which case it returns 1 ms. The soft follow spring takes the
/// frame branch; a stiffer spring (a future cursor or transition spring)
/// takes the 1 ms branch.
pub fn spring_adaptive_step_ms(spring: CameraSpring) -> f64 {
    let frame_ms = 1000.0 / 60.0;
    let mass = spring.mass.max(f64::EPSILON);
    let natural = (spring.stiffness / mass).sqrt();
    let damping_rate = spring.damping / mass;
    if frame_ms / 1000.0 * natural.max(damping_rate) <= 0.5 {
        frame_ms
    } else {
        1.0
    }
}

/// One semi-implicit Euler step of a mass-spring-damper toward `target`.
///
/// Ports the studied integrator, including its clamp: when the mass is moving
/// toward the target too fast to stop without overshooting, damping rises to
/// critical (`2 * sqrt(stiffness * mass)`) for the step. The clamp keeps the
/// camera from swinging past the pointer the way an unclamped spring would.
pub fn spring_step(
    value: f64,
    velocity: f64,
    target: f64,
    spring: CameraSpring,
    dt: f64,
) -> (f64, f64) {
    let dt = dt.max(0.0);
    if dt <= 0.0 || !value.is_finite() || !velocity.is_finite() || !target.is_finite() {
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
    let velocity = velocity + acceleration * dt;
    let value = value + velocity * dt;
    (value, velocity)
}

/// Dwell-weighted centre of `points`: each sample holds until the next one
/// (or `end_time`, or a 100 ms grace for a trailing sample), so a pause
/// outweighs a fly-by. Ports the studied movement-group centre.
///
/// `points` holds `(time_seconds, x, y)` in video pixels. Returns `None`
/// for no finite samples.
pub fn time_weighted_center(
    points: &[(f64, f64, f64)],
    end_time: Option<f64>,
) -> Option<(f64, f64)> {
    let mut weight_sum = 0.0;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for (index, (t, x, y)) in points.iter().enumerate() {
        if !t.is_finite() || !x.is_finite() || !y.is_finite() {
            continue;
        }
        let next_t = points
            .get(index + 1)
            .map(|next| next.0)
            .filter(|next| next.is_finite())
            .or(end_time.filter(|end| end.is_finite()))
            .unwrap_or(*t + 0.1);
        let weight = (next_t - *t).max(0.0);
        if weight <= 0.0 {
            continue;
        }
        weight_sum += weight;
        x_sum += *x * weight;
        y_sum += *y * weight;
    }
    if weight_sum <= 0.0 {
        None
    } else {
        Some((x_sum / weight_sum, y_sum / weight_sum))
    }
}

/// Half the zoomed view: the movement-group budget at this scale.
///
/// The studied grouping breaks a run when its bounding box exceeds half the
/// visible source; at our fixed per-clip scale the visible source is
/// `crop / scale`, so the budget is half of that. A tighter zoom groups more
/// finely, a wider one lets the pointer roam further before splitting.
pub fn movement_group_budget(crop: (f64, f64, f64, f64), scale: f64) -> (f64, f64) {
    let (_, _, crop_w, crop_h) = crop;
    let scale = scale.max(1.0);
    (
        (crop_w.max(1.0) / scale * 0.5).max(1.0),
        (crop_h.max(1.0) / scale * 0.5).max(1.0),
    )
}

/// Split time-ordered pointer samples into movement groups.
///
/// A group keeps accepting samples while its bounding box (including the
/// candidate) fits inside `max_distance`; the first sample that would burst
/// the box opens a new group. Ports the studied grouping, which keys the
/// active group by its first sample's time. `points` must be ordered by time
/// (the sidecar appends in order); a single out-of-order sample only splits
/// an extra group, it never panics.
pub fn movement_groups(
    points: &[(f64, f64, f64)],
    max_distance: (f64, f64),
) -> Vec<Vec<(f64, f64, f64)>> {
    let mut groups: Vec<Vec<(f64, f64, f64)>> = Vec::new();
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(t, x, y) in points {
        if !t.is_finite() || !x.is_finite() || !y.is_finite() {
            continue;
        }
        let fits = match groups.last() {
            None => false,
            Some(_) => {
                let next_min = (min.0.min(x), min.1.min(y));
                let next_max = (max.0.max(x), max.1.max(y));
                next_max.0 - next_min.0 <= max_distance.0.max(0.0)
                    && next_max.1 - next_min.1 <= max_distance.1.max(0.0)
            }
        };
        if !fits {
            groups.push(Vec::new());
            min = (x, y);
            max = (x, y);
        } else {
            min = (min.0.min(x), min.1.min(y));
            max = (max.0.max(x), max.1.max(y));
        }
        if let Some(group) = groups.last_mut() {
            group.push((t, x, y));
        }
    }
    groups
}

/// Precompute each movement group's first sample time and dwell-weighted
/// centre. The centre does not depend on the query time — only which group is
/// active does — so the follow camera can index this once per evaluation
/// instead of rebuilding every group on every integration step.
fn movement_group_centers(
    points: &[(f64, f64, f64)],
    max_distance: (f64, f64),
) -> Vec<(f64, (f64, f64))> {
    let groups = movement_groups(points, max_distance);
    groups
        .iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let first_t = group.first()?.0;
            let end_time = groups
                .get(index + 1)
                .and_then(|next| next.first().map(|first| first.0));
            // The studied centre weights by dwell, with the next group's start
            // as the trailing sample's hold end so a pause at a control
            // outweighs the travel that reached it.
            let center = time_weighted_center(group, end_time).or_else(|| {
                let (mut sx, mut sy, mut count) = (0.0, 0.0, 0);
                for &(_, x, y) in group {
                    sx += x;
                    sy += y;
                    count += 1;
                }
                (count > 0).then_some((sx / count as f64, sy / count as f64))
            })?;
            Some((first_t, center))
        })
        .collect()
}

/// Centre of the movement group active at `t` from precomputed group centres:
/// the latest group whose first sample starts at or before `t`. Falls back to
/// the first group when `t` predates every sample, and to `None` without
/// groups.
fn movement_group_center_in(
    centers: &[(f64, (f64, f64))],
    t: f64,
) -> Option<(f64, f64)> {
    if centers.is_empty() || !t.is_finite() {
        return None;
    }
    let mut active = 0;
    for (index, (first_t, _)) in centers.iter().enumerate() {
        if *first_t <= t {
            active = index;
        } else {
            break;
        }
    }
    Some(centers[active].1)
}

/// Centre of the movement group active at source time `t`: the latest group
/// whose first sample starts at or before `t`. Falls back to the first
/// group when `t` predates every sample, and to `None` without samples.
/// This is the follow target — the studied camera chases this centre, not
/// the raw cursor.
pub fn movement_group_center_at(
    points: &[(f64, f64, f64)],
    t: f64,
    max_distance: (f64, f64),
) -> Option<(f64, f64)> {
    if points.is_empty() || !t.is_finite() {
        return None;
    }
    let centers = movement_group_centers(points, max_distance);
    movement_group_center_in(&centers, t)
}

/// Chase the movement-group centre from `start` (at `from_source`) to
/// `to_source` on `spring`, sampling the group target at each step. Returns
/// the camera centre at `to_source`, unclamped: the caller clamps it into the
/// crop with [`clamp_zoom_center`]. Pure — same inputs, same output — so
/// random seeks match sequential playback.
///
/// The step is the studied adaptive step: 1000/60 ms for the soft follow
/// spring, 1 ms for a spring stiff enough that the frame step would be
/// unstable (omega * dt above the semi-implicit Euler limit).
pub fn evaluate_spring_camera(
    start: (f64, f64),
    points: &[(f64, f64, f64)],
    from_source: f64,
    to_source: f64,
    max_distance: (f64, f64),
    spring: CameraSpring,
) -> (f64, f64) {
    if !to_source.is_finite() || !from_source.is_finite() || to_source <= from_source {
        return start;
    }
    // No movement to chase: hold the stored framing.
    if points.is_empty() {
        return start;
    }
    let step_seconds = spring_adaptive_step_ms(spring) / 1000.0;
    if !step_seconds.is_finite() || step_seconds <= 0.0 {
        return start;
    }
    let centers = movement_group_centers(points, max_distance);
    if centers.is_empty() {
        return start;
    }
    let mut x = start.0;
    let mut y = start.1;
    let mut vx = 0.0;
    let mut vy = 0.0;
    let mut s = from_source;
    let mut guard = 0;
    let max_steps = ((to_source - from_source) / step_seconds).ceil() as usize + 2;
    while s < to_source && guard < max_steps {
        let step = step_seconds.min(to_source - s);
        let sample_t = (s + step).min(to_source);
        let target = movement_group_center_in(&centers, sample_t).unwrap_or(start);
        let (nx, nvx) = spring_step(x, vx, target.0, spring, step);
        let (ny, nvy) = spring_step(y, vy, target.1, spring, step);
        x = nx;
        y = ny;
        vx = nvx;
        vy = nvy;
        s += step;
        guard += 1;
    }
    (x, y)
}

fn lerp(from: f64, to: f64, alpha: f64) -> f64 {
    from + (to - from) * alpha
}

pub fn zoom_fill_transform(scale: f64, target: f64, ox: f64, oy: f64) -> (f64, f64, f64) {
    let target = target.max(1.0);
    let scale = scale.max(1.0);
    let progress = if target <= 1.01 {
        0.0
    } else {
        ((scale - 1.0) / (target - 1.0)).clamp(0.0, 1.0)
    };
    (
        (0.5 - ox * target) * progress,
        (0.5 - oy * target) * progress,
        scale,
    )
}

pub fn view_to_source(
    view: (f64, f64, f64, f64),
    px: f64,
    py: f64,
    widget_w: f64,
    widget_h: f64,
) -> (f64, f64) {
    let (vx, vy, vw, vh) = view;
    (
        vx + (px / widget_w.max(1.0)) * vw,
        vy + (py / widget_h.max(1.0)) * vh,
    )
}

pub fn clamp_zoom_center(crop: (f64, f64, f64, f64), scale: f64, center: (f64, f64)) -> (f64, f64) {
    let (crop_x, crop_y, crop_w, crop_h) = crop;
    let (_, _, zw, zh) = even_crop_rect(
        scale.max(1.0),
        (center.0 - crop_x, center.1 - crop_y),
        crop_w.max(2.0) as u32,
        crop_h.max(2.0) as u32,
    );
    let half_w = zw as f64 / 2.0;
    let half_h = zh as f64 / 2.0;
    (
        center.0.clamp(
            crop_x + half_w,
            (crop_x + crop_w - half_w).max(crop_x + half_w),
        ),
        center.1.clamp(
            crop_y + half_h,
            (crop_y + crop_h - half_h).max(crop_y + half_h),
        ),
    )
}

pub fn even_crop_rect(
    scale: f64,
    center: (f64, f64),
    src_w: u32,
    src_h: u32,
) -> (u32, u32, u32, u32) {
    let src_w = src_w.max(2);
    let src_h = src_h.max(2);
    let scale = scale.max(1.0);
    let crop_w = even_dimension(((src_w as f64 / scale).round() as u32).max(2).min(src_w));
    let crop_h = even_dimension(((src_h as f64 / scale).round() as u32).max(2).min(src_h));
    let max_x = src_w.saturating_sub(crop_w);
    let max_y = src_h.saturating_sub(crop_h);
    let x = ((center.0 - crop_w as f64 / 2.0).round() as i32).clamp(0, max_x as i32) as u32;
    let y = ((center.1 - crop_h as f64 / 2.0).round() as i32).clamp(0, max_y as i32) as u32;
    (
        even_dimension(x.min(max_x)),
        even_dimension(y.min(max_y)),
        crop_w,
        crop_h,
    )
}

/// Size and offset a full-source picture so `view` fills `clip`.
/// Returns (picture_w, picture_h, margin_x, margin_y).
pub fn picture_layout(
    view: (f64, f64, f64, f64),
    src_w: f64,
    src_h: f64,
    clip_w: f64,
    clip_h: f64,
) -> (i32, i32, i32, i32) {
    let (vx, vy, vw, vh) = view;
    let sx = clip_w / vw.max(1.0);
    let sy = clip_h / vh.max(1.0);
    (
        (src_w * sx).round() as i32,
        (src_h * sy).round() as i32,
        (-vx * sx).round() as i32,
        (-vy * sy).round() as i32,
    )
}

/// Translate+scale that maps source `view` onto a clip, origin top-left.
/// Picture is the full source fitted to the clip: `x' = x * sx + tx`.
pub fn zoom_camera_transform(
    view: (f64, f64, f64, f64),
    src_w: f64,
    src_h: f64,
    clip_w: f64,
    clip_h: f64,
) -> (f64, f64, f64, f64) {
    let (vx, vy, vw, vh) = view;
    (
        -vx / vw.max(1.0) * clip_w,
        -vy / vh.max(1.0) * clip_h,
        src_w / vw.max(1.0),
        src_h / vh.max(1.0),
    )
}

/// Map a source-pixel point onto widget/output space through the visible zoom view.
/// Sprite size is not part of this mapping — draw at `cursor.size` only.
pub fn source_to_zoomed_point(
    x: f64,
    y: f64,
    view: (f64, f64, f64, f64),
    widget_w: f64,
    widget_h: f64,
) -> (f64, f64) {
    let (vx, vy, vw, vh) = view;
    (
        ((x - vx) / vw.max(1.0)) * widget_w,
        ((y - vy) / vh.max(1.0)) * widget_h,
    )
}

/// Fit `src` inside `box` (aspect preserved, centered) for the Frame canvas.
/// Unlike a thumbnail fit this scales up: an explicit Frame is a chosen
/// output size, so a 720p recording in a 1080p frame still fills the canvas.
/// Even dimensions keep the MP4 encoder's yuv420p happy.
pub fn contain_fit(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let src_w = src_w.max(1) as f64;
    let src_h = src_h.max(1) as f64;
    let box_w = box_w.max(MIN_DIMENSION) as f64;
    let box_h = box_h.max(MIN_DIMENSION) as f64;
    let scale = (box_w / src_w).min(box_h / src_h);
    let width = even_dimension(((src_w * scale).round() as u32).max(2));
    let height = even_dimension(((src_h * scale).round() as u32).max(2));
    // Matching aspects only miss the box by the even-dimension rounding; snap
    // so a recording that fills its Frame leaves no hairline letterbox.
    if box_w - f64::from(width) <= 2.0 && box_h - f64::from(height) <= 2.0 {
        return (box_w as u32, box_h as u32);
    }
    (width.max(2), height.max(2))
}

pub fn card_depth(hw: f64, hh: f64, perspective: f64) -> f64 {
    // `perspectiveIntensity` is a camera setting, not a per-axis skew. Use
    // the card's half diagonal as the film-size reference so the same value
    // has comparable yaw and pitch on wide, square, and portrait captures.
    // This is ApexShot's aspect-invariant focal heuristic. The corresponding
    // CIPerspectiveTransform parameter construction is not inferred
    // from this value.
    let half_diagonal = hw.hypot(hh).max(1.0);
    half_diagonal * (2.44 - perspective.clamp(0.0, 1.0) * 1.30).max(1.15)
}

pub fn project_point(mut x: f64, mut y: f64, transform: MotionTransform, depth: f64) -> (f64, f64) {
    let mut z = 0.0;
    let ry = transform.rotation_y.to_radians();
    let rx = transform.rotation_x.to_radians();
    let rz = transform.rotation_z.to_radians();
    let (cos_y, sin_y) = (ry.cos(), ry.sin());
    let (x2, z2) = (x * cos_y + z * sin_y, -x * sin_y + z * cos_y);
    x = x2;
    z = z2;
    let (cos_x, sin_x) = (rx.cos(), rx.sin());
    let (y2, z3) = (y * cos_x - z * sin_x, y * sin_x + z * cos_x);
    y = y2;
    z = z3;
    let (cos_z, sin_z) = (rz.cos(), rz.sin());
    let (x3, y3) = (x * cos_z - y * sin_z, x * sin_z + y * cos_z);
    x = x3;
    y = y3;
    // Keep the card in front of the virtual camera. The clamp is only a
    // near-plane guard for deliberately extreme authored rotations; normal
    // Motion limits remain fully projective rather than flattening early.
    let w = 1.0 / (1.0 + z / depth.max(1.0)).clamp(0.35, 2.5);
    (x * w, y * w)
}

pub fn project_card_corners(
    img_w: f64,
    img_h: f64,
    fit: f64,
    transform: MotionTransform,
    cx: f64,
    cy: f64,
) -> [(f64, f64); 4] {
    let hw = img_w * fit * transform.scale / 2.0;
    let hh = img_h * fit * transform.scale / 2.0;
    let depth = card_depth(hw, hh, transform.perspective);
    let locals = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
    locals.map(|(x, y)| {
        let (x, y) = project_point(x, y, transform, depth);
        (cx + x, cy + y)
    })
}

pub fn affine_from_three_points(
    src: [(f64, f64); 3],
    dest: [(f64, f64); 3],
) -> Option<(f64, f64, f64, f64, f64, f64)> {
    let (x1, y1) = src[0];
    let (x2, y2) = src[1];
    let (x3, y3) = src[2];
    let det = x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2);
    if det.abs() < 1e-8 {
        return None;
    }
    let (u1, v1) = dest[0];
    let (u2, v2) = dest[1];
    let (u3, v3) = dest[2];
    let xx = (u1 * (y2 - y3) + u2 * (y3 - y1) + u3 * (y1 - y2)) / det;
    let xy = (u1 * (x3 - x2) + u2 * (x1 - x3) + u3 * (x2 - x1)) / det;
    let x0 = (u1 * (x2 * y3 - x3 * y2) + u2 * (x3 * y1 - x1 * y3) + u3 * (x1 * y2 - x2 * y1)) / det;
    let yx = (v1 * (y2 - y3) + v2 * (y3 - y1) + v3 * (y1 - y2)) / det;
    let yy = (v1 * (x3 - x2) + v2 * (x1 - x3) + v3 * (x2 - x1)) / det;
    let y0 = (v1 * (x2 * y3 - x3 * y2) + v2 * (x3 * y1 - x1 * y3) + v3 * (x1 * y2 - x2 * y1)) / det;
    Some((xx, yx, xy, yy, x0, y0))
}

pub fn motion_clip_matrix(
    transform: MotionTransform,
    width: f64,
    height: f64,
) -> Option<(f64, f64, f64, f64, f64, f64)> {
    let width = width.max(2.0);
    let height = height.max(2.0);
    let corners = project_card_corners(width, height, 1.0, transform, width / 2.0, height / 2.0);
    affine_from_three_points(
        [(0.0, 0.0), (width, 0.0), (0.0, height)],
        [corners[0], corners[1], corners[3]],
    )
}

pub fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(2);
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        clamped - 1
    }
}

pub fn estimate_size_bytes(state: &VideoEditState, trim_only: bool) -> u64 {
    let duration = state.metadata.duration_seconds.max(0.0);
    if duration <= f64::EPSILON {
        return 0;
    }

    let selected_duration_ratio =
        ((state.kept_duration() + state.timeline_offset_seconds) / duration).max(0.0);
    let base_size = state.metadata.file_size_bytes as f64 * selected_duration_ratio;

    if trim_only {
        return base_size.round().max(0.0) as u64;
    }

    // Export tiers bracket the source: Balanced shrinks the file, Ultra grows
    // it. High keeps the previous default factor, so the default estimate is
    // unchanged by the tier switch.
    let quality_factor = match state.quality {
        ExportQuality::Balanced => 1.0,
        ExportQuality::High => 1.18,
        ExportQuality::Ultra => 1.5,
    };
    let (target_width, target_height) = state.output_dimensions();
    let original_pixels = (state.metadata.width as f64 * state.metadata.height as f64).max(1.0);
    let target_pixels = target_width as f64 * target_height as f64;
    let dimension_factor = (target_pixels / original_pixels).max(0.0);
    let audio_factor = match state.audio_mode {
        AudioMode::Unchanged => 1.0,
        AudioMode::Mono => 0.95,
        AudioMode::Muted => 0.88,
    };

    (base_size * quality_factor * dimension_factor * audio_factor)
        .round()
        .max(0.0) as u64
}

pub fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1024.0 / 1024.0;
    if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{mb:.0} MB")
    }
}

/// Frame picker sizes. Every ratio fits a 1920x1080 envelope with the short
/// edge at 1080 where the ratio allows, so portrait picks export 1080x1920
/// (the vertical standard Tella and Screen Studio use) instead of a 608px
/// wide letterbox, and 21:9 caps its long edge at 1920 like scope ratios do.
/// Even values keep the MP4 encoder's yuv420p happy.
pub const FRAME_ASPECT_RATIOS: [(&str, u32, u32); 6] = [
    ("21:9", 1920, 822),
    ("16:9", 1920, 1080),
    ("4:3", 1440, 1080),
    ("9:16", 1080, 1920),
    ("3:4", 1080, 1440),
    ("1:1", 1080, 1080),
];

/// Playhead and duration labels, formatted as HH:MM:SS.mmm.
pub fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_sec = total_ms / 1000;
    let sec = total_sec % 60;
    let min = (total_sec / 60) % 60;
    let hour = total_sec / 3600;
    format!("{hour:02}:{min:02}:{sec:02}.{ms:03}")
}

pub fn closest_aspect_ratio(width: u32, height: u32) -> &'static str {
    let aspect = width as f64 / height.max(1) as f64;
    FRAME_ASPECT_RATIOS
        .iter()
        .min_by(|(_, aw, ah), (_, bw, bh)| {
            let da = ((*aw as f64 / *ah as f64) - aspect).abs();
            let db = ((*bw as f64 / *bh as f64) - aspect).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(label, _, _)| *label)
        .unwrap_or("16:9")
}

pub fn title_from_path(path: &Path) -> String {
    sanitize_title(
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled"),
    )
}

pub fn sanitize_title(raw: &str) -> String {
    let mut title = String::with_capacity(raw.len());
    let mut last_was_space = false;
    for ch in raw.chars() {
        let invalid = matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
        if invalid || ch.is_control() {
            continue;
        }
        if ch.is_whitespace() {
            if !title.is_empty() && !last_was_space {
                title.push(' ');
                last_was_space = true;
            }
            continue;
        }
        last_was_space = false;
        title.push(ch);
    }
    let title = title.trim().to_string();
    if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    }
}

pub fn edited_output_path(input: &Path) -> PathBuf {
    unique_edited_path(
        input.parent().unwrap_or_else(|| Path::new("")),
        &title_from_path(input),
    )
}

fn unique_edited_path(parent: &Path, stem: &str) -> PathBuf {
    let stem = sanitize_title(stem);
    let mut candidate = parent.join(format!("{stem}-edited.mp4"));
    if !candidate.exists() {
        return candidate;
    }

    for index in 2.. {
        candidate = parent.join(format!("{stem}-edited-{index}.mp4"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded edited output path search should always return")
}
