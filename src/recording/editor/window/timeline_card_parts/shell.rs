pub fn build_timeline_card(
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    on_change: Rc<dyn Fn()>,
) -> (GtkBox, Rc<dyn Fn()>) {
    let shell = GtkBox::new(Orientation::Vertical, 0);
    shell.add_css_class("recording-editor-timeline-dock");
    shell.set_hexpand(true);
    shell.set_vexpand(false);
    shell.set_valign(Align::End);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-card");
    card.set_hexpand(true);

    let playing = Rc::new(Cell::new(false));
    let (playhead_clock, duration_clock) = {
        let guard = state.lock().unwrap();
        let playhead_clock = Label::new(Some(&format_clock(guard.playhead_seconds)));
        playhead_clock.add_css_class("recording-editor-timeline-clock");
        let duration_clock = Label::new(Some(&format_clock(guard.source_duration())));
        duration_clock.add_css_class("recording-editor-timeline-clock");
        (playhead_clock, duration_clock)
    };

    let play_button = icon_button("media-playback-start-symbolic", "Play");
    play_button.add_css_class("recording-editor-timeline-play");
    let skip_back = icon_button("media-skip-backward-symbolic", "Skip back 1s");
    let skip_forward = icon_button("media-skip-forward-symbolic", "Skip forward 1s");

    let zoom = labeled_tool_button("zoom-fit-best-symbolic", "Zoom", "Add zoom at playhead");
    let split = labeled_tool_button("edit-cut-symbolic", "Split", "Split at playhead");

    let zoom_out = icon_button("zoom-out-symbolic", "Zoom out timeline");
    let zoom_in = icon_button("zoom-in-symbolic", "Zoom in timeline");
    let zoom_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 10.0);
    zoom_scale.add_css_class("recording-editor-timeline-zoom");
    zoom_scale.set_draw_value(false);
    zoom_scale.set_value(state.lock().unwrap().timeline_scale.clamp(0.0, 100.0));
    zoom_scale.set_size_request(88, 16);
    zoom_scale.set_valign(Align::Center);

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_hexpand(true);

    let left = GtkBox::new(Orientation::Horizontal, 4);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.append(&zoom);
    left.append(&split);

    let center = GtkBox::new(Orientation::Horizontal, 8);
    center.add_css_class("recording-editor-timeline-transport");
    center.set_halign(Align::Center);
    center.set_hexpand(false);
    center.append(&playhead_clock);
    center.append(&skip_back);
    center.append(&play_button);
    center.append(&skip_forward);
    center.append(&duration_clock);

    let right = GtkBox::new(Orientation::Horizontal, 6);
    right.add_css_class("recording-editor-timeline-zoom-row");
    right.set_halign(Align::End);
    right.set_hexpand(true);
    right.append(&zoom_out);
    right.append(&zoom_scale);
    right.append(&zoom_in);

    toolbar.append(&left);
    toolbar.append(&center);
    toolbar.append(&right);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-card-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 36);
    ruler.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| draw_ruler(&state, cr, width, height)
    });

    let hovered_video = Rc::new(Cell::new(None::<usize>));
    let hovered_zoom = Rc::new(Cell::new(None::<usize>));
    let hover_time = Rc::new(Cell::new(None::<f64>));
    let hover_zoom_time = Rc::new(Cell::new(None::<f64>));
    let dragging_video = Rc::new(Cell::new(None::<usize>));
    let dragging_zoom = Rc::new(Cell::new(None::<usize>));

    let video_track = DrawingArea::new();
    video_track.add_css_class("recording-editor-card-video-track");
    video_track.set_hexpand(true);
    video_track.set_size_request(-1, 80);
    video_track.set_draw_func({
        let state = state.clone();
        let hovered_video = hovered_video.clone();
        let dragging_video = dragging_video.clone();
        move |_, cr, width, height| {
            draw_video_clip(
                &state,
                hovered_video.get(),
                dragging_video.get(),
                cr,
                width,
                height,
            )
        }
    });

    let zoom_track = DrawingArea::new();
    zoom_track.add_css_class("recording-editor-card-zoom-track");
    zoom_track.set_hexpand(true);
    zoom_track.set_size_request(-1, 56);
    zoom_track.set_draw_func({
        let state = state.clone();
        let hovered_zoom = hovered_zoom.clone();
        let hover_zoom_time = hover_zoom_time.clone();
        let dragging_zoom = dragging_zoom.clone();
        move |_, cr, width, height| {
            draw_zoom_clips(
                &state,
                hovered_zoom.get(),
                hover_zoom_time.get(),
                dragging_zoom.get(),
                cr,
                width,
                height,
            )
        }
    });

    let tracks = GtkBox::new(Orientation::Vertical, 10);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&video_track);
    tracks.append(&zoom_track);

    let board = Overlay::new();
    board.add_css_class("recording-editor-card-board");
    board.set_hexpand(true);
    board.set_child(Some(&tracks));

    let playhead = DrawingArea::new();
    playhead.add_css_class("recording-editor-card-playhead");
    playhead.set_hexpand(true);
    playhead.set_vexpand(true);
    playhead.set_can_target(false);
    playhead.set_draw_func({
        let state = state.clone();
        let hover_time = hover_time.clone();
        move |_, cr, width, height| draw_playhead(&state, hover_time.get(), cr, width, height)
    });
    board.add_overlay(&playhead);
    bind_board_hover(&tracks, state.clone(), hover_time, playhead.clone());

    let scroll_adj = Adjustment::new(0.0, 0.0, 1.0, 0.1, 1.0, 1.0);
    let scroll_syncing = Rc::new(Cell::new(false));
    sync_scroll_adj(&scroll_adj, &state.lock().unwrap(), &scroll_syncing);

    let paint: Rc<dyn Fn()> = {
        let ruler = ruler.clone();
        let video_track = video_track.clone();
        let zoom_track = zoom_track.clone();
        let playhead = playhead.clone();
        let playhead_clock = playhead_clock.clone();
        let duration_clock = duration_clock.clone();
        let zoom = zoom.clone();
        let state = state.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        Rc::new(move || {
            {
                let guard = state.lock().unwrap();
                playhead_clock.set_text(&format_clock(guard.playhead_seconds));
                duration_clock.set_text(&format_clock(guard.source_duration()));
                sync_scroll_adj(&scroll_adj, &guard, &scroll_syncing);
                if guard.selected_zoom.is_some() {
                    zoom.add_css_class("recording-editor-timeline-tool-active");
                } else {
                    zoom.remove_css_class("recording-editor-timeline-tool-active");
                }
            }
            ruler.queue_draw();
            video_track.queue_draw();
            zoom_track.queue_draw();
            playhead.queue_draw();
        })
    };
    let redraw: Rc<dyn Fn()> = {
        let paint = paint.clone();
        let on_change = on_change.clone();
        Rc::new(move || {
            paint();
            on_change();
        })
    };

    scroll_adj.connect_value_changed({
        let state = state.clone();
        let redraw = redraw.clone();
        let scroll_syncing = scroll_syncing.clone();
        move |adj| {
            if scroll_syncing.get() {
                return;
            }
            state.lock().unwrap().set_timeline_scroll(adj.value());
            redraw();
        }
    });

    let wheel = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::HORIZONTAL,
    );
    wheel.connect_scroll({
        let state = state.clone();
        let redraw = redraw.clone();
        move |_, dx, dy| {
            let delta = if dx.abs() > f64::EPSILON { dx } else { dy };
            if delta.abs() < f64::EPSILON {
                return glib::Propagation::Proceed;
            }
            let mut guard = state.lock().unwrap();
            let step = guard.visible_span_seconds() * 0.08 * delta;
            let next = guard.timeline_scroll_seconds + step;
            guard.set_timeline_scroll(next);
            drop(guard);
            redraw();
            glib::Propagation::Stop
        }
    });
    board.add_controller(wheel);

    zoom.connect_clicked({
        let state = state.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut guard = state.lock().unwrap();
            if guard.add_zoom_at_playhead().is_none() {
                let playhead = guard.playhead_seconds;
                if let Some(index) = guard
                    .zoom_clips
                    .iter()
                    .position(|clip| playhead >= clip.start && playhead <= clip.end)
                {
                    select_zoom(&mut guard, Some(index));
                }
            }
            drop(guard);
            redraw();
        }
    });

    split.connect_clicked({
        let state = state.clone();
        let redraw = redraw.clone();
        move |_| {
            let cut_at = state.lock().unwrap().source_playhead();
            state.lock().unwrap().add_cut(cut_at);
            redraw();
        }
    });

    play_button.connect_clicked({
        let state = state.clone();
        let media = media.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        let redraw = redraw.clone();
        move |_| toggle_playback(&state, &media, &playing, &play_button, &redraw)
    });

    skip_back.connect_clicked({
        let state = state.clone();
        let media = media.clone();
        let redraw = redraw.clone();
        move |_| nudge_playhead(&state, &media, -1.0, &redraw)
    });
    skip_forward.connect_clicked({
        let state = state.clone();
        let media = media.clone();
        let redraw = redraw.clone();
        move |_| nudge_playhead(&state, &media, 1.0, &redraw)
    });

    zoom_scale.connect_value_changed({
        let state = state.clone();
        let redraw = redraw.clone();
        move |scale| {
            state.lock().unwrap().timeline_scale = scale.value().clamp(0.0, 100.0);
            redraw();
        }
    });
    zoom_out.connect_clicked({
        let zoom_scale = zoom_scale.clone();
        move |_| zoom_scale.set_value((zoom_scale.value() - 10.0).max(0.0))
    });
    zoom_in.connect_clicked({
        let zoom_scale = zoom_scale.clone();
        move |_| zoom_scale.set_value((zoom_scale.value() + 10.0).min(100.0))
    });

    bind_playhead_drag(&ruler, state.clone(), media.clone(), redraw.clone());
    bind_video_clip(
        &video_track,
        state.clone(),
        media.clone(),
        hovered_video.clone(),
        dragging_video.clone(),
        redraw.clone(),
    );
    bind_zoom_track(
        &zoom_track,
        state.clone(),
        media.clone(),
        hovered_zoom.clone(),
        hover_zoom_time.clone(),
        dragging_zoom.clone(),
        redraw.clone(),
    );

    {
        let state = state.clone();
        let media = media.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        let redraw = redraw.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            tick_playback(&state, &media, &playing, &play_button, &redraw);
            glib::ControlFlow::Continue
        });
    }

    let well = GtkBox::new(Orientation::Vertical, 0);
    well.add_css_class("recording-editor-timeline-well");
    well.set_hexpand(true);
    let hbar = Scrollbar::new(Orientation::Horizontal, Some(&scroll_adj));
    hbar.add_css_class("recording-editor-timeline-scroll");
    hbar.set_hexpand(true);
    well.append(&hbar);

    card.append(&toolbar);
    card.append(&board);
    card.append(&well);
    shell.append(&card);
    (shell, paint)
}

