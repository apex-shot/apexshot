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
        let duration_clock = Label::new(Some(&format_clock(guard.source_duration())));
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
        &t("Detect automatic zooms from clicks and cursor motion"),
    );
    let analyzing = Rc::new(Cell::new(false));

    let zoom_out = icon_button("zoom-out-symbolic", &t("Zoom out timeline"));
    let zoom_in = icon_button("zoom-in-symbolic", &t("Zoom in timeline"));
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
    // `centre_transport_over_canvas`). It sits ahead of the zoom row so that
    // row keeps hugging the window edge.
    let balance = GtkBox::new(Orientation::Horizontal, 0);
    balance.set_size_request(0, -1);
    right.append(&balance);
    right.append(&zoom_out);
    right.append(&zoom_scale);
    right.append(&zoom_in);

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

    let tracks = GtkBox::new(Orientation::Vertical, 10);
    tracks.add_css_class("recording-editor-card-tracks");
    tracks.set_hexpand(true);
    tracks.append(&ruler);
    tracks.append(&video_track);
    tracks.append(&zoom_track);
    tracks.append(&hide_track);

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
                duration_clock.set_text(&format_clock(guard.source_duration()));
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
                let changed = guard.redetect_zoom_clips();
                if changed {
                    crate::recording::editor::project::persist_video_session(&guard);
                }
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
                if changed {
                    redraw();
                }
                if auto_zooms == 0 {
                    let message = if manual_zooms > 0 {
                        t("No Auto Zooms were added. Manual zooms are preserved, and overlapping detections are skipped.")
                    } else {
                        t("No clear clicks or purposeful pointer pauses were found.")
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
                                "Detect automatic zooms from clicks and cursor motion",
                            )));
                            return glib::ControlFlow::Break;
                        }
                        let samples = sidecar.pointer.len();
                        guard.sidecar = Some(sidecar);
                        if guard.redetect_zoom_clips() {
                            crate::recording::editor::project::persist_video_session(&guard);
                        }
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
                        analyzing.set(false);
                        button.set_tooltip_text(Some(&t(
                            "Detect automatic zooms from clicks and cursor motion",
                        )));
                        redraw();
                        if auto_zooms > 0 {
                            crate::utils::notify::desktop_notification(
                                &t("Auto Zoom detection complete"),
                                &tfmt(
                                    "Added {auto_zooms} Auto Zooms from {samples} cursor-motion samples.",
                                    &[
                                        ("auto_zooms", &auto_zooms.to_string()),
                                        ("samples", &samples.to_string()),
                                    ],
                                ),
                            );
                        } else {
                            let message = if manual_zooms > 0 {
                                t("Cursor motion was found, but Manual zooms already cover the detected moments.")
                            } else {
                                t("Cursor motion was found, but no purposeful pauses were clear enough to place zooms.")
                            };
                            crate::utils::notify::desktop_notification(
                                &t("No Auto Zooms added"),
                                &message,
                            );
                        }
                        glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        analyzing.set(false);
                        button.set_sensitive(true);
                        button.set_tooltip_text(Some(&t(
                            "Detect automatic zooms from clicks and cursor motion",
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
                            "Detect automatic zooms from clicks and cursor motion",
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
    bind_hide_track(
        &hide_track,
        state.clone(),
        media.clone(),
        hovered_hide.clone(),
        hover_hide_time.clone(),
        dragging_hide.clone(),
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

#[cfg(test)]
mod tests {
    use super::balance_width;
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
}
