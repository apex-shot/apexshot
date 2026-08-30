pub fn widget_is_light(widget: &impl gtk4::prelude::IsA<Widget>) -> bool {
    let mut current = Some(widget.clone().upcast::<Widget>());
    while let Some(node) = current {
        if node.has_css_class("editor-theme-light") {
            return true;
        }
        current = node.parent();
    }
    false
}

pub fn draw_ruler(
    state: &Arc<Mutex<VideoEditState>>,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let visible = state.visible_span_seconds().max(0.001);
    let major = ruler_major_step(visible);
    let minor = (major / 5.0).max(0.05);
    let view_start = state.x_to_time(0.0, w).max(0.0);
    let view_end = state.x_to_time(w, w).max(view_start);
    let playhead = state.playhead_seconds;
    let (tick_r, tick_g, tick_b) = if light {
        (0.11, 0.13, 0.16)
    } else {
        (1.0, 1.0, 1.0)
    };

    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.set_line_width(1.0);

    let mut t = (view_start / minor).floor() * minor;
    if t < 0.0 {
        t = 0.0;
    }
    while t <= view_end + 0.0001 {
        let x = state.time_to_x(t, w).floor() + 0.5;
        if x >= -8.0 && x <= w + 8.0 {
            let on_major = near_step(t, major);
            cr.set_source_rgba(
                tick_r,
                tick_g,
                tick_b,
                if light {
                    if on_major {
                        0.42
                    } else {
                        0.16
                    }
                } else if on_major {
                    0.28
                } else {
                    0.10
                },
            );
            cr.move_to(x, if on_major { h - 11.0 } else { h - 5.0 });
            cr.line_to(x, h);
            let _ = cr.stroke();
            if on_major {
                let label = format_ruler_label(t, major);
                let near_playhead = (t - playhead).abs() < major * 0.08;
                cr.set_source_rgba(
                    tick_r,
                    tick_g,
                    tick_b,
                    if light {
                        if near_playhead {
                            0.88
                        } else {
                            0.50
                        }
                    } else if near_playhead {
                        0.92
                    } else {
                        0.52
                    },
                );
                if let Ok(ext) = cr.text_extents(&label) {
                    let label_x = if t <= 0.001 {
                        0.0
                    } else {
                        (x - ext.width() / 2.0).clamp(0.0, w - ext.width())
                    };
                    cr.move_to(label_x, 12.0);
                    let _ = cr.show_text(&label);
                }
            }
        }
        t += minor;
    }
}

pub fn ruler_major_step(visible: f64) -> f64 {
    const STEPS: [f64; 10] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0];
    *STEPS
        .iter()
        .find(|step| visible / *step <= 10.0)
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

pub fn near_step(value: f64, step: f64) -> bool {
    let scaled = value / step;
    (scaled - scaled.round()).abs() < 0.02
}

pub fn format_ruler_label(seconds: f64, major: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor() as u64;
    let secs = total - minutes as f64 * 60.0;
    if major < 1.0 {
        format!("{minutes}:{secs:04.1}")
    } else {
        format!("{minutes}:{:02}", secs.floor() as u64)
    }
}

pub fn draw_video_clip(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: Option<usize>,
    dragging: Option<usize>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let title = if state.title.trim().is_empty() {
        "Screen Recording"
    } else {
        state.title.as_str()
    };
    let bounds = state.segment_boundaries();
    let layout = video_layout(&state, w);
    let mut lifted = None;
    for &(_, seg_idx, x0, x1) in &layout {
        if dragging == Some(seg_idx) {
            lifted = Some((seg_idx, x0, x1));
            continue;
        }
        let selected = state.selected_segment == Some(seg_idx);
        let faint = dragging.is_some() || (state.selected_segment.is_some() && !selected);
        let label = if selected || state.selected_segment.is_none() {
            bounds.get(seg_idx).map(|&(start, end)| (title, start, end))
        } else {
            None
        };
        draw_video_segment(
            cr,
            x0,
            x1,
            h,
            clip_tone(selected, faint),
            selected,
            hovered == Some(seg_idx),
            false,
            label,
        );
    }
    if let Some((seg_idx, x0, x1)) = lifted {
        draw_video_segment(
            cr,
            x0,
            x1,
            h,
            clip_tone(true, false),
            true,
            true,
            true,
            bounds.get(seg_idx).map(|&(start, end)| (title, start, end)),
        );
    }
}

pub fn clip_tone(selected: bool, faint: bool) -> ClipTone {
    if faint {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.10),
            handle: (0.98, 0.86, 0.42, 0.30),
        }
    } else if selected {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.46),
            handle: (0.98, 0.86, 0.42, 1.0),
        }
    } else {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.28),
            handle: (0.98, 0.86, 0.42, 0.96),
        }
    }
}

pub fn draw_video_segment(
    cr: &gtk4::cairo::Context,
    x0: f64,
    x1: f64,
    h: f64,
    tone: ClipTone,
    selected: bool,
    show_handles: bool,
    lifted: bool,
    label: Option<(&str, f64, f64)>,
) {
    let clip_w = (x1 - x0).max(24.0);
    let y = if lifted { 2.0 } else { 8.0 };
    let height = h - 16.0;
    draw_translucent_clip(cr, x0, y, clip_w, height, tone, show_handles);
    if selected {
        rounded_rect(cr, x0, y, clip_w, height, 5.0);
        cr.set_source_rgba(0.98, 0.86, 0.42, 0.88);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }

    let Some((title, start, end)) = label else {
        return;
    };
    let thumb = 28.0;
    if clip_w > 86.0 {
        rounded_rect(cr, x0 + 18.0, y + (height - thumb) / 2.0, thumb, thumb, 5.0);
        cr.set_source_rgba(0.08, 0.08, 0.08, 0.55);
        let _ = cr.fill();
    }
    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.88);
    cr.set_font_size(12.0);
    let text_x = if clip_w > 86.0 { x0 + 54.0 } else { x0 + 18.0 };
    if clip_w > 72.0 {
        cr.move_to(text_x, y + height * 0.42);
        let _ = cr.show_text(title);
        cr.set_font_size(10.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.48);
        cr.move_to(text_x, y + height * 0.68);
        let _ = cr.show_text(&format_range(start, end));
    }
}

pub fn draw_zoom_clips(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: Option<usize>,
    hover_time: Option<f64>,
    dragging: Option<usize>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let clips: Vec<(usize, f64, f64)> = state
        .zoom_clips
        .iter()
        .enumerate()
        .map(|(index, clip)| (index, clip.start, clip.end))
        .collect();
    for &(index, start, end) in &clips {
        if dragging == Some(index) {
            continue;
        }
        draw_one_zoom(
            &state, cr, w, h, index, start, end, hovered, dragging, false,
        );
    }
    if let Some(index) = dragging {
        if let Some(&(_, start, end)) = clips.iter().find(|(i, _, _)| *i == index) {
            draw_one_zoom(&state, cr, w, h, index, start, end, hovered, dragging, true);
        }
    }
    if let Some(start) = hover_time {
        if let Some((start, end)) = suggested_zoom_range(&state, start) {
            draw_zoom_suggestion(&state, cr, w, h, start, end);
        }
    }
}

pub fn draw_one_zoom(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    index: usize,
    start: f64,
    end: f64,
    hovered: Option<usize>,
    dragging: Option<usize>,
    lifted: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let selected = lifted || state.selected_zoom == Some(index);
    let faint = !lifted && (dragging.is_some() || (state.selected_zoom.is_some() && !selected));
    let blue = ClipTone {
        fill: if faint {
            (0.27, 0.43, 0.82, 0.10)
        } else if selected {
            (0.30, 0.48, 0.86, 0.36)
        } else {
            (0.27, 0.43, 0.82, 0.26)
        },
        handle: (0.72, 0.84, 1.0, if faint { 0.30 } else { 0.98 }),
    };
    let y = if lifted { 1.0 } else { 7.0 };
    let height = h - 14.0;
    draw_translucent_clip(
        cr,
        x0,
        y,
        clip_w,
        height,
        blue,
        lifted || hovered == Some(index),
    );
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.82);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let label = if clip_w > 92.0 {
            let mode = match state.zoom_clips[index].mode {
                crate::recording::editor::model::ZoomMode::Auto => "Auto",
                crate::recording::editor::model::ZoomMode::Manual => "Manual",
            };
            format!("{mode}  {}", format_zoom_scale(state.zoom_clips[index].scale))
        } else {
            "Zoom".into()
        };
        let _ = cr.show_text(&label);
    }
}

pub fn suggested_zoom_range(state: &VideoEditState, start: f64) -> Option<(f64, f64)> {
    let start = start.max(0.0);
    let end = start + DEFAULT_ZOOM_DURATION_SECONDS;
    let overlaps = state
        .zoom_clips
        .iter()
        .any(|clip| start < clip.end && end > clip.start);
    (!overlaps).then_some((start, end))
}

pub fn draw_cursor_hide_clips(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: Option<usize>,
    hover_time: Option<f64>,
    dragging: Option<usize>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let clips: Vec<(usize, f64, f64)> = state
        .cursor_hide_clips
        .iter()
        .enumerate()
        .map(|(index, clip)| (index, clip.start, clip.end))
        .collect();
    for &(index, start, end) in &clips {
        if dragging == Some(index) {
            continue;
        }
        draw_one_hide(
            &state, cr, w, h, index, start, end, hovered, dragging, false,
        );
    }
    if let Some(index) = dragging {
        if let Some(&(_, start, end)) = clips.iter().find(|(i, _, _)| *i == index) {
            draw_one_hide(&state, cr, w, h, index, start, end, hovered, dragging, true);
        }
    }
    if let Some(start) = hover_time {
        if let Some((start, end)) = suggested_hide_range(&state, start) {
            draw_hide_suggestion(&state, cr, w, h, start, end);
        }
    }
}

pub fn draw_one_hide(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    index: usize,
    start: f64,
    end: f64,
    hovered: Option<usize>,
    dragging: Option<usize>,
    lifted: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let selected = lifted || state.selected_cursor_hide == Some(index);
    let faint =
        !lifted && (dragging.is_some() || (state.selected_cursor_hide.is_some() && !selected));
    let rose = ClipTone {
        fill: if faint {
            (0.72, 0.28, 0.32, 0.10)
        } else if selected {
            (0.78, 0.32, 0.36, 0.36)
        } else {
            (0.72, 0.28, 0.32, 0.26)
        },
        handle: (1.0, 0.78, 0.80, if faint { 0.30 } else { 0.98 }),
    };
    let y = if lifted { 1.0 } else { 7.0 };
    let height = h - 14.0;
    draw_translucent_clip(
        cr,
        x0,
        y,
        clip_w,
        height,
        rose,
        lifted || hovered == Some(index),
    );
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.82);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let _ = cr.show_text("Hide");
    }
}

pub fn suggested_hide_range(state: &VideoEditState, start: f64) -> Option<(f64, f64)> {
    let start = start.max(0.0);
    let end = start + DEFAULT_CURSOR_HIDE_DURATION_SECONDS;
    let overlaps = state
        .cursor_hide_clips
        .iter()
        .any(|clip| start < clip.end && end > clip.start);
    (!overlaps).then_some((start, end))
}

pub fn draw_hide_suggestion(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    start: f64,
    end: f64,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let height = h - 14.0;
    rounded_rect(cr, x0, y, clip_w, height, 5.0);
    cr.set_source_rgba(0.78, 0.32, 0.36, 0.12);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(1.0, 0.78, 0.80, 0.38);
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let _ = cr.show_text("Hide");
    }
}

pub fn draw_zoom_suggestion(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    start: f64,
    end: f64,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let height = h - 14.0;
    rounded_rect(cr, x0, y, clip_w, height, 5.0);
    cr.set_source_rgba(0.30, 0.48, 0.86, 0.12);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.72, 0.84, 1.0, 0.38);
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let _ = cr.show_text("Zoom");
    }
}

#[derive(Clone, Copy)]
pub struct ClipTone {
    fill: (f64, f64, f64, f64),
    handle: (f64, f64, f64, f64),
}

pub fn draw_translucent_clip(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    tone: ClipTone,
    show_handles: bool,
) {
    rounded_rect(cr, x, y, width, height, 5.0);
    cr.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
    let _ = cr.fill();
    if !show_handles {
        return;
    }

    let inset = HANDLE_INSET;
    let handle_w = HANDLE_WIDTH;
    let handle_h = (height - inset * 2.0).max(10.0);
    let handle_y = y + (height - handle_h) / 2.0;
    draw_edge_handle(cr, x + inset, handle_y, handle_w, handle_h, tone);
    draw_edge_handle(
        cr,
        x + width - handle_w - inset,
        handle_y,
        handle_w,
        handle_h,
        tone,
    );
}

pub fn draw_edge_handle(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    tone: ClipTone,
) {
    cr.set_source_rgba(tone.handle.0, tone.handle.1, tone.handle.2, tone.handle.3);
    rounded_rect(cr, x, y, width, height, 2.0);
    let _ = cr.fill();
}

pub fn draw_playhead(
    state: &Arc<Mutex<VideoEditState>>,
    hover_time: Option<f64>,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    if let Some(time) = hover_time {
        let hover_x = state.time_to_x(time, w);
        if (hover_x - state.time_to_x(state.playhead_seconds, w)).abs() > 1.0 {
            paint_playhead_mark(cr, hover_x, h, light, 0.22);
        }
    }
    paint_playhead_mark(
        cr,
        state.time_to_x(state.playhead_seconds, w),
        h,
        light,
        1.0,
    );
}

pub fn paint_playhead_mark(cr: &gtk4::cairo::Context, x: f64, h: f64, light: bool, alpha: f64) {
    let x = x.floor() + 0.5;
    if light {
        cr.set_source_rgba(0.07, 0.08, 0.09, alpha);
    } else {
        cr.set_source_rgba(0.86, 0.90, 0.98, alpha);
    }
    cr.set_line_width(if alpha < 1.0 { 1.5 } else { 2.0 });
    cr.move_to(x, 26.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
    if light {
        cr.set_source_rgba(0.07, 0.08, 0.09, alpha);
    } else {
        cr.set_source_rgba(0.86, 0.90, 0.98, alpha);
    }
    cr.move_to(x - 5.0, 20.0);
    cr.line_to(x + 5.0, 20.0);
    cr.line_to(x, 29.0);
    cr.close_path();
    let _ = cr.fill();
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
