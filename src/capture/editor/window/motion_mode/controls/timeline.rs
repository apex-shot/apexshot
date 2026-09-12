use gtk4::{
    gdk, glib, prelude::*, DrawingArea, EventControllerMotion, GestureClick, GestureDrag, Overlay,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::super::{MotionHoverTrack, MotionModeParts, MotionRuntime, MotionSession};
use super::Redraw;

/// Whether a board-relative pointer height falls inside `lane`.
/// The lane allocation is parent-relative, so it must be translated into
/// board coordinates first: comparing it raw drops hover mid-lane whenever
/// the tracks box sits below other card chrome, which snaps the hover preview
/// back to the playhead frame and reads as a reset/flicker while scrubbing
/// (forwards or backwards).
fn pointer_in_lane(lane: &DrawingArea, board: &gtk4::Widget, pointer_y: f64) -> bool {
    let height = lane.allocated_height() as f64;
    let top = lane
        .translate_coordinates(board, 0.0, 0.0)
        .map(|(_, top)| top)
        .unwrap_or_else(|| lane.allocation().y() as f64);
    pointer_y >= top && pointer_y <= top + height
}

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
            let next = (((start_x + offset_x) / width) * duration).clamp(0.0, duration);
            if (next - runtime.motion.playhead).abs() < f64::EPSILON {
                return;
            }
            runtime.motion.playhead = next;
            drop(runtime);
            redraw_playhead();
        }
    });
    parts.timeline.ruler.add_controller(ruler_drag);

    // The source thumbnail lane is selectable chrome, not a scrub surface:
    // scrubbing belongs to the ruler and the playhead handle. Clicking it
    // selects the source clip and clears any effect-clip selection.
    let source_click = GestureClick::new();
    source_click.set_button(1);
    source_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_, _, _, _| {
            let mut runtime = session.borrow_mut();
            runtime.source_selected = true;
            runtime.motion.selected = None;
            runtime.motion.selected_text = None;
            drop(runtime);
            redraw();
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
        move |gesture, _n_press, x, _| {
            if motion_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            runtime.source_selected = false;
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = ((x / width) * duration).clamp(0.0, duration);
            // Selection tests the real pointer position: snapping it first
            // could land on the next clip's edge and select it while the user
            // clicked in the empty gap before it.
            if let Some(index) = runtime.motion.segment_index_at(raw_time) {
                runtime.motion.selected = Some(index);
                runtime.motion.selected_text = None;
            } else {
                let time =
                    runtime
                        .motion
                        .snap_effect_time(raw_time, (10.0 / width) * duration, None);
                runtime.begin_motion_edit();
                if runtime.motion.add_segment_at(time).is_none() {
                    runtime.motion.selected = None;
                }
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
                runtime.source_selected = false;
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
        move |gesture, _n_press, x, _| {
            if text_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let mut runtime = session.borrow_mut();
            runtime.source_selected = false;
            let duration = runtime.motion.duration.max(0.001);
            let raw_time = ((x / width) * duration).clamp(0.0, duration);
            // Same single-click rule as the motion lane.
            if let Some(index) = runtime.motion.text_index_at(raw_time) {
                runtime.motion.selected_text = Some(index);
                runtime.motion.selected = None;
            } else {
                let time = runtime
                    .motion
                    .snap_text_time(raw_time, (10.0 / width) * duration, None);
                runtime.begin_motion_edit();
                if runtime.motion.add_text_at(time).is_none() {
                    runtime.motion.selected_text = None;
                }
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
                runtime.source_selected = false;
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

    // The playhead only moves by dragging its handle. The handle's allocation
    // is frozen for the duration of the drag (see motion_timeline.rs), so the
    // board position captured at drag begin plus the gesture offset tracks the
    // pointer exactly instead of chasing a layout that changes under it.
    let playhead_drag = GestureDrag::new();
    playhead_drag.set_button(1);
    let playhead_start_x = Rc::new(Cell::new(0.0f64));
    // Board width is stable for the duration of a drag; resolving the Overlay
    // ancestor per motion event walks GObjects on the hot path for no gain.
    let playhead_board_w = Rc::new(Cell::new(1.0f64));
    playhead_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let handle = parts.timeline.playhead_handle.clone();
        let start_x = playhead_start_x.clone();
        let board_w = playhead_board_w.clone();
        let dragging = parts.timeline.playhead_dragging.clone();
        let hover_playhead = parts.timeline.hover_playhead.clone();
        let motion_track = parts.timeline.motion_track.clone();
        let text_track = parts.timeline.text_track.clone();
        let preview = parts.shell.preview.clone();
        move |gesture, x, _| {
            // Holding the handle pauses playback; otherwise the timer keeps
            // advancing the playhead the drag is trying to reposition.
            // A stale hover would otherwise keep driving the preview (hover
            // scrub wins while idle), so drop it: the drag owns the preview
            // until release.
            let had_hover = {
                let mut runtime = session.borrow_mut();
                runtime.playing = false;
                runtime.last_tick = None;
                runtime.preview_end = None;
                let had = runtime.hover_time.is_some() || runtime.hover_track.is_some();
                runtime.hover_time = None;
                runtime.hover_track = None;
                had
            };
            start_x.set(handle.allocation().x() as f64 + x);
            board_w.set(
                gesture
                    .widget()
                    .and_then(|widget| widget.ancestor(Overlay::static_type()))
                    .map(|board| board.allocated_width().max(1) as f64)
                    .unwrap_or(1.0),
            );
            dragging.set(true);
            if had_hover {
                hover_playhead.queue_draw();
                motion_track.queue_draw();
                text_track.queue_draw();
                preview.queue_draw();
            }
        }
    });
    playhead_drag.connect_drag_update({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        let start_x = playhead_start_x.clone();
        let board_w = playhead_board_w.clone();
        move |_, offset_x, _| {
            let width = board_w.get().max(1.0);
            let pointer_board_x = start_x.get() + offset_x;
            let mut runtime = session.borrow_mut();
            let duration = runtime.motion.duration.max(0.001);
            let next = (pointer_board_x / width * duration).clamp(0.0, duration);
            // Shoving against either end emits events with an identical
            // clamped value; skip the redraw entirely instead of re-queuing
            // a preview render that would paint the same frame.
            if (next - runtime.motion.playhead).abs() < f64::EPSILON {
                return;
            }
            runtime.motion.playhead = next;
            drop(runtime);
            redraw_playhead();
        }
    });
    playhead_drag.connect_drag_end({
        let dragging = parts.timeline.playhead_dragging.clone();
        let hovered = parts.timeline.playhead_hovered.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |_, _, _| {
            dragging.set(false);
            // Pointer grabs suppress board motion events, so the pre-drag
            // hover value can survive a drag that ends away from the head and
            // leave the clock pill stuck open. Collapse until motion proves
            // the pointer is back on the capsule.
            hovered.set(false);
            redraw_playhead();
        }
    });
    parts.timeline.playhead_handle.add_controller(playhead_drag);

    // The handle strip is only as wide as the idle capsule, so hover is
    // tracked on the whole board: only the capsule head expands into the clock
    // pill, so the stem and lanes below it stay inert.
    if let Some(board) = parts
        .timeline
        .playhead_handle
        .ancestor(Overlay::static_type())
    {
        let hover = EventControllerMotion::new();
        hover.connect_motion({
            let session = session.runtime.clone();
            let hovered = parts.timeline.playhead_hovered.clone();
            let hover_playhead = parts.timeline.hover_playhead.clone();
            let motion_track = parts.timeline.motion_track.clone();
            let text_track = parts.timeline.text_track.clone();
            let preview = parts.shell.preview.clone();
            let redraw_playhead = redraw_playhead.clone();
            move |controller, x, y| {
                let board = controller.widget();
                let width = board
                    .as_ref()
                    .map(|widget| widget.allocated_width().max(1) as f64)
                    .unwrap_or(1.0);
                // Hover time is directionless: scrubbing right-to-left drives
                // the same hover frame as left-to-right, so clips also play
                // backwards under the red line.
                let (near, was_previewing, is_previewing) = {
                    let mut runtime = session.borrow_mut();
                    let was = super::super::preview::hover_preview_frame(
                        &runtime.motion,
                        runtime.hover_time,
                        runtime.hover_track,
                        runtime.playing,
                    )
                    .is_some();
                    let duration = runtime.motion.duration.max(0.001);
                    runtime.hover_time = Some((x / width).clamp(0.0, 1.0) * duration);
                    let line_x = (runtime.motion.playhead / duration).clamp(0.0, 1.0) * width;
                    runtime.hover_track = match board.as_ref() {
                        Some(board) if pointer_in_lane(&motion_track, board, y) => {
                            Some(MotionHoverTrack::Motion)
                        }
                        Some(board) if pointer_in_lane(&text_track, board, y) => {
                            Some(MotionHoverTrack::Text)
                        }
                        _ => None,
                    };
                    let is = super::super::preview::hover_preview_frame(
                        &runtime.motion,
                        runtime.hover_time,
                        runtime.hover_track,
                        runtime.playing,
                    )
                    .is_some();
                    (
                        super::super::super::motion_timeline::playhead_head_hit(
                            x,
                            y,
                            line_x,
                            hovered.get(),
                        ),
                        was,
                        is,
                    )
                };
                // The add ghost is anchored to hover_time, so the lanes must
                // repaint on every motion, not just when the lane changes.
                hover_playhead.queue_draw();
                motion_track.queue_draw();
                text_track.queue_draw();
                // Hover scrub drives the preview (red line), never the
                // playhead. Repaint when entering, scrubbing inside, or
                // leaving a lane so the preview snaps back to the playhead.
                if was_previewing || is_previewing {
                    preview.queue_draw();
                }
                if hovered.replace(near) != near {
                    redraw_playhead();
                }
            }
        });
        hover.connect_leave({
            let session = session.runtime.clone();
            let hovered = parts.timeline.playhead_hovered.clone();
            let hover_playhead = parts.timeline.hover_playhead.clone();
            let motion_track = parts.timeline.motion_track.clone();
            let text_track = parts.timeline.text_track.clone();
            let preview = parts.shell.preview.clone();
            let redraw_playhead = redraw_playhead.clone();
            move |_| {
                let (lane_changed, was_previewing) = {
                    let mut runtime = session.borrow_mut();
                    let was = super::super::preview::hover_preview_frame(
                        &runtime.motion,
                        runtime.hover_time,
                        runtime.hover_track,
                        runtime.playing,
                    )
                    .is_some();
                    runtime.hover_time = None;
                    let changed = runtime.hover_track.is_some();
                    runtime.hover_track = None;
                    (changed, was)
                };
                hover_playhead.queue_draw();
                if lane_changed {
                    motion_track.queue_draw();
                    text_track.queue_draw();
                }
                // Snap the preview back from the hover frame to the
                // playhead frame, but only if it was showing a hover frame:
                // leaving empty lane space changes nothing.
                if was_previewing {
                    preview.queue_draw();
                }
                if hovered.replace(false) {
                    redraw_playhead();
                }
            }
        });
        board.add_controller(hover);
    }

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
            runtime.source_selected = false;
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
            runtime.source_selected = false;
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
