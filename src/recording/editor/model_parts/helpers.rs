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
        return match morph_gap_predecessor(clips, t)
            .filter(|&previous| morphs_into_neighbour(clips, previous))
        {
            Some(previous) => (clips[previous].scale.max(1.0), clips[previous].center),
            None => (1.0, frame_center),
        };
    };
    let clip = &clips[index];
    let to_scale = clip.scale.max(1.0);
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
/// out of the output, so it must not shape a visible transition.
fn morph_predecessor(clips: &[ZoomClip], index: usize) -> Option<usize> {
    let clip = &clips[index];
    if clip.mode != ZoomMode::Auto {
        return None;
    }
    clips
        .iter()
        .enumerate()
        .filter(|(other, previous)| {
            *other != index
                && previous.mode == ZoomMode::Auto
                && !previous.hidden
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
        && clips.iter().enumerate().any(|(other, next)| {
            other != index
                && next.mode == ZoomMode::Auto
                && !next.hidden
                && next.start >= clip.end
                && next.start - clip.end <= ZOOM_MORPH_GAP_SECONDS
        })
}

/// The closest auto zoom whose framing is held while `t` sits in a gap.
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

fn recenter_if_near_edge(
    view_center: (f64, f64),
    cursor: (f64, f64),
    scale: f64,
    frame_w: f64,
    frame_h: f64,
) -> (f64, f64) {
    let crop_w = (frame_w / scale.max(1.0)).min(frame_w);
    let crop_h = (frame_h / scale.max(1.0)).min(frame_h);
    let half_w = crop_w / 2.0;
    let half_h = crop_h / 2.0;
    let margin_x = crop_w * 0.22;
    let margin_y = crop_h * 0.22;
    let feather_x = crop_w * 0.12;
    let feather_y = crop_h * 0.12;
    let left = view_center.0 - half_w;
    let right = view_center.0 + half_w;
    let top = view_center.1 - half_h;
    let bottom = view_center.1 + half_h;

    let mut cx = view_center.0;
    let mut cy = view_center.1;
    if cursor.0 < left + margin_x {
        let offset = cursor.0 - (left + margin_x);
        cx += feathered_camera_offset(offset, feather_x);
    } else if cursor.0 > right - margin_x {
        let offset = cursor.0 - (right - margin_x);
        cx += feathered_camera_offset(offset, feather_x);
    }
    if cursor.1 < top + margin_y {
        let offset = cursor.1 - (top + margin_y);
        cy += feathered_camera_offset(offset, feather_y);
    } else if cursor.1 > bottom - margin_y {
        let offset = cursor.1 - (bottom - margin_y);
        cy += feathered_camera_offset(offset, feather_y);
    }
    (
        cx.clamp(half_w, (frame_w - half_w).max(half_w)),
        cy.clamp(half_h, (frame_h - half_h).max(half_h)),
    )
}

fn feathered_camera_offset(offset: f64, feather: f64) -> f64 {
    let amount = (offset.abs() / feather.max(1.0)).clamp(0.0, 1.0);
    let smoothstep = amount * amount * (3.0 - 2.0 * amount);
    offset * smoothstep
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
