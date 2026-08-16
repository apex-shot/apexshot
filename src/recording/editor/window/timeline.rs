use super::footer;
use crate::recording::editor::model::VideoEditState;
use gtk4::gdk;
use gtk4::glib;
use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, EventControllerMotion, GestureClick,
    GestureDrag, Image, Label, MediaFile, Orientation, Overlay, Picture,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) fn build_timeline(
    state: Arc<Mutex<VideoEditState>>,
    estimate_label: Label,
    thumbnails: Vec<PathBuf>,
    waveform: Option<PathBuf>,
    media: MediaFile,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 6);
    root.add_css_class("recording-editor-timeline");
    root.set_hexpand(true);
    root.set_vexpand(false);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-timeline-shell");
    card.set_hexpand(true);
    card.set_vexpand(false);

    let play_button = Button::new();
    play_button.add_css_class("recording-editor-play-button");
    let play_icon = Image::from_icon_name("media-playback-start-symbolic");
    play_icon.set_pixel_size(22);
    play_button.set_child(Some(&play_icon));
    play_button.set_valign(Align::Center);
    play_button.set_tooltip_text(Some("Play"));

    // Create buttons and modes first
    let cut_button = Button::new();
    cut_button.add_css_class("recording-editor-cut-button");
    let cut_icon = Image::from_icon_name("edit-cut-symbolic");
    cut_icon.set_pixel_size(18);
    cut_button.set_child(Some(&cut_icon));
    cut_button.set_valign(Align::Center);
    cut_button.set_tooltip_text(Some("Cut mode — click timeline to place cuts"));
    let cut_mode = Rc::new(Cell::new(false));

    let move_button = Button::new();
    move_button.add_css_class("recording-editor-cut-button");
    let move_icon = Image::from_icon_name("view-sort-ascending-symbolic");
    move_icon.set_pixel_size(18);
    move_button.set_child(Some(&move_icon));
    move_button.set_valign(Align::Center);
    move_button.set_tooltip_text(Some("Move mode — drag a segment to reorder it"));
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
                cut_button.add_css_class("recording-editor-cut-button-active");
                move_mode.set(false);
                move_button.remove_css_class("recording-editor-cut-button-active");
            } else {
                cut_button.remove_css_class("recording-editor-cut-button-active");
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
                cut_button.remove_css_class("recording-editor-cut-button-active");
                move_button.add_css_class("recording-editor-cut-button-active");
            } else {
                move_button.remove_css_class("recording-editor-cut-button-active");
            }
        }
    });

    let revert_button = Button::new();
    revert_button.add_css_class("recording-editor-revert-button");
    let revert_icon = Image::from_icon_name("edit-undo-symbolic");
    revert_icon.set_pixel_size(18);
    revert_button.set_child(Some(&revert_icon));
    revert_button.set_valign(Align::Center);
    revert_button.set_tooltip_text(Some("Revert cuts"));

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
                icon.set_pixel_size(22);
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
                    icon.set_pixel_size(22);
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
                icon.set_pixel_size(22);
                play_button_ref.set_child(Some(&icon));
            }
        }
    });

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-trim-area");
    overlay.set_hexpand(true);
    overlay.set_vexpand(false);
    overlay.set_size_request(-1, 52);

    let strip = GtkBox::new(Orientation::Horizontal, 2);
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_vexpand(false);
    strip.set_halign(Align::Fill);
    strip.set_valign(Align::Center);
    strip.set_size_request(-1, 52);
    if thumbnails.is_empty() {
        for _ in 0..12 {
            let placeholder = GtkBox::new(Orientation::Vertical, 0);
            placeholder.add_css_class("recording-editor-thumbnail");
            placeholder.set_hexpand(true);
            strip.append(&placeholder);
        }
    } else {
        for path in thumbnails {
            let picture = Picture::for_filename(path);
            picture.add_css_class("recording-editor-thumbnail");
            picture.set_hexpand(true);
            picture.set_vexpand(false);
            picture.set_can_shrink(true);
            picture.set_size_request(-1, 48);
            strip.append(&picture);
        }
    }
    overlay.set_child(Some(&strip));

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
            cut_button.remove_css_class("recording-editor-cut-button-active");
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
        move |gesture, x, _| {
            scrubbing.set(true);
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut state_guard = state.lock().unwrap();
            let duration = state_guard.metadata.duration_seconds.max(0.001);
            let start_x = (state_guard.trim_start_seconds / duration) * width;
            let end_x = (state_guard.trim_end_seconds / duration) * width;
            let handle_threshold = 18.0;
            let seconds = (x.clamp(0.0, width) / width) * duration;

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
            let duration = state_guard.metadata.duration_seconds.max(0.001);
            let scale = if state_guard.cuts.is_empty() {
                duration
            } else {
                state_guard.kept_duration().max(0.001)
            };
            let delta_t = (offset_x / width) * scale;
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
        let duration = {
            let s = state_for_dbl.lock().unwrap();
            s.metadata.duration_seconds.max(0.001)
        };
        let seconds = (x.clamp(0.0, width) / width) * duration;
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
        let duration = s.metadata.duration_seconds.max(0.001);
        let seconds = (x.clamp(0.0, width) / width) * duration;

        // Check if near a cut line (remove it)
        let cut_threshold_seconds = (12.0 / width) * duration;
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
            let duration = state.metadata.duration_seconds.max(0.001);
            let start_x = (state.trim_start_seconds / duration) * width;
            let end_x = (state.trim_end_seconds / duration) * width;
            let handle_threshold = 18.0;
            let cursor_name = if !state.cuts.is_empty() {
                let layout = compute_visual_layout(&state, width);
                match hit_segment_drag(&state, &layout, x, handle_threshold, move_mode.get()) {
                    Some(TrimDragKind::Start) | Some(TrimDragKind::Cut(_))
                        if layout.iter().any(|(_, vx, _)| (x - vx).abs() <= handle_threshold) =>
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
                    let cx = (c / duration) * width;
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
                    icon.set_pixel_size(22);
                    play_button_timer.set_child(Some(&icon));
                }
                selection_playhead.queue_draw();
            }
        }
        glib::ControlFlow::Continue
    });

    let skip_back = icon_tool_button("media-skip-backward-symbolic", "Skip back");
    skip_back.connect_clicked({
        let state = state.clone();
        let media = media.clone();
        let selection = selection.clone();
        move |_| {
            let mut state = state.lock().unwrap();
            state.playhead_seconds = (state.playhead_seconds - 5.0).max(state.trim_start_seconds);
            media.seek((state.playhead_seconds * 1_000_000.0) as i64);
            drop(state);
            selection.queue_draw();
        }
    });
    let skip_forward = icon_tool_button("media-skip-forward-symbolic", "Skip forward");
    skip_forward.connect_clicked({
        let state = state.clone();
        let media = media.clone();
        let selection = selection.clone();
        move |_| {
            let mut state = state.lock().unwrap();
            state.playhead_seconds = (state.playhead_seconds + 5.0).min(state.trim_end_seconds);
            media.seek((state.playhead_seconds * 1_000_000.0) as i64);
            drop(state);
            selection.queue_draw();
        }
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

    let transport = GtkBox::new(Orientation::Horizontal, 0);
    transport.add_css_class("recording-editor-transport");
    transport.set_hexpand(true);

    let transport_left = GtkBox::new(Orientation::Horizontal, 4);
    transport_left.set_halign(Align::Start);
    transport_left.set_hexpand(true);
    transport_left.append(&revert_button);
    transport_left.append(&redo_button);
    transport_left.append(&cut_button);

    let transport_center = GtkBox::new(Orientation::Horizontal, 6);
    transport_center.set_halign(Align::Center);
    transport_center.set_hexpand(true);
    transport_center.append(&skip_back);
    play_button.add_css_class("recording-editor-play-button-hero");
    transport_center.append(&play_button);
    transport_center.append(&skip_forward);
    transport_center.append(&move_button);

    let transport_right = GtkBox::new(Orientation::Horizontal, 4);
    transport_right.set_halign(Align::End);
    transport_right.set_hexpand(true);
    transport_right.append(&delete_button);

    transport.append(&transport_left);
    transport.append(&transport_center);
    transport.append(&transport_right);
    card.append(&transport);

    let ruler = DrawingArea::new();
    ruler.add_css_class("recording-editor-ruler");
    ruler.set_hexpand(true);
    ruler.set_size_request(-1, 20);
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
        move |gesture, x, _| {
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
            seek_from_x(&state, &media, gesture.widget().as_ref(), start_x + offset_x);
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

    let video_add = icon_tool_button("list-add-symbolic", "Cut at playhead");
    video_add.add_css_class("recording-editor-track-add");
    video_add.connect_clicked({
        let state = state.clone();
        let selection = selection.clone();
        let estimate_label = estimate_label.clone();
        move |_| {
            {
                let mut guard = state.lock().unwrap();
                let playhead = guard.playhead_seconds;
                guard.add_cut(playhead);
            }
            selection.queue_draw();
            footer::update_estimate(&estimate_label, &state, false);
        }
    });

    let video_row = track_row("camera-video-symbolic", &overlay, Some(&video_add));

    add_zoom.add_css_class("recording-editor-track-add");
    let waveform_body = build_waveform_body(&state, waveform);
    let audio_add = icon_tool_button("list-add-symbolic", "Audio stays as recorded");
    audio_add.add_css_class("recording-editor-track-add");
    audio_add.set_sensitive(false);
    let audio_row = track_row("audio-volume-high-symbolic", &waveform_body, Some(&audio_add));

    let zoom_body = build_zoom_track(state.clone(), estimate_label.clone());
    let zoom_row = track_row("zoom-in-symbolic", &zoom_body, Some(&add_zoom));

    let tracks = GtkBox::new(Orientation::Vertical, 6);
    tracks.add_css_class("recording-editor-tracks");
    tracks.set_hexpand(true);
    tracks.append(&track_row("", &ruler, None));
    tracks.append(&video_row);
    tracks.append(&audio_row);
    tracks.append(&zoom_row);

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
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            ruler.queue_draw();
            playhead_layer.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    card.append(&tracks_overlay);
    root.append(&card);
    root
}

fn track_row(icon_name: &str, body: &impl IsA<gtk4::Widget>, add: Option<&Button>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-track-row");
    row.set_hexpand(true);

    let icon = if icon_name.is_empty() {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_size_request(16, 16);
        spacer.upcast::<gtk4::Widget>()
    } else {
        let icon = Image::from_icon_name(icon_name);
        icon.add_css_class("recording-editor-track-icon");
        icon.set_pixel_size(16);
        icon.set_valign(Align::Center);
        icon.upcast::<gtk4::Widget>()
    };
    row.append(&icon);

    let body_wrap = GtkBox::new(Orientation::Horizontal, 0);
    body_wrap.set_hexpand(true);
    body_wrap.append(body);
    row.append(&body_wrap);

    if let Some(add) = add {
        add.set_valign(Align::Center);
        row.append(add);
    } else {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_size_request(32, 32);
        row.append(&spacer);
    }
    row
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
    let duration = state.metadata.duration_seconds.max(0.001);
    let seconds = (x.clamp(0.0, width) / width) * duration;
    state.playhead_seconds = seconds;
    media.seek((seconds * 1_000_000.0) as i64);
}

fn icon_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-cut-button");
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button
}

fn build_waveform_body(state: &Arc<Mutex<VideoEditState>>, waveform: Option<PathBuf>) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("recording-editor-waveform");
    row.set_hexpand(true);
    if let Some(path) = waveform {
        let picture = Picture::for_filename(path);
        picture.add_css_class("recording-editor-waveform-image");
        picture.set_hexpand(true);
        picture.set_can_shrink(true);
        picture.set_size_request(-1, 36);
        row.append(&picture);
    } else {
        let empty = Label::new(Some(if state.lock().unwrap().metadata.has_audio {
            "Waveform unavailable"
        } else {
            "No audio"
        }));
        empty.add_css_class("recording-editor-time-label");
        empty.set_hexpand(true);
        row.append(&empty);
    }
    row
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
            let duration = state.metadata.duration_seconds.max(0.001);
            let seconds = (x.clamp(0.0, width) / width) * duration;
            if n_press >= 2 {
                state.playhead_seconds = seconds;
                state.add_zoom_at_playhead();
            } else {
                let selected = state.zoom_clips.iter().position(|clip| {
                    seconds >= clip.start && seconds <= clip.end
                });
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
            let duration = state.metadata.duration_seconds.max(0.001);
            let seconds = (x.clamp(0.0, width) / width) * duration;
            let edge = state.zoom_clips.iter().enumerate().find_map(|(index, clip)| {
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
            let duration = {
                let state = state.lock().unwrap();
                state.metadata.duration_seconds.max(0.001)
            };
            let seconds = ((start_x + offset_x).clamp(0.0, width) / width) * duration;
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
    let step = if duration > 90.0 {
        10.0
    } else if duration > 40.0 {
        5.0
    } else {
        2.0
    };
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.38);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    cr.set_line_width(1.0);
    let mut t = 0.0;
    while t <= duration + 0.001 {
        let x = (t / duration) * w;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.18);
        cr.move_to(x, h - 4.0);
        cr.line_to(x, h);
        let _ = cr.stroke();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
        let label = format!("{:.0}s", t);
        if let Ok(ext) = cr.text_extents(&label) {
            cr.move_to((x - ext.width() / 2.0).max(0.0), 11.0);
            let _ = cr.show_text(&label);
        }
        t += step;
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
    let inset_left = 24.0;
    let inset_right = 40.0;
    let usable = (w - inset_left - inset_right).max(1.0);
    let duration = state.metadata.duration_seconds.max(0.001);
    let x = inset_left + (state.playhead_seconds / duration) * usable;
    cr.set_source_rgba(0.69, 0.36, 0.22, 0.95);
    cr.set_line_width(1.5);
    cr.move_to(x, 18.0);
    cr.line_to(x, h - 2.0);
    let _ = cr.stroke();
    cr.arc(x, 14.0, 5.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
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
    let duration = state.metadata.duration_seconds.max(0.001);
    rounded_rect(cr, 0.0, 2.0, w, h - 4.0, 10.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.04);
    let _ = cr.fill();
    for (index, clip) in state.zoom_clips.iter().enumerate() {
        let x0 = (clip.start / duration) * w;
        let x1 = (clip.end / duration) * w;
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
            layout.push((seg_idx, x, x + seg_w));
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
    let duration = state.metadata.duration_seconds.max(0.001);
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
        let start_x = (state.trim_start_seconds / duration) * w;
        let end_x = (state.trim_end_seconds / duration) * w;
        draw_trim_capsule(cr, start_x, end_x, w, h);
    }
}

fn draw_trim_capsule(cr: &gtk4::cairo::Context, start_x: f64, end_x: f64, w: f64, h: f64) {
    let range_width = (end_x - start_x).max(36.0);
    let end_x = start_x + range_width;

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.48);
    cr.rectangle(0.0, 0.0, start_x.max(0.0), h);
    let _ = cr.fill();
    cr.rectangle(end_x.min(w), 0.0, (w - end_x).max(0.0), h);
    let _ = cr.fill();
    draw_capsule_frame(cr, start_x, end_x, h, false);
}

fn draw_capsule_frame(cr: &gtk4::cairo::Context, start_x: f64, end_x: f64, h: f64, emphasize: bool) {
    let range_width = (end_x - start_x).max(36.0);
    let end_x = start_x + range_width;
    let r = (h / 2.0).min(18.0);
    let handle_w = 18.0;
    let pad = 1.5;
    let stroke = if emphasize { 5.0 } else { 4.0 };

    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.set_line_width(stroke);
    rounded_rect(cr, start_x + pad, pad, range_width - pad * 2.0, h - pad * 2.0, r);
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

fn draw_trim_handle(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
    left: bool,
) {
    cr.set_source_rgb(0.69, 0.36, 0.22);
    cr.new_sub_path();
    if left {
        cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
        cr.line_to(x + w, y);
        cr.line_to(x + w, y + h);
        cr.arc(x + r, y + h - r, r, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
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
    cr.arc(x + r, y + h - r, r, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}:{seconds:04.1}")
}
