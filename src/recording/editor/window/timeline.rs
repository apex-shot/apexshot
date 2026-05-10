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

    let cut_button = Button::new();
    cut_button.add_css_class("recording-editor-cut-button");
    let cut_icon = Image::from_icon_name("edit-cut-symbolic");
    cut_icon.set_pixel_size(18);
    cut_button.set_child(Some(&cut_icon));
    cut_button.set_valign(Align::Center);
    cut_button.set_tooltip_text(Some("Cut"));
    let cut_mode = Rc::new(Cell::new(false));
    cut_button.connect_clicked({
        let cut_mode = cut_mode.clone();
        let cut_button = cut_button.clone();
        move |_| {
            let enabled = !cut_mode.get();
            cut_mode.set(enabled);
            if enabled {
                cut_button.add_css_class("recording-editor-cut-button-active");
            } else {
                cut_button.remove_css_class("recording-editor-cut-button-active");
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
    let state_for_play = state.clone();
    play_button.connect_clicked(move |_| {
        let is_playing = playing.get();
        if is_playing {
            media_play.pause();
            playing.set(false);
            let icon = Image::from_icon_name("media-playback-start-symbolic");
            icon.set_pixel_size(22);
            play_button_ref.set_child(Some(&icon));
        } else {
            // Skip to next kept segment if playhead is in a removed segment
            {
                let s = state_for_play.lock().unwrap();
                let boundaries = s.segment_boundaries();
                let playhead = s.playhead_seconds;
                let mut skip_to: Option<f64> = None;
                for (i, (seg_start, seg_end)) in boundaries.iter().enumerate() {
                    if playhead >= *seg_start && playhead < *seg_end {
                        if !s.segments_kept.get(i).copied().unwrap_or(true) {
                            for j in (i + 1)..boundaries.len() {
                                if s.segments_kept.get(j).copied().unwrap_or(true) {
                                    skip_to = Some(boundaries[j].0);
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
                drop(s);
                if let Some(target) = skip_to {
                    let mut s2 = state_for_play.lock().unwrap();
                    s2.playhead_seconds = target;
                    drop(s2);
                    media_play.seek((target * 1_000_000.0) as i64);
                }
            }
            media_play.play();
            playing.set(true);
            let icon = Image::from_icon_name("media-playback-pause-symbolic");
            icon.set_pixel_size(22);
            play_button_ref.set_child(Some(&icon));
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
        move |_, cr, width, height| {
            draw_trim_overlay(&state, cr, width, height);
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
            let kind = if let Some(cut_index) =
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
        move |_, _, _| {
            *drag_kind.borrow_mut() = None;
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
            let cursor_name = if cut_mode.get()
                && (x - start_x).abs() > handle_threshold
                && (x - end_x).abs() > handle_threshold
            {
                Some("crosshair")
            } else if (x - start_x).abs() <= handle_threshold {
                Some("w-resize")
            } else if (x - end_x).abs() <= handle_threshold {
                Some("e-resize")
            } else {
                // Check if near a cut line
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

    // Periodically sync playhead — skip removed segments during playback
    let media_playhead = media.clone();
    let selection_playhead = selection.clone();
    let state_playhead = state.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        if media_playhead.is_playing() {
            let ts_us = media_playhead.timestamp();
            if ts_us > 0 {
                let seconds = ts_us as f64 / 1_000_000.0;
                let mut seek_target = None;
                let mut should_pause = false;
                {
                    let mut s = state_playhead.lock().unwrap();
                    s.playhead_seconds = seconds;

                    // If playhead entered a removed segment, skip to next kept segment
                    let boundaries = s.segment_boundaries();
                    for (i, (seg_start, seg_end)) in boundaries.iter().enumerate() {
                        if seconds >= *seg_start && seconds < *seg_end {
                            if !s.segments_kept.get(i).copied().unwrap_or(true) {
                                if let Some(j) = ((i + 1)..boundaries.len())
                                    .find(|&j| s.segments_kept.get(j).copied().unwrap_or(true))
                                {
                                    let target = boundaries[j].0;
                                    s.playhead_seconds = target;
                                    seek_target = Some(target);
                                } else {
                                    should_pause = true;
                                }
                            }
                            break;
                        }
                    }
                }
                if let Some(target) = seek_target {
                    media_playhead.seek((target * 1_000_000.0) as i64);
                } else if should_pause {
                    media_playhead.pause();
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
}

fn draw_trim_overlay(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let duration = state.metadata.duration_seconds.max(0.001);
    let w = width as f64;
    let h = height as f64;
    let start_x = (state.trim_start_seconds / duration) * w;
    let end_x = (state.trim_end_seconds / duration) * w;
    let range_width = (end_x - start_x).max(1.0);
    let r = 4.0;
    let handle_w = 10.0;

    // Dimmed area outside trim (left)
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    cr.rectangle(0.0, 0.0, start_x, h);
    let _ = cr.fill();
    // Dimmed area outside trim (right)
    cr.rectangle(end_x, 0.0, w - end_x, h);
    let _ = cr.fill();

    // Draw removed segments as dimmed overlays
    let boundaries = state.segment_boundaries();
    for (i, (seg_start, seg_end)) in boundaries.iter().enumerate() {
        if !state.segments_kept.get(i).copied().unwrap_or(true) {
            let sx = (seg_start / duration) * w;
            let ex = (seg_end / duration) * w;
            // Dim the removed segment
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
            cr.rectangle(sx, 0.0, ex - sx, h);
            let _ = cr.fill();
            // Diagonal stripes to indicate removal
            cr.set_source_rgba(1.0, 0.3, 0.3, 0.25);
            cr.set_line_width(1.0);
            let stripe_spacing = 8.0;
            let mut offset = 0.0;
            while offset < (ex - sx) + h {
                cr.move_to(sx + (offset - h).max(0.0), (offset).min(h));
                cr.line_to(sx + offset.min(ex - sx), (offset - (ex - sx)).max(0.0));
                offset += stripe_spacing;
            }
            let _ = cr.stroke();
        }
    }

    // Draw cut lines
    cr.set_source_rgba(1.0, 0.4, 0.2, 0.9);
    cr.set_line_width(2.0);
    for &cut in &state.cuts {
        let cx = (cut / duration) * w;
        cr.move_to(cx, 0.0);
        cr.line_to(cx, h);
        let _ = cr.stroke();
        // Small diamond marker at top
        cr.move_to(cx, 0.0);
        cr.line_to(cx - 3.0, 5.0);
        cr.line_to(cx, 10.0);
        cr.line_to(cx + 3.0, 5.0);
        cr.close_path();
        let _ = cr.fill();
    }

    // Trim selection border (rounded)
    cr.set_source_rgba(0.69, 0.36, 0.22, 0.85);
    cr.set_line_width(1.5);
    let _ = cr.new_sub_path();
    cr.arc(start_x + r, r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.arc(start_x + range_width - r, r, r, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(start_x + range_width - r, h - r, r, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(start_x + r, h - r, r, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
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

    // Playhead line
    let playhead_x = (state.playhead_seconds / duration) * w;
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    cr.set_line_width(1.5);
    cr.move_to(playhead_x, 0.0);
    cr.line_to(playhead_x, h);
    let _ = cr.stroke();

    // Playhead top triangle
    cr.move_to(playhead_x - 4.0, 0.0);
    cr.line_to(playhead_x + 4.0, 0.0);
    cr.line_to(playhead_x, 6.0);
    cr.close_path();
    let _ = cr.fill();
}

fn update_time_labels(
    start_label: &Label,
    end_label: &Label,
    state: &Arc<Mutex<VideoEditState>>,
) {
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
