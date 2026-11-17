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
    if major < 1.0 {
        let minutes = (total / 60.0).floor() as u64;
        let secs = total - minutes as f64 * 60.0;
        return format!("{minutes}:{secs:04.1}");
    }
    let whole = total.round().max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

pub fn draw_video_clip(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: Option<usize>,
    dragging: Option<usize>,
    light: bool,
    filmstrip: &[gtk4::gdk_pixbuf::Pixbuf],
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    if !state.has_source_video() {
        // Before a video is loaded there is nothing to clip: an empty lane,
        // not a 24px orange nub forced out of the zero-length placeholder.
        return;
    }
    let w = width as f64;
    let h = height as f64;
    let tiles = filmstrip_tile_spans(&state, w);
    let layout = video_layout(&state, w);
    // The hold is drawn inside the final segment (it repeats the last frame),
    // so its width is the only new information the painter needs.
    let pps = pixels_per_second(&state, w);
    let mut lifted = None;
    for &(_, seg_idx, x0, x1) in &layout {
        if dragging == Some(seg_idx) {
            lifted = Some((x0, x1));
            continue;
        }
        let selected = state.selected_segment == Some(seg_idx);
        let faint = dragging.is_some() || (state.selected_segment.is_some() && !selected);
        // Only the final segment can hold a frame open.
        let hold = if state.freeze_applies_to_segment(seg_idx) {
            state.freeze_tail_seconds()
        } else {
            0.0
        };
        draw_video_segment(
            cr,
            x0,
            x1,
            h,
            clip_tone(selected, faint, light),
            selected,
            selected || hovered == Some(seg_idx),
            false,
            &tiles,
            filmstrip,
            hold,
            pps,
        );
    }
    if let Some((x0, x1)) = lifted {
        // A lifted clip is the one being dragged; if it is the last segment
        // its hold rides along, otherwise it trims as plain footage.
        let hold = if state.freeze_applies_to_segment(
            state.segment_order.last().copied().unwrap_or(usize::MAX),
        ) {
            state.freeze_tail_seconds()
        } else {
            0.0
        };
        draw_video_segment(
            cr,
            x0,
            x1,
            h,
            clip_tone(true, false, light),
            true,
            true,
            true,
            &tiles,
            filmstrip,
            hold,
            pps,
        );
    }
}

/// The "extend duration" region across every lane below the ruler.
///
/// The band used to live inside the video lane alone, so it stopped at the
/// lane seam while the ruler carried its own dim. Drawing it once, over the
/// video lane and the secondary rows, makes the whole area past the clip read
/// as one frosted region from the ruler to the bottom of the stack.
pub fn draw_extend_region(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: bool,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    if !state.has_source_video() {
        // Before a video is loaded there is no clip to extend past, so the
        // lanes stay unfrosted rather than becoming one full-width band.
        return;
    }
    let w = width as f64;
    let h = height as f64;
    // The hint belongs to the frosted region itself: it shows while the
    // pointer is over the band, not merely because a clip is selected.
    draw_extend_band(video_end_x(&state, w), w, h, light, hovered, cr);
}

/// How far the white mover line sits inside the frosted panel's left edge.
const EXTEND_MOVER_INSET: f64 = 10.0;

/// The "extend duration" band: everything to the right of the clip, across the
/// whole lane stack.
///
/// The frosted panel starts at the clip's edge and is outlined in black. The
/// white mover line sits inside it, just in from that edge — the handle you
/// drag to grow the clip. While the clip is selected (or hovered) the band
/// carries its hint. Dragging it is `ClipDrag::End`, so the region is the
/// affordance, not decoration.
pub fn draw_extend_band(
    end_x: f64,
    width: f64,
    height: f64,
    light: bool,
    show_hint: bool,
    cr: &gtk4::cairo::Context,
) {
    let band = width - end_x;
    if band < 1.0 {
        return;
    }

    let (r, g, b, a) = if light {
        (0.95, 0.96, 0.99, 0.55)
    } else {
        (0.03, 0.04, 0.06, 0.55)
    };
    let _ = cr.save();
    rounded_rect(cr, end_x, 0.0, band, height, 4.0);
    cr.clip();
    cr.set_source_rgba(r, g, b, a);
    let _ = cr.paint();
    // Fade the scrim in from the panel's edge, so it reads as frosted glass
    // rather than a slab butted against the clip.
    let fade = band.min(96.0);
    let grad = gtk4::cairo::LinearGradient::new(end_x, 0.0, end_x + fade, 0.0);
    grad.add_color_stop_rgba(0.0, r, g, b, a * 0.35);
    grad.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let _ = cr.set_source(&grad);
    let _ = cr.paint();
    let _ = cr.restore();

    // The panel's border: a thin black outline. Black is only ever the border.
    rounded_rect(
        cr,
        end_x + 0.5,
        0.5,
        (band - 1.0).max(0.0),
        (height - 1.0).max(0.0),
        4.0,
    );
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // The mover: a short white line inside the panel, just in from its left
    // edge. This is the grab point for extending the clip.
    let (mr, mg, mb) = if light {
        (0.10, 0.11, 0.13)
    } else {
        (1.0, 1.0, 1.0)
    };
    cr.set_source_rgba(mr, mg, mb, 0.9);
    let mover_y = (height * 0.30).max(2.0);
    let mover_h = (height * 0.40).max(6.0);
    rounded_rect(
        cr,
        end_x + EXTEND_MOVER_INSET - 1.0,
        mover_y,
        2.0,
        mover_h,
        1.0,
    );
    let _ = cr.fill();

    if !show_hint {
        return;
    }
    let label = crate::i18n::t("Drag to adjust duration");
    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(13.0);
    if let Ok(ext) = cr.text_extents(&label) {
        // Only spell it out when the band is wide enough for the words to sit
        // clear of the divider instead of crowding it.
        if band > ext.width() + 36.0 {
            let (tr, tg, tb, ta) = if light {
                (0.10, 0.12, 0.15, 0.60)
            } else {
                (1.0, 1.0, 1.0, 0.45)
            };
            cr.set_source_rgba(tr, tg, tb, ta);
            cr.move_to(
                end_x + EXTEND_MOVER_INSET + 14.0,
                (height + ext.height()) / 2.0 - 1.0,
            );
            let _ = cr.show_text(&label);
        }
    }
}

pub fn clip_tone(selected: bool, faint: bool, light: bool) -> ClipTone {
    if light {
        return if faint {
            ClipTone {
                fill: (0.78, 0.52, 0.14, 0.16),
                edge: (0.45, 0.25, 0.02, 0.92),
                handle: (0.48, 0.28, 0.03, 0.38),
            }
        } else if selected {
            ClipTone {
                fill: (0.78, 0.52, 0.14, 0.54),
                edge: (0.45, 0.25, 0.02, 0.92),
                handle: (0.48, 0.28, 0.03, 1.0),
            }
        } else {
            ClipTone {
                fill: (0.78, 0.52, 0.14, 0.38),
                edge: (0.45, 0.25, 0.02, 0.92),
                handle: (0.48, 0.28, 0.03, 0.88),
            }
        };
    }

    if faint {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.10),
            edge: (0.98, 0.86, 0.42, 0.88),
            handle: (0.98, 0.86, 0.42, 0.30),
        }
    } else if selected {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.46),
            edge: (0.98, 0.86, 0.42, 0.88),
            handle: (0.98, 0.86, 0.42, 1.0),
        }
    } else {
        ClipTone {
            fill: (0.78, 0.58, 0.18, 0.28),
            edge: (0.98, 0.86, 0.42, 0.88),
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
    tiles: &[(f64, f64)],
    filmstrip: &[gtk4::gdk_pixbuf::Pixbuf],
    freeze: f64,
    px_per_second: f64,
) {
    let clip_w = (x1 - x0).max(24.0);
    // Inset matches the Motion source lane: a 6px reveal top and bottom inside
    // the 56px lane, so the clip body is 44px in both editors.
    let y = if lifted { 2.0 } else { 6.0 };
    let height = h - 12.0;
    // While frames decode (or ffmpeg is unavailable) the body reads as an
    // empty media lane in Motion's source-lane tone, not an orange slab.
    let tone = if filmstrip.is_empty() {
        ClipTone {
            fill: (0.10, 0.11, 0.13, 0.9),
            ..tone
        }
    } else {
        tone
    };
    draw_translucent_clip(cr, x0, y, clip_w, height, tone, show_handles);
    if !filmstrip.is_empty() && !tiles.is_empty() {
        let _ = cr.save();
        rounded_rect(cr, x0, y, clip_w, height, 5.0);
        cr.clip();
        for (index, pixbuf) in filmstrip.iter().enumerate() {
            let Some(&(span_start, span_end)) = tiles.get(index) else {
                continue;
            };
            if span_end <= x0 || span_start >= x0 + clip_w {
                continue;
            }
            let tile_x = span_start.max(x0);
            let tile_w = (span_end.min(x0 + clip_w) - tile_x).max(1.0);
            paint_cover_pixbuf(cr, pixbuf, tile_x, y, tile_w, height);
        }
        // Past the last frame there is nothing to sample, so the hold repeats
        // the final frame — that is what a freeze *is*, and it keeps the clip
        // reading as footage instead of a black slab.
        if freeze > 0.0 {
            if let Some(last) = filmstrip.last() {
                let hold_x0 = x0 + clip_w - freeze * px_per_second;
                let mut tile_x = hold_x0;
                // One held frame per natural frame-width, at the lane's height.
                // Sizing the repeats to the hold instead (`freeze * pps / 3`)
                // stretched each copy: `paint_cover_pixbuf` then cropped into a
                // zoomed slice of the frame, which read as a smear.
                let src_w = last.width().max(1) as f64;
                let src_h = last.height().max(1) as f64;
                let tile_w = (height * src_w / src_h).clamp(8.0, 240.0);
                while tile_x < x0 + clip_w {
                    paint_cover_pixbuf(
                        cr,
                        last,
                        tile_x,
                        y,
                        tile_w.min(x0 + clip_w - tile_x),
                        height,
                    );
                    tile_x += tile_w;
                }
            }
        }
        let _ = cr.restore();
    }
    if freeze > 0.0 {
        // Mark where the footage stops and the hold begins.
        let mark = (x0 + clip_w - freeze * px_per_second).floor() + 0.5;
        if mark > x0 + 1.0 && mark < x0 + clip_w - 1.0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            cr.set_line_width(1.0);
            cr.set_dash(&[3.0, 3.0], 0.0);
            cr.move_to(mark, y + 3.0);
            cr.line_to(mark, y + height - 3.0);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
    }
    if selected {
        rounded_rect(cr, x0 + 0.5, y + 0.5, (clip_w - 1.0).max(0.0), (height - 1.0).max(0.0), 4.5);
        let (r, g, b, a) = tone.edge;
        cr.set_source_rgba(r, g, b, a);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
}

fn paint_cover_pixbuf(
    cr: &gtk4::cairo::Context,
    pixbuf: &gtk4::gdk_pixbuf::Pixbuf,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let src_w = f64::from(pixbuf.width()).max(1.0);
    let src_h = f64::from(pixbuf.height()).max(1.0);
    let scale = (width / src_w).max(height / src_h);
    let painted_w = src_w * scale;
    let painted_h = src_h * scale;
    let _ = cr.save();
    cr.rectangle(x, y, width, height);
    cr.clip();
    cr.translate(x + (width - painted_w) / 2.0, y + (height - painted_h) / 2.0);
    cr.scale(scale, scale);
    cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

pub fn draw_zoom_clips(
    state: &Arc<Mutex<VideoEditState>>,
    hover_time: Option<f64>,
    dragging: Option<usize>,
    light: bool,
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
        draw_one_zoom(&state, cr, w, h, index, start, end, dragging, false, light);
    }
    if let Some(index) = dragging {
        if let Some(&(_, start, end)) = clips.iter().find(|(i, _, _)| *i == index) {
            draw_one_zoom(&state, cr, w, h, index, start, end, dragging, true, light);
        }
    }
    if let Some(start) = hover_time {
        // A hovering pointer previews the next click. With a clip on the
        // clipboard that click pastes, so the "add a clip here" box would be
        // describing the wrong action — and at 1.8s it is narrower than most
        // pasted spans, so its dashed edge showed through the ghost as a stray
        // vertical line.
        if state.clipboard_duration_for(true).is_none() {
            if let Some((start, end)) = suggested_zoom_range(&state, start) {
                draw_zoom_suggestion(&state, cr, w, h, start, end, light);
            }
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
    dragging: Option<usize>,
    lifted: bool,
    light: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let selected = lifted || state.selected_zoom == Some(index);
    let faint = !lifted && (dragging.is_some() || (state.selected_zoom.is_some() && !selected));
    // Solid #0000FF body, opaque in both themes. Only unselected clips recede,
    // and they do it by fading the same solid fill rather than lightening the
    // hue, so every pill stays the same blue.
    let ring = if light {
        (0.0, 0.0, 0.45)
    } else {
        (1.0, 1.0, 1.0)
    };
    // A clip disabled from its context menu keeps its place on the timeline but
    // no longer feeds the preview, so it is drawn hollow: the ring and handles
    // stay, and the fill drops to a hint. That is the difference between "this
    // clip is not the selection" (a faded solid) and "this clip is off".
    let disabled = state.zoom_clips[index].hidden;
    let blue = ClipTone {
        fill: if disabled {
            (0.0, 0.0, 1.0, 0.12)
        } else {
            (0.0, 0.0, 1.0, if faint { 0.55 } else { 1.0 })
        },
        edge: (ring.0, ring.1, ring.2, 0.95),
        handle: (ring.0, ring.1, ring.2, if faint { 0.40 } else { 0.95 }),
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
        lifted || selected,
    );
    if selected {
        rounded_rect(cr, x0 + 0.5, y + 0.5, (clip_w - 1.0).max(0.0), (height - 1.0).max(0.0), 4.5);
        let (r, g, b, a) = blue.edge;
        cr.set_source_rgba(r, g, b, a);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let label = if clip_w > 92.0 {
            let mode = match state.zoom_clips[index].mode {
                crate::recording::editor::model::ZoomMode::Auto => t("Auto"),
                crate::recording::editor::model::ZoomMode::Manual => t("Manual"),
            };
            format!("{mode}  {}", format_zoom_scale(state.zoom_clips[index].scale))
        } else {
            t("Zoom")
        };
        let _ = cr.show_text(&label);
    }
}

/// The span a click on the empty zoom lane would place, so the lane can draw
/// its ghost at the length the click really produces.
pub fn suggested_zoom_range(state: &VideoEditState, start: f64) -> Option<(f64, f64)> {
    let start = start.max(0.0);
    let end = start + ADD_ZOOM_DURATION_SECONDS;
    let overlaps = state
        .zoom_clips
        .iter()
        .any(|clip| start < clip.end && end > clip.start);
    (!overlaps).then_some((start, end))
}

pub fn draw_cursor_hide_clips(
    state: &Arc<Mutex<VideoEditState>>,
    hover_time: Option<f64>,
    dragging: Option<usize>,
    light: bool,
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
        draw_one_hide(&state, cr, w, h, index, start, end, dragging, false, light);
    }
    if let Some(index) = dragging {
        if let Some(&(_, start, end)) = clips.iter().find(|(i, _, _)| *i == index) {
            draw_one_hide(&state, cr, w, h, index, start, end, dragging, true, light);
        }
    }
    if let Some(start) = hover_time {
        // Same rule as the Zoom lane: with a clip held for this track the click
        // pastes, so the add-a-hide preview must stand down.
        if state.clipboard_duration_for(false).is_none() {
            if let Some((start, end)) = suggested_hide_range(&state, start) {
                draw_hide_suggestion(&state, cr, w, h, start, end, light);
            }
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
    dragging: Option<usize>,
    lifted: bool,
    light: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let selected = lifted || state.selected_cursor_hide == Some(index);
    let faint =
        !lifted && (dragging.is_some() || (state.selected_cursor_hide.is_some() && !selected));
    // Solid #660033, opaque in both themes. Unselected clips recede by fading
    // that same fill, so the hue never shifts between states.
    let ring = if light {
        (0.20, 0.0, 0.10)
    } else {
        (1.0, 0.80, 0.90)
    };
    // Same hollow treatment as a disabled zoom clip: the pill keeps its span
    // and its handles, and only the fill drops away.
    let disabled = state.cursor_hide_clips[index].hidden;
    let rose = ClipTone {
        fill: if disabled {
            (0.4, 0.0, 0.2, 0.12)
        } else {
            (0.4, 0.0, 0.2, if faint { 0.55 } else { 1.0 })
        },
        edge: (ring.0, ring.1, ring.2, 0.95),
        handle: (ring.0, ring.1, ring.2, if faint { 0.40 } else { 0.95 }),
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
        lifted || selected,
    );
    if selected {
        rounded_rect(cr, x0 + 0.5, y + 0.5, (clip_w - 1.0).max(0.0), (height - 1.0).max(0.0), 4.5);
        let (r, g, b, a) = rose.edge;
        cr.set_source_rgba(r, g, b, a);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
    if clip_w > 40.0 {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let hide_label = t("Hide");
        let _ = cr.show_text(&hide_label);
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
    light: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let height = h - 14.0;
    rounded_rect(cr, x0, y, clip_w, height, 5.0);
    if light {
        cr.set_source_rgba(0.66, 0.16, 0.23, 0.20);
    } else {
        cr.set_source_rgba(0.78, 0.32, 0.36, 0.12);
    }
    let _ = cr.fill_preserve();
    if light {
        cr.set_source_rgba(0.42, 0.05, 0.10, 0.58);
    } else {
        cr.set_source_rgba(1.0, 0.78, 0.80, 0.38);
    }
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    if clip_w > 40.0 {
        if light {
            cr.set_source_rgba(0.30, 0.03, 0.07, 0.72);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
        }
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let hide_label = t("Hide");
        let _ = cr.show_text(&hide_label);
    }
}

pub fn draw_zoom_suggestion(
    state: &VideoEditState,
    cr: &gtk4::cairo::Context,
    w: f64,
    h: f64,
    start: f64,
    end: f64,
    light: bool,
) {
    let x0 = state.time_to_x(start, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).max(22.0);
    let y = 7.0;
    let height = h - 14.0;
    rounded_rect(cr, x0, y, clip_w, height, 5.0);
    if light {
        cr.set_source_rgba(0.18, 0.37, 0.72, 0.20);
    } else {
        cr.set_source_rgba(0.30, 0.48, 0.86, 0.12);
    }
    let _ = cr.fill_preserve();
    if light {
        cr.set_source_rgba(0.10, 0.24, 0.52, 0.58);
    } else {
        cr.set_source_rgba(0.72, 0.84, 1.0, 0.38);
    }
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 3.0], 0.0);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    if clip_w > 40.0 {
        if light {
            cr.set_source_rgba(0.05, 0.13, 0.31, 0.72);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
        }
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let zoom_label = t("Zoom");
        let _ = cr.show_text(&zoom_label);
    }
}

/// One secondary lane at whatever height it is eased to: the full drawing, the
/// thin resting strip, or a cross-fade of both while the lanes trade height.
pub fn draw_fading_lane(
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    progress: f64,
    full: &dyn Fn(&gtk4::cairo::Context, i32, i32),
    thin: &dyn Fn(&gtk4::cairo::Context, i32, i32),
) {
    if progress >= 0.999 {
        full(cr, width, height);
    } else if progress <= 0.001 {
        thin(cr, width, height);
    } else {
        cr.push_group();
        thin(cr, width, height);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(1.0 - progress);
        cr.push_group();
        full(cr, width, height);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(progress);
    }
}

/// A resting lane: its name at the left edge and one slim bar per clip. A bar
/// running under the name fades in from transparent, so the name never sits on
/// solid colour.
pub fn draw_thin_lane(
    state: &VideoEditState,
    label: &str,
    bars: &[(f64, f64, (f64, f64, f64, f64))],
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Bold,
    );
    cr.set_font_size(12.0);
    let extents = cr.text_extents(label).ok();
    let text_w = extents.map_or(40.0, |extents| extents.x_advance());
    let text_h = extents.map_or(9.0, |extents| extents.height());
    let label_x = 6.0;
    let label_end = label_x + text_w;
    let fade_end = label_end + 40.0;

    let bar_h = (h - 4.0).max(3.0);
    let y = (h - bar_h) / 2.0;
    for &(start, end, fill) in bars {
        let x0 = state.time_to_x(start, w);
        let x1 = state.time_to_x(end, w).max(x0 + 4.0);
        rounded_rect(cr, x0, y, x1 - x0, bar_h, (bar_h * 0.35).min(5.0));
        if x0 < fade_end {
            let fade = gtk4::cairo::LinearGradient::new(0.0, 0.0, fade_end, 0.0);
            fade.add_color_stop_rgba(0.0, fill.0, fill.1, fill.2, 0.0);
            fade.add_color_stop_rgba(label_end / fade_end, fill.0, fill.1, fill.2, fill.3 * 0.15);
            fade.add_color_stop_rgba(1.0, fill.0, fill.1, fill.2, fill.3);
            let _ = cr.set_source(&fade);
        } else {
            cr.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
        }
        let _ = cr.fill();
    }
    let ink = if light { 0.0 } else { 1.0 };
    cr.set_source_rgba(ink, ink, ink, 0.6);
    cr.move_to(label_x, (h + text_h) / 2.0);
    let _ = cr.show_text(label);
}

/// The Zoom lane resting: its clips as slim blue bars.
pub fn draw_thin_zoom_lane(
    state: &VideoEditState,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let bars: Vec<(f64, f64, (f64, f64, f64, f64))> = state
        .zoom_clips
        .iter()
        .map(|clip| (clip.start, clip.end, (0.0, 0.0, 1.0, 0.95)))
        .collect();
    draw_thin_lane(state, &t("Zoom"), &bars, light, cr, width, height);
}

/// The Cursor lane resting: its hide clips as slim rose bars.
pub fn draw_thin_cursor_lane(
    state: &VideoEditState,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let bars: Vec<(f64, f64, (f64, f64, f64, f64))> = state
        .cursor_hide_clips
        .iter()
        .map(|clip| (clip.start, clip.end, (0.4, 0.0, 0.2, 0.95)))
        .collect();
    draw_thin_lane(state, &t("Cursor"), &bars, light, cr, width, height);
}

/// The Sound lane resting: one slim bar per kept piece, in its sound fill so a
/// silent piece already reads quieter than the rest.
pub fn draw_thin_sound_lane(
    state: &VideoEditState,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let bars: Vec<(f64, f64, (f64, f64, f64, f64))> = state
        .placed_segment_slots()
        .into_iter()
        .map(|(index, start, _, _)| {
            let muted = state.is_muted() || state.segment_is_muted(index);
            (
                start,
                start + state.segment_timeline_duration(index),
                sound_tone(light, muted).fill,
            )
        })
        .collect();
    draw_thin_lane(state, &t("Sound"), &bars, light, cr, width, height);
}

/// The sound lane's colours: the pill, the waveform inside it and the ring a
/// selected piece draws.
#[derive(Clone, Copy)]
struct SoundTone {
    fill: (f64, f64, f64, f64),
    wave: (f64, f64, f64, f64),
    edge: (f64, f64, f64, f64),
}

fn sound_tone(light: bool, muted: bool) -> SoundTone {
    let mut tone = if light {
        SoundTone {
            fill: (0.90, 0.86, 0.99, 1.0),
            wave: (0.40, 0.24, 0.76, 0.95),
            edge: (0.34, 0.18, 0.68, 1.0),
        }
    } else {
        SoundTone {
            fill: (0.19, 0.13, 0.34, 1.0),
            wave: (0.72, 0.58, 1.0, 0.95),
            edge: (0.80, 0.70, 1.0, 1.0),
        }
    };
    if muted {
        // A muted piece keeps its span and its ring; the fill and the waveform
        // drop back so "silent" reads differently from "quiet".
        tone.fill.3 = 0.45;
        tone.wave.3 = 0.35;
    }
    tone
}

/// The expanded sound lane: one pill per cut piece, its waveform inside it.
///
/// A piece's pill is the piece itself, so the row reads as the same
/// composition the video lane shows. The waveform is the recording's loudness
/// over the footage under each column; a piece with no measurement yet shows a
/// flat line instead of an empty pill.
pub fn draw_sound_lane(
    state: &Arc<Mutex<VideoEditState>>,
    peaks: Option<&Peaks>,
    light: bool,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    if !state.has_source_video() {
        return;
    }
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let step = 3.0;
    let inset = 4.0;

    for (index, composition_start, _, _) in state.placed_segment_slots() {
        let x0 = state.time_to_x(composition_start, w);
        let x1 = state
            .time_to_x(
                composition_start + state.segment_timeline_duration(index),
                w,
            )
            .max(x0 + 22.0);
        let pill_w = x1 - x0;
        let y = 2.0;
        let pill_h = (h - 4.0).max(10.0);
        let muted = state.is_muted() || state.segment_is_muted(index);
        let tone = sound_tone(light, muted);

        rounded_rect(cr, x0, y, pill_w, pill_h, 8.0);
        cr.set_source_rgba(tone.fill.0, tone.fill.1, tone.fill.2, tone.fill.3);
        let _ = cr.fill();

        let wave_left = x0 + inset;
        let wave_right = x1 - inset;
        match peaks {
            Some(peaks) if wave_right > wave_left => {
                let tallest = (pill_h - 8.0).max(2.0);
                let bar_w = 2.0;
                let mut x = wave_left;
                while x < wave_right {
                    let level = sound_level(&state, peaks, w, x, (x + step).min(wave_right));
                    let bar_h = (f64::from(level).sqrt() * tallest).max(1.0);
                    let bar_y = y + (pill_h - bar_h) / 2.0;
                    rounded_rect(cr, x, bar_y, bar_w, bar_h, bar_w / 2.0);
                    cr.set_source_rgba(tone.wave.0, tone.wave.1, tone.wave.2, tone.wave.3);
                    let _ = cr.fill();
                    x += step;
                }
            }
            _ => {
                cr.set_source_rgba(tone.wave.0, tone.wave.1, tone.wave.2, tone.wave.3);
                cr.set_line_width(1.0);
                cr.move_to(wave_left, y + pill_h / 2.0);
                cr.line_to(wave_right.max(wave_left), y + pill_h / 2.0);
                let _ = cr.stroke();
            }
        }

        if muted {
            // Hatch the pill so a silent piece reads as struck out at a
            // glance, without changing the span it keeps.
            let _ = cr.save();
            rounded_rect(cr, x0, y, pill_w, pill_h, 8.0);
            cr.clip();
            cr.set_source_rgba(tone.wave.0, tone.wave.1, tone.wave.2, 0.30);
            cr.set_line_width(1.0);
            let pitch = 6.0;
            let mut offset = -pill_h;
            while offset < pill_w {
                cr.move_to(x0 + offset, y + pill_h);
                cr.line_to(x0 + offset + pill_h, y);
                let _ = cr.stroke();
                offset += pitch;
            }
            let _ = cr.restore();
        }

        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        let padding = 6.0;
        let baseline = y + pill_h * 0.62;
        let right = x1 - inset;
        let speed = state.segment_speed(index);
        let speed_label = ((speed - 1.0).abs() > 1e-9).then(|| format_clip_speed(speed));
        // A muted piece carries one chip across its mark, its word and its
        // speed, so no label sits straight on the waveform bars.
        let mut chip_painted = false;
        if muted {
            let glyph = 12.0;
            let glyph_x = x0 + inset;
            let text_x = glyph_x + glyph + 4.0;
            let muted_label = t("Muted");
            let muted_w = text_width(cr, &muted_label);
            if text_x + muted_w <= right {
                let speed_x = text_x + muted_w + 6.0;
                let chip_right = match speed_label.as_ref() {
                    Some(label) if speed_x + text_width(cr, label) <= right => {
                        speed_x + text_width(cr, label)
                    }
                    _ => text_x + muted_w,
                };
                draw_label_chip(cr, glyph_x - 4.0, chip_right + 4.0, baseline, tone.fill);
                chip_painted = true;
                draw_mute_glyph(cr, glyph_x, y + (pill_h - glyph) / 2.0, glyph, tone.edge);
                cr.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
                cr.move_to(text_x, baseline);
                let _ = cr.show_text(&muted_label);
                // The speed follows the mute mark and its word, so the two
                // never sit on top of each other on a silent piece.
                if chip_right > text_x + muted_w {
                    if let Some(label) = speed_label.as_ref() {
                        cr.move_to(speed_x, baseline);
                        let _ = cr.show_text(label);
                    }
                }
            }
        }
        if let Some(label) = speed_label.as_ref() {
            if !chip_painted {
                let label_w = text_width(cr, label);
                if x0 + padding + label_w <= right {
                    draw_label_chip(
                        cr,
                        x0 + padding - 4.0,
                        x0 + padding + label_w + 4.0,
                        baseline,
                        tone.fill,
                    );
                    cr.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
                    cr.move_to(x0 + padding, baseline);
                    let _ = cr.show_text(label);
                }
            }
        }

        if state.selected_segment == Some(index) {
            rounded_rect(
                cr,
                x0 + 1.0,
                y + 1.0,
                (pill_w - 2.0).max(0.0),
                (pill_h - 2.0).max(0.0),
                7.0,
            );
            cr.set_source_rgba(tone.edge.0, tone.edge.1, tone.edge.2, tone.edge.3);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    }
}

/// The recording's loudness between two x positions, mapped through the
/// retained footage so a cut, a reordered piece or a speed change reads the
/// source it actually plays.
fn sound_level(
    state: &VideoEditState,
    peaks: &Peaks,
    width: f64,
    from_x: f64,
    to_x: f64,
) -> f32 {
    let source_at = |x: f64| {
        state
            .playback_position(state.x_to_time(x, width))
            .map(|(_, source)| source)
    };
    match (source_at(from_x), source_at(to_x)) {
        (Some(from), Some(to)) => peaks.level(from, to),
        _ => 0.0,
    }
}

/// The width `label` takes at the current font.
fn text_width(cr: &gtk4::cairo::Context, label: &str) -> f64 {
    cr.text_extents(label)
        .map(|extents| {
            extents
                .x_advance()
                .max(extents.x_bearing() + extents.width())
        })
        .unwrap_or(0.0)
}

/// The opaque chip a lane label sits on: without it the text and the waveform
/// bars share the same violet and the label is hard to read.
fn draw_label_chip(
    cr: &gtk4::cairo::Context,
    left: f64,
    right: f64,
    baseline: f64,
    fill: (f64, f64, f64, f64),
) {
    let chip_h = 16.0;
    // Centre the chip on the text's own box rather than on its baseline, so
    // the ink sits evenly inside it.
    let centre = cr.font_extents().map_or(baseline - chip_h / 4.0, |extents| {
        baseline + (extents.descent() - extents.ascent()) / 2.0
    });
    rounded_rect(
        cr,
        left,
        centre - chip_h / 2.0,
        (right - left).max(0.0),
        chip_h,
        4.0,
    );
    cr.set_source_rgba(fill.0, fill.1, fill.2, 1.0);
    let _ = cr.fill();
}

/// A speaker with a cross, `size` square, drawn from its top-left corner.
fn draw_mute_glyph(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    size: f64,
    colour: (f64, f64, f64, f64),
) {
    let _ = cr.save();
    cr.set_source_rgba(colour.0, colour.1, colour.2, colour.3);
    let point = |across: f64, down: f64| (x + across * size, y + down * size);
    let body = [
        (0.08, 0.36),
        (0.30, 0.36),
        (0.55, 0.14),
        (0.55, 0.86),
        (0.30, 0.64),
        (0.08, 0.64),
    ];
    for (index, (across, down)) in body.iter().enumerate() {
        let (across, down) = point(*across, *down);
        if index == 0 {
            cr.move_to(across, down);
        } else {
            cr.line_to(across, down);
        }
    }
    cr.close_path();
    let _ = cr.fill();

    cr.set_line_width(0.10 * size);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    for (from_across, from_down, to_across, to_down) in
        [(0.68, 0.36, 0.94, 0.64), (0.94, 0.36, 0.68, 0.64)]
    {
        let (from_across, from_down) = point(from_across, from_down);
        let (to_across, to_down) = point(to_across, to_down);
        cr.move_to(from_across, from_down);
        cr.line_to(to_across, to_down);
        let _ = cr.stroke();
    }
    let _ = cr.restore();
}

#[derive(Clone, Copy)]
pub struct ClipTone {
    fill: (f64, f64, f64, f64),
    edge: (f64, f64, f64, f64),
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
            paint_hover_line(cr, hover_x, h);
        }
    }
    paint_playhead_mark(cr, state.time_to_x(state.playhead_seconds, w), h, light);
}

/// Pointer read-out line, matching the Motion timeline: a red hairline with no
/// capsule, drawn under the playhead.
pub fn paint_hover_line(cr: &gtk4::cairo::Context, x: f64, h: f64) {
    let x = x.floor() + 0.5;
    cr.set_source_rgba(0.80, 0.22, 0.20, 0.95);
    cr.set_line_width(2.0);
    cr.move_to(x, 0.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
}

pub fn paint_playhead_mark(cr: &gtk4::cairo::Context, x: f64, h: f64, light: bool) {
    let x = x.floor() + 0.5;
    let (stem_r, stem_g, stem_b) = if light {
        (0.07, 0.08, 0.09)
    } else {
        (0.86, 0.90, 0.98)
    };
    const HANDLE_W: f64 = 12.0;
    const HANDLE_H: f64 = 26.0;
    const HANDLE_TOP: f64 = 2.0;

    cr.set_source_rgba(stem_r, stem_g, stem_b, 0.9);
    cr.set_line_width(1.5);
    cr.move_to(x, HANDLE_TOP + HANDLE_H);
    cr.line_to(x, h);
    let _ = cr.stroke();

    let hx = x - HANDLE_W / 2.0;
    rounded_rect(cr, hx, HANDLE_TOP, HANDLE_W, HANDLE_H, HANDLE_H / 2.0);
    if light {
        cr.set_source_rgba(0.94, 0.96, 1.0, 1.0);
    } else {
        cr.set_source_rgba(0.04, 0.05, 0.07, 1.0);
    }
    let _ = cr.fill_preserve();
    cr.set_source_rgba(stem_r, stem_g, stem_b, 1.0);
    cr.set_line_width(1.5);
    let _ = cr.stroke();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::{
        CursorHideClip, VideoEditState, VideoMetadata, ZoomClip, ZoomMode,
    };
    use gtk4::cairo::{Context, Format, ImageSurface};
    use std::path::PathBuf;

    fn metadata() -> VideoMetadata {
        VideoMetadata {
            path: PathBuf::from("/tmp/input.mp4"),
            duration_seconds: 10.0,
            width: 1920,
            height: 1080,
            file_size_bytes: 100 * 1024 * 1024,
            has_audio: true,
            frame_rate: 30.0,
        }
    }

    /// ARgb32 stores each pixel as one native-endian u32; the top byte is the
    /// alpha channel, so the RGB below is premultiplied by it.
    fn zoom_pill_pixel(light: bool) -> [u8; 4] {
        let mut state = VideoEditState::new(metadata());
        state.timeline_scale = 0.0;
        state.zoom_clips.push(ZoomClip {
            start: 1.0,
            end: 4.0,
            scale: 1.5,
            mode: ZoomMode::Auto,
            ..Default::default()
        });
        state.selected_zoom = Some(0);

        let mut surface = ImageSurface::create(Format::ARgb32, 400, 56).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw_one_zoom(&state, &cr, 400.0, 56.0, 0, 1.0, 4.0, None, false, light);
        }
        surface.flush();

        let width = surface.width() as usize;
        let x = state.time_to_x(2.0, 400.0).round() as usize;
        let data = surface.data().unwrap();
        let offset = (28 * width + x) * 4;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn zoom_pills_are_solid_blue_in_both_themes() {
        for light in [false, true] {
            let [b, g, r, a] = zoom_pill_pixel(light);
            assert_eq!(a, 255, "the pill body must be opaque (light: {light})");
            assert_eq!(
                (r, g, b),
                (0, 0, 255),
                "the pill body must be solid #0000FF, not a translucent tint",
            );
        }
    }

    fn hide_pill_pixel(light: bool) -> [u8; 4] {
        let mut state = VideoEditState::new(metadata());
        state.timeline_scale = 0.0;
        state.cursor_hide_clips.push(CursorHideClip {
            start: 1.0,
            end: 4.0,
            hidden: false,
        });
        state.selected_cursor_hide = Some(0);

        let mut surface = ImageSurface::create(Format::ARgb32, 400, 56).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw_one_hide(&state, &cr, 400.0, 56.0, 0, 1.0, 4.0, None, false, light);
        }
        surface.flush();

        let width = surface.width() as usize;
        let x = state.time_to_x(2.0, 400.0).round() as usize;
        let data = surface.data().unwrap();
        let offset = (28 * width + x) * 4;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn hide_pills_are_solid_maroon_in_both_themes() {
        for light in [false, true] {
            let [b, g, r, a] = hide_pill_pixel(light);
            assert_eq!(a, 255, "the pill body must be opaque (light: {light})");
            assert_eq!(
                (r, g, b),
                (102, 0, 51),
                "the pill body must be solid #660033, not a translucent tint",
            );
        }
    }

    #[test]
    fn whole_second_ruler_labels_round_and_carry() {
        assert_eq!(super::format_ruler_label(0.0, 2.0), "0:00");
        assert_eq!(super::format_ruler_label(2.0, 2.0), "0:02");
        assert_eq!(super::format_ruler_label(4.0, 2.0), "0:04");
        assert_eq!(super::format_ruler_label(6.0, 2.0), "0:06");
        assert_eq!(super::format_ruler_label(8.0, 2.0), "0:08");
        assert_eq!(
            super::format_ruler_label(3.9999999999999996, 2.0),
            "0:04",
            "an accumulated tick error must still read as the second it is"
        );
        assert_eq!(
            super::format_ruler_label(59.60000000000001, 2.0),
            "1:00",
            "rounding must carry into the next minute"
        );
        assert_eq!(super::format_ruler_label(0.4, 0.5), "0:00.4");
    }
}
