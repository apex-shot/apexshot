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
    media: MediaFile,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("recording-editor-timeline");
    root.set_hexpand(true);
    root.set_vexpand(false);
    root.set_size_request(-1, 64);

    let card = GtkBox::new(Orientation::Horizontal, 0);
    card.add_css_class("recording-editor-timeline-card");
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

    card.append(&play_button);

    let timeline_vbox = GtkBox::new(Orientation::Vertical, 4);
    timeline_vbox.set_hexpand(true);
    timeline_vbox.set_vexpand(false);

    let overlay = Overlay::new();
    overlay.add_css_class("recording-editor-trim-area");
    overlay.set_hexpand(true);
    overlay.set_vexpand(false);
    overlay.set_size_request(-1, 48);

    let strip = GtkBox::new(Orientation::Horizontal, 0);
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_vexpand(false);
    strip.set_halign(Align::Fill);
    strip.set_valign(Align::Center);
    strip.set_size_request(-1, 48);
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
            picture.set_size_request(-1, 44);
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
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
                .map(|area| area.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut state_guard = state.lock().unwrap();
            let duration = state_guard.metadata.duration_seconds.max(0.001);
            let start_x = (state_guard.trim_start_seconds / duration) * width;
            let end_x = (state_guard.trim_end_seconds / duration) * width;
            let handle_threshold = 12.0;
            let seconds = (x.clamp(0.0, width) / width) * duration;
            let cut_threshold_seconds = (10.0 / width) * duration;

            let kind = if move_mode.get() && !state_guard.cuts.is_empty() {
                // Move mode: use visual layout to find which segment was clicked
                let layout = compute_visual_layout(&state_guard, width);
                if let Some(opos) = visual_x_to_order_pos(&layout, x) {
                    let seg_idx = layout[opos].0;
                    dragging_segment.set(Some(seg_idx));
                    TrimDragKind::Segment(opos)
                } else {
                    TrimDragKind::Playhead
                }
            } else if let Some(cut_index) =
                nearest_cut_index(&state_guard, seconds, cut_threshold_seconds)
            {
                TrimDragKind::Cut(cut_index)
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
            } else {
                state_guard.playhead_seconds = seconds;
                media.seek((seconds * 1_000_000.0) as i64);
                selection.queue_draw();
                TrimDragKind::Playhead
            };
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
            let seconds = (value_x / width) * duration;
            match kind {
                TrimDragKind::Start => state_guard.set_trim_start(seconds),
                TrimDragKind::End => state_guard.set_trim_end(seconds),
                TrimDragKind::Cut(cut_index) => state_guard.move_cut(cut_index, seconds),
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
            if matches!(kind, TrimDragKind::Start | TrimDragKind::End) {
                update_time_labels(&start_label, &end_label, &state);
            }
        }
    });
    drag.connect_drag_end({
        let drag_kind = drag_kind.clone();
        let dragging_segment = dragging_segment.clone();
        let selection = selection.clone();
        move |_, _, _| {
            *drag_kind.borrow_mut() = None;
            dragging_segment.set(None);
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
            let handle_threshold = 12.0;
            let cursor_name = if move_mode.get() && !state.cuts.is_empty() {
                Some("grab")
            } else if cut_mode.get()
                && (x - start_x).abs() > handle_threshold
                && (x - end_x).abs() > handle_threshold
            {
                Some("crosshair")
            } else if (x - start_x).abs() <= handle_threshold {
                Some("w-resize")
            } else if (x - end_x).abs() <= handle_threshold {
                Some("e-resize")
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
    // After a seek, ignore timer until media reaches near the target (avoids seek loops)
    let pending_seek: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if media_playhead.is_playing() {
            let ts_us = media_playhead.timestamp();
            if ts_us > 0 {
                let seconds = ts_us as f64 / 1_000_000.0;

                // If we're waiting for a seek to land, check if we're close enough
                if let Some(target) = pending_seek.get() {
                    if (seconds - target).abs() > 0.5 {
                        // Media hasn't reached the seek target yet, skip this tick
                        selection_playhead.queue_draw();
                        return glib::ControlFlow::Continue;
                    }
                    // Seek landed, clear the flag
                    pending_seek.set(None);
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
                    pending_seek.set(Some(target));
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

    timeline_vbox.append(&overlay);
    let time_row = GtkBox::new(Orientation::Horizontal, 0);
    time_row.append(&start_label);
    time_row.append(&end_label);
    timeline_vbox.append(&time_row);

    card.append(&timeline_vbox);
    let tools_box = GtkBox::new(Orientation::Horizontal, 6);
    tools_box.add_css_class("recording-editor-timeline-tools");
    tools_box.set_halign(Align::End);
    tools_box.set_valign(Align::Center);
    tools_box.append(&cut_button);
    tools_box.append(&move_button);
    tools_box.append(&revert_button);
    card.append(&tools_box);
    root.append(&card);
    root
}

#[derive(Clone, Copy)]
enum TrimDragKind {
    Start,
    End,
    Playhead,
    Cut(usize),
    Segment(usize), // order position being dragged
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

/// Map a chronological playhead time to a visual x position using the layout.
fn playhead_to_visual_x(state: &VideoEditState, layout: &[(usize, f64, f64)]) -> f64 {
    let boundaries = state.segment_boundaries();
    let ph = state.playhead_seconds;
    for &(seg_idx, vx_start, vx_end) in layout {
        if let Some(&(seg_start, seg_end)) = boundaries.get(seg_idx) {
            if ph >= seg_start && ph < seg_end {
                let frac = (ph - seg_start) / (seg_end - seg_start).max(0.001);
                return vx_start + frac * (vx_end - vx_start);
            }
        }
    }
    // Fallback: after last segment
    layout.last().map(|&(_, _, xe)| xe).unwrap_or(0.0)
}

/// Map a visual x position to the order position index in the layout.
fn visual_x_to_order_pos(layout: &[(usize, f64, f64)], x: f64) -> Option<usize> {
    for (i, &(_, vx_start, vx_end)) in layout.iter().enumerate() {
        if x >= vx_start && x < vx_end {
            return Some(i);
        }
    }
    // If past end, return last
    if !layout.is_empty() {
        Some(layout.len() - 1)
    } else {
        None
    }
}

/// Alternating segment tint colors
fn segment_color(order_pos: usize) -> (f64, f64, f64) {
    match order_pos % 4 {
        0 => (0.69, 0.36, 0.22),
        1 => (0.30, 0.55, 0.65),
        2 => (0.50, 0.65, 0.30),
        _ => (0.60, 0.35, 0.60),
    }
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
        // === Segment mode: draw segments in output order with proportional widths ===
        let layout = compute_visual_layout(&state, w);

        // Dark background behind everything
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // Draw each segment as a colored block
        cr.select_font_face(
            "sans-serif",
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Bold,
        );
        cr.set_font_size(11.0);
        for (order_pos, &(seg_idx, vx_start, vx_end)) in layout.iter().enumerate() {
            let seg_w = vx_end - vx_start;
            let (cr_r, cr_g, cr_b) = segment_color(order_pos);
            let is_dragging = dragging_seg_idx == Some(seg_idx);

            // Segment fill
            let alpha = if is_dragging { 0.45 } else { 0.25 };
            cr.set_source_rgba(cr_r, cr_g, cr_b, alpha);
            cr.rectangle(vx_start, 0.0, seg_w, h);
            let _ = cr.fill();

            // Segment border
            if is_dragging {
                cr.set_source_rgba(cr_r, cr_g, cr_b, 0.95);
                cr.set_line_width(2.5);
            } else {
                cr.set_source_rgba(cr_r, cr_g, cr_b, 0.6);
                cr.set_line_width(1.0);
            }
            cr.rectangle(vx_start + 0.5, 0.5, seg_w - 1.0, h - 1.0);
            let _ = cr.stroke();

            // Order number badge
            let mid_x = (vx_start + vx_end) / 2.0;
            let num = format!("{}", order_pos + 1);
            let radius = 9.0;
            cr.set_source_rgba(cr_r, cr_g, cr_b, 0.9);
            cr.arc(mid_x, h / 2.0, radius, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
            if let Ok(ext) = cr.text_extents(&num) {
                cr.move_to(mid_x - ext.width() / 2.0, h / 2.0 + ext.height() / 2.0);
                let _ = cr.show_text(&num);
            }
        }

        // Divider lines between segments
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.3);
        cr.set_line_width(1.0);
        for &(_, _, vx_end) in layout.iter().take(layout.len().saturating_sub(1)) {
            cr.move_to(vx_end, 0.0);
            cr.line_to(vx_end, h);
            let _ = cr.stroke();
        }

        // Draw removed segments indicator at bottom
        let boundaries = state.segment_boundaries();
        let removed_count = state.segments_kept.iter().filter(|&&k| !k).count();
        if removed_count > 0 {
            cr.set_font_size(9.0);
            cr.set_source_rgba(1.0, 0.4, 0.4, 0.7);
            let text = format!("{removed_count} removed");
            if let Ok(ext) = cr.text_extents(&text) {
                cr.move_to(w - ext.width() - 4.0, h - 3.0);
                let _ = cr.show_text(&text);
            }
            let _ = boundaries; // suppress unused
        }

        // Playhead mapped to visual position
        let playhead_vx = playhead_to_visual_x(&state, &layout);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        cr.set_line_width(1.5);
        cr.move_to(playhead_vx, 0.0);
        cr.line_to(playhead_vx, h);
        let _ = cr.stroke();
        cr.move_to(playhead_vx - 4.0, 0.0);
        cr.line_to(playhead_vx + 4.0, 0.0);
        cr.line_to(playhead_vx, 6.0);
        cr.close_path();
        let _ = cr.fill();
    } else {
        // === Simple trim mode: no cuts ===
        let start_x = (state.trim_start_seconds / duration) * w;
        let end_x = (state.trim_end_seconds / duration) * w;
        let range_width = (end_x - start_x).max(1.0);
        let r = 4.0;
        let handle_w = 10.0;

        // Dimmed area outside trim
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
        cr.rectangle(0.0, 0.0, start_x, h);
        let _ = cr.fill();
        cr.rectangle(end_x, 0.0, w - end_x, h);
        let _ = cr.fill();

        // Trim selection border (rounded)
        cr.set_source_rgba(0.69, 0.36, 0.22, 0.85);
        cr.set_line_width(1.5);
        cr.new_sub_path();
        cr.arc(
            start_x + r,
            r,
            r,
            std::f64::consts::PI,
            1.5 * std::f64::consts::PI,
        );
        cr.arc(
            start_x + range_width - r,
            r,
            r,
            -0.5 * std::f64::consts::PI,
            0.0,
        );
        cr.arc(
            start_x + range_width - r,
            h - r,
            r,
            0.0,
            0.5 * std::f64::consts::PI,
        );
        cr.arc(
            start_x + r,
            h - r,
            r,
            0.5 * std::f64::consts::PI,
            std::f64::consts::PI,
        );
        cr.close_path();
        let _ = cr.stroke();

        // Left handle grip
        cr.set_source_rgba(0.91, 0.46, 0.29, 0.9);
        cr.set_line_width(1.5);
        let grip_y_start = h * 0.25;
        let grip_y_end = h * 0.75;
        let grip_spacing = 4.0;
        let mut y = grip_y_start;
        while y + 2.0 <= grip_y_end {
            cr.move_to(start_x + handle_w / 2.0 - 2.0, y);
            cr.line_to(start_x + handle_w / 2.0 - 2.0, y + 2.0);
            cr.move_to(start_x + handle_w / 2.0 + 2.0, y);
            cr.line_to(start_x + handle_w / 2.0 + 2.0, y + 2.0);
            y += grip_spacing;
        }
        let _ = cr.stroke();

        // Right handle grip
        y = grip_y_start;
        while y + 2.0 <= grip_y_end {
            cr.move_to(end_x - handle_w / 2.0 - 2.0, y);
            cr.line_to(end_x - handle_w / 2.0 - 2.0, y + 2.0);
            cr.move_to(end_x - handle_w / 2.0 + 2.0, y);
            cr.line_to(end_x - handle_w / 2.0 + 2.0, y + 2.0);
            y += grip_spacing;
        }
        let _ = cr.stroke();

        // Playhead
        let playhead_x = (state.playhead_seconds / duration) * w;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
        cr.set_line_width(1.5);
        cr.move_to(playhead_x, 0.0);
        cr.line_to(playhead_x, h);
        let _ = cr.stroke();
        cr.move_to(playhead_x - 4.0, 0.0);
        cr.line_to(playhead_x + 4.0, 0.0);
        cr.line_to(playhead_x, 6.0);
        cr.close_path();
        let _ = cr.fill();
    }
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

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}:{seconds:04.1}")
}
