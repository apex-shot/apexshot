use super::footer;
use crate::recording::editor::model::{ProjectMedia, ProjectMediaKind, VideoEditState};
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    gdk::prelude::GdkCairoContextExt, prelude::*, Align, Box as GtkBox, Button, DrawingArea,
    EventControllerMotion, GestureClick, GestureDrag, Image, Label, MediaFile, Orientation,
    Overlay,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const RAIL_HEADER_WIDTH: i32 = 120;
const TRACK_GAP: f64 = 8.0;
const ZERO_INSET: f64 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RailKind {
    Video,
    Audio,
    Zoom,
}

pub(super) fn build_timeline(
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
        move |_, cr, width, height| {
            draw_trim_overlay(&state, cr, width, height, dragging_segment.get());
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
            let start_x = state_guard.time_to_x(state_guard.trim_start_seconds, width);
            let end_x = state_guard.time_to_x(state_guard.trim_end_seconds, width);
            let handle_threshold = 18.0;
            let seconds = state_guard.x_to_time(x.clamp(0.0, width), width);

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
                    origin_start: state_guard.trim_start_seconds,
                    origin_end: state_guard.trim_end_seconds,
                    origin_seconds: seconds,
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
            let duration = state_guard.metadata.duration_seconds.max(0.001);
            let scale = if state_guard.cuts.is_empty() {
                duration
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
                TrimDragKind::Range {
                    origin_start,
                    origin_end,
                    origin_seconds,
                } => {
                    let delta = seconds - origin_seconds;
                    state_guard.trim_start_seconds = origin_start;
                    state_guard.trim_end_seconds = origin_end;
                    state_guard.shift_trim(delta);
                }
                TrimDragKind::Playhead => {
                    state_guard.playhead_seconds = seconds;
                    media.seek((seconds * 1_000_000.0) as i64);
                }
            }
            drop(state_guard);
            selection.queue_draw();
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
            s.x_to_time(x.clamp(0.0, width), width)
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
        let seconds = s.x_to_time(x.clamp(0.0, width), width);

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

    // Cursor hints
    let motion = EventControllerMotion::new();
    motion.connect_motion({
        let state = state.clone();
        let cut_mode = cut_mode.clone();
        let move_mode = move_mode.clone();
        move |controller, x, _| {
            let Some(widget) = controller.widget() else {
                return;
            };
            let width = widget.allocated_width().max(1) as f64;
            let state = state.lock().unwrap();
            let start_x = state.time_to_x(state.trim_start_seconds, width);
            let end_x = state.time_to_x(state.trim_end_seconds, width);
            let handle_threshold = 18.0;
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
                    let cx = state.time_to_x(c, width);
                    (x - cx).abs() <= cut_threshold
                });
                if near_cut {
                    Some("crosshair")
                } else {
                    None
                }
            };
            let cursor = cursor_name.and_then(|name| gdk::Cursor::from_name(name, None));
            widget.set_cursor(cursor.as_ref());
        }
    });
    motion.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
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
            let extras: Vec<ProjectMedia> = guard
                .extra_video_tracks()
                .into_iter()
                .cloned()
                .collect();
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

    let playhead_layer = DrawingArea::new();
    playhead_layer.add_css_class("recording-editor-playhead-layer");
    playhead_layer.set_hexpand(true);
    playhead_layer.set_vexpand(true);
    playhead_layer.set_can_target(false);
    playhead_layer.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| draw_spanning_playhead(&state, cr, width, height)
    });
    tracks_overlay.add_overlay(&playhead_layer);

    {
        let ruler = ruler.clone();
        let playhead_layer = playhead_layer.clone();
        let filmstrip = filmstrip.clone();
        let waveform_body = waveform_body.clone();
        let extra_video_tracks = extra_video_tracks.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
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
    board_inner.append(&well);
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
        move |scale| {
            state.lock().unwrap().timeline_scale = scale.value().clamp(0.0, 100.0);
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

fn rail_divider() -> GtkBox {
    let divider = GtkBox::new(Orientation::Vertical, 0);
    divider.add_css_class("recording-editor-rail-divider");
    divider.set_halign(Align::Start);
    divider.set_valign(Align::Fill);
    divider.set_hexpand(false);
    divider.set_vexpand(true);
    divider.set_can_target(false);
    divider.set_size_request(1, -1);
    divider.set_margin_start(RAIL_HEADER_WIDTH);
    divider
}

#[derive(Clone)]
struct RailChrome {
    row: GtkBox,
    lock: Button,
    hide: Option<Button>,
    mute: Option<Button>,
    delete: Button,
}

fn track_row(header: Option<&RailChrome>, body: &impl IsA<gtk4::Widget>) -> (GtkBox, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-track-row");
    row.set_hexpand(true);

    let header_box = if let Some(chrome) = header {
        chrome.row.clone()
    } else {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.add_css_class("recording-editor-track-header");
        spacer.set_size_request(RAIL_HEADER_WIDTH, 16);
        spacer
    };
    header_box.set_hexpand(false);
    header_box.set_halign(Align::Start);
    header_box.set_valign(Align::Center);
    header_box.set_size_request(RAIL_HEADER_WIDTH, -1);
    row.append(&header_box);

    let body_wrap = GtkBox::new(Orientation::Horizontal, 0);
    body_wrap.add_css_class("recording-editor-track-body");
    body_wrap.set_hexpand(true);
    body_wrap.set_size_request(0, -1);
    body_wrap.set_overflow(gtk4::Overflow::Hidden);
    body_wrap.append(body);
    row.append(&body_wrap);
    (row, body_wrap)
}

fn queue_draw_tree(widget: &impl IsA<gtk4::Widget>) {
    widget.queue_draw();
    let mut child = widget.first_child();
    while let Some(next) = child {
        queue_draw_tree(&next);
        child = next.next_sibling();
    }
}

fn video_track_signature(state: &Arc<Mutex<VideoEditState>>) -> u64 {
    let guard = state.lock().unwrap();
    let mut hash = guard.video_tracks().len() as u64;
    for item in guard.video_tracks() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(item.path.as_os_str().len() as u64);
    }
    hash
}

fn rebuild_extra_video_tracks(
    host: &GtkBox,
    extras: &[ProjectMedia],
    state: &Arc<Mutex<VideoEditState>>,
    estimate_label: &Label,
) {
    while let Some(child) = host.first_child() {
        host.remove(&child);
    }
    for item in extras {
        let header = build_rail_header(RailKind::Video);
        let strip = extra_video_strip(state, item);
        let (row, _) = track_row(Some(&header), &strip);
        row.add_css_class("recording-editor-empty-track-row");
        header.delete.set_sensitive(true);
        header.delete.set_tooltip_text(Some("Remove video track"));
        header.delete.connect_clicked({
            let state = state.clone();
            let estimate_label = estimate_label.clone();
            let path = item.path.clone();
            let host = host.clone();
            move |_| {
                state
                    .lock()
                    .unwrap()
                    .remove_project_media(&path, ProjectMediaKind::Video);
                let extras: Vec<ProjectMedia> = state
                    .lock()
                    .unwrap()
                    .extra_video_tracks()
                    .into_iter()
                    .cloned()
                    .collect();
                rebuild_extra_video_tracks(&host, &extras, &state, &estimate_label);
                footer::update_estimate(&estimate_label, &state, false);
            }
        });
        if let Some(hide) = &header.hide {
            hide.set_sensitive(false);
        }
        if let Some(mute) = &header.mute {
            mute.set_sensitive(false);
        }
        header.lock.set_sensitive(false);
        host.append(&row);
    }
}

fn extra_video_strip(state: &Arc<Mutex<VideoEditState>>, item: &ProjectMedia) -> Overlay {
    let strip = Overlay::new();
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.add_css_class("recording-editor-empty-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_size_request(-1, 64);
    let clip = DrawingArea::new();
    clip.set_hexpand(true);
    clip.set_vexpand(true);
    let title = if item.display_name.trim().is_empty() {
        "Video".to_string()
    } else {
        item.display_name.clone()
    };
    let duration = item.duration_seconds.unwrap_or(0.0);
    clip.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            draw_extra_video_clip(&state, cr, width, height, duration, &title);
        }
    });
    strip.set_child(Some(&clip));
    strip
}

fn draw_extra_video_clip(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    duration: f64,
    title: &str,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let end = if duration > 0.0 {
        duration
    } else {
        state.metadata.duration_seconds.max(0.001)
    };
    let x0 = state.time_to_x(0.0, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).abs().max(72.0).min(w);
    cr.set_source_rgb(0.22, 0.22, 0.22);
    cr.rectangle(x0.max(0.0), 0.0, clip_w, h);
    let _ = cr.fill();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(11.0);
    if let Ok(ext) = cr.text_extents(title) {
        cr.move_to(x0.max(0.0) + 12.0, h / 2.0 + ext.height() / 2.0);
        let _ = cr.show_text(title);
    }
}

fn set_track_body_look(body: &GtkBox, faded: bool, locked: bool) {
    body.set_opacity(if faded { 0.3 } else { 1.0 });
    if faded {
        body.add_css_class("recording-editor-track-faded");
    } else {
        body.remove_css_class("recording-editor-track-faded");
    }
    if locked {
        body.add_css_class("recording-editor-track-locked");
    } else {
        body.remove_css_class("recording-editor-track-locked");
    }
}

fn rail_icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-rail-action");
    button.set_tooltip_text(Some(tooltip));
    button.set_halign(Align::Center);
    button.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button
}

fn set_rail_icon(button: &Button, icon_name: &str) {
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
}

fn build_rail_header(kind: RailKind) -> RailChrome {
    let row = GtkBox::new(Orientation::Horizontal, 4);
    row.add_css_class("recording-editor-track-header");
    row.set_hexpand(false);
    row.set_halign(Align::Start);
    row.set_valign(Align::Center);
    row.set_size_request(RAIL_HEADER_WIDTH, -1);

    let type_name = match kind {
        RailKind::Video => "camera-video-symbolic",
        RailKind::Audio => "audio-volume-high-symbolic",
        RailKind::Zoom => "zoom-in-symbolic",
    };
    let type_icon = Image::from_icon_name(type_name);
    type_icon.add_css_class("recording-editor-track-icon");
    type_icon.set_pixel_size(14);
    type_icon.set_halign(Align::Start);
    type_icon.set_valign(Align::Center);
    row.append(&type_icon);

    let lock = rail_icon_button("changes-allow-symbolic", "Lock track");
    row.append(&lock);

    let hide = if matches!(kind, RailKind::Video | RailKind::Zoom) {
        let hide = rail_icon_button("view-reveal-symbolic", "Hide track");
        row.append(&hide);
        Some(hide)
    } else {
        None
    };

    let mute = if matches!(kind, RailKind::Video | RailKind::Audio) {
        let mute = rail_icon_button("audio-volume-high-symbolic", "Mute");
        row.append(&mute);
        Some(mute)
    } else {
        None
    };

    let delete = rail_icon_button("user-trash-symbolic", "Remove track");
    row.append(&delete);

    RailChrome {
        row,
        lock,
        hide,
        mute,
        delete,
    }
}

fn apply_rail_state(
    state: &VideoEditState,
    media: &MediaFile,
    video: &RailChrome,
    audio: &RailChrome,
    zoom: &RailChrome,
    audio_row: &GtkBox,
    zoom_row: &GtkBox,
    video_body: &GtkBox,
    audio_body: &GtkBox,
    zoom_body: &GtkBox,
) {
    set_rail_icon(
        &video.lock,
        if state.video_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    video.lock.set_tooltip_text(Some(if state.video_locked {
        "Unlock video"
    } else {
        "Lock video"
    }));
    if let Some(hide) = &video.hide {
        set_rail_icon(
            hide,
            if state.video_hidden {
                "view-conceal-symbolic"
            } else {
                "view-reveal-symbolic"
            },
        );
        hide.set_tooltip_text(Some(if state.video_hidden {
            "Show video"
        } else {
            "Hide video"
        }));
    }
    if let Some(mute) = &video.mute {
        let muted = state.is_muted();
        set_rail_icon(
            mute,
            if muted {
                "audio-volume-muted-symbolic"
            } else {
                "audio-volume-high-symbolic"
            },
        );
        mute.set_sensitive(state.has_audio_track() && !state.audio_locked);
        mute.set_tooltip_text(Some(if muted { "Unmute" } else { "Mute" }));
    }
    video
        .delete
        .set_sensitive(!state.video_locked && state.video_has_edits());
    video
        .delete
        .set_tooltip_text(Some(if state.video_has_edits() {
            "Reset video edits"
        } else {
            "No video edits to remove"
        }));

    audio_row.set_visible(state.has_audio_track());
    set_rail_icon(
        &audio.lock,
        if state.audio_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    audio.lock.set_tooltip_text(Some(if state.audio_locked {
        "Unlock audio"
    } else {
        "Lock audio"
    }));
    if let Some(mute) = &audio.mute {
        let muted = state.is_muted();
        set_rail_icon(
            mute,
            if muted {
                "audio-volume-muted-symbolic"
            } else {
                "audio-volume-high-symbolic"
            },
        );
        mute.set_sensitive(!state.audio_locked);
        mute.set_tooltip_text(Some(if muted { "Unmute" } else { "Mute" }));
    }
    audio.delete.set_sensitive(!state.audio_locked);
    audio.delete.set_tooltip_text(Some("Remove audio track"));

    zoom_row.set_visible(state.has_zoom_track());
    set_rail_icon(
        &zoom.lock,
        if state.zoom_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    zoom.lock.set_tooltip_text(Some(if state.zoom_locked {
        "Unlock zoom"
    } else {
        "Lock zoom"
    }));
    if let Some(hide) = &zoom.hide {
        set_rail_icon(
            hide,
            if state.zoom_hidden {
                "view-conceal-symbolic"
            } else {
                "view-reveal-symbolic"
            },
        );
        hide.set_tooltip_text(Some(if state.zoom_hidden {
            "Show zoom"
        } else {
            "Hide zoom"
        }));
    }
    zoom.delete
        .set_sensitive(!state.zoom_locked && state.has_zoom_track());
    zoom.delete.set_tooltip_text(Some("Remove zoom track"));

    set_track_body_look(video_body, state.video_hidden, state.video_locked);
    set_track_body_look(audio_body, state.is_muted(), state.audio_locked);
    set_track_body_look(zoom_body, state.zoom_hidden, state.zoom_locked);

    media.set_muted(state.is_muted() || !state.has_audio_track());
}

fn seek_from_x(
    state: &Arc<Mutex<VideoEditState>>,
    media: &MediaFile,
    widget: Option<&gtk4::Widget>,
    x: f64,
) {
    let width = widget
        .map(|widget| widget.allocated_width().max(1) as f64)
        .unwrap_or(1.0);
    let mut state = state.lock().unwrap();
    let seconds = state.x_to_time(x.clamp(0.0, width), width);
    state.playhead_seconds = seconds;
    media.seek((seconds * 1_000_000.0) as i64);
}

fn pause_playback(media: &MediaFile, playing: &Cell<bool>, play_button: &Button) {
    if !playing.get() && !media.is_playing() {
        return;
    }
    media.pause();
    playing.set(false);
    let icon = Image::from_icon_name("media-playback-start-symbolic");
    icon.set_pixel_size(18);
    play_button.set_child(Some(&icon));
}

fn icon_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-tool-icon");
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn build_waveform_body(
    state: &Arc<Mutex<VideoEditState>>,
    waveform: Option<PathBuf>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("recording-editor-waveform");
    area.set_hexpand(true);
    area.set_size_request(0, 36);
    let pixbuf = waveform.and_then(|path| gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok());
    let empty_label = if state.lock().unwrap().metadata.has_audio {
        "Waveform unavailable"
    } else {
        "No audio"
    };
    area.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            draw_waveform_image(&state, pixbuf.as_ref(), empty_label, cr, width, height);
        }
    });
    area
}

fn load_track_pixbufs(paths: &[PathBuf]) -> Vec<gtk4::gdk_pixbuf::Pixbuf> {
    paths
        .iter()
        .filter_map(|path| gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok())
        .collect()
}

fn paint_pixbuf(
    cr: &gtk4::cairo::Context,
    pixbuf: &gtk4::gdk_pixbuf::Pixbuf,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let src_w = f64::from(pixbuf.width()).max(1.0);
    let src_h = f64::from(pixbuf.height()).max(1.0);
    cr.save().ok();
    cr.rectangle(x, y, width, height);
    cr.clip();
    cr.translate(x, y);
    cr.scale(width / src_w, height / src_h);
    cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
    let _ = cr.paint();
    cr.restore().ok();
}

fn draw_filmstrip(
    state: &Arc<Mutex<VideoEditState>>,
    pixbufs: &[gtk4::gdk_pixbuf::Pixbuf],
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let duration = state.metadata.duration_seconds.max(0.001);
    let count = pixbufs.len().max(12);
    let slice = duration / count as f64;
    for index in 0..count {
        let x0 = state.time_to_x(index as f64 * slice, w);
        let x1 = state.time_to_x((index + 1) as f64 * slice, w);
        if x1 < 0.0 || x0 > w {
            continue;
        }
        let dest_w = (x1 - x0).max(1.0);
        if let Some(pixbuf) = pixbufs.get(index) {
            paint_pixbuf(cr, pixbuf, x0, 0.0, dest_w, h);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.04);
            cr.rectangle(x0, 0.0, dest_w, h);
            let _ = cr.fill();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
            cr.rectangle(x0 + 0.5, 0.5, dest_w - 1.0, h - 1.0);
            let _ = cr.stroke();
        }
    }
}

fn draw_waveform_image(
    state: &Arc<Mutex<VideoEditState>>,
    pixbuf: Option<&gtk4::gdk_pixbuf::Pixbuf>,
    empty_label: &str,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    cr.set_source_rgb(0.08, 0.08, 0.08);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let duration = state.metadata.duration_seconds.max(0.001);
    let x0 = state.time_to_x(0.0, w);
    let x1 = state.time_to_x(duration, w);
    let dest_w = (x1 - x0).max(1.0);
    if let Some(pixbuf) = pixbuf {
        paint_pixbuf(cr, pixbuf, x0, 0.0, dest_w, h);
        return;
    }
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.38);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    if let Ok(ext) = cr.text_extents(empty_label) {
        cr.move_to(
            ((w - ext.width()) / 2.0).max(4.0),
            h / 2.0 + ext.height() / 2.0,
        );
        let _ = cr.show_text(empty_label);
    }
}

fn build_zoom_track(state: Arc<Mutex<VideoEditState>>, estimate_label: Label) -> DrawingArea {
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
            let seconds = state.x_to_time(x.clamp(0.0, width), width);
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
            let seconds = state.x_to_time(x.clamp(0.0, width), width);
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
                state.x_to_time((start_x + offset_x).clamp(0.0, width), width)
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

fn draw_ruler(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let duration = state.metadata.duration_seconds.max(0.001);
    let (_, span) = state.timeline_view();
    let visible = (duration * span).max(0.001);
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
    let mut t = 0.0;
    while t <= duration + 0.001 {
        let inner = (w - ZERO_INSET).max(1.0);
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

fn draw_spanning_playhead(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let inset_left = f64::from(RAIL_HEADER_WIDTH) + TRACK_GAP + ZERO_INSET;
    let usable = (w - inset_left).max(1.0);
    let x = (inset_left + state.time_to_x(state.playhead_seconds, usable)).floor() + 0.5;

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
}

fn draw_zoom_track(
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
        let x0 = state.time_to_x(clip.start, w);
        let x1 = state.time_to_x(clip.end, w);
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

#[derive(Clone, Copy)]
enum TrimDragKind {
    Start,
    End,
    Playhead,
    Cut(usize),
    Segment(usize),
    Range {
        origin_start: f64,
        origin_end: f64,
        origin_seconds: f64,
    },
}

const SEGMENT_GAP: f64 = 8.0;

fn visual_capsules(layout: &[(usize, f64, f64)]) -> Vec<(usize, usize, f64, f64)> {
    let last = layout.len().saturating_sub(1);
    layout
        .iter()
        .enumerate()
        .map(|(opos, &(seg_idx, vx_start, vx_end))| {
            let start = vx_start + if opos == 0 { 0.0 } else { SEGMENT_GAP / 2.0 };
            let end = vx_end - if opos == last { 0.0 } else { SEGMENT_GAP / 2.0 };
            (opos, seg_idx, start, end)
        })
        .collect()
}

fn visual_x_to_source_seconds(state: &VideoEditState, width: f64, x: f64) -> f64 {
    let layout = compute_visual_layout(state, width);
    let capsules = visual_capsules(&layout);
    let boundaries = state.segment_boundaries();
    for &(_, seg_idx, start, end) in &capsules {
        if x >= start && x <= end {
            let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) else {
                continue;
            };
            let frac = ((x - start) / (end - start).max(1.0)).clamp(0.0, 1.0);
            return seg_start + frac * (seg_end - seg_start);
        }
    }
    if let Some(&(_, _, first, _)) = capsules.first() {
        if x < first {
            return state.trim_start_seconds;
        }
    }
    state.trim_end_seconds
}

fn hit_segment_drag(
    state: &VideoEditState,
    layout: &[(usize, f64, f64)],
    x: f64,
    handle_threshold: f64,
    allow_reorder: bool,
) -> Option<TrimDragKind> {
    let boundaries = state.segment_boundaries();
    let capsules = visual_capsules(layout);
    for &(_, seg_idx, start, end) in &capsules {
        let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) else {
            continue;
        };
        if (x - start).abs() <= handle_threshold {
            return Some(bound_drag_kind(state, seg_start, true));
        }
        if (x - end).abs() <= handle_threshold {
            return Some(bound_drag_kind(state, seg_end, false));
        }
    }
    if !allow_reorder {
        return None;
    }
    for &(opos, _, start, end) in &capsules {
        if x > start + handle_threshold && x < end - handle_threshold {
            return Some(TrimDragKind::Segment(opos));
        }
    }
    None
}

fn bound_drag_kind(state: &VideoEditState, bound: f64, is_start: bool) -> TrimDragKind {
    if is_start && (bound - state.trim_start_seconds).abs() < 1e-4 {
        return TrimDragKind::Start;
    }
    if !is_start && (bound - state.trim_end_seconds).abs() < 1e-4 {
        return TrimDragKind::End;
    }
    if let Some(index) = state
        .cuts
        .iter()
        .position(|cut| (*cut - bound).abs() < 1e-4)
    {
        TrimDragKind::Cut(index)
    } else if is_start {
        TrimDragKind::Start
    } else {
        TrimDragKind::End
    }
}

/// Visual layout entry: (chronological_seg_index, visual_x_start, visual_x_end)
fn compute_visual_layout(state: &VideoEditState, total_width: f64) -> Vec<(usize, f64, f64)> {
    let boundaries = state.segment_boundaries();
    let total_dur: f64 = state
        .segment_order
        .iter()
        .filter(|&&i| state.segments_kept.get(i).copied().unwrap_or(true))
        .filter_map(|&i| boundaries.get(i))
        .map(|(s, e)| (e - s).max(0.0))
        .sum();
    if total_dur <= 0.0 {
        return vec![];
    }
    let mut layout = Vec::new();
    let mut x = 0.0;
    for &seg_idx in &state.segment_order {
        if !state.segments_kept.get(seg_idx).copied().unwrap_or(true) {
            continue;
        }
        if let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) {
            let seg_dur = (seg_end - seg_start).max(0.0);
            let seg_w = (seg_dur / total_dur) * total_width;
            layout.push((
                seg_idx,
                state.frac_to_x(x / total_width, total_width),
                state.frac_to_x((x + seg_w) / total_width, total_width),
            ));
            x += seg_w;
        }
    }
    layout
}

fn draw_trim_overlay(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    dragging_seg_idx: Option<usize>,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let has_cuts = !state.cuts.is_empty();

    if has_cuts {
        let layout = compute_visual_layout(&state, w);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.48);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        for &(_, seg_idx, start, end) in &visual_capsules(&layout) {
            let emphasize = dragging_seg_idx == Some(seg_idx);
            draw_capsule_frame(cr, start, end, h, emphasize);
        }
        let removed_count = state.segments_kept.iter().filter(|&&k| !k).count();
        if removed_count > 0 {
            cr.select_font_face(
                "sans-serif",
                gtk4::cairo::FontSlant::Normal,
                gtk4::cairo::FontWeight::Bold,
            );
            cr.set_font_size(9.0);
            cr.set_source_rgba(1.0, 0.4, 0.4, 0.7);
            let text = format!("{removed_count} removed");
            if let Ok(ext) = cr.text_extents(&text) {
                cr.move_to(w - ext.width() - 4.0, h - 3.0);
                let _ = cr.show_text(&text);
            }
        }
    } else {
        let start_x = state.time_to_x(state.trim_start_seconds, w);
        let end_x = state.time_to_x(state.trim_end_seconds, w);
        draw_trim_capsule(cr, start_x, end_x, w, h);
    }
}

fn draw_clip_frame(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let stroke = 2.5;
    let inset = stroke / 2.0;
    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.set_line_width(stroke);
    rounded_rect(
        cr,
        x + inset,
        y + inset,
        (w - stroke).max(1.0),
        (h - stroke).max(1.0),
        10.0,
    );
    let _ = cr.stroke();
}

fn draw_trim_capsule(cr: &gtk4::cairo::Context, start_x: f64, end_x: f64, w: f64, h: f64) {
    let range_width = (end_x - start_x).max(36.0);
    let end_x = start_x + range_width;
    let is_full_clip = start_x <= 1.0 && end_x >= w - 1.0;

    if is_full_clip {
        draw_clip_frame(cr, 0.0, 0.0, w, h);
        return;
    }

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.48);
    cr.rectangle(0.0, 0.0, start_x.max(0.0), h);
    let _ = cr.fill();
    cr.rectangle(end_x.min(w), 0.0, (w - end_x).max(0.0), h);
    let _ = cr.fill();
    draw_capsule_frame(cr, start_x, end_x, h, false);
}

fn draw_capsule_frame(
    cr: &gtk4::cairo::Context,
    start_x: f64,
    end_x: f64,
    h: f64,
    emphasize: bool,
) {
    let range_width = (end_x - start_x).max(36.0);
    let end_x = start_x + range_width;
    let r = 10.0;
    let handle_w = 8.0;
    let pad = 1.5;
    let stroke = if emphasize { 3.0 } else { 2.5 };

    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.set_line_width(stroke);
    rounded_rect(
        cr,
        start_x + pad,
        pad,
        range_width - pad * 2.0,
        h - pad * 2.0,
        r,
    );
    let _ = cr.stroke();

    draw_trim_handle(cr, start_x + pad, pad, handle_w, h - pad * 2.0, r, true);
    draw_trim_handle(
        cr,
        end_x - pad - handle_w,
        pad,
        handle_w,
        h - pad * 2.0,
        r,
        false,
    );
}

fn draw_trim_handle(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64, left: bool) {
    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.new_sub_path();
    if left {
        cr.arc(
            x + r,
            y + r,
            r,
            std::f64::consts::PI,
            1.5 * std::f64::consts::PI,
        );
        cr.line_to(x + w, y);
        cr.line_to(x + w, y + h);
        cr.arc(
            x + r,
            y + h - r,
            r,
            0.5 * std::f64::consts::PI,
            std::f64::consts::PI,
        );
    } else {
        cr.move_to(x, y);
        cr.arc(x + w - r, y + r, r, -0.5 * std::f64::consts::PI, 0.0);
        cr.arc(x + w - r, y + h - r, r, 0.0, 0.5 * std::f64::consts::PI);
        cr.line_to(x, y + h);
    }
    cr.close_path();
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    cr.set_line_width(2.0);
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    let cx = x + w / 2.0;
    let gy0 = y + h * 0.32;
    let gy1 = y + h * 0.68;
    cr.move_to(cx - 2.5, gy0);
    cr.line_to(cx - 2.5, gy1);
    cr.move_to(cx + 2.5, gy0);
    cr.line_to(cx + 2.5, gy1);
    let _ = cr.stroke();
}

fn update_time_labels(start_label: &Label, end_label: &Label, state: &Arc<Mutex<VideoEditState>>) {
    let state = state.lock().unwrap();
    start_label.set_text(&format!(
        "Start {}",
        format_duration(state.trim_start_seconds)
    ));
    end_label.set_text(&format!("End {}", format_duration(state.trim_end_seconds)));
}

fn nearest_cut_index(
    state: &VideoEditState,
    seconds: f64,
    threshold_seconds: f64,
) -> Option<usize> {
    state
        .cuts
        .iter()
        .enumerate()
        .filter_map(|(index, cut)| {
            let distance = (cut - seconds).abs();
            (distance <= threshold_seconds).then_some((index, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
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

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}:{seconds:04.1}")
}

fn format_timecode(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let secs = (seconds - minutes as f64 * 60.0).floor() as u64;
    format!("{minutes:02}:{secs:02}")
}
