pub fn toggle_playback(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    redraw: &Rc<dyn Fn()>,
) {
    if playing.get() {
        pause_playback(media, playing, play_button, &redraw);
        return;
    }

    let (seek_to, speed, muted) = {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds =
            playhead_for_replay(guard.playhead_seconds, guard.content_end_seconds());
        let seek_to = guard.source_playhead();
        (
            seek_to,
            guard.speed_for_source(seek_to),
            guard.muted_for_source(seek_to),
        )
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.pause();
        media_file.seek((seek_to * 1_000_000.0) as i64);
        media_file.set_muted(muted);
        if (speed - 1.0).abs() <= 1e-6 {
            media_file.play();
        }
    }
    playing.set(true);
    set_play_icon(play_button, "media-playback-pause-symbolic");
    redraw();
}

pub fn pause_playback(
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    refresh: &Rc<dyn Fn()>,
) {
    if !playing.get() && !media.borrow().as_ref().is_some_and(|media| media.is_playing()) {
        return;
    }
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.pause();
    }
    playing.set(false);
    set_play_icon(play_button, "media-playback-start-symbolic");
    refresh();
}

pub fn set_play_icon(button: &Button, icon_name: &str) {
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
}

pub fn nudge_playhead(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    delta: f64,
    redraw: &Rc<dyn Fn()>,
) {
    let seek_to = {
        let mut guard = state.lock().unwrap();
        let max_t = guard.content_end_seconds().max(guard.source_duration());
        let next = (guard.playhead_seconds + delta).clamp(0.0, max_t);
        guard.playhead_seconds = next;
        guard.source_playhead()
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.seek((seek_to * 1_000_000.0) as i64);
    }
    redraw();
}

pub fn tick_playback(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    redraw: &Rc<dyn Fn()>,
) {
    if !playing.get() {
        return;
    }

    if let Some(media_file) = media.borrow().as_ref() {
        if media_file.is_seeking() {
            redraw();
            return;
        }
        if media_file.is_ended() {
            stop_playback_at_end(state, media, playing, play_button, redraw);
            return;
        }
    }

    let (mut next, end, speed, source_t) = {
        let guard = state.lock().unwrap();
        let source_t = guard.source_playhead();
        (
            guard.playhead_seconds,
            guard.content_end_seconds(),
            guard.speed_for_source(source_t),
            source_t,
        )
    };
    let mut reached_end = false;
    let drive_seek = (speed - 1.0).abs() > 1e-6;

    if drive_seek {
        next += 0.05;
        if let Some(media_file) = media.borrow().as_ref() {
            if media_file.is_playing() {
                media_file.pause();
            }
        }
    } else if let Some(media_file) = media.borrow().as_ref() {
        if media_file.is_playing() {
            if let Some(seconds) =
                usable_media_timestamp_seconds(media_file.timestamp(), media_file.is_seeking())
            {
                next = state.lock().unwrap().source_to_timeline(seconds);
            }
        } else {
            media_file.play();
        }
    } else {
        next += 0.05;
    }

    if next >= end {
        next = end;
        reached_end = true;
    }
    let (muted, seek_to) = {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = next;
        let seek_to = guard.source_playhead();
        (guard.muted_for_source(seek_to), seek_to)
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.set_muted(muted);
        if drive_seek && (seek_to - source_t).abs() > 1e-4 {
            media_file.seek((seek_to * 1_000_000.0) as i64);
        }
    }
    if reached_end {
        stop_playback_at_end(state, media, playing, play_button, redraw);
        return;
    }
    redraw();
}

fn stop_playback_at_end(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    redraw: &Rc<dyn Fn()>,
) {
    playing.set(false);
    {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = guard.content_end_seconds();
    }
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.pause();
    }
    set_play_icon(play_button, "media-playback-start-symbolic");
    redraw();
}

pub fn bind_board_hover(
    tracks: &GtkBox,
    state: Arc<Mutex<VideoEditState>>,
    hover_time: Rc<Cell<Option<f64>>>,
    playhead: DrawingArea,
) {
    let motion = EventControllerMotion::new();
    motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
    motion.connect_motion({
        let state = state.clone();
        let hover_time = hover_time.clone();
        let playhead = playhead.clone();
        move |controller, x, _| {
            let width = controller
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let time = {
                let guard = state.lock().unwrap();
                guard.x_to_time(x.clamp(0.0, width), width).max(0.0)
            };
            hover_time.set(Some(time));
            playhead.queue_draw();
        }
    });
    motion.connect_leave({
        let hover_time = hover_time.clone();
        let playhead = playhead.clone();
        move |_| {
            hover_time.set(None);
            playhead.queue_draw();
        }
    });
    tracks.add_controller(motion);
}

pub fn bind_playhead_drag(
    area: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    redraw: Rc<dyn Fn()>,
) {
    let dragging = Rc::new(Cell::new(false));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let dragging = dragging.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            dragging.set(near_playhead(&state.lock().unwrap(), width, x));
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let media = media.clone();
        let dragging = dragging.clone();
        let redraw = redraw.clone();
        move |gesture, offset_x, _| {
            if !dragging.get() {
                return;
            }
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            seek_to_x(&state, &media, width, start_x + offset_x);
            redraw();
        }
    });
    area.add_controller(drag);
}

pub fn bind_video_clip(
    area: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    hover: Rc<Cell<Option<usize>>>,
    dragging: Rc<Cell<Option<usize>>>,
    redraw: Rc<dyn Fn()>,
) {
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let state = state.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                return;
            }
            let hit = video_hit(&guard, width, x);
            select_video(&mut guard, hit.segment);
            drop(guard);
            redraw();
        }
    });
    area.add_controller(click);

    let drag_kind = Rc::new(Cell::new(None::<ClipDrag>));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let dragging = dragging.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                dragging.set(None);
                drag_kind.set(Some(ClipDrag::Seek));
                return;
            }
            let hit = video_hit(&guard, width, x);
            if let Some(seg) = hit.segment {
                select_video(&mut guard, Some(seg));
            }
            let lift = matches!(
                hit.drag,
                Some(ClipDrag::Move { .. }) | Some(ClipDrag::Segment { .. })
            );
            dragging.set(if lift { hit.segment } else { None });
            drag_kind.set(hit.drag);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let media = media.clone();
        let drag_kind = drag_kind.clone();
        let redraw = redraw.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let x = start_x + offset_x;
            match drag_kind.get() {
                Some(ClipDrag::Start) => {
                    let mut guard = state.lock().unwrap();
                    let seconds =
                        snap_source_to_playhead(&guard, width, x_to_source(&guard, width, x));
                    guard.set_trim_start(seconds);
                }
                Some(ClipDrag::End) => {
                    let mut guard = state.lock().unwrap();
                    let seconds =
                        snap_source_to_playhead(&guard, width, x_to_source(&guard, width, x));
                    guard.set_trim_end(seconds);
                }
                Some(ClipDrag::Cut(index)) => {
                    let mut guard = state.lock().unwrap();
                    let seconds =
                        snap_source_to_playhead(&guard, width, x_to_source(&guard, width, x));
                    guard.move_cut(index, seconds);
                }
                Some(ClipDrag::Move {
                    origin_offset,
                    pixels_per_second,
                }) => {
                    let mut guard = state.lock().unwrap();
                    let delta = (x - start_x) / pixels_per_second.max(1e-6);
                    let start = snap_range_start_to_playhead(
                        &guard,
                        width,
                        origin_offset + delta,
                        guard.trim_duration(),
                    );
                    guard.set_timeline_offset(start);
                }
                Some(ClipDrag::Segment {
                    index,
                    origin_start,
                    pixels_per_second,
                }) => {
                    let mut guard = state.lock().unwrap();
                    let delta = (x - start_x) / pixels_per_second.max(1e-6);
                    let duration = guard
                        .segment_timeline_duration(index);
                    let start =
                        snap_range_start_to_playhead(&guard, width, origin_start + delta, duration);
                    guard.set_segment_start(index, start);
                }
                Some(ClipDrag::Seek) => seek_to_x(&state, &media, width, x),
                None => {}
            }
            redraw();
        }
    });
    drag.connect_drag_end({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let dragging = dragging.clone();
        let redraw = redraw.clone();
        move |_, _, _| {
            if let Some(ClipDrag::Segment { index, .. }) = drag_kind.get() {
                state.lock().unwrap().settle_segment_start(index);
            }
            dragging.set(None);
            redraw();
        }
    });
    area.add_controller(drag);
    bind_track_cursor(
        area,
        {
            let state = state.clone();
            Rc::new(move |width, x| {
                let guard = state.lock().unwrap();
                if near_playhead(&guard, width, x) {
                    return (TrackCursor::Playhead, None, None);
                }
                let hit = video_hit(&guard, width, x);
                (hit.cursor, hit.segment, None)
            })
        },
        hover,
        Rc::new(Cell::new(None)),
    );
}

pub fn bind_zoom_track(
    area: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    hover: Rc<Cell<Option<usize>>>,
    hover_time: Rc<Cell<Option<f64>>>,
    dragging: Rc<Cell<Option<usize>>>,
    redraw: Rc<dyn Fn()>,
) {
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let state = state.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                return;
            }
            if let Some(index) = zoom_clip_at(&guard, width, x) {
                select_zoom(&mut guard, Some(index));
            } else {
                let at = snap_timeline_to_playhead(&guard, width, x_to_timeline(&guard, width, x));
                if guard.add_zoom_at(at).is_none() {
                    select_zoom(&mut guard, None);
                }
            }
            drop(guard);
            redraw();
        }
    });
    area.add_controller(click);

    let drag_kind = Rc::new(Cell::new(None::<ZoomDrag>));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let dragging = dragging.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                dragging.set(None);
                drag_kind.set(Some(ZoomDrag::Seek));
                return;
            }
            let kind = if let Some((index, is_start)) = zoom_edge_at(&guard, width, x) {
                select_zoom(&mut guard, Some(index));
                Some(ZoomDrag::Edge { index, is_start })
            } else if let Some(index) = zoom_clip_at(&guard, width, x) {
                select_zoom(&mut guard, Some(index));
                let clip = &guard.zoom_clips[index];
                Some(ZoomDrag::Move {
                    index,
                    origin_start: clip.start,
                    pixels_per_second: pixels_per_second(&guard, width),
                })
            } else {
                None
            };
            dragging.set(match kind {
                Some(ZoomDrag::Move { index, .. }) => Some(index),
                _ => None,
            });
            drag_kind.set(kind);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let media = media.clone();
        let drag_kind = drag_kind.clone();
        let redraw = redraw.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            match drag_kind.get() {
                Some(ZoomDrag::Edge { index, is_start }) => {
                    let mut guard = state.lock().unwrap();
                    let seconds = snap_timeline_to_playhead(
                        &guard,
                        width,
                        x_to_timeline(&guard, width, start_x + offset_x),
                    );
                    if let Some(clip) = guard.zoom_clips.get(index).cloned() {
                        if is_start {
                            guard.set_zoom_range(index, seconds, clip.end);
                        } else {
                            guard.set_zoom_range(index, clip.start, seconds);
                        }
                    }
                }
                Some(ZoomDrag::Move {
                    index,
                    origin_start,
                    pixels_per_second,
                }) => {
                    let mut guard = state.lock().unwrap();
                    let duration = guard
                        .zoom_clips
                        .get(index)
                        .map(|clip| clip.duration())
                        .unwrap_or(0.0);
                    let delta = offset_x / pixels_per_second.max(1e-6);
                    let start =
                        snap_range_start_to_playhead(&guard, width, origin_start + delta, duration);
                    guard.move_zoom_clip(index, start);
                }
                Some(ZoomDrag::Seek) => {
                    seek_to_x(&state, &media, width, start_x + offset_x);
                }
                None => {}
            }
            redraw();
        }
    });
    drag.connect_drag_end({
        let dragging = dragging.clone();
        let redraw = redraw.clone();
        move |_, _, _| {
            dragging.set(None);
            redraw();
        }
    });
    area.add_controller(drag);
    bind_track_cursor(
        area,
        {
            let state = state.clone();
            Rc::new(move |width, x| {
                let guard = state.lock().unwrap();
                if near_playhead(&guard, width, x) {
                    return (TrackCursor::Playhead, None, None);
                }
                match zoom_edge_at(&guard, width, x) {
                    Some((index, true)) => (TrackCursor::ResizeStart, Some(index), None),
                    Some((index, false)) => (TrackCursor::ResizeEnd, Some(index), None),
                    None => match zoom_clip_at(&guard, width, x) {
                        Some(index) => (TrackCursor::Grab, Some(index), None),
                        None => (
                            TrackCursor::None,
                            None,
                            Some(x_to_timeline(&guard, width, x)),
                        ),
                    },
                }
            })
        },
        hover,
        hover_time,
    );
}

pub fn bind_hide_track(
    area: &DrawingArea,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    hover: Rc<Cell<Option<usize>>>,
    hover_time: Rc<Cell<Option<f64>>>,
    dragging: Rc<Cell<Option<usize>>>,
    redraw: Rc<dyn Fn()>,
) {
    let click = GestureClick::new();
    click.set_button(1);
    click.connect_pressed({
        let state = state.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                return;
            }
            if let Some(index) = cursor_hide_clip_at(&guard, width, x) {
                select_cursor_hide(&mut guard, Some(index));
            } else {
                let at = snap_timeline_to_playhead(&guard, width, x_to_timeline(&guard, width, x));
                if guard.add_cursor_hide_at(at).is_none() {
                    select_cursor_hide(&mut guard, None);
                }
            }
            drop(guard);
            redraw();
        }
    });
    area.add_controller(click);

    let drag_kind = Rc::new(Cell::new(None::<HideDrag>));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let state = state.clone();
        let drag_kind = drag_kind.clone();
        let dragging = dragging.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut guard = state.lock().unwrap();
            if near_playhead(&guard, width, x) {
                dragging.set(None);
                drag_kind.set(Some(HideDrag::Seek));
                return;
            }
            let kind = if let Some((index, is_start)) = cursor_hide_edge_at(&guard, width, x) {
                select_cursor_hide(&mut guard, Some(index));
                Some(HideDrag::Edge { index, is_start })
            } else if let Some(index) = cursor_hide_clip_at(&guard, width, x) {
                select_cursor_hide(&mut guard, Some(index));
                let clip = &guard.cursor_hide_clips[index];
                Some(HideDrag::Move {
                    index,
                    origin_start: clip.start,
                    pixels_per_second: pixels_per_second(&guard, width),
                })
            } else {
                None
            };
            dragging.set(match kind {
                Some(HideDrag::Move { index, .. }) => Some(index),
                _ => None,
            });
            drag_kind.set(kind);
        }
    });
    drag.connect_drag_update({
        let state = state.clone();
        let media = media.clone();
        let drag_kind = drag_kind.clone();
        let redraw = redraw.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            match drag_kind.get() {
                Some(HideDrag::Edge { index, is_start }) => {
                    let mut guard = state.lock().unwrap();
                    let seconds = snap_timeline_to_playhead(
                        &guard,
                        width,
                        x_to_timeline(&guard, width, start_x + offset_x),
                    );
                    if let Some(clip) = guard.cursor_hide_clips.get(index).cloned() {
                        if is_start {
                            guard.set_cursor_hide_range(index, seconds, clip.end);
                        } else {
                            guard.set_cursor_hide_range(index, clip.start, seconds);
                        }
                    }
                }
                Some(HideDrag::Move {
                    index,
                    origin_start,
                    pixels_per_second,
                }) => {
                    let mut guard = state.lock().unwrap();
                    let duration = guard
                        .cursor_hide_clips
                        .get(index)
                        .map(|clip| clip.duration())
                        .unwrap_or(0.0);
                    let delta = offset_x / pixels_per_second.max(1e-6);
                    let start =
                        snap_range_start_to_playhead(&guard, width, origin_start + delta, duration);
                    guard.move_cursor_hide_clip(index, start);
                }
                Some(HideDrag::Seek) => {
                    seek_to_x(&state, &media, width, start_x + offset_x);
                }
                None => {}
            }
            redraw();
        }
    });
    drag.connect_drag_end({
        let dragging = dragging.clone();
        let redraw = redraw.clone();
        move |_, _, _| {
            dragging.set(None);
            redraw();
        }
    });
    area.add_controller(drag);
    bind_track_cursor(
        area,
        {
            let state = state.clone();
            Rc::new(move |width, x| {
                let guard = state.lock().unwrap();
                if near_playhead(&guard, width, x) {
                    return (TrackCursor::Playhead, None, None);
                }
                match cursor_hide_edge_at(&guard, width, x) {
                    Some((index, true)) => (TrackCursor::ResizeStart, Some(index), None),
                    Some((index, false)) => (TrackCursor::ResizeEnd, Some(index), None),
                    None => match cursor_hide_clip_at(&guard, width, x) {
                        Some(index) => (TrackCursor::Grab, Some(index), None),
                        None => (
                            TrackCursor::None,
                            None,
                            Some(x_to_timeline(&guard, width, x)),
                        ),
                    },
                }
            })
        },
        hover,
        hover_time,
    );
}

pub fn bind_track_cursor(
    area: &DrawingArea,
    hit: Rc<dyn Fn(f64, f64) -> (TrackCursor, Option<usize>, Option<f64>)>,
    hover: Rc<Cell<Option<usize>>>,
    hover_time: Rc<Cell<Option<f64>>>,
) {
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let hover = hover.clone();
        let hover_time = hover_time.clone();
        let area = area.clone();
        move |controller, x, _| {
            let Some(widget) = controller.widget() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let (kind, index, time) = hit(width, x);
            hover.set(index);
            hover_time.set(time);
            area.queue_draw();
            let cursor = match kind {
                TrackCursor::ResizeStart => gdk::Cursor::from_name("w-resize", None),
                TrackCursor::ResizeEnd => gdk::Cursor::from_name("e-resize", None),
                TrackCursor::Grab => gdk::Cursor::from_name("grab", None),
                TrackCursor::Playhead => gdk::Cursor::from_name("ew-resize", None),
                TrackCursor::None => None,
            };
            widget.set_cursor(cursor.as_ref());
        }
    });
    motion.connect_leave({
        let hover = hover.clone();
        let hover_time = hover_time.clone();
        let area = area.clone();
        move |controller| {
            hover.set(None);
            hover_time.set(None);
            area.queue_draw();
            if let Some(widget) = controller.widget() {
                widget.set_cursor(None);
            }
        }
    });
    area.add_controller(motion);
}
