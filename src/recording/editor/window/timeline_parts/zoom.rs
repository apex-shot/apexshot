pub fn build_zoom_track(state: Arc<Mutex<VideoEditState>>, estimate_label: Label) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("recording-editor-zoom-track");
    area.set_hexpand(true);
    area.set_size_request(-1, 32);
    area.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            draw_zoom_track(&state, cr, width, height);
        }
    });

    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let state = state.clone();
        let area = area.clone();
        move |gesture, n_press, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut state = state.lock().unwrap();
            let seconds = composition_x_to_source(&state, width, x);
            if n_press >= 2 {
                state.playhead_seconds = seconds;
                state.add_zoom_at_playhead();
            } else {
                let selected = state
                    .zoom_clips
                    .iter()
                    .position(|clip| seconds >= clip.start && seconds <= clip.end);
                state.selected_zoom = selected;
            }
            drop(state);
            area.queue_draw();
        }
    });
    area.add_controller(click);

    let drag = GestureDrag::new();
    drag.set_button(1);
    let drag_edge = Rc::new(Cell::new(None::<(usize, bool)>));
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_edge = drag_edge.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let state = state.lock().unwrap();
            let seconds = composition_x_to_source(&state, width, x);
            let edge = state
                .zoom_clips
                .iter()
                .enumerate()
                .find_map(|(index, clip)| {
                    if (clip.start - seconds).abs() < 0.12 {
                        Some((index, true))
                    } else if (clip.end - seconds).abs() < 0.12 {
                        Some((index, false))
                    } else {
                        None
                    }
                });
            drag_edge.set(edge);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let drag_edge = drag_edge.clone();
        let area = area.clone();
        let estimate_label = estimate_label.clone();
        move |gesture, offset_x, _| {
            let Some((index, is_start)) = drag_edge.get() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let seconds = {
                let state = state.lock().unwrap();
                composition_x_to_source(&state, width, start_x + offset_x)
            };
            {
                let mut state = state.lock().unwrap();
                if let Some(clip) = state.zoom_clips.get(index).cloned() {
                    if is_start {
                        state.set_zoom_range(index, seconds, clip.end);
                    } else {
                        state.set_zoom_range(index, clip.start, seconds);
                    }
                }
            }
            area.queue_draw();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    area.add_controller(drag);

    {
        let area = area.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            area.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    area
}

pub fn draw_ruler(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    // Tick density follows the zoom slider, not clip offset. Sliding the
    // clip must not change how many seconds sit on screen.
    let visible = state.visible_span_seconds();
    let major: f64 = if visible > 180.0 {
        30.0
    } else if visible > 90.0 {
        10.0
    } else if visible > 40.0 {
        5.0
    } else if visible > 12.0 {
        2.0
    } else {
        1.0
    };
    let minor = (major / 5.0).max(0.2);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.set_line_width(1.0);
    let inner = (w - ZERO_INSET).max(1.0);
    // Paint ticks across the whole visible strip, not just up to the clip
    // or playhead. The axis stays open so the track can start later.
    let view_start = state.x_to_time(-ZERO_INSET, inner).max(0.0);
    let view_end = state.x_to_time(inner + 40.0, inner);
    let mut t = (view_start / minor).floor() * minor;
    if t < 0.0 {
        t = 0.0;
    }
    while t <= view_end + 0.001 {
        let x = (ZERO_INSET + state.time_to_x(t, inner)).floor() + 0.5;
        if x < -20.0 || x > w + 20.0 {
            t += minor;
            continue;
        }
        let is_major = (t / major).fract().abs() < 0.02 || (1.0 - (t / major).fract()).abs() < 0.02;
        cr.set_source_rgba(1.0, 1.0, 1.0, if is_major { 0.28 } else { 0.12 });
        cr.move_to(x, if is_major { h - 8.0 } else { h - 4.0 });
        cr.line_to(x, h);
        let _ = cr.stroke();
        if is_major {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.48);
            let label = format_timecode(t);
            if let Ok(ext) = cr.text_extents(&label) {
                cr.move_to(x - ext.width() / 2.0, 11.0);
                let _ = cr.show_text(&label);
            }
        }
        t += minor;
    }
}

pub fn draw_spanning_playhead(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    // The overlay spans the rail headers too. Clip to the track canvas so
    // scrolling the playhead off the left never paints over lock/eye icons.
    let clip_left = f64::from(RAIL_HEADER_WIDTH);
    if w <= clip_left {
        return;
    }
    cr.save().ok();
    cr.rectangle(clip_left, 0.0, w - clip_left, h);
    cr.clip();

    let inset_left = clip_left + TRACK_GAP + ZERO_INSET;
    let usable = (w - inset_left).max(1.0);
    let x = (inset_left + state.source_to_x(state.playhead_seconds, usable)).floor() + 0.5;

    // Home-plate head: standing rectangle with a downward triangle tip.
    let head_w = 10.0;
    let body_h = 7.0;
    let tip_h = 5.0;
    let left = x - head_w / 2.0;
    let right = x + head_w / 2.0;

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.move_to(left, 0.0);
    cr.line_to(right, 0.0);
    cr.line_to(right, body_h);
    cr.line_to(x, body_h + tip_h);
    cr.line_to(left, body_h);
    cr.close_path();
    let _ = cr.fill();

    cr.set_line_width(1.0);
    cr.set_line_cap(gtk4::cairo::LineCap::Butt);
    cr.move_to(x, body_h + tip_h);
    cr.line_to(x, h);
    let _ = cr.stroke();
    cr.restore().ok();
}

pub fn draw_zoom_track(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    rounded_rect(cr, 0.0, 2.0, w, h - 4.0, 10.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.04);
    let _ = cr.fill();
    for (index, clip) in state.zoom_clips.iter().enumerate() {
        let x0 = state.source_to_x(clip.start, w);
        let x1 = state.source_to_x(clip.end, w);
        let selected = state.selected_zoom == Some(index);
        rounded_rect(cr, x0, 4.0, (x1 - x0).max(18.0), h - 8.0, 9.0);
        cr.set_source_rgba(0.69, 0.36, 0.22, if selected { 0.92 } else { 0.55 });
        let _ = cr.fill();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.82);
        cr.select_font_face(
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        cr.set_font_size(10.0);
        let label = format!("{:.1}x", clip.scale);
        if let Ok(ext) = cr.text_extents(&label) {
            cr.move_to(x0 + 8.0, h / 2.0 + ext.height() / 2.0);
            let _ = cr.show_text(&label);
        }
    }
}

