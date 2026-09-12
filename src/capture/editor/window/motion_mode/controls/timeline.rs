use gtk4::{
    gdk, glib, prelude::*, DrawingArea, EventControllerMotion, GestureClick, GestureDrag, Overlay,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::super::{MotionModeParts, MotionRuntime, MotionSession};
use super::Redraw;

#[derive(Clone, Copy)]
enum DragKind {
    Start(usize),
    End(usize),
    Body { index: usize, origin: f64 },
}

fn drag_kind_index(kind: DragKind) -> usize {
    match kind {
        DragKind::Start(index) | DragKind::End(index) | DragKind::Body { index, .. } => index,
    }
}

pub(super) fn install(
    parts: &MotionModeParts,
    session: &MotionSession,
    redraw: Redraw,
    redraw_playhead: Redraw,
    redraw_motion_track: Redraw,
    redraw_text_track: Redraw,
) {
    let ruler_click = GestureClick::new();
    ruler_click.set_button(1);
    ruler_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead = ((x / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw_playhead();
        }
    });
    parts.timeline.ruler.add_controller(ruler_click);

    // The ruler is a scrub surface: dragging anywhere moves the playhead,
    // rather than requiring a pixel-perfect hit on the thin playhead line.
    let ruler_drag = GestureDrag::new();
    ruler_drag.set_button(1);
    ruler_drag.connect_drag_update({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead =
                (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw_playhead();
        }
    });
    parts.timeline.ruler.add_controller(ruler_drag);

    // The source thumbnail lane is an actual scrub target, matching the
    // timeline's visible source segment instead of being decorative chrome.
    let source_click = GestureClick::new();
    source_click.set_button(1);
    source_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |gesture, _, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead = ((x / width) * duration).clamp(0.0, duration);
            drop(runtime);
            redraw_playhead();
        }
    });
    parts.timeline.source_track.add_controller(source_click);

    // A press on a clip may become a drag. Defer the expensive inspector
    // refresh until release so the first pointer move is never blocked by a
    // full preview render. Clicking empty track space only changes selection;
    // the playhead is moved by dragging the playhead handle.
    let motion_track_dragged = Rc::new(Cell::new(false));
    let track_click = GestureClick::new();
    track_click.set_button(1);
    track_click.connect_released({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let motion_track_dragged = motion_track_dragged.clone();
        move |gesture, n_press, x, _| {
            if motion_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = runtime.motion.snap_effect_time(
                ((x / width) * duration).clamp(0.0, duration),
                (10.0 / width) * duration,
                None,
            );
            if n_press >= 2 {
                runtime.begin_motion_edit();
                if runtime.motion.add_segment_at(time).is_none() {
                    runtime.motion.selected = runtime.motion.segment_index_at(time);
                    runtime.motion.selected_text = None;
                }
            } else if let Some(index) = runtime.motion.segment_index_at(time) {
                runtime.motion.selected = Some(index);
                runtime.motion.selected_text = None;
            } else {
                runtime.motion.selected = None;
                runtime.motion.selected_text = None;
            }
            drop(runtime);
            redraw();
        }
    });
    parts.timeline.motion_track.add_controller(track_click);

    let drag_kind = Rc::new(Cell::new(None::<DragKind>));
    let drag = GestureDrag::new();
    drag.set_button(1);
    drag.connect_drag_begin({
        let session = session.runtime.clone();
        let drag_kind = drag_kind.clone();
        let motion_track_dragged = motion_track_dragged.clone();
        let redraw_motion_track = redraw_motion_track.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let kind = runtime
                .motion
                .segments
                .iter()
                .enumerate()
                .find_map(|(index, segment)| {
                    if (segment.start - time).abs() <= edge_seconds {
                        Some(DragKind::Start(index))
                    } else if (segment.end - time).abs() <= edge_seconds {
                        Some(DragKind::End(index))
                    } else if time >= segment.start && time <= segment.end {
                        Some(DragKind::Body {
                            index,
                            origin: segment.start,
                        })
                    } else {
                        None
                    }
                });
            if let Some(index) = kind.map(drag_kind_index) {
                runtime.motion.selected = Some(index);
                runtime.motion.selected_text = None;
            }
            // One checkpoint per drag: the pre-drag state is what Undo
            // restores, and the drag updates themselves stay checkpoint-free.
            if kind.is_some() {
                runtime.begin_motion_edit();
            }
            drop(runtime);
            drag_kind.set(kind);
            motion_track_dragged.set(kind.is_some());
            if kind.is_some() {
                redraw_motion_track();
            }
        }
    });
    drag.connect_drag_update({
        let session = session.runtime.clone();
        let drag_kind = drag_kind.clone();
        let redraw_motion_track = redraw_motion_track.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = drag_kind.get() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            let tolerance = (10.0 / width) * duration;
            let index = match kind {
                DragKind::Start(index) | DragKind::End(index) => index,
                DragKind::Body { index, .. } => index,
            };
            let before = runtime
                .motion
                .segments
                .get(index)
                .map(|segment| (segment.start, segment.end));
            match kind {
                DragKind::Start(index) => {
                    let time = runtime
                        .motion
                        .snap_effect_time(raw_time, tolerance, Some(index));
                    let end = runtime
                        .motion
                        .segments
                        .get(index)
                        .map(|segment| segment.end)
                        .unwrap_or(time);
                    runtime.motion.set_segment_range(index, time, end);
                }
                DragKind::End(index) => {
                    let time = runtime
                        .motion
                        .snap_effect_time(raw_time, tolerance, Some(index));
                    let start = runtime
                        .motion
                        .segments
                        .get(index)
                        .map(|segment| segment.start)
                        .unwrap_or(time);
                    runtime.motion.set_segment_range(index, start, time);
                }
                DragKind::Body { index, origin } => {
                    let delta = (offset_x / width) * duration;
                    let start =
                        runtime
                            .motion
                            .snap_effect_time(origin + delta, tolerance, Some(index));
                    runtime.motion.move_segment(index, start);
                }
            }
            let changed = before
                != runtime
                    .motion
                    .segments
                    .get(index)
                    .map(|segment| (segment.start, segment.end));
            drop(runtime);
            if changed {
                redraw_motion_track();
            }
        }
    });
    drag.connect_drag_end({
        let drag_kind = drag_kind.clone();
        let redraw = redraw.clone();
        let motion_track_dragged = motion_track_dragged.clone();
        move |_, _, _| {
            drag_kind.set(None);
            redraw();
            // GestureClick's release can arrive before or after GestureDrag's
            // end callback. Keep this suppression through the release phase,
            // then clear it even on toolkits that do not emit that click.
            let reset_dragged = motion_track_dragged.clone();
            glib::idle_add_local_once(move || reset_dragged.set(false));
        }
    });
    parts.timeline.motion_track.add_controller(drag);
    install_track_end_cursor(&parts.timeline.motion_track, session.runtime.clone(), false);

    let text_track_dragged = Rc::new(Cell::new(false));
    let text_click = GestureClick::new();
    text_click.set_button(1);
    text_click.connect_released({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let text_track_dragged = text_track_dragged.clone();
        move |gesture, n_press, x, _| {
            if text_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = runtime.motion.snap_text_time(
                ((x / width) * duration).clamp(0.0, duration),
                (10.0 / width) * duration,
                None,
            );
            if n_press >= 2 {
                runtime.begin_motion_edit();
                if runtime.motion.add_text_at(time).is_none() {
                    runtime.motion.selected_text = runtime.motion.text_index_at(time);
                    runtime.motion.selected = None;
                }
            } else if let Some(index) = runtime.motion.text_index_at(time) {
                runtime.motion.selected_text = Some(index);
                runtime.motion.selected = None;
            } else {
                runtime.motion.selected_text = None;
                runtime.motion.selected = None;
            }
            drop(runtime);
            redraw();
        }
    });
    parts.timeline.text_track.add_controller(text_click);

    let text_drag_kind = Rc::new(Cell::new(None::<DragKind>));
    let text_drag = GestureDrag::new();
    text_drag.set_button(1);
    text_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let text_drag_kind = text_drag_kind.clone();
        let text_track_dragged = text_track_dragged.clone();
        let redraw_text_track = redraw_text_track.clone();
        move |gesture, x, _| {
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let kind =
                runtime
                    .motion
                    .text_segments
                    .iter()
                    .enumerate()
                    .find_map(|(index, segment)| {
                        if (segment.start - time).abs() <= edge_seconds {
                            Some(DragKind::Start(index))
                        } else if (segment.end - time).abs() <= edge_seconds {
                            Some(DragKind::End(index))
                        } else if time >= segment.start && time <= segment.end {
                            Some(DragKind::Body {
                                index,
                                origin: segment.start,
                            })
                        } else {
                            None
                        }
                    });
            if let Some(index) = kind.map(drag_kind_index) {
                runtime.motion.selected_text = Some(index);
                runtime.motion.selected = None;
            }
            // Same checkpoint-per-drag rule as the motion lane.
            if kind.is_some() {
                runtime.begin_motion_edit();
            }
            drop(runtime);
            text_drag_kind.set(kind);
            text_track_dragged.set(kind.is_some());
            if kind.is_some() {
                redraw_text_track();
            }
        }
    });
    text_drag.connect_drag_update({
        let session = session.runtime.clone();
        let text_drag_kind = text_drag_kind.clone();
        let redraw_text_track = redraw_text_track.clone();
        move |gesture, offset_x, _| {
            let Some(kind) = text_drag_kind.get() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            let tolerance = (10.0 / width) * duration;
            let index = match kind {
                DragKind::Start(index) | DragKind::End(index) => index,
                DragKind::Body { index, .. } => index,
            };
            let before = runtime
                .motion
                .text_segments
                .get(index)
                .map(|segment| (segment.start, segment.end));
            match kind {
                DragKind::Start(index) => {
                    let time = runtime
                        .motion
                        .snap_text_time(raw_time, tolerance, Some(index));
                    let end = runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|segment| segment.end)
                        .unwrap_or(time);
                    runtime.motion.set_text_range(index, time, end);
                }
                DragKind::End(index) => {
                    let time = runtime
                        .motion
                        .snap_text_time(raw_time, tolerance, Some(index));
                    let start = runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|segment| segment.start)
                        .unwrap_or(time);
                    runtime.motion.set_text_range(index, start, time);
                }
                DragKind::Body { index, origin } => {
                    let delta = (offset_x / width) * duration;
                    let start =
                        runtime
                            .motion
                            .snap_text_time(origin + delta, tolerance, Some(index));
                    runtime.motion.move_text(index, start);
                }
            }
            let changed = before
                != runtime
                    .motion
                    .text_segments
                    .get(index)
                    .map(|segment| (segment.start, segment.end));
            drop(runtime);
            if changed {
                redraw_text_track();
            }
        }
    });
    text_drag.connect_drag_end({
        let text_drag_kind = text_drag_kind.clone();
        let redraw = redraw.clone();
        let text_track_dragged = text_track_dragged.clone();
        move |_, _, _| {
            text_drag_kind.set(None);
            redraw();
            let reset_dragged = text_track_dragged.clone();
            glib::idle_add_local_once(move || reset_dragged.set(false));
        }
    });
    parts.timeline.text_track.add_controller(text_drag);
    install_track_end_cursor(&parts.timeline.text_track, session.runtime.clone(), true);

    // The playhead only moves by dragging its handle. The pointer's board x
    // is the handle's *allocated* position plus the widget-local pointer x;
    // reading the margin property instead lags the real layout by a frame and
    // made the playhead overshoot and oscillate under a moving pointer.
    let playhead_drag = GestureDrag::new();
    playhead_drag.set_button(1);
    playhead_drag.connect_drag_begin({
        let session = session.runtime.clone();
        move |_, _, _| {
            // Holding the handle pauses playback; otherwise the timer keeps
            // advancing the playhead the drag is trying to reposition.
            let mut runtime = session.borrow_mut();
            runtime.playing = false;
            runtime.last_tick = None;
            runtime.preview_end = None;
        }
    });
    playhead_drag.connect_drag_update({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        let handle = parts.timeline.playhead_handle.clone();
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = gesture
                .widget()
                .and_then(|widget| widget.ancestor(Overlay::static_type()))
                .map(|board| board.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let pointer_board_x = handle.allocation().x() as f64 + start_x + offset_x;
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            runtime.motion.playhead = (pointer_board_x / width * duration).clamp(0.0, duration);
            drop(runtime);
            redraw_playhead();
        }
    });
    parts.timeline.playhead_handle.add_controller(playhead_drag);

    let handle_pointer = EventControllerMotion::new();
    handle_pointer.connect_enter(move |controller, _, _| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(gdk::Cursor::from_name("ew-resize", None).as_ref());
        }
    });
    handle_pointer.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });
    parts
        .timeline
        .playhead_handle
        .add_controller(handle_pointer);

    parts.timeline.add_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            let playhead = runtime.motion.playhead;
            runtime.begin_motion_edit();
            let _ = runtime.motion.add_segment_at(playhead);
            drop(runtime);
            redraw();
        }
    });

    parts.timeline.add_text_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            let playhead = runtime.motion.playhead;
            runtime.begin_motion_edit();
            let _ = runtime.motion.add_text_at(playhead);
            drop(runtime);
            redraw();
        }
    });
}

/// Advertise the existing trim handles before a drag starts. Motion and Text
/// clips both accept edge drags, so the pointer must make those narrow targets
/// discoverable rather than looking like ordinary timeline space.
fn install_track_end_cursor(
    track: &DrawingArea,
    runtime: Rc<RefCell<MotionRuntime>>,
    text_track: bool,
) {
    let pointer = EventControllerMotion::new();
    pointer.connect_motion(move |controller, x, _| {
        let width = controller
            .widget()
            .map(|widget| widget.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let cursor_name = {
            let runtime = runtime.borrow();
            let duration = runtime.motion.duration.max(0.001);
            let time = ((x / width) * duration).clamp(0.0, duration);
            let edge_seconds = (8.0 / width) * duration;
            let segments = if text_track {
                runtime
                    .motion
                    .text_segments
                    .iter()
                    .map(|segment| (segment.start, segment.end))
                    .collect::<Vec<_>>()
            } else {
                runtime
                    .motion
                    .segments
                    .iter()
                    .map(|segment| (segment.start, segment.end))
                    .collect::<Vec<_>>()
            };
            segments.iter().find_map(|(start, end)| {
                if (time - end).abs() <= edge_seconds {
                    Some("e-resize")
                } else if (time - start).abs() <= edge_seconds {
                    Some("w-resize")
                } else {
                    None
                }
            })
        };
        if let Some(widget) = controller.widget() {
            widget.set_cursor(
                gdk::Cursor::from_name(cursor_name.unwrap_or("default"), None).as_ref(),
            );
        }
    });
    pointer.connect_leave(|controller| {
        if let Some(widget) = controller.widget() {
            widget.set_cursor(None);
        }
    });
    track.add_controller(pointer);
}
