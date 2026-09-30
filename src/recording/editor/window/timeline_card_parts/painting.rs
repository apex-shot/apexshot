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

    // Dim the ruler past the clip, so the extend band reads as one region
    // across the card instead of stopping at the lane seam.
    let clip_end_x = video_end_x(&state, w);
    if clip_end_x < w {
        let (r, g, b, a) = if light {
            (0.97, 0.98, 1.0, 0.55)
        } else {
            (0.03, 0.04, 0.06, 0.55)
        };
        cr.set_source_rgba(r, g, b, a);
        cr.rectangle(clip_end_x, 0.0, w - clip_end_x, h);
        let _ = cr.fill();
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
/// three lanes, makes the whole area past the clip read as one frosted region
/// from the ruler to the bottom of the stack.
pub fn draw_extend_region(
    state: &Arc<Mutex<VideoEditState>>,
    hovered: Option<usize>,
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
    let end_x = video_end_x(&state, w);
    let last_kept = state
        .segment_order
        .iter()
        .rev()
        .find(|&&index| state.segments_kept.get(index).copied().unwrap_or(true))
        .copied();
    let show_hint = last_kept.is_some()
        && (state.selected_segment == last_kept || hovered == last_kept);
    draw_extend_band(end_x, w, h, light, show_hint, cr);
}

/// The "extend duration" band: everything to the right of the clip, across the
/// whole lane stack.
///
/// Past the clip there is no footage in any lane, so the region is scrimmed to
/// read as frosted rather than usable, and the clip's edge gets a bright
/// divider — the handle you drag to grow the clip. While the clip is selected
/// (or hovered) the band carries its hint. Dragging it is `ClipDrag::End`, so
/// the region is the affordance, not decoration.
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
    // Fade the scrim in from the divider, so the edge reads as frosted glass
    // rather than a slab butted against the clip.
    let fade = band.min(96.0);
    let grad = gtk4::cairo::LinearGradient::new(end_x, 0.0, end_x + fade, 0.0);
    grad.add_color_stop_rgba(0.0, r, g, b, a * 0.35);
    grad.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let _ = cr.set_source(&grad);
    let _ = cr.paint();
    let _ = cr.restore();

    // The divider: a bright full-height pill at the clip's edge.
    let (dr, dg, db) = if light {
        (0.07, 0.08, 0.09)
    } else {
        (0.86, 0.90, 0.98)
    };
    cr.set_source_rgba(dr, dg, db, 0.9);
    rounded_rect(cr, end_x - 1.0, 2.0, 3.0, (height - 4.0).max(4.0), 1.5);
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
            cr.move_to(end_x + 16.0, (height + ext.height()) / 2.0 - 1.0);
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
}
