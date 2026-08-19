use crate::recording::editor::model::{
    format_zoom_scale, VideoEditState, DEFAULT_ZOOM_DURATION_SECONDS,
};
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    prelude::*, Adjustment, Align, Box as GtkBox, Button, DrawingArea, EventControllerMotion,
    EventControllerScroll, EventControllerScrollFlags, GestureClick, GestureDrag, Image, Label,
    MediaFile, Orientation, Overlay, Scale, Scrollbar,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) fn build_timeline_card(
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

fn labeled_tool_button(icon_name: &str, label: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-tool");
    button.set_tooltip_text(Some(tooltip));
    let row = GtkBox::new(Orientation::Horizontal, 6);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    let text = Label::new(Some(label));
    row.append(&icon);
    row.append(&text);
    button.set_child(Some(&row));
    button
}

fn icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-timeline-icon");
    button.set_tooltip_text(Some(tooltip));
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button
}

fn format_clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn format_range(start: f64, end: f64) -> String {
    format!("{} - {}", format_mmss(start), format_mmss(end))
}

fn format_mmss(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn toggle_playback(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    redraw: &Rc<dyn Fn()>,
) {
    if playing.get() {
        if let Some(media_file) = media.borrow().as_ref() {
            media_file.pause();
        }
        playing.set(false);
        set_play_icon(play_button, "media-playback-start-symbolic");
        redraw();
        return;
    }

    let seek_to = {
        let mut guard = state.lock().unwrap();
        if guard.playhead_seconds >= guard.content_end_seconds() - 0.05 {
            guard.playhead_seconds = 0.0;
        }
        guard.source_playhead()
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.seek((seek_to * 1_000_000.0) as i64);
        media_file.set_muted(state.lock().unwrap().muted_for_source(seek_to));
        media_file.play();
    }
    playing.set(true);
    set_play_icon(play_button, "media-playback-pause-symbolic");
    redraw();
}

fn set_play_icon(button: &Button, icon_name: &str) {
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
}

fn nudge_playhead(
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

fn tick_playback(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    playing: &Rc<Cell<bool>>,
    play_button: &Button,
    redraw: &Rc<dyn Fn()>,
) {
    if !playing.get() {
        return;
    }

    let (mut next, end) = {
        let guard = state.lock().unwrap();
        (guard.playhead_seconds, guard.content_end_seconds())
    };
    let mut reached_end = false;

    if let Some(media_file) = media.borrow().as_ref() {
        if media_file.is_playing() {
            let ts = media_file.timestamp();
            if ts > 0 {
                let source_t = ts as f64 / 1_000_000.0;
                next = state.lock().unwrap().source_to_timeline(source_t);
            }
        } else {
            next += 0.05;
        }
    } else {
        next += 0.05;
    }

    if next >= end {
        next = end;
        reached_end = true;
    }
    {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = next;
        let muted = guard.muted_for_source(guard.source_playhead());
        drop(guard);
        if let Some(media_file) = media.borrow().as_ref() {
            media_file.set_muted(muted);
        }
    }
    if reached_end {
        playing.set(false);
        if let Some(media_file) = media.borrow().as_ref() {
            media_file.pause();
        }
        set_play_icon(play_button, "media-playback-start-symbolic");
    }
    redraw();
}

fn bind_board_hover(
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

fn bind_playhead_drag(
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

fn bind_video_clip(
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
                    let seconds = {
                        let guard = state.lock().unwrap();
                        x_to_source(&guard, width, x)
                    };
                    state.lock().unwrap().set_trim_start(seconds);
                }
                Some(ClipDrag::End) => {
                    let seconds = {
                        let guard = state.lock().unwrap();
                        x_to_source(&guard, width, x)
                    };
                    state.lock().unwrap().set_trim_end(seconds);
                }
                Some(ClipDrag::Cut(index)) => {
                    let seconds = {
                        let guard = state.lock().unwrap();
                        x_to_source(&guard, width, x)
                    };
                    state.lock().unwrap().move_cut(index, seconds);
                }
                Some(ClipDrag::Move {
                    origin_offset,
                    pixels_per_second,
                }) => {
                    let delta = (x - start_x) / pixels_per_second.max(1e-6);
                    state
                        .lock()
                        .unwrap()
                        .set_timeline_offset(origin_offset + delta);
                }
                Some(ClipDrag::Segment {
                    index,
                    origin_start,
                    pixels_per_second,
                }) => {
                    let delta = (x - start_x) / pixels_per_second.max(1e-6);
                    state
                        .lock()
                        .unwrap()
                        .set_segment_start(index, origin_start + delta);
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

fn bind_zoom_track(
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
                let at = x_to_timeline(&guard, width, x);
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
                    let seconds = {
                        let guard = state.lock().unwrap();
                        x_to_timeline(&guard, width, start_x + offset_x)
                    };
                    let mut guard = state.lock().unwrap();
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
                    let delta = offset_x / pixels_per_second.max(1e-6);
                    state
                        .lock()
                        .unwrap()
                        .move_zoom_clip(index, origin_start + delta);
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

fn bind_track_cursor(
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

fn near_playhead(state: &VideoEditState, width: f64, x: f64) -> bool {
    (x - state.time_to_x(state.playhead_seconds, width)).abs() <= PLAYHEAD_HIT
}

fn video_layout(state: &VideoEditState, width: f64) -> Vec<(usize, usize, f64, f64)> {
    let bounds = state.segment_boundaries();
    let mut layout = Vec::new();
    for (order_pos, &seg_idx) in state.segment_order.iter().enumerate() {
        if !state.segments_kept.get(seg_idx).copied().unwrap_or(true) {
            continue;
        }
        let Some(&(start, end)) = bounds.get(seg_idx) else {
            continue;
        };
        let comp = state.segment_start(seg_idx);
        let x0 = state.time_to_x(comp, width);
        let x1 = state.time_to_x(comp + (end - start).max(0.0), width);
        layout.push((order_pos, seg_idx, x0, x1.max(x0 + 8.0)));
    }
    layout
}

struct VideoHit {
    cursor: TrackCursor,
    segment: Option<usize>,
    drag: Option<ClipDrag>,
}

fn video_hit(state: &VideoEditState, width: f64, x: f64) -> VideoHit {
    for &(_, seg_idx, x0, x1) in &video_layout(state, width) {
        if x < x0 || x > x1 {
            continue;
        }
        let (cursor, drag) = match clip_edge_at(x, x0, x1) {
            Some(true) => (
                TrackCursor::ResizeStart,
                segment_edge_drag(state, seg_idx, true),
            ),
            Some(false) => (
                TrackCursor::ResizeEnd,
                segment_edge_drag(state, seg_idx, false),
            ),
            None if state.cuts.is_empty() => (
                TrackCursor::Grab,
                Some(ClipDrag::Move {
                    origin_offset: state.timeline_offset_seconds,
                    pixels_per_second: pixels_per_second(state, width),
                }),
            ),
            None => (
                TrackCursor::Grab,
                Some(ClipDrag::Segment {
                    index: seg_idx,
                    origin_start: state.segment_start(seg_idx),
                    pixels_per_second: pixels_per_second(state, width),
                }),
            ),
        };
        return VideoHit {
            cursor,
            segment: Some(seg_idx),
            drag,
        };
    }
    VideoHit {
        cursor: TrackCursor::None,
        segment: None,
        drag: None,
    }
}

fn segment_edge_drag(state: &VideoEditState, seg_idx: usize, is_start: bool) -> Option<ClipDrag> {
    let Some(&(start, end)) = state.segment_boundaries().get(seg_idx) else {
        return Some(if is_start {
            ClipDrag::Start
        } else {
            ClipDrag::End
        });
    };
    let edge = if is_start { start } else { end };
    if is_start && (edge - state.trim_start_seconds).abs() < 1e-3 {
        return Some(ClipDrag::Start);
    }
    if !is_start && (edge - state.trim_end_seconds).abs() < 1e-3 {
        return Some(ClipDrag::End);
    }
    state
        .cuts
        .iter()
        .position(|cut| (*cut - edge).abs() < 1e-3)
        .map(ClipDrag::Cut)
}

fn select_video(state: &mut VideoEditState, segment: Option<usize>) {
    state.selected_segment = segment;
    if segment.is_some() {
        state.selected_zoom = None;
    }
}

fn select_zoom(state: &mut VideoEditState, index: Option<usize>) {
    state.selected_zoom = index;
    if index.is_some() {
        state.selected_segment = None;
    }
}

fn clip_edge_at(x: f64, start_x: f64, end_x: f64) -> Option<bool> {
    let left = start_x + HANDLE_INSET;
    let right = end_x - HANDLE_INSET - HANDLE_WIDTH;
    if x >= left - HANDLE_HIT && x <= left + HANDLE_WIDTH + HANDLE_HIT {
        Some(true)
    } else if x >= right - HANDLE_HIT && x <= right + HANDLE_WIDTH + HANDLE_HIT {
        Some(false)
    } else {
        None
    }
}

fn zoom_edge_at(state: &VideoEditState, width: f64, x: f64) -> Option<(usize, bool)> {
    state
        .zoom_clips
        .iter()
        .enumerate()
        .find_map(|(index, clip)| {
            let start_x = state.time_to_x(clip.start, width);
            let end_x = state.time_to_x(clip.end, width);
            clip_edge_at(x, start_x, end_x).map(|is_start| (index, is_start))
        })
}

fn zoom_clip_at(state: &VideoEditState, width: f64, x: f64) -> Option<usize> {
    state.zoom_clips.iter().position(|clip| {
        let start_x = state.time_to_x(clip.start, width);
        let end_x = state.time_to_x(clip.end, width);
        x >= start_x && x <= end_x
    })
}

fn seek_to_x(
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    width: f64,
    x: f64,
) {
    let seek_to = {
        let mut guard = state.lock().unwrap();
        guard.playhead_seconds = x_to_timeline(&guard, width, x);
        guard.source_playhead()
    };
    if let Some(media_file) = media.borrow().as_ref() {
        media_file.seek((seek_to * 1_000_000.0) as i64);
    }
}

fn x_to_source(state: &VideoEditState, width: f64, x: f64) -> f64 {
    state
        .timeline_to_source(x_to_timeline(state, width, x))
        .clamp(0.0, state.metadata.duration_seconds.max(0.0))
}

fn x_to_timeline(state: &VideoEditState, width: f64, x: f64) -> f64 {
    state.x_to_time(x.clamp(0.0, width), width).max(0.0)
}

fn pixels_per_second(state: &VideoEditState, width: f64) -> f64 {
    width.max(1.0) / state.visible_span_seconds().max(0.001)
}

fn sync_scroll_adj(adj: &Adjustment, state: &VideoEditState, syncing: &Cell<bool>) {
    let visible = state.visible_span_seconds();
    let upper = state.timeline_canvas_seconds().max(visible);
    let value = state.timeline_scroll_seconds;
    if (adj.page_size() - visible).abs() < 1e-6
        && (adj.upper() - upper).abs() < 1e-6
        && (adj.value() - value).abs() < 1e-4
    {
        return;
    }
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

#[derive(Clone, Copy)]
enum ClipDrag {
    Start,
    End,
    Cut(usize),
    Move {
        origin_offset: f64,
        pixels_per_second: f64,
    },
    Segment {
        index: usize,
        origin_start: f64,
        pixels_per_second: f64,
    },
    Seek,
}

#[derive(Clone, Copy)]
enum ZoomDrag {
    Edge {
        index: usize,
        is_start: bool,
    },
    Move {
        index: usize,
        origin_start: f64,
        pixels_per_second: f64,
    },
    Seek,
}

#[derive(Clone, Copy)]
enum TrackCursor {
    None,
    ResizeStart,
    ResizeEnd,
    Grab,
    Playhead,
}

const HANDLE_INSET: f64 = 6.0;
const HANDLE_WIDTH: f64 = 4.0;
const HANDLE_HIT: f64 = 6.0;
const PLAYHEAD_HIT: f64 = 6.0;

fn draw_ruler(
    state: &Arc<Mutex<VideoEditState>>,
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

    cr.select_font_face(
        "sans-serif",
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
            cr.set_source_rgba(1.0, 1.0, 1.0, if on_major { 0.28 } else { 0.10 });
            cr.move_to(x, if on_major { h - 11.0 } else { h - 5.0 });
            cr.line_to(x, h);
            let _ = cr.stroke();
            if on_major {
                let label = format_ruler_label(t, major);
                cr.set_source_rgba(
                    1.0,
                    1.0,
                    1.0,
                    if (t - playhead).abs() < major * 0.08 {
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

fn ruler_major_step(visible: f64) -> f64 {
    const STEPS: [f64; 10] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0];
    *STEPS
        .iter()
        .find(|step| visible / *step <= 10.0)
        .unwrap_or(&STEPS[STEPS.len() - 1])
}

fn near_step(value: f64, step: f64) -> bool {
    let scaled = value / step;
    (scaled - scaled.round()).abs() < 0.02
}

fn format_ruler_label(seconds: f64, major: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor() as u64;
    let secs = total - minutes as f64 * 60.0;
    if major < 1.0 {
        format!("{minutes}:{secs:04.1}")
    } else {
        format!("{minutes}:{:02}", secs.floor() as u64)
    }
}

fn draw_video_clip(
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

fn clip_tone(selected: bool, faint: bool) -> ClipTone {
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

fn draw_video_segment(
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
    if lifted {
        rounded_rect(cr, x0 + 3.0, y + 8.0, clip_w, height, 5.0);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.38);
        let _ = cr.fill();
    }
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
        "sans-serif",
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

fn draw_zoom_clips(
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

fn draw_one_zoom(
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
    if lifted {
        rounded_rect(cr, x0 + 3.0, y + 8.0, clip_w, height, 5.0);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.38);
        let _ = cr.fill();
    }
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
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let label = if clip_w > 92.0 {
            format!("Zoom  {}", format_zoom_scale(state.zoom_clips[index].scale))
        } else {
            "Zoom".into()
        };
        let _ = cr.show_text(&label);
    }
}

fn suggested_zoom_range(state: &VideoEditState, start: f64) -> Option<(f64, f64)> {
    let start = start.max(0.0);
    let end = start + DEFAULT_ZOOM_DURATION_SECONDS;
    let overlaps = state
        .zoom_clips
        .iter()
        .any(|clip| start < clip.end && end > clip.start);
    (!overlaps).then_some((start, end))
}

fn draw_zoom_suggestion(
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
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.0);
        cr.move_to(x0 + 14.0, y + height * 0.62);
        let _ = cr.show_text("Zoom");
    }
}

#[derive(Clone, Copy)]
struct ClipTone {
    fill: (f64, f64, f64, f64),
    handle: (f64, f64, f64, f64),
}

fn draw_translucent_clip(
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

fn draw_edge_handle(
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

fn draw_playhead(
    state: &Arc<Mutex<VideoEditState>>,
    hover_time: Option<f64>,
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
            paint_playhead_mark(cr, hover_x, h, 0.22);
        }
    }
    paint_playhead_mark(cr, state.time_to_x(state.playhead_seconds, w), h, 1.0);
}

fn paint_playhead_mark(cr: &gtk4::cairo::Context, x: f64, h: f64, alpha: f64) {
    let x = x.floor() + 0.5;
    cr.set_source_rgba(0.86, 0.90, 0.98, alpha);
    cr.set_line_width(if alpha < 1.0 { 1.5 } else { 2.0 });
    cr.move_to(x, 26.0);
    cr.line_to(x, h);
    let _ = cr.stroke();
    cr.set_source_rgba(0.86, 0.90, 0.98, alpha);
    cr.move_to(x - 5.0, 20.0);
    cr.line_to(x + 5.0, 20.0);
    cr.line_to(x, 29.0);
    cr.close_path();
    let _ = cr.fill();
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
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
