pub fn build_timeline(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    thumbnails: Vec<PathBuf>,
    waveform: Option<PathBuf>,
    media: MediaFile,
    play_button: Button,
) -> GtkBox {
    {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = 0.0;
    }
    media.seek(0);
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-timeline");
    root.set_hexpand(true);
    root.set_vexpand(false);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-shell");
    card.set_hexpand(true);
    card.set_vexpand(false);

    // Create buttons and modes first
    let cut_button = icon_tool_button(
        "edit-cut-symbolic",
        "Cut mode — click timeline to place cuts",
    );
    let cut_mode = Rc::new(Cell::new(false));

    let move_button = icon_tool_button(
        "view-sort-ascending-symbolic",
        "Move mode — drag a segment to reorder it",
    );
    let move_mode = Rc::new(Cell::new(false));

    // Track which chronological segment index is being dragged (for visual feedback)
    let dragging_segment: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let clip_hovered = Rc::new(Cell::new(false));

    // Wire cut button
    cut_button.connect_clicked({
        let cut_mode = cut_mode.clone();
        let cut_button = cut_button.clone();
        let move_mode = move_mode.clone();
        let move_button = move_button.clone();
        move |_| {
            let enabled = !cut_mode.get();
            cut_mode.set(enabled);
            if enabled {
                cut_button.add_css_class("recording-editor-tool-icon-active");
                move_mode.set(false);
                move_button.remove_css_class("recording-editor-tool-icon-active");
            } else {
                cut_button.remove_css_class("recording-editor-tool-icon-active");
            }
        }
    });

    // Wire move button
    move_button.connect_clicked({
        let move_mode = move_mode.clone();
        let move_button = move_button.clone();
        let cut_mode = cut_mode.clone();
        let cut_button = cut_button.clone();
        move |_| {
            let enabled = !move_mode.get();
            move_mode.set(enabled);
            if enabled {
                cut_mode.set(false);
                cut_button.remove_css_class("recording-editor-tool-icon-active");
                move_button.add_css_class("recording-editor-tool-icon-active");
            } else {
                move_button.remove_css_class("recording-editor-tool-icon-active");
            }
        }
    });

    let revert_button = icon_tool_button("edit-undo-symbolic", "Revert cuts");

    let media_play = media.clone();
    let play_button_ref = play_button.clone();
    let playing = Rc::new(Cell::new(false));
    let finished = Rc::new(Cell::new(false));
    let state_for_play = state.clone();
    // Track which order position is currently playing
    let play_order_pos: Rc<Cell<usize>> = Rc::new(Cell::new(0));

    play_button.connect_clicked({
        let play_order_pos = play_order_pos.clone();
        let finished = finished.clone();
        let playing = playing.clone();
        move |_| {
            let is_playing = playing.get();
            if is_playing {
                media_play.pause();
                playing.set(false);
                let icon = Image::from_icon_name("media-playback-start-symbolic");
                icon.set_pixel_size(18);
                play_button_ref.set_child(Some(&icon));
            } else {
                let s = state_for_play.lock().unwrap();
                let ordered_segs = s.ordered_kept_segments();
                let playhead = s.playhead_seconds;
                drop(s);

                if ordered_segs.is_empty() {
                    return;
                }

                // If finished, restart from the beginning
                if finished.get() {
                    finished.set(false);
                    play_order_pos.set(0);
                    let seek_to = ordered_segs[0].0;
                    {
                        let mut s2 = state_for_play.lock().unwrap();
                        s2.playhead_seconds = seek_to;
                    }
                    media_play.seek((seek_to * 1_000_000.0) as i64);
                    media_play.play();
                    playing.set(true);
                    let icon = Image::from_icon_name("media-playback-pause-symbolic");
                    icon.set_pixel_size(18);
                    play_button_ref.set_child(Some(&icon));
                    return;
                }

                // Find if playhead is inside any ordered segment
                let mut start_pos = 0;
                let mut seek_to = ordered_segs[0].0;
                for (i, &(seg_start, seg_end)) in ordered_segs.iter().enumerate() {
                    if playhead >= seg_start && playhead < seg_end {
                        start_pos = i;
                        seek_to = playhead;
                        break;
                    }
                }

                play_order_pos.set(start_pos);
                {
                    let mut s2 = state_for_play.lock().unwrap();
                    s2.playhead_seconds = seek_to;
                }
                media_play.seek((seek_to * 1_000_000.0) as i64);
                media_play.play();
                playing.set(true);
                let icon = Image::from_icon_name("media-playback-pause-symbolic");
                icon.set_pixel_size(18);
                play_button_ref.set_child(Some(&icon));
            }
        }
    });

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-trim-area");
    overlay.set_hexpand(true);
    overlay.set_vexpand(false);
    overlay.set_overflow(gtk4::Overflow::Hidden);
    overlay.set_size_request(0, 64);

    let filmstrip = DrawingArea::new();
    filmstrip.add_css_class("recording-editor-thumbnail-strip");
    filmstrip.set_hexpand(true);
    filmstrip.set_vexpand(true);
    let film_pixbufs = load_track_pixbufs(&thumbnails);
    filmstrip.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| draw_filmstrip(&state, &film_pixbufs, cr, width, height)
    });
    overlay.set_child(Some(&filmstrip));

    let selection = DrawingArea::new();
    selection.set_hexpand(true);
    selection.set_vexpand(true);
    selection.set_draw_func({
        let state = state.clone();
        let dragging_segment = dragging_segment.clone();
        let clip_hovered = clip_hovered.clone();
        move |_, cr, width, height| {
            draw_trim_overlay(
                &state,
                cr,
                width,
                height,
                dragging_segment.get(),
                clip_hovered.get(),
            );
        }
    });
    overlay.add_overlay(&selection);

    revert_button.connect_clicked({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let cut_mode = cut_mode.clone();
        let cut_button = cut_button.clone();
        let selection = selection.clone();
        move |_| {
            {
                let mut state = state.lock().unwrap();
                state.clear_cuts();
            }
            cut_mode.set(false);
            cut_button.remove_css_class("recording-editor-tool-icon-active");
            selection.queue_draw();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });

    let scroll_adj = Adjustment::new(0.0, 0.0, 1.0, 0.1, 1.0, 1.0);
    let scroll_syncing = Rc::new(Cell::new(false));
    sync_timeline_scroll_adj(&scroll_adj, &state.lock().unwrap(), &scroll_syncing);
    let playhead_layer = DrawingArea::new();
    playhead_layer.add_css_class("recording-editor-playhead-layer");
    playhead_layer.set_hexpand(true);
    playhead_layer.set_vexpand(true);
    playhead_layer.set_can_target(false);
    playhead_layer.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| draw_spanning_playhead(&state, cr, width, height)
    });

    scroll_adj.connect_value_changed({
        let state = state.clone();
        let selection = selection.clone();
        let filmstrip = filmstrip.clone();
        let playhead_layer = playhead_layer.clone();
        let scroll_syncing = scroll_syncing.clone();
        move |adj| {
            if scroll_syncing.get() {
                return;
            }
            let Ok(mut guard) = state.try_lock() else {
                return;
            };
            if (guard.timeline_scroll_seconds - adj.value()).abs() < 1e-4 {
                return;
            }
            guard.set_timeline_scroll(adj.value());
            drop(guard);
            selection.queue_draw();
            filmstrip.queue_draw();
            playhead_layer.queue_draw();
        }
    });

    // Drag gesture for trim handles and playhead
    let drag_kind = Rc::new(RefCell::new(None::<TrimDragKind>));
    let drag_origin_time = Rc::new(Cell::new(0.0));
    let scrubbing = Rc::new(Cell::new(false));
    let pending_seek: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let media = media.clone();
        let selection = selection.clone();
        let estimate_label = estimate_label.clone();
        let cut_mode = cut_mode.clone();
        let move_mode = move_mode.clone();
        let dragging_segment = dragging_segment.clone();
        let scrubbing = scrubbing.clone();
        let drag_origin_time = drag_origin_time.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        move |gesture, x, _| {
            scrubbing.set(true);
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut state_guard = state.lock().unwrap();
            let start_x = state_guard.source_to_x(state_guard.trim_start_seconds, width);
            let end_x = state_guard.source_to_x(state_guard.trim_end_seconds, width);
            let handle_threshold = 18.0;
            let seconds = composition_x_to_source(&state_guard, width, x);

            if state_guard.video_locked {
                pause_playback(&media, &playing, &play_button);
                state_guard.playhead_seconds = seconds;
                media.seek((seconds * 1_000_000.0) as i64);
                selection.queue_draw();
                drag_origin_time.set(seconds);
                *drag_kind.borrow_mut() = Some(TrimDragKind::Playhead);
                return;
            }

            let kind = if !state_guard.cuts.is_empty() {
                let layout = compute_visual_layout(&state_guard, width);
                if let Some(kind) =
                    hit_segment_drag(&state_guard, &layout, x, handle_threshold, move_mode.get())
                {
                    if let TrimDragKind::Segment(opos) = kind {
                        dragging_segment.set(layout.get(opos).map(|entry| entry.0));
                    }
                    kind
                } else if cut_mode.get() {
                    state_guard.add_cut(seconds);
                    *drag_kind.borrow_mut() = None;
                    drop(state_guard);
                    selection.queue_draw();
                    footer::update_estimate(&estimate_label, &state, false);
                    return;
                } else {
                    pause_playback(&media, &playing, &play_button);
                    state_guard.playhead_seconds = seconds;
                    media.seek((seconds * 1_000_000.0) as i64);
                    selection.queue_draw();
                    TrimDragKind::Playhead
                }
            } else if cut_mode.get()
                && (x - start_x).abs() > handle_threshold
                && (x - end_x).abs() > handle_threshold
            {
                state_guard.add_cut(seconds);
                *drag_kind.borrow_mut() = None;
                drop(state_guard);
                selection.queue_draw();
                footer::update_estimate(&estimate_label, &state, false);
                return;
            } else if (x - start_x).abs() <= handle_threshold
                && (x - start_x).abs() <= (x - end_x).abs()
            {
                TrimDragKind::Start
            } else if (x - end_x).abs() <= handle_threshold {
                TrimDragKind::End
            } else if x > start_x && x < end_x {
                TrimDragKind::Range {
                    origin_offset: state_guard.timeline_offset_seconds,
                }
            } else {
                pause_playback(&media, &playing, &play_button);
                state_guard.playhead_seconds = seconds;
                media.seek((seconds * 1_000_000.0) as i64);
                selection.queue_draw();
                TrimDragKind::Playhead
            };
            drag_origin_time.set(match kind {
                TrimDragKind::Start => state_guard.trim_start_seconds,
                TrimDragKind::End => state_guard.trim_end_seconds,
                TrimDragKind::Cut(index) => state_guard.cuts.get(index).copied().unwrap_or(seconds),
                _ => seconds,
            });
            *drag_kind.borrow_mut() = Some(kind);
        }
    });

    let start_label = Label::new(None);
    start_label.add_css_class("recording-editor-time-label");
    start_label.set_xalign(0.0);
    start_label.set_hexpand(true);
    let end_label = Label::new(None);
    end_label.add_css_class("recording-editor-time-label");
    end_label.set_xalign(1.0);
    end_label.set_hexpand(true);
    update_time_labels(&start_label, &end_label, &state);

    drag.connect_drag_update({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let selection = selection.clone();
        let estimate_label = estimate_label.clone();
        let start_label = start_label.clone();
        let end_label = end_label.clone();
        let media = media.clone();
        let dragging_segment = dragging_segment.clone();
        let drag_origin_time = drag_origin_time.clone();
        let filmstrip = filmstrip.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = *drag_kind.borrow() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let value_x = (start_x + offset_x).clamp(0.0, width);
            let mut state_guard = state.lock().unwrap();
            if state_guard.video_locked && !matches!(kind, TrimDragKind::Playhead) {
                return;
            }
            let scale = if state_guard.cuts.is_empty() {
                state_guard.source_duration()
            } else {
                state_guard.kept_duration().max(0.001)
            };
            let (_, span) = state_guard.timeline_view();
            let delta_t = (offset_x / width) * scale * span;
            let origin = drag_origin_time.get();
            let seconds = visual_x_to_source_seconds(&state_guard, width, value_x);
            match kind {
                TrimDragKind::Start => state_guard.set_trim_start(origin + delta_t),
                TrimDragKind::End => state_guard.set_trim_end(origin + delta_t),
                TrimDragKind::Cut(cut_index) => state_guard.move_cut(cut_index, origin + delta_t),
                TrimDragKind::Segment(from_pos) => {
                    // Use visual layout to find nearest segment by visual midpoint
                    let layout = compute_visual_layout(&state_guard, width);
                    let mut best_opos = from_pos;
                    let mut best_dist = f64::MAX;
                    for (opos, &(_, vx_start, vx_end)) in layout.iter().enumerate() {
                        let mid = (vx_start + vx_end) / 2.0;
                        let dist = (value_x - mid).abs();
                        if dist < best_dist {
                            best_dist = dist;
                            best_opos = opos;
                        }
                    }
                    if best_opos != from_pos {
                        state_guard.move_segment(from_pos, best_opos);
                        let new_seg_idx = state_guard.segment_order[best_opos];
                        drop(state_guard);
                        dragging_segment.set(Some(new_seg_idx));
                        *drag_kind.borrow_mut() = Some(TrimDragKind::Segment(best_opos));
                        selection.queue_draw();
                        footer::update_estimate(&estimate_label, &state, false);
                        return;
                    }
                }
                TrimDragKind::Range { origin_offset } => {
                    let move_delta = (offset_x / width) * state_guard.source_duration() * span;
                    state_guard.set_timeline_offset(origin_offset + move_delta);
                    state_guard.follow_clip_on_timeline();
                }
                TrimDragKind::Playhead => {
                    state_guard.playhead_seconds = seconds;
                    media.seek((seconds * 1_000_000.0) as i64);
                }
            }
            if matches!(kind, TrimDragKind::Range { .. }) {
                sync_timeline_scroll_adj(&scroll_adj, &state_guard, &scroll_syncing);
            }
            drop(state_guard);
            selection.queue_draw();
            filmstrip.queue_draw();
            if !matches!(kind, TrimDragKind::Playhead) {
                footer::update_estimate(&estimate_label, &state, false);
            }
            if matches!(
                kind,
                TrimDragKind::Start | TrimDragKind::End | TrimDragKind::Range { .. }
            ) {
                update_time_labels(&start_label, &end_label, &state);
            }
        }
    });
    drag.connect_drag_end({
        let drag_kind = drag_kind.clone();
        let dragging_segment = dragging_segment.clone();
        let selection = selection.clone();
        let filmstrip = filmstrip.clone();
        let scrubbing = scrubbing.clone();
        let pending_seek = pending_seek.clone();
        let state = state.clone();
        move |_, _, _| {
            if matches!(*drag_kind.borrow(), Some(TrimDragKind::Playhead)) {
                pending_seek.set(Some(state.lock().unwrap().playhead_seconds));
            }
            *drag_kind.borrow_mut() = None;
            dragging_segment.set(None);
            scrubbing.set(false);
            selection.queue_draw();
            filmstrip.queue_draw();
        }
    });
    selection.add_controller(drag);

    // Double-click to add a cut point
    let double_click = GestureClick::new();
    double_click.set_button(1);
    let selection_for_dbl = selection.clone();
    let state_for_dbl = state.clone();
    let estimate_for_dbl = estimate_label.clone();
    double_click.connect_pressed(move |gesture, n_press, x, _| {
        if n_press != 2 {
            return;
        }
        let width = gesture
            .widget()
            .and_then(|widget| widget.downcast::<DrawingArea>().ok())
            .map(|area| area.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let seconds = {
            let s = state_for_dbl.lock().unwrap();
            composition_x_to_source(&s, width, x)
        };
        {
            let mut s = state_for_dbl.lock().unwrap();
            s.add_cut(seconds);
        }
        selection_for_dbl.queue_draw();
        footer::update_estimate(&estimate_for_dbl, &state_for_dbl, false);
    });
    selection.add_controller(double_click);

    // Right-click to toggle segment or remove cut
    let right_click = GestureClick::new();
    right_click.set_button(3);
    let selection_for_rc = selection.clone();
    let state_for_rc = state.clone();
    let estimate_for_rc = estimate_label.clone();
    right_click.connect_pressed(move |gesture, _n_press, x, _| {
        let width = gesture
            .widget()
            .and_then(|widget| widget.downcast::<DrawingArea>().ok())
            .map(|area| area.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let mut s = state_for_rc.lock().unwrap();
        let seconds = composition_x_to_source(&s, width, x);

        // Check if near a cut line (remove it)
        let cut_threshold_seconds = {
            let (_, span) = s.timeline_view();
            (12.0 / width) * s.metadata.duration_seconds.max(0.001) * span
        };
        let mut removed_cut = false;
        for i in 0..s.cuts.len() {
            if (s.cuts[i] - seconds).abs() < cut_threshold_seconds {
                s.remove_cut(i);
                removed_cut = true;
                break;
            }
        }

        if !removed_cut {
            // Toggle the segment under the click
            let boundaries = s.segment_boundaries();
            for (i, (seg_start, seg_end)) in boundaries.iter().enumerate() {
                if seconds >= *seg_start && seconds < *seg_end {
                    s.toggle_segment(i);
                    break;
                }
            }
        }
        drop(s);
        selection_for_rc.queue_draw();
        footer::update_estimate(&estimate_for_rc, &state_for_rc, false);
    });
    selection.add_controller(right_click);

    // Cursor hints + reveal trim/move handles while the pointer is on the clip
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let state = state.clone();
        let cut_mode = cut_mode.clone();
        let move_mode = move_mode.clone();
        let clip_hovered = clip_hovered.clone();
        let selection = selection.clone();
        move |controller, x, _| {
            let Some(widget) = controller.widget() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let (cursor_name, hovered) = {
                let state = state.lock().unwrap();
                let start_x = state.source_to_x(state.trim_start_seconds, width);
                let end_x = state.source_to_x(state.trim_end_seconds, width);
                let handle_threshold = 18.0;
                let hovered = if !state.cuts.is_empty() {
                    compute_visual_layout(&state, width)
                        .iter()
                        .any(|(_, start, end)| x >= *start && x <= *end)
                } else {
                    x >= start_x && x <= end_x
                };
                let cursor_name = if !state.cuts.is_empty() {
                    let layout = compute_visual_layout(&state, width);
                    match hit_segment_drag(&state, &layout, x, handle_threshold, move_mode.get()) {
                        Some(TrimDragKind::Start) | Some(TrimDragKind::Cut(_))
                            if layout
                                .iter()
                                .any(|(_, vx, _)| (x - vx).abs() <= handle_threshold) =>
                        {
                            Some("w-resize")
                        }
                        Some(TrimDragKind::End) => Some("e-resize"),
                        Some(TrimDragKind::Cut(_)) => Some("col-resize"),
                        Some(TrimDragKind::Segment(_)) => Some("grab"),
                        _ => None,
                    }
                } else if cut_mode.get()
                    && (x - start_x).abs() > handle_threshold
                    && (x - end_x).abs() > handle_threshold
                {
                    Some("crosshair")
                } else if (x - start_x).abs() <= handle_threshold {
                    Some("w-resize")
                } else if (x - end_x).abs() <= handle_threshold {
                    Some("e-resize")
                } else if x > start_x && x < end_x {
                    Some("grab")
                } else {
                    let cut_threshold = 8.0;
                    let near_cut = state.cuts.iter().any(|&c| {
                        let cx = state.source_to_x(c, width);
                        (x - cx).abs() <= cut_threshold
                    });
                    if near_cut {
                        Some("crosshair")
                    } else {
                        None
                    }
                };
                (cursor_name, hovered)
            };
            if clip_hovered.get() != hovered {
                clip_hovered.set(hovered);
                selection.queue_draw();
            }
            let cursor = cursor_name.and_then(|name| gdk::Cursor::from_name(name, None));
            widget.set_cursor(cursor.as_ref());
        }
    });
    motion.connect_leave({
        let clip_hovered = clip_hovered.clone();
        let selection = selection.clone();
        move |controller| {
            if clip_hovered.get() {
                clip_hovered.set(false);
                selection.queue_draw();
            }
            if let Some(widget) = controller.widget() {
                widget.set_cursor(None);
            }
        }
    });
    selection.add_controller(motion);

    // Periodically sync playhead — follow ordered segment sequence during playback
    let media_playhead = media.clone();
    let selection_playhead = selection.clone();
    let state_playhead = state.clone();
    let play_order_pos_timer = play_order_pos.clone();
    let finished_timer = finished.clone();
    let playing_timer = playing.clone();
    let play_button_timer = play_button.clone();
    let scrubbing_timer = scrubbing.clone();
    let pending_seek_timer = pending_seek.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if scrubbing_timer.get() {
            selection_playhead.queue_draw();
            return glib::ControlFlow::Continue;
        }
        if media_playhead.is_playing() {
            let ts_us = media_playhead.timestamp();
            if ts_us > 0 {
                let seconds = ts_us as f64 / 1_000_000.0;

                // If we're waiting for a seek to land, check if we're close enough
                if let Some(target) = pending_seek_timer.get() {
                    if (seconds - target).abs() > 0.5 {
                        // Media hasn't reached the seek target yet, skip this tick
                        selection_playhead.queue_draw();
                        return glib::ControlFlow::Continue;
                    }
                    // Seek landed, clear the flag
                    pending_seek_timer.set(None);
                }

                let mut seek_target = None;
                let mut should_pause = false;
                {
                    let mut s = state_playhead.lock().unwrap();
                    s.playhead_seconds = seconds;

                    let ordered_segs = s.ordered_kept_segments();
                    let current_pos = play_order_pos_timer.get();

                    if let Some(&(_seg_start, seg_end)) = ordered_segs.get(current_pos) {
                        if seconds >= seg_end - 0.08 {
                            // Current segment ended, advance to next in order
                            let next_pos = current_pos + 1;
                            if let Some(&(next_start, _)) = ordered_segs.get(next_pos) {
                                play_order_pos_timer.set(next_pos);
                                s.playhead_seconds = next_start;
                                seek_target = Some(next_start);
                            } else {
                                should_pause = true;
                            }
                        }
                        // Don't force-seek if before seg_start — let pending_seek handle it
                    } else if !ordered_segs.is_empty() {
                        let (first_start, _) = ordered_segs[0];
                        play_order_pos_timer.set(0);
                        s.playhead_seconds = first_start;
                        seek_target = Some(first_start);
                    } else {
                        should_pause = true;
                    }
                }
                if let Some(target) = seek_target {
                    pending_seek_timer.set(Some(target));
                    media_playhead.seek((target * 1_000_000.0) as i64);
                } else if should_pause {
                    media_playhead.pause();
                    finished_timer.set(true);
                    playing_timer.set(false);
                    let icon = Image::from_icon_name("media-playlist-repeat-symbolic");
                    icon.set_pixel_size(18);
                    play_button_timer.set_child(Some(&icon));
                }
                selection_playhead.queue_draw();
            }
        }
        glib::ControlFlow::Continue
    });

    let delete_button = icon_tool_button("edit-delete-symbolic", "Delete zoom or cut");
    delete_button.connect_clicked({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        let selection = selection.clone();
        move |_| {
            {
                let mut guard = state.lock().unwrap();
                if guard.selected_zoom.is_some() {
                    guard.remove_selected_zoom();
                } else if !guard.cuts.is_empty() {
                    let playhead = guard.playhead_seconds;
                    if let Some(index) = nearest_cut_index(&guard, playhead, 0.4) {
                        guard.remove_cut(index);
                    }
                }
            }
            selection.queue_draw();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });
    let add_zoom = icon_tool_button("zoom-in-symbolic", "Add zoom at playhead");
    add_zoom.connect_clicked({
        let state = state.clone();
        let estimate_label = estimate_label.clone();
        move |_| {
            state.lock().unwrap().add_zoom_at_playhead();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });

    let redo_button = icon_tool_button("edit-redo-symbolic", "Redo");
    redo_button.set_sensitive(false);

    let transport = GtkBox::new(Orientation::Horizontal, 4);
    transport.add_css_class("recording-editor-transport");
    transport.set_hexpand(true);
    transport.set_valign(Align::Center);

    let transport_gutter = GtkBox::new(Orientation::Horizontal, 0);
    transport_gutter.add_css_class("recording-editor-track-header");
    transport_gutter.add_css_class("recording-editor-transport-gutter");
    transport_gutter.set_size_request(RAIL_HEADER_WIDTH, -1);
    transport_gutter.set_hexpand(false);
    transport_gutter.set_halign(Align::Start);

    let transport_tools = GtkBox::new(Orientation::Horizontal, 8);
    transport_tools.add_css_class("recording-editor-transport-tools");
    transport_tools.set_halign(Align::Start);
    transport_tools.set_hexpand(true);
    transport_tools.append(&revert_button);
    transport_tools.append(&redo_button);
    transport_tools.append(&delete_button);
    transport_tools.append(&cut_button);
    transport_tools.append(&move_button);
    transport_tools.append(&add_zoom);

    let zoom_out = icon_tool_button("zoom-out-symbolic", "Zoom out timeline");
    let zoom_in = icon_tool_button("zoom-in-symbolic", "Zoom in timeline");
    let timeline_scale = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 10.0);
    timeline_scale.add_css_class("recording-editor-timeline-scale");
    timeline_scale.set_draw_value(false);
    timeline_scale.set_hexpand(true);
    timeline_scale.set_value(0.0);
    timeline_scale.set_size_request(72, 12);
    timeline_scale.set_valign(Align::Center);
    timeline_scale.set_vexpand(false);

    let transport_scale = GtkBox::new(Orientation::Horizontal, 6);
    transport_scale.add_css_class("recording-editor-timeline-scale-control");
    transport_scale.set_halign(Align::End);
    transport_scale.set_hexpand(false);
    transport_scale.append(&zoom_out);
    transport_scale.append(&timeline_scale);
    transport_scale.append(&zoom_in);

    transport.append(&transport_gutter);
    transport.append(&transport_tools);
    transport.append(&transport_scale);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 28);
    ruler.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| draw_ruler(&state, cr, width, height)
    });
    let ruler_drag = GestureDrag::new();
    ruler_drag.set_button(1);
    ruler_drag.connect_drag_begin({
        let state = state.clone();
        let media = media.clone();
        let scrubbing = scrubbing.clone();
        let playing = playing.clone();
        let play_button = play_button.clone();
        move |gesture, x, _| {
            pause_playback(&media, &playing, &play_button);
            scrubbing.set(true);
            seek_from_x(&state, &media, gesture.widget().as_ref(), x);
        }
    });
    ruler_drag.connect_drag_update({
        let state = state.clone();
        let media = media.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            seek_from_x(
                &state,
                &media,
                gesture.widget().as_ref(),
                start_x + offset_x,
            );
        }
    });
    ruler_drag.connect_drag_end({
        let scrubbing = scrubbing.clone();
        let pending_seek = pending_seek.clone();
        let state = state.clone();
        move |_, _, _| {
            pending_seek.set(Some(state.lock().unwrap().playhead_seconds));
            scrubbing.set(false);
        }
    });
    ruler.add_controller(ruler_drag);

    let video_header = build_rail_header(RailKind::Video);
    let (video_row, video_body) = track_row(Some(&video_header), &overlay);
    video_row.add_css_class("recording-editor-empty-track-row");

    let waveform_body = build_waveform_body(&state, waveform);
    let audio_header = build_rail_header(RailKind::Audio);
    let (audio_row, audio_body) = track_row(Some(&audio_header), &waveform_body);

    let zoom_body = build_zoom_track(state.clone(), estimate_label.clone());
    let zoom_header = build_rail_header(RailKind::Zoom);
    let (zoom_row, zoom_track_body) = track_row(Some(&zoom_header), &zoom_body);

    let tracks = GtkBox::new(Orientation::Vertical, 6);
    tracks.add_css_class("recording-editor-tracks");
    tracks.set_hexpand(true);
    let (ruler_row, _) = track_row(None, &ruler);
    let extra_video_tracks = GtkBox::new(Orientation::Vertical, 6);
    extra_video_tracks.add_css_class("recording-editor-extra-video-tracks");
    extra_video_tracks.set_hexpand(true);

    tracks.append(&ruler_row);
    tracks.append(&video_row);
    tracks.append(&extra_video_tracks);
    tracks.append(&audio_row);
    tracks.append(&zoom_row);

    let refresh_rails = {
        let state = state.clone();
        let media = media.clone();
        let video_header = video_header.clone();
        let audio_header = audio_header.clone();
        let zoom_header = zoom_header.clone();
        let audio_row = audio_row.clone();
        let zoom_row = zoom_row.clone();
        let extra_video_tracks = extra_video_tracks.clone();
        let video_body = video_body.clone();
        let audio_body = audio_body.clone();
        let zoom_track_body = zoom_track_body.clone();
        let selection = selection.clone();
        let estimate_label = estimate_label.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            apply_rail_state(
                &guard,
                &media,
                &video_header,
                &audio_header,
                &zoom_header,
                &audio_row,
                &zoom_row,
                &video_body,
                &audio_body,
                &zoom_track_body,
            );
            let extras: Vec<ProjectMedia> =
                guard.extra_video_tracks().into_iter().cloned().collect();
            drop(guard);
            rebuild_extra_video_tracks(&extra_video_tracks, &extras, &state, &estimate_label);
            selection.queue_draw();
            footer::update_estimate(&estimate_label, &state, false);
        })
    };
    refresh_rails();

    let last_video_sig = Rc::new(Cell::new(video_track_signature(&state)));
    glib::timeout_add_local(std::time::Duration::from_millis(400), {
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move || {
            let sig = video_track_signature(&state);
            if sig != last_video_sig.get() {
                last_video_sig.set(sig);
                refresh_rails();
            }
            glib::ControlFlow::Continue
        }
    });

    video_header.lock.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            let mut guard = state.lock().unwrap();
            guard.video_locked = !guard.video_locked;
            drop(guard);
            refresh_rails();
        }
    });
    if let Some(hide) = video_header.hide.clone() {
        hide.connect_clicked({
            let state = state.clone();
            let refresh_rails = refresh_rails.clone();
            move |_| {
                let mut guard = state.lock().unwrap();
                guard.video_hidden = !guard.video_hidden;
                drop(guard);
                refresh_rails();
            }
        });
    }
    if let Some(mute) = video_header.mute.clone() {
        mute.connect_clicked({
            let state = state.clone();
            let refresh_rails = refresh_rails.clone();
            move |_| {
                state.lock().unwrap().toggle_mute();
                refresh_rails();
            }
        });
    }
    video_header.delete.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            state.lock().unwrap().reset_video_edits();
            refresh_rails();
        }
    });

    audio_header.lock.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            let mut guard = state.lock().unwrap();
            guard.audio_locked = !guard.audio_locked;
            drop(guard);
            refresh_rails();
        }
    });
    if let Some(mute) = audio_header.mute.clone() {
        mute.connect_clicked({
            let state = state.clone();
            let refresh_rails = refresh_rails.clone();
            move |_| {
                state.lock().unwrap().toggle_mute();
                refresh_rails();
            }
        });
    }
    audio_header.delete.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            state.lock().unwrap().remove_audio_track();
            refresh_rails();
        }
    });

    zoom_header.lock.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            let mut guard = state.lock().unwrap();
            guard.zoom_locked = !guard.zoom_locked;
            drop(guard);
            refresh_rails();
        }
    });
    if let Some(hide) = zoom_header.hide.clone() {
        hide.connect_clicked({
            let state = state.clone();
            let refresh_rails = refresh_rails.clone();
            move |_| {
                let mut guard = state.lock().unwrap();
                guard.zoom_hidden = !guard.zoom_hidden;
                drop(guard);
                refresh_rails();
            }
        });
    }
    zoom_header.delete.connect_clicked({
        let state = state.clone();
        let refresh_rails = refresh_rails.clone();
        move |_| {
            state.lock().unwrap().clear_zoom_clips();
            refresh_rails();
        }
    });

    add_zoom.connect_clicked({
        let refresh_rails = refresh_rails.clone();
        move |_| refresh_rails()
    });

    let tracks_overlay = Overlay::new();
    tracks_overlay.set_hexpand(true);
    tracks_overlay.set_child(Some(&tracks));
    tracks_overlay.add_overlay(&playhead_layer);

    {
        let ruler = ruler.clone();
        let playhead_layer = playhead_layer.clone();
        let filmstrip = filmstrip.clone();
        let waveform_body = waveform_body.clone();
        let extra_video_tracks = extra_video_tracks.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        let state = state.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            sync_timeline_scroll_adj(&scroll_adj, &state.lock().unwrap(), &scroll_syncing);
            ruler.queue_draw();
            playhead_layer.queue_draw();
            filmstrip.queue_draw();
            waveform_body.queue_draw();
            queue_draw_tree(&extra_video_tracks);
            glib::ControlFlow::Continue
        });
    }

    let board = Overlay::new();
    board.add_css_class("recording-editor-timeline-board");
    board.set_hexpand(true);
    board.set_vexpand(false);
    let board_inner = GtkBox::new(Orientation::Vertical, 0);
    board_inner.append(&transport);
    board_inner.append(&tracks_overlay);
    let well = GtkBox::new(Orientation::Vertical, 0);
    well.add_css_class("recording-editor-timeline-well");
    well.set_hexpand(true);
    let hbar = Scrollbar::new(Orientation::Horizontal, Some(&scroll_adj));
    hbar.add_css_class("recording-editor-timeline-scroll");
    hbar.set_hexpand(true);
    hbar.set_margin_start(RAIL_HEADER_WIDTH + TRACK_GAP as i32);
    well.append(&hbar);
    board_inner.append(&well);

    let wheel = EventControllerScroll::new(
        EventControllerScrollFlags::VERTICAL | EventControllerScrollFlags::HORIZONTAL,
    );
    wheel.connect_scroll({
        let state = state.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        let selection = selection.clone();
        let filmstrip = filmstrip.clone();
        let playhead_layer = playhead_layer.clone();
        move |_, dx, dy| {
            let delta = if dx.abs() > f64::EPSILON { dx } else { dy };
            if delta.abs() < f64::EPSILON {
                return glib::Propagation::Proceed;
            }
            let mut guard = state.lock().unwrap();
            if guard.timeline_scale > 0.001 {
                return glib::Propagation::Proceed;
            }
            let step = guard.visible_span_seconds() * 0.08 * delta;
            let next = guard.timeline_scroll_seconds + step;
            guard.set_timeline_scroll(next);
            sync_timeline_scroll_adj(&scroll_adj, &guard, &scroll_syncing);
            drop(guard);
            selection.queue_draw();
            filmstrip.queue_draw();
            playhead_layer.queue_draw();
            glib::Propagation::Stop
        }
    });
    tracks_overlay.add_controller(wheel);
    board.set_child(Some(&board_inner));
    board.add_overlay(&rail_divider());

    let redraw_timeline = {
        let selection = selection.clone();
        let ruler = ruler.clone();
        let playhead_layer = playhead_layer.clone();
        let filmstrip = filmstrip.clone();
        let waveform_body = waveform_body.clone();
        let extra_video_tracks = extra_video_tracks.clone();
        Rc::new(move || {
            selection.queue_draw();
            ruler.queue_draw();
            playhead_layer.queue_draw();
            filmstrip.queue_draw();
            waveform_body.queue_draw();
            queue_draw_tree(&extra_video_tracks);
        })
    };
    timeline_scale.connect_value_changed({
        let state = state.clone();
        let redraw_timeline = redraw_timeline.clone();
        let scroll_adj = scroll_adj.clone();
        let scroll_syncing = scroll_syncing.clone();
        move |scale| {
            let mut guard = state.lock().unwrap();
            guard.timeline_scale = scale.value().clamp(0.0, 100.0);
            let scroll = guard.timeline_scroll_seconds;
            guard.set_timeline_scroll(scroll);
            sync_timeline_scroll_adj(&scroll_adj, &guard, &scroll_syncing);
            drop(guard);
            redraw_timeline();
        }
    });
    zoom_out.connect_clicked({
        let timeline_scale = timeline_scale.clone();
        move |_| {
            timeline_scale.set_value((timeline_scale.value() - 10.0).max(0.0));
        }
    });
    zoom_in.connect_clicked({
        let timeline_scale = timeline_scale.clone();
        move |_| {
            timeline_scale.set_value((timeline_scale.value() + 10.0).min(100.0));
        }
    });

    card.append(&board);
    root.append(&card);
    root
}
