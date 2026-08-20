#[derive(Clone, Copy)]
pub enum TrimDragKind {
    Start,
    End,
    Playhead,
    Cut(usize),
    Segment(usize),
    Range { origin_offset: f64 },
}

const SEGMENT_GAP: f64 = 8.0;

pub fn visual_capsules(layout: &[(usize, f64, f64)]) -> Vec<(usize, usize, f64, f64)> {
    let last = layout.len().saturating_sub(1);
    layout
        .iter()
        .enumerate()
        .map(|(opos, &(seg_idx, vx_start, vx_end))| {
            let start = vx_start + if opos == 0 { 0.0 } else { SEGMENT_GAP / 2.0 };
            let end = vx_end - if opos == last { 0.0 } else { SEGMENT_GAP / 2.0 };
            (opos, seg_idx, start, end)
        })
        .collect()
}

pub fn sync_timeline_scroll_adj(adj: &Adjustment, state: &VideoEditState, syncing: &Cell<bool>) {
    let visible = state.visible_span_seconds();
    let upper = state.timeline_canvas_seconds().max(visible);
    let value = state.timeline_scroll_seconds;
    if (adj.page_size() - visible).abs() < 1e-6
        && (adj.upper() - upper).abs() < 1e-6
        && (adj.value() - value).abs() < 1e-4
    {
        return;
    }
    // GTK emits value-changed from set_upper/set_value. The caller often
    // already holds `state`, so the callback must not lock again.
    syncing.set(true);
    adj.set_lower(0.0);
    adj.set_page_size(visible);
    adj.set_upper(upper);
    adj.set_step_increment((visible * 0.05).max(0.05));
    adj.set_page_increment((visible * 0.8).max(0.1));
    if (adj.value() - value).abs() > 1e-4 {
        adj.set_value(value);
    }
    syncing.set(false);
}

pub fn composition_x_to_source(state: &VideoEditState, width: f64, x: f64) -> f64 {
    let timeline_t = state.x_to_time(x.clamp(0.0, width), width);
    state
        .timeline_to_source(timeline_t)
        .clamp(0.0, state.metadata.duration_seconds.max(0.0))
}

pub fn visual_x_to_source_seconds(state: &VideoEditState, width: f64, x: f64) -> f64 {
    if state.cuts.is_empty() {
        return composition_x_to_source(state, width, x);
    }
    let layout = compute_visual_layout(state, width);
    let capsules = visual_capsules(&layout);
    let boundaries = state.segment_boundaries();
    for &(_, seg_idx, start, end) in &capsules {
        if x >= start && x <= end {
            let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) else {
                continue;
            };
            let frac = ((x - start) / (end - start).max(1.0)).clamp(0.0, 1.0);
            return seg_start + frac * (seg_end - seg_start);
        }
    }
    if let Some(&(_, _, first, _)) = capsules.first() {
        if x < first {
            return state.trim_start_seconds;
        }
    }
    state.trim_end_seconds
}

pub fn hit_segment_drag(
    state: &VideoEditState,
    layout: &[(usize, f64, f64)],
    x: f64,
    handle_threshold: f64,
    allow_reorder: bool,
) -> Option<TrimDragKind> {
    let boundaries = state.segment_boundaries();
    let capsules = visual_capsules(layout);
    for &(_, seg_idx, start, end) in &capsules {
        let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) else {
            continue;
        };
        if (x - start).abs() <= handle_threshold {
            return Some(bound_drag_kind(state, seg_start, true));
        }
        if (x - end).abs() <= handle_threshold {
            return Some(bound_drag_kind(state, seg_end, false));
        }
    }
    if !allow_reorder {
        return None;
    }
    for &(opos, _, start, end) in &capsules {
        if x > start + handle_threshold && x < end - handle_threshold {
            return Some(TrimDragKind::Segment(opos));
        }
    }
    None
}

pub fn bound_drag_kind(state: &VideoEditState, bound: f64, is_start: bool) -> TrimDragKind {
    if is_start && (bound - state.trim_start_seconds).abs() < 1e-4 {
        return TrimDragKind::Start;
    }
    if !is_start && (bound - state.trim_end_seconds).abs() < 1e-4 {
        return TrimDragKind::End;
    }
    if let Some(index) = state
        .cuts
        .iter()
        .position(|cut| (*cut - bound).abs() < 1e-4)
    {
        TrimDragKind::Cut(index)
    } else if is_start {
        TrimDragKind::Start
    } else {
        TrimDragKind::End
    }
}

/// Visual layout entry: (chronological_seg_index, visual_x_start, visual_x_end)
pub fn compute_visual_layout(state: &VideoEditState, total_width: f64) -> Vec<(usize, f64, f64)> {
    let boundaries = state.segment_boundaries();
    let total_dur: f64 = state
        .segment_order
        .iter()
        .filter(|&&i| state.segments_kept.get(i).copied().unwrap_or(true))
        .filter_map(|&i| boundaries.get(i))
        .map(|(s, e)| (e - s).max(0.0))
        .sum();
    if total_dur <= 0.0 {
        return vec![];
    }
    let mut layout = Vec::new();
    let mut x = 0.0;
    for &seg_idx in &state.segment_order {
        if !state.segments_kept.get(seg_idx).copied().unwrap_or(true) {
            continue;
        }
        if let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) {
            let seg_dur = (seg_end - seg_start).max(0.0);
            let seg_w = (seg_dur / total_dur) * total_width;
            layout.push((
                seg_idx,
                state.frac_to_x(x / total_width, total_width),
                state.frac_to_x((x + seg_w) / total_width, total_width),
            ));
            x += seg_w;
        }
    }
    layout
}

pub fn draw_trim_overlay(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    dragging_seg_idx: Option<usize>,
    show_handles: bool,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let has_cuts = !state.cuts.is_empty();

    if has_cuts {
        let layout = compute_visual_layout(&state, w);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.48);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        for &(_, seg_idx, start, end) in &visual_capsules(&layout) {
            let emphasize = dragging_seg_idx == Some(seg_idx);
            draw_capsule_frame(cr, start, end, h, emphasize);
        }
        let removed_count = state.segments_kept.iter().filter(|&&k| !k).count();
        if removed_count > 0 {
            cr.select_font_face(
                "sans-serif",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            cr.set_font_size(9.0);
            cr.set_source_rgba(1.0, 0.4, 0.4, 0.7);
            let text = format!("{removed_count} removed");
            if let Ok(ext) = cr.text_extents(&text) {
                cr.move_to(w - ext.width() - 4.0, h - 3.0);
                let _ = cr.show_text(&text);
            }
        }
    } else {
        let (start_x, end_x) = trim_span_x(&state, w);
        draw_trim_capsule(cr, start_x, end_x, w, h, show_handles);
    }
}

pub fn draw_clip_frame(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let stroke = 2.5;
    let inset = stroke / 2.0;
    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.set_line_width(stroke);
    rounded_rect(
        cr,
        x + inset,
        y + inset,
        (w - stroke).max(1.0),
        (h - stroke).max(1.0),
        10.0,
    );
    let _ = cr.stroke();
}

pub fn trim_span_x(state: &VideoEditState, width: f64) -> (f64, f64) {
    let start = state.source_to_x(state.trim_start_seconds, width);
    let end = state.source_to_x(state.trim_end_seconds, width);
    (start, start + (end - start).max(36.0))
}

pub fn draw_trim_capsule(
    cr: &gtk4::cairo::Context,
    start_x: f64,
    end_x: f64,
    w: f64,
    h: f64,
    show_handles: bool,
) {
    let range_width = (end_x - start_x).max(1.0);
    let is_full_clip = start_x <= 1.0 && end_x >= w - 1.0;

    if show_handles || !is_full_clip {
        draw_capsule_frame(cr, start_x, end_x, h, false);
    } else {
        draw_clip_frame(cr, start_x, 0.0, range_width, h);
    }
}

pub fn draw_capsule_frame(
    cr: &gtk4::cairo::Context,
    start_x: f64,
    end_x: f64,
    h: f64,
    emphasize: bool,
) {
    let range_width = (end_x - start_x).max(36.0);
    let end_x = start_x + range_width;
    let r = 10.0;
    let handle_w = 8.0;
    let pad = 1.5;
    let stroke = if emphasize { 3.0 } else { 2.5 };

    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.set_line_width(stroke);
    rounded_rect(
        cr,
        start_x + pad,
        pad,
        range_width - pad * 2.0,
        h - pad * 2.0,
        r,
    );
    let _ = cr.stroke();

    draw_trim_handle(cr, start_x + pad, pad, handle_w, h - pad * 2.0, r, true);
    draw_trim_handle(
        cr,
        end_x - pad - handle_w,
        pad,
        handle_w,
        h - pad * 2.0,
        r,
        false,
    );
}

pub fn draw_trim_handle(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64, left: bool) {
    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.new_sub_path();
    if left {
        cr.arc(
            x + r,
            y + r,
            r,
            std::f64::consts::PI,
            1.5 * std::f64::consts::PI,
        );
        cr.line_to(x + w, y);
        cr.line_to(x + w, y + h);
        cr.arc(
            x + r,
            y + h - r,
            r,
            0.5 * std::f64::consts::PI,
            std::f64::consts::PI,
        );
    } else {
        cr.move_to(x, y);
        cr.arc(x + w - r, y + r, r, -0.5 * std::f64::consts::PI, 0.0);
        cr.arc(x + w - r, y + h - r, r, 0.0, 0.5 * std::f64::consts::PI);
        cr.line_to(x, y + h);
    }
    cr.close_path();
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    cr.set_line_width(2.0);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    let cx = x + w / 2.0;
    let gy0 = y + h * 0.32;
    let gy1 = y + h * 0.68;
    cr.move_to(cx - 2.5, gy0);
    cr.line_to(cx - 2.5, gy1);
    cr.move_to(cx + 2.5, gy0);
    cr.line_to(cx + 2.5, gy1);
    let _ = cr.stroke();
}

pub fn update_time_labels(start_label: &Label, end_label: &Label, state: &Arc<Mutex<VideoEditState>>) {
    let state = state.lock().unwrap();
    start_label.set_text(&format!(
        "Start {}",
        format_duration(state.trim_start_seconds)
    ));
    end_label.set_text(&format!("End {}", format_duration(state.trim_end_seconds)));
}

pub fn nearest_cut_index(
    state: &VideoEditState,
    seconds: f64,
    threshold_seconds: f64,
) -> Option<usize> {
    state
        .cuts
        .iter()
        .enumerate()
        .filter_map(|(index, cut)| {
            let distance = (cut - seconds).abs();
            (distance <= threshold_seconds).then_some((index, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
}

pub fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(
        x + r,
        y + h - r,
        r,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}

pub fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}:{seconds:04.1}")
}

pub fn format_timecode(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let secs = (seconds - minutes as f64 * 60.0).floor() as u64;
    format!("{minutes:02}:{secs:02}")
}
