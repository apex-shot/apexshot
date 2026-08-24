fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
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

pub fn eval_zoom(
    clips: &[ZoomClip],
    t: f64,
    frame_width: f64,
    frame_height: f64,
) -> (f64, (f64, f64)) {
    let frame_center = (frame_width / 2.0, frame_height / 2.0);
    let Some(clip) = clips.iter().find(|clip| t >= clip.start && t <= clip.end) else {
        return (1.0, frame_center);
    };
    let ease = (clip.ease_ms as f64 / 1000.0).clamp(0.0, clip.duration() / 2.0);
    let scale = eased_value(t, clip.start, clip.end, ease, 1.0, clip.scale.max(1.0));
    (scale, clip.center)
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
    let left = view_center.0 - half_w;
    let right = view_center.0 + half_w;
    let top = view_center.1 - half_h;
    let bottom = view_center.1 + half_h;

    let mut cx = view_center.0;
    let mut cy = view_center.1;
    if cursor.0 < left + margin_x {
        cx = cursor.0 - margin_x + half_w;
    } else if cursor.0 > right - margin_x {
        cx = cursor.0 + margin_x - half_w;
    }
    if cursor.1 < top + margin_y {
        cy = cursor.1 - margin_y + half_h;
    } else if cursor.1 > bottom - margin_y {
        cy = cursor.1 + margin_y - half_h;
    }
    (
        cx.clamp(half_w, (frame_w - half_w).max(half_w)),
        cy.clamp(half_h, (frame_h - half_h).max(half_h)),
    )
}

fn eased_value(t: f64, start: f64, end: f64, ease: f64, from: f64, to: f64) -> f64 {
    if ease <= f64::EPSILON {
        return to;
    }
    if t < start + ease {
        let alpha = ((t - start) / ease).clamp(0.0, 1.0);
        return lerp(from, to, smoothstep(alpha));
    }
    if t > end - ease {
        let alpha = ((end - t) / ease).clamp(0.0, 1.0);
        return lerp(from, to, smoothstep(alpha));
    }
    to
}

fn lerp(from: f64, to: f64, alpha: f64) -> f64 {
    from + (to - from) * alpha
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
        center.0.clamp(crop_x + half_w, (crop_x + crop_w - half_w).max(crop_x + half_w)),
        center.1.clamp(crop_y + half_h, (crop_y + crop_h - half_h).max(crop_y + half_h)),
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

/// Fit `src` inside `box` without upscaling or stretching (aspect preserved).
pub fn fit_dimensions(src_w: u32, src_h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    let src_w = src_w.max(1);
    let src_h = src_h.max(1);
    let box_w = box_w.max(MIN_DIMENSION);
    let box_h = box_h.max(MIN_DIMENSION);
    // Cap the box at the source so we never upscale past the original.
    let max_w = box_w.min(src_w);
    let max_h = box_h.min(src_h);
    let scale = (max_w as f64 / src_w as f64).min(max_h as f64 / src_h as f64);
    let width = even_dimension(((src_w as f64 * scale).round() as u32).max(2));
    let height = even_dimension(((src_h as f64 * scale).round() as u32).max(2));
    (width.max(2), height.max(2))
}

pub fn even_dimension(value: u32) -> u32 {
    let clamped = value.max(2);
    if clamped.is_multiple_of(2) {
        clamped
    } else {
        clamped - 1
    }
}

pub fn quality_to_crf(quality: u8) -> u8 {
    let quality = quality.min(100) as f64;
    (32.0 - ((quality / 100.0) * 14.0).round()).clamp(18.0, 32.0) as u8
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

    let quality_factor = 0.55 + (state.quality.min(100) as f64 / 100.0) * 0.9;
    let (target_width, target_height) = state.padded_output_dimensions();
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

pub const WEBCUT_ASPECT_RATIOS: [(&str, u32, u32); 6] = [
    ("21:9", 1792, 768),
    ("16:9", 1920, 1080),
    ("4:3", 1440, 1080),
    ("9:16", 608, 1080),
    ("3:4", 810, 1080),
    ("1:1", 1080, 1080),
];

pub fn format_webcut_time(seconds: f64) -> String {
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
    WEBCUT_ASPECT_RATIOS
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

