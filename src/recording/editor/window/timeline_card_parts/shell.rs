pub fn build_timeline_card(
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    filmstrip: Rc<RefCell<Vec<gtk4::gdk_pixbuf::Pixbuf>>>,
    on_change: Rc<dyn Fn()>,
    window: &impl IsA<Widget>,
    canvas: &impl IsA<Widget>,
) -> (GtkBox, Rc<dyn Fn()>, Rc<dyn Fn()>) {
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
        let duration_clock = Label::new(Some(&format_clock(guard.content_end_seconds())));
        duration_clock.add_css_class("recording-editor-timeline-clock");
        (playhead_clock, duration_clock)
    };

    let play_button = icon_button("media-playback-start-symbolic", &t("Play"));
    play_button.add_css_class("recording-editor-timeline-play");
    let skip_back = icon_button("media-skip-backward-symbolic", &t("Skip back 1s"));
    let skip_forward = icon_button("media-skip-forward-symbolic", &t("Skip forward 1s"));

    let zoom = labeled_tool_button("zoom-fit-best-symbolic", &t("Zoom"), &t("Add zoom at playhead"));
    let hide = labeled_tool_button("view-conceal-symbolic", &t("Hide"), &t("Hide cursor at playhead"));
    let split = labeled_tool_button(
        icon_names::custom::SQUARE_SPLIT_HORIZONTAL_SYMBOLIC,
        &t("Split"),
        &t("Split at playhead"),
    );
    let detect = labeled_tool_button(
        icon_names::custom::WAND_SPARKLES_SYMBOLIC,
        &t("Detect"),
        &t("Detect automatic zooms from recorded clicks"),
    );
    let analyzing = Rc::new(Cell::new(false));

    let zoom_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 10.0);
    zoom_scale.add_css_class("recording-editor-timeline-zoom");
    zoom_scale.set_draw_value(false);
    zoom_scale.set_value(state.lock().unwrap().timeline_scale.clamp(0.0, 100.0));
    zoom_scale.set_valign(Align::Center);
    zoom_scale.set_tooltip_text(Some(&t("Zoom")));

    // One control rather than a −/+ pair around a bare track: a pill, the
    // magnifier at its left, and a rounded thumb for the level.
    //
    // The thumb is drawn here rather than left to the scale. A GtkScale paints
    // a trough, a `highlight` and a `fill` that the theme fills in with its own
    // colours, and no combination of CSS reliably cleared all three — the
    // theme's track kept showing through as a second, lighter pill inside this
    // one. The scale is therefore drawn at zero opacity and kept only for the
    // behaviour (click, drag, keyboard, scroll), sitting over the area the
    // thumb travels, so the pill reads as a single flat surface.
    let zoom_level_bar = GtkBox::new(Orientation::Horizontal, 0);
    zoom_level_bar.add_css_class("recording-editor-timeline-zoom-bar");
    zoom_level_bar.set_valign(Align::Center);
    let zoom_level_track = GtkBox::new(Orientation::Horizontal, 0);
    zoom_level_track.set_hexpand(true);
    zoom_level_track.append(&zoom_level_bar);

    // The magnifier is the pill's base layer and the track rides above it, so
    // the thumb's travel starts at the pill's left edge and slides over the
    // glyph at the low end of the range. Laying the two side by side instead
    // would push the whole travel to the right of the icon.
    let zoom_glyph = Image::from_icon_name("zoom-in-symbolic");
    zoom_glyph.set_pixel_size(14);
    zoom_glyph.set_can_target(false);
    let zoom_glyph_slot = GtkBox::new(Orientation::Horizontal, 0);
    zoom_glyph_slot.add_css_class("recording-editor-timeline-zoom-glyph");
    zoom_glyph_slot.set_halign(Align::Start);
    zoom_glyph_slot.set_valign(Align::Center);
    zoom_glyph_slot.append(&zoom_glyph);

    let zoom_area = Overlay::new();
    zoom_area.set_hexpand(true);
    zoom_area.set_child(Some(&zoom_glyph_slot));
    zoom_area.add_overlay(&zoom_level_track);
    zoom_area.add_overlay(&zoom_scale);

    let zoom_pill = GtkBox::new(Orientation::Horizontal, 0);
    zoom_pill.add_css_class("recording-editor-timeline-zoom-pill");
    zoom_pill.append(&zoom_area);

    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_hexpand(true);

    let left = GtkBox::new(Orientation::Horizontal, 4);
    left.set_halign(Align::Start);
    left.set_hexpand(true);
    left.append(&zoom);
    left.append(&hide);
    left.append(&split);
    left.append(&detect);

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
    // Leading spacer, widened once the first frame is laid out: it carries the
    // transport back onto the canvas axis (see
    // `centre_transport_over_canvas`). It sits ahead of the zoom pill so that
    // pill keeps hugging the window edge.
    let balance = GtkBox::new(Orientation::Horizontal, 0);
    balance.set_size_request(0, -1);
    right.append(&balance);
    right.append(&zoom_pill);

    toolbar.append(&left);
    toolbar.append(&center);
    toolbar.append(&right);
    centre_transport_over_canvas(window, canvas, &center, &balance);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-card-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 36);
    ruler.set_draw_func({
        let state = state.clone();
        move |area, cr, width, height| {
            draw_ruler(&state, widget_is_light(area), cr, width, height)
        }
    });

    let hovered_video = Rc::new(Cell::new(None::<usize>));
    let hovered_extend = Rc::new(Cell::new(false));
    let hovered_zoom = Rc::new(Cell::new(None::<usize>));
    let hovered_hide = Rc::new(Cell::new(None::<usize>));
    let hover_time = Rc::new(Cell::new(None::<f64>));
    let hover_zoom_time = Rc::new(Cell::new(None::<f64>));
    let hover_hide_time = Rc::new(Cell::new(None::<f64>));
    let dragging_video = Rc::new(Cell::new(None::<usize>));
    let dragging_zoom = Rc::new(Cell::new(None::<usize>));
    let dragging_hide = Rc::new(Cell::new(None::<usize>));

    let video_track = DrawingArea::new();
    video_track.add_css_class("recording-editor-card-video-track");
    video_track.set_hexpand(true);
    // Motion's source lane is the reference for this clip, and it sits on the
    // 56px `card-zoom-track` floor. Matching it is what makes the two editors
    // line up, so this lane is 56px rather than a shorter 48px one.
    video_track.set_size_request(-1, 56);
    video_track.set_draw_func({
        let state = state.clone();
        let hovered_video = hovered_video.clone();
        let dragging_video = dragging_video.clone();
        let filmstrip = filmstrip.clone();
        move |area, cr, width, height| {
            let frames = filmstrip.borrow();
            draw_video_clip(
                &state,
                hovered_video.get(),
                dragging_video.get(),
                widget_is_light(area),
                &frames,
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
        let hover_zoom_time = hover_zoom_time.clone();
        let dragging_zoom = dragging_zoom.clone();
        move |area, cr, width, height| {
            draw_zoom_clips(
                &state,
                hover_zoom_time.get(),
                dragging_zoom.get(),
                widget_is_light(area),
                cr,
                width,
                height,
            );
            // The ghost draws on top of the lane: it previews what a click
            // would place, so it has to read over whatever is already there.
            if let Some(start) = hover_zoom_time.get() {
                if dragging_zoom.get().is_none() {
                    let guard = state.lock().unwrap();
                    draw_clipboard_ghost(
                        &guard,
                        cr,
                        width as f64,
                        height as f64,
                        start,
                        widget_is_light(area),
                        true,
                    );
                }
            }
        }
    });

    let hide_track = DrawingArea::new();
    hide_track.add_css_class("recording-editor-card-hide-track");
    hide_track.set_hexpand(true);
    hide_track.set_size_request(-1, 56);
    hide_track.set_draw_func({
        let state = state.clone();
        let hover_hide_time = hover_hide_time.clone();
        let dragging_hide = dragging_hide.clone();
        move |area, cr, width, height| {
            draw_cursor_hide_clips(
                &state,
                hover_hide_time.get(),
                dragging_hide.get(),
                widget_is_light(area),
                cr,
                width,
                height,
            );
            // Same copied-clip preview as the Zoom lane above.
            if let Some(start) = hover_hide_time.get() {
                if dragging_hide.get().is_none() {
                    let guard = state.lock().unwrap();
                    draw_clipboard_ghost(
                        &guard,
                        cr,
                        width as f64,
                        height as f64,
                        start,
                        widget_is_light(area),
                        false,
                    );
                }
            }
        }
    });

    // The frosted extend region is one layer over the three lanes, so it spans
    // the video, zoom and hide rows (and the gaps between them) instead of
    // stopping at the video lane's seam.
    let extend_layer = DrawingArea::new();
    extend_layer.add_css_class("recording-editor-card-extend");
    extend_layer.set_hexpand(true);
    extend_layer.set_vexpand(true);
    extend_layer.set_can_target(false);
    extend_layer.set_draw_func({
        let state = state.clone();
        let hovered_extend = hovered_extend.clone();
        let dragging_video = dragging_video.clone();
        move |area, cr, width, height| {
            if dragging_video.get().is_some() {
                return;
            }
            draw_extend_region(
                &state,
                hovered_extend.get(),
                widget_is_light(area),
                cr,
                width,
                height,
            )
        }
    });

    // The band is its own widget, so a hover change has to repaint it too —
    // the lane that caught the motion only queues its own draw.
    let set_band_hover: Rc<dyn Fn(bool)> = {
        let hovered_extend = hovered_extend.clone();
        let extend_layer = extend_layer.clone();
        Rc::new(move |on| {
            if hovered_extend.get() != on {
                hovered_extend.set(on);
                extend_layer.queue_draw();
            }
        })
    };

    let lanes = GtkBox::new(Orientation::Vertical, 10);
    lanes.add_css_class("recording-editor-card-tracks");
    lanes.set_hexpand(true);
    lanes.append(&video_track);
    lanes.append(&zoom_track);
    lanes.append(&hide_track);

    let lane_overlay = Overlay::new();
    lane_overlay.set_hexpand(true);
    lane_overlay.set_child(Some(&lanes));
    // The band rides above the lanes: the space past the clip is empty in
    // every lane, and sitting on top keeps the edge divider crisp against the
    // clip instead of being covered by its rounded corner.
    lane_overlay.add_overlay(&extend_layer);

    let tracks = GtkBox::new(Orientation::Vertical, 10);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&lane_overlay);

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
        move |area, cr, width, height| {
            draw_playhead(
                &state,
                hover_time.get(),
                widget_is_light(area),
                cr,
                width,
                height,
            )
        }
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
        let hide_track = hide_track.clone();
        let extend_layer = extend_layer.clone();
        let playhead = playhead.clone();
        let playhead_clock = playhead_clock.clone();
        let duration_clock = duration_clock.clone();
        let zoom = zoom.clone();
        let hide = hide.clone();
        let detect = detect.clone();
        let analyzing = analyzing.clone();
        let state = state.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        Rc::new(move || {
            {
                let guard = state.lock().unwrap();
                playhead_clock.set_text(&format_clock(guard.playhead_seconds));
                duration_clock.set_text(&format_clock(guard.content_end_seconds()));
                sync_scroll_adj(&scroll_adj, &guard, &scroll_syncing);
                if guard.selected_zoom.is_some() {
                    zoom.add_css_class("recording-editor-timeline-tool-active");
                } else {
                    zoom.remove_css_class("recording-editor-timeline-tool-active");
                }
                if guard.selected_cursor_hide.is_some() {
                    hide.add_css_class("recording-editor-timeline-tool-active");
                } else {
                    hide.remove_css_class("recording-editor-timeline-tool-active");
                }
                detect.set_sensitive(
                    guard.has_source_video() && !guard.zoom_locked && !analyzing.get(),
                );
            }
            ruler.queue_draw();
            extend_layer.queue_draw();
            video_track.queue_draw();
            zoom_track.queue_draw();
            hide_track.queue_draw();
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

    let pause: Rc<dyn Fn()> = {
        let media = media.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        let paint = paint.clone();
        Rc::new(move || {
            pause_playback(&media, &playing, &play_button, &paint);
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

    hide.connect_clicked({
        let state = state.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut guard = state.lock().unwrap();
            if guard.add_cursor_hide_at_playhead().is_none() {
                let playhead = guard.playhead_seconds;
                if let Some(index) = guard
                    .cursor_hide_clips
                    .iter()
                    .position(|clip| playhead >= clip.start && playhead <= clip.end)
                {
                    select_cursor_hide(&mut guard, Some(index));
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

    detect.connect_clicked({
        let state = state.clone();
        let redraw = redraw.clone();
        let analyzing = analyzing.clone();
        move |button| {
            if analyzing.get() {
                return;
            }
            let guard = state.lock().unwrap();
            if guard.supports_auto_zoom() {
                drop(guard);
                let mut guard = state.lock().unwrap();
                // Detect applies what it finds: the generated clips land on
                // the timeline directly, as one undo step.
                guard.redetect_zoom_clips();
                let auto_zooms = guard
                    .zoom_clips
                    .iter()
                    .filter(|clip| clip.mode == ZoomMode::Auto)
                    .count();
                let manual_zooms = guard
                    .zoom_clips
                    .iter()
                    .filter(|clip| clip.mode == ZoomMode::Manual)
                    .count();
                drop(guard);
                redraw();
                if auto_zooms == 0 {
                    let message = if manual_zooms > 0 {
                        t("No new zooms were added. Manual zooms are preserved.")
                    } else {
                        t("No clear clicks were found.")
                    };
                    crate::utils::notify::desktop_notification(&t("No Auto Zooms added"), &message);
                }
                return;
            }
            if !guard.has_source_video() || guard.zoom_locked {
                return;
            }
            let metadata = guard.metadata.clone();
            drop(guard);

            analyzing.set(true);
            button.set_sensitive(false);
            button.set_tooltip_text(Some(&t("Analyzing visible cursor motion…")));
            let (sender, receiver) = mpsc::channel::<Result<
                crate::recording::editor::sidecar::PointerSidecar,
                String,
            >>();
            let analyzed_path = metadata.path.clone();
            std::thread::spawn(move || {
                let result = crate::recording::editor::imported_pointer::analyze(&metadata)
                    .and_then(|sidecar| {
                        sidecar.write_next_to_video(&metadata.path)?;
                        Ok(sidecar)
                    })
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });

            let state = state.clone();
            let redraw = redraw.clone();
            let analyzing = analyzing.clone();
            let button = button.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                match receiver.try_recv() {
                    Ok(Ok(sidecar)) => {
                        let mut guard = state.lock().unwrap();
                        if guard.metadata.path != analyzed_path {
                            analyzing.set(false);
                            button.set_tooltip_text(Some(&t(
                                "Detect automatic zooms from recorded clicks",
                            )));
                            return glib::ControlFlow::Break;
                        }
                        guard.sidecar = Some(sidecar);
                        guard.redetect_zoom_clips();
                        let auto_zooms = guard
                            .zoom_clips
                            .iter()
                            .filter(|clip| clip.mode == ZoomMode::Auto)
                            .count();
                        drop(guard);
                        analyzing.set(false);
                        button.set_tooltip_text(Some(&t(
                            "Detect automatic zooms from recorded clicks",
                        )));
                        redraw();
                        if auto_zooms == 0 {
                            crate::utils::notify::desktop_notification(
                                &t("No Auto Zooms added"),
                                &t("This video has no recorded clicks, so there is nothing to zoom on."),
                            );
                        }
                        glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        analyzing.set(false);
                        button.set_sensitive(true);
                        button.set_tooltip_text(Some(&t(
                            "Detect automatic zooms from recorded clicks",
                        )));
                        crate::utils::notify::desktop_notification(
                            &t("Cursor analysis could not place Auto Zooms"),
                            &error,
                        );
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        analyzing.set(false);
                        button.set_sensitive(true);
                        button.set_tooltip_text(Some(&t(
                            "Detect automatic zooms from recorded clicks",
                        )));
                        crate::utils::notify::desktop_notification(
                            &t("Cursor analysis stopped"),
                            &t("The analysis worker stopped unexpectedly. Manual Zoom is still available."),
                        );
                        glib::ControlFlow::Break
                    }
                }
            });
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
        let zoom_level_track = zoom_level_track.clone();
        let zoom_level_bar = zoom_level_bar.clone();
        move |scale| {
            let value = scale.value().clamp(0.0, 100.0);
            state.lock().unwrap().timeline_scale = value;
            place_zoom_level_bar(&zoom_level_track, &zoom_level_bar, value / 100.0);
            redraw();
        }
    });

    bind_playhead_drag(&ruler, state.clone(), media.clone(), redraw.clone());
    bind_video_clip(
        &video_track,
        state.clone(),
        media.clone(),
        hovered_video.clone(),
        set_band_hover.clone(),
        dragging_video.clone(),
        redraw.clone(),
    );
    bind_zoom_track(
        &zoom_track,
        state.clone(),
        media.clone(),
        hovered_zoom.clone(),
        hover_zoom_time.clone(),
        set_band_hover.clone(),
        dragging_zoom.clone(),
        redraw.clone(),
    );
    bind_hide_track(
        &hide_track,
        state.clone(),
        media.clone(),
        hovered_hide.clone(),
        hover_hide_time.clone(),
        set_band_hover.clone(),
        dragging_hide.clone(),
        redraw.clone(),
    );

    {
        let state = state.clone();
        let media = media.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        let redraw = redraw.clone();
        let zoom_level_track = zoom_level_track.clone();
        let zoom_level_bar = zoom_level_bar.clone();
        let zoom_scale = zoom_scale.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            tick_playback(&state, &media, &playing, &play_button, &redraw);
            // The bar's travel follows the track, so a resize has to re-place
            // it; `place_zoom_level_bar` is a no-op when nothing moved.
            place_zoom_level_bar(&zoom_level_track, &zoom_level_bar, zoom_scale.value() / 100.0);
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

    (shell, paint, pause)
}

/// Line the transport up with the canvas, not with the dock it sits in.
///
/// The dock spans the window's whole width — the tool sidebar included — while
/// the video lives in the stage column off to its left. A row centred inside
/// the dock therefore lands half the sidebar to the right of the picture, so
/// the play button floats off to the side of the chrome it belongs to instead
/// of sitting under it, level with the `Original / Crop video / High` chips.
///
/// The zoom group carries a spacer (widened here) that pushes the transport
/// back onto the canvas axis. Twice the drift, because the box then splits the
/// leftover width evenly between the two groups flanking the transport.
///
/// The offset is a constant of the chrome, but it is measured off the first
/// laid-out frame rather than hard-coded: a change to either column's padding
/// or width then cannot quietly reintroduce the drift.
fn centre_transport_over_canvas(
    window: &impl IsA<Widget>,
    canvas: &impl IsA<Widget>,
    transport: &GtkBox,
    balance: &GtkBox,
) {
    let window = window.clone().upcast::<Widget>();
    let canvas = canvas.clone().upcast::<Widget>();
    let transport = transport.clone();
    let balance = balance.clone();
    transport.clone().add_tick_callback(move |_, _| {
        // Tick callbacks run before the frame is allocated, so the first
        // reading of a freshly built card is the not-yet-laid-out one.
        if canvas.allocated_width() <= 1 || transport.allocated_width() <= 1 {
            return glib::ControlFlow::Continue;
        }
        let (Some(canvas_origin), Some(transport_origin)) = (
            canvas.compute_point(&window, &gtk4::graphene::Point::new(0.0, 0.0)),
            transport.compute_point(&window, &gtk4::graphene::Point::new(0.0, 0.0)),
        ) else {
            return glib::ControlFlow::Continue;
        };
        let canvas_centre = canvas_origin.x() as f64 + canvas.allocated_width() as f64 / 2.0;
        let transport_centre = transport_origin.x() as f64 + transport.allocated_width() as f64 / 2.0;
        if let Some(width) = balance_width(canvas_centre, transport_centre) {
            balance.set_size_request(width, -1);
        }
        glib::ControlFlow::Break
    });
}

/// Width for the spacer that pulls the transport onto the canvas axis.
///
/// The two arguments are window-space x coordinates for the middle of the
/// canvas and of the transport. `None` means the transport already lines up
/// (or sits left of the canvas), so the spacer stays out of the way. Twice
/// the drift, because the toolbar splits the width it has left over evenly
/// between the groups flanking the transport, so only half of the spacer
/// moves it.
fn balance_width(canvas_centre: f64, transport_centre: f64) -> Option<i32> {
    let drift = transport_centre - canvas_centre;
    (drift > 0.5).then(|| (drift * 2.0).round() as i32)
}

/// Width of the zoom pill's thumb, in px. Mirrors
/// `.recording-editor-timeline-zoom-bar` in 07.css, which is what gives the
/// thumb its size — the thumb is a plain box, so the travel has to subtract it
/// by hand.
const ZOOM_BAR_WIDTH: i32 = 6;

/// Vertical inset of the thumb inside the track, in px. The thumb is a rounded
/// handle that floats inside the pill, not a hairline that spans it top to
/// bottom, so its height is the track's less this on each side.
const ZOOM_BAR_INSET: i32 = 3;

/// Floor for the thumb's height, in px. Mirrors the same declaration in 07.css:
/// a track too short to inset must still show a full thumb rather than let CSS
/// clamp the height out from under the travel maths below.
const ZOOM_BAR_MIN_HEIGHT: i32 = 18;

/// Height of the zoom thumb in a track `track_height` px tall.
fn zoom_thumb_height(track_height: i32) -> i32 {
    (track_height - ZOOM_BAR_INSET * 2).max(ZOOM_BAR_MIN_HEIGHT)
}

/// Put the zoom level thumb at `fraction` (0..1) along the track.
///
/// Returns whether it moved. GTK4 dropped `GtkAlignment`, so nothing places a
/// fixed-width child at an arbitrary fraction any more; the thumb is offset by
/// the distance it has left to travel, which is the track's width less the
/// thumb itself.
///
/// The thumb's height is also re-derived from the track here (less
/// [`ZOOM_BAR_INSET`] top and bottom), so it stays centred and inset through a
/// resize instead of stretching back across the pill.
fn place_zoom_level_bar(track: &GtkBox, bar: &GtkBox, fraction: f64) -> bool {
    let width = track.allocated_width();
    let height = track.allocated_height();
    if height <= 0 {
        // Not laid out yet; the caller's next tick will catch it.
        return false;
    }
    let mut moved = false;
    let thumb_height = zoom_thumb_height(height);
    if bar.height() != thumb_height {
        bar.set_size_request(-1, thumb_height);
        moved = true;
    }
    if width <= ZOOM_BAR_WIDTH {
        return moved;
    }
    let travel = (width - ZOOM_BAR_WIDTH) as f64 * fraction.clamp(0.0, 1.0);
    let offset = travel.round() as i32;
    if bar.margin_start() != offset {
        bar.set_margin_start(offset);
        moved = true;
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::{
        balance_width, zoom_thumb_height, ZOOM_BAR_INSET, ZOOM_BAR_MIN_HEIGHT, ZOOM_BAR_WIDTH,
    };
    use crate::recording::editor::window::tool_sidebar::TOOL_SIDEBAR_WIDTH;

    /// Measured on a 1400px-wide window: the canvas runs 14..1094 and the
    /// transport, centred in the full-width dock, lands at 775.5. The spacer
    /// below is what has to carry it back to the canvas centre.
    #[test]
    fn spacer_covers_twice_the_drift_from_the_canvas_axis() {
        let canvas_centre = (14.0 + 1094.0) / 2.0;
        let width = balance_width(canvas_centre, 775.5).expect("drifted transport");
        assert_eq!(width, 443);
        // Half of the spacer is what the transport actually travels.
        assert!((canvas_centre + width as f64 / 2.0 - 775.5).abs() < 0.5);
    }

    #[test]
    fn spacer_stays_out_of_the_way_when_already_centred() {
        assert_eq!(balance_width(554.0, 554.0), None);
        assert_eq!(balance_width(554.0, 553.0), None);
        // Sub-pixel settling must not grow the spacer a pixel a frame.
        assert_eq!(balance_width(554.0, 554.4), None);
    }

    /// The drift is what the dock adds on the sidebar's side of the canvas,
    /// so it has to be at least half the sidebar; the rest is the tools group
    /// being wider than the zoom group.
    #[test]
    fn drift_covers_the_sidebar_the_dock_spans() {
        let drift = 775.5 - (14.0 + 1094.0) / 2.0;
        assert!(drift >= TOOL_SIDEBAR_WIDTH as f64 / 2.0);
        assert!(drift < TOOL_SIDEBAR_WIDTH as f64);
    }

    /// The thumb's travel maths assumes the box is the size 07.css gives it, so
    /// pin the two against each other: a stylesheet tweak that silently
    /// desyncs them makes the thumb jump, stop short of the pill's end, or
    /// stop reading as a handle at all.
    #[test]
    fn thumb_geometry_matches_the_stylesheet() {
        let css = include_str!("../../ui_support_css/07.css");
        let thumb = css_rule(css, ".recording-editor-root .recording-editor-timeline-zoom-bar");
        assert!(thumb.contains(&format!("min-width: {ZOOM_BAR_WIDTH}px;")));
        assert!(thumb.contains(&format!("min-height: {ZOOM_BAR_MIN_HEIGHT}px;")));
        // Rounded ends: the radius is half the width, so the thumb reads as a
        // handle rather than a bar with clipped corners.
        assert!(thumb.contains(&format!("border-radius: {}px;", ZOOM_BAR_WIDTH / 2)));

        let pill = css_rule(css, ".recording-editor-root .recording-editor-timeline-zoom-pill");
        // The pill is the thumb plus its inset on each side, which is what
        // makes the thumb float inside it instead of touching its edges.
        assert!(pill.contains(&format!(
            "min-height: {}px;",
            ZOOM_BAR_MIN_HEIGHT + ZOOM_BAR_INSET * 2
        )));
    }

    #[test]
    fn zoom_thumb_floats_inside_the_pill() {
        // At the stylesheet's pill height the thumb is inset, not edge to edge.
        assert_eq!(
            zoom_thumb_height(ZOOM_BAR_MIN_HEIGHT + ZOOM_BAR_INSET * 2),
            ZOOM_BAR_MIN_HEIGHT
        );
        // A taller track keeps the inset all round rather than a fixed height.
        assert_eq!(zoom_thumb_height(56), 50);
        // Degenerate tracks clamp to the floor instead of pinching to nothing.
        assert_eq!(zoom_thumb_height(4), ZOOM_BAR_MIN_HEIGHT);
    }

    #[test]
    fn moving_a_clip_hides_the_extend_band_until_the_pointer_releases_it() {
        let shell = include_str!("shell.rs");
        let start = shell
            .find("extend_layer.set_draw_func({")
            .expect("the extend layer's draw callback");
        let draw = {
            let rest = &shell[start + 1..];
            let end = rest
                .find("\n    });")
                .expect("the draw callback spans one closure argument");
            &shell[start..start + 1 + end]
        };
        assert!(
            draw.contains("let dragging_video = dragging_video.clone();"),
            "the extend layer's draw callback must watch the clip drag it shares with the lanes"
        );
        let guard = draw
            .find("if dragging_video.get().is_some() {")
            .expect("a moving clip must suppress the extend band");
        assert!(
            guard < draw.find("draw_extend_region(").expect("the band draw"),
            "the guard must return before anything is painted"
        );

        let interaction = include_str!("interaction.rs");
        let start = interaction
            .find("pub fn bind_video_clip(")
            .expect("the video clip's gesture bindings");
        let bind = {
            let rest = &interaction[start + 1..];
            let end = rest
                .find("\n    area.add_controller(drag);")
                .expect("the drag controller is added once both ends are bound");
            &interaction[start..start + 1 + end]
        };
        let begin_start = bind
            .find("drag.connect_drag_begin({")
            .expect("the drag's begin handler");
        let begin = &bind[begin_start..];
        assert!(
            begin.contains("let redraw = redraw.clone();"),
            "the begin handler must repaint, so the band goes as the clip starts moving"
        );
        assert!(
            begin.contains("dragging.set(if lift { hit.segment } else { None });"),
            "a body drag must set the shared lift the lanes and the band read"
        );
        let lift = begin
            .find("dragging.set(if lift { hit.segment } else { None });")
            .expect("the lift assignment");
        assert!(
            lift < begin.find("drop(guard);").expect("the state lock released"),
            "the state lock must be released before the repaint takes it again"
        );
        assert!(
            begin.find("drop(guard);").expect("the state lock released")
                < begin.find("redraw();").expect("the begin repaint"),
            "releasing the lock before redraw is what keeps the repaint from deadlocking"
        );

        let end_start = bind
            .find("drag.connect_drag_end({")
            .expect("the drag's end handler");
        let end = &bind[end_start..];
        assert!(
            end.find("dragging.set(None);").expect("the lift cleared")
                < end.find("redraw();").expect("the end repaint"),
            "the release must clear the lift before the last repaint restores the band"
        );
    }

    #[test]
    fn a_right_click_targets_the_clip_under_it_and_ignores_the_empty_band() {
        let interaction = include_str!("interaction.rs");
        let production = interaction
            .split("#[cfg(test)]")
            .next()
            .expect("the interaction source");
        let start = production
            .find("pub fn bind_video_clip(")
            .expect("the video clip's gesture bindings");
        let bind = {
            let rest = &production[start + 1..];
            let end = rest
                .find("\npub fn bind_zoom_track(")
                .expect("the video bindings end where the zoom lane's begin");
            &production[start..start + 1 + end]
        };
        let menu_start = bind
            .find("menu.set_button(3);")
            .expect("the video lane's right-click binding");
        let menu = &bind[menu_start..];
        assert!(
            menu.contains("video_layout(&guard, width)"),
            "the target must come from the drawn segment rectangles"
        );
        assert!(
            !menu.contains("video_hit("),
            "the extender handle in video_hit would claim the last segment from the empty band"
        );
        assert!(
            menu.contains("x >= x0 && x <= x1"),
            "the rectangle test must be inclusive so a clip edge still targets its clip"
        );
        let locked = menu
            .find("if guard.video_locked {")
            .expect("a locked video offers no menu");
        let hit = menu
            .find("video_layout(&guard, width)")
            .expect("the segment hit test");
        assert!(
            locked < hit,
            "the lock check must come before anything is selected or opened"
        );
        assert!(
            menu.contains("ClipMenuTarget::Video(index)"),
            "the menu must open on the clicked video segment"
        );
    }

    #[test]
    fn the_player_seeks_the_retained_source_and_not_the_raw_timeline() {
        let interaction = include_str!("interaction.rs");
        let production = interaction
            .split("#[cfg(test)]")
            .next()
            .expect("the interaction source");
        let toggle_start = production
            .find("pub fn toggle_playback(")
            .expect("the play toggle");
        let toggle = {
            let rest = &production[toggle_start + 1..];
            let end = rest
                .find("\npub fn pause_playback(")
                .expect("the play toggle ends at the pause control");
            &production[toggle_start..toggle_start + 1 + end]
        };
        assert!(
            toggle.contains("guard.playback_position(replay)"),
            "starting playback must resolve a retained source position"
        );
        assert!(
            !toggle.contains("guard.source_playhead()"),
            "the scrubbing fallback would play deleted footage"
        );
        assert!(
            toggle.contains("playing.set(false);"),
            "a composition with nothing retained must start paused"
        );

        let tick_start = production
            .find("pub fn tick_playback(")
            .expect("the playback tick");
        let tick = {
            let rest = &production[tick_start + 1..];
            let end = rest
                .find("\nfn stop_playback_at_end(")
                .expect("the tick ends at the end-of-playback control");
            &production[tick_start..tick_start + 1 + end]
        };
        let resolved = tick
            .find("playback_position(guard.playhead_seconds)")
            .expect("the tick resolves the retained position before it advances");
        let stopped = tick
            .find("stop_playback_at_end(state, media, playing, play_button, redraw);")
            .expect("a composition with nothing retained must stop");
        assert!(
            resolved < stopped,
            "the no-retained-footage stop must precede any playback work"
        );
        assert!(
            tick.contains("next += (actual.unwrap_or(source_t) - source_t).max(0.0);"),
            "the advance must add the media delta to the resolved logical position"
        );
        assert!(
            !tick.contains("source_to_timeline(seconds)"),
            "mapping a raw source time can jump straight into a deleted range"
        );
        assert!(
            tick.contains("usable_media_timestamp_seconds("),
            "the drift check must read the media's own timestamp"
        );
        assert!(
            tick.contains("let off_source = actual.is_none_or(|seconds| (seconds - seek_to).abs() > 0.05);"),
            "normal playback may only seek off its expected source by more than the tolerance"
        );
        assert!(
            tick.contains("if is_ended || off_source {"),
            "native EOF must seek the retained source instead of ending playback"
        );
        assert!(
            tick.contains("if !is_playing || is_ended {"),
            "playback resumes the media only when it stalled or ran out"
        );
        assert!(
            !tick.contains("if media_file.is_ended() {\n            stop_playback_at_end"),
            "EOF may not stop playback while retained footage remains"
        );
        assert!(
            !production.contains("playback_restart_source"),
            "the mixed-coordinate restart heuristic is gone"
        );
    }

    #[test]
    fn every_lane_resizes_from_the_anchor_captured_at_drag_begin() {
        let interaction = include_str!("interaction.rs");
        let production = interaction
            .split("#[cfg(test)]")
            .next()
            .expect("the interaction source");
        for (lane, drag) in [
            ("pub fn bind_video_clip(", "ClipDrag"),
            ("pub fn bind_zoom_track(", "ZoomDrag"),
            ("pub fn bind_hide_track(", "HideDrag"),
        ] {
            let start = production.find(lane).unwrap_or_else(|| panic!("{lane}"));
            let rest = &production[start + 1..];
            let end = rest.find("\n    area.add_controller(drag);").expect("the drag controller");
            let bind = &production[start..start + 1 + end];
            assert!(
                bind.contains("Rc::new(Cell::new(None::<ClipResizeAnchor>))"),
                "{lane} must hold the anchor captured once at drag begin"
            );
            assert!(
                bind.contains("ClipResizeAnchor::for_video(&guard, width, segment, drag)")
                    || bind.contains("ClipResizeAnchor::for_extend(&guard, width)"),
                "{lane} must capture an anchor for the resize gestures"
            );
            assert!(
                bind.contains("apply_clip_resize(&mut guard, width, &anchor, offset_x)"),
                "{lane} must apply the pointer offset against the captured anchor"
            );
            assert!(
                !bind.contains("time_to_x(anchor"),
                "{lane} must not re-read the mutated timeline while dragging"
            );
            assert!(bind.contains(drag), "{lane} must keep its own drag kinds");
        }
    }

    /// Body of the first rule for `selector`, without its braces.
    fn css_rule<'a>(css: &'a str, selector: &str) -> &'a str {
        css.split(selector)
            .nth(1)
            .expect("rule for selector")
            .split('}')
            .next()
            .expect("rule body")
    }
}
