use gtk4::{
    gdk, glib, prelude::*, DrawingArea, EventControllerMotion, GestureClick, GestureDrag, Overlay,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::super::{MotionHoverTrack, MotionModeParts, MotionRuntime, MotionSession};
use super::clip_menu::{show_clip_menu, ClipMenuTarget};
use super::{Redraw, RequestTextTransitionPreview, RequestTransitionPreview};

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
    request_transition_preview: RequestTransitionPreview,
    request_text_transition_preview: RequestTextTransitionPreview,
) {
    let ruler_click = GestureClick::new();
    ruler_click.set_button(1);
    ruler_click.connect_pressed({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        let preview = parts.shell.preview.clone();
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
            preview.queue_draw();
        }
    });
    parts.timeline.ruler.add_controller(ruler_click);

    // The ruler is the scrub surface: pressing it jumps the playhead and
    // dragging keeps it under the pointer, exactly like the video editor
    // card. The playhead itself is paint-only, so no widget follows the drag.
    let ruler_drag = GestureDrag::new();
    ruler_drag.set_button(1);
    ruler_drag.connect_drag_begin({
        let session = session.runtime.clone();
        let hover_playhead = parts.timeline.hover_playhead.clone();
        let motion_track = parts.timeline.motion_track.clone();
        let text_track = parts.timeline.text_track.clone();
        let preview = parts.shell.preview.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |_, _, _| {
            // A scrub owns the playhead: holding it pauses playback, and the
            // parked hover read-out is dropped so it cannot keep driving the
            // preview while the drag is in flight.
            let had_hover = {
                let mut runtime = session.borrow_mut();
                runtime.playing = false;
                runtime.last_tick = None;
                runtime.preview_end = None;
                // Scrub compositing stays inline (live) so the card tracks
                // the pointer: the paused worker path would keep blitting
                // the pre-drag frame while the playhead moves.
                runtime.live_preview = true;
                let had = runtime.hover_time.is_some() || runtime.hover_track.is_some();
                runtime.hover_time = None;
                runtime.hover_track = None;
                had
            };
            if had_hover {
                hover_playhead.queue_draw();
                motion_track.queue_draw();
                text_track.queue_draw();
                preview.queue_draw();
            }
            redraw_playhead();
        }
    });
    ruler_drag.connect_drag_update({
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        let preview = parts.shell.preview.clone();
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
            runtime.live_preview = true;
            drop(runtime);
            redraw_playhead();
            preview.queue_draw();
        }
    });
    ruler_drag.connect_drag_end({
        let session = session.runtime.clone();
        let hovered = parts.timeline.playhead_hovered.clone();
        let preview = parts.shell.preview.clone();
        let redraw_playhead = redraw_playhead.clone();
        move |_, _, _| {
            hovered.set(false);
            // Drop back to paused quality: the next paint schedules the
            // sharp (Good-filter) worker for the landed frame.
            session.borrow_mut().live_preview = false;
            redraw_playhead();
            // Release always lands the exact frame.
            preview.queue_draw();
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
        let request_transition_preview = request_transition_preview.clone();
        let motion_track_dragged = motion_track_dragged.clone();
        move |gesture, _n_press, x, _| {
            if motion_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let new_clip_start = {
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
                    None
                } else {
                    let time =
                        runtime
                            .motion
                            .snap_effect_time(raw_time, (10.0 / width) * duration, None);
                    runtime.begin_motion_edit();
                    let added = runtime.motion.add_segment_at(time);
                    runtime.motion.selected_text = None;
                    added.and_then(|index| runtime.motion.segments.get(index).map(|s| s.start))
                }
            };
            // A new clip starts on its identity frame, so leaving the playhead
            // where it was would keep the preview static. Replay the new move
            // immediately (same as editing a transform) so the motion is visible.
            redraw();
            if let Some(start) = new_clip_start {
                request_transition_preview(start);
            }
        }
    });
    parts.timeline.motion_track.add_controller(track_click);

    let motion_menu = GestureClick::new();
    motion_menu.set_button(3);
    motion_menu.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, y| {
            let Some(area) = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
            else {
                return;
            };
            let width = area.allocated_width().max(1) as f64;
            let height = area.allocated_height() as f64;
            if y < 7.0 || y > height - 7.0 {
                return;
            }
            let index = {
                let mut runtime = session.borrow_mut();
                let duration = runtime.motion.duration.max(0.001);
                let Some(index) =
                    runtime
                        .motion
                        .segments
                        .iter()
                        .enumerate()
                        .find_map(|(index, segment)| {
                            let x0 = (segment.start / duration).clamp(0.0, 1.0) * width;
                            let x1 = (segment.end / duration).clamp(0.0, 1.0) * width;
                            (x >= x0 && x <= x0 + (x1 - x0).max(22.0)).then_some(index)
                        })
                else {
                    return;
                };
                runtime.motion.selected = Some(index);
                runtime.motion.selected_text = None;
                runtime.source_selected = false;
                index
            };
            redraw();
            show_clip_menu(
                &area,
                ClipMenuTarget::Motion(index),
                x,
                y,
                session.clone(),
                redraw.clone(),
            );
        }
    });
    parts.timeline.motion_track.add_controller(motion_menu);

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
        let request_text_transition_preview = request_text_transition_preview.clone();
        let text_track_dragged = text_track_dragged.clone();
        move |gesture, _n_press, x, _| {
            if text_track_dragged.replace(false) {
                return;
            }
            let width = gesture
                .widget()
                .map(|widget| widget.allocated_width().max(1) as f64)
                .unwrap_or(1.0);
            let new_text = {
                let mut runtime = session.borrow_mut();
                runtime.source_selected = false;
                let duration = runtime.motion.duration.max(0.001);
                let raw_time = ((x / width) * duration).clamp(0.0, duration);
                // Same single-click rule as the motion lane.
                if let Some(index) = runtime.motion.text_index_at(raw_time) {
                    runtime.motion.selected_text = Some(index);
                    runtime.motion.selected = None;
                    None
                } else {
                    let time =
                        runtime
                            .motion
                            .snap_text_time(raw_time, (10.0 / width) * duration, None);
                    runtime.begin_motion_edit();
                    let added = runtime.motion.add_text_at(time);
                    runtime.motion.selected = None;
                    added.and_then(|index| {
                        runtime
                            .motion
                            .text_segments
                            .get(index)
                            .map(|s| (s.start, s.entrance_seconds()))
                    })
                }
            };
            redraw();
            if let Some((start, entrance_seconds)) = new_text {
                request_text_transition_preview(start, entrance_seconds);
            }
        }
    });
    parts.timeline.text_track.add_controller(text_click);

    let text_menu = GestureClick::new();
    text_menu.set_button(3);
    text_menu.connect_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |gesture, _, x, y| {
            let Some(area) = gesture
                .widget()
                .and_then(|widget| widget.downcast::<DrawingArea>().ok())
            else {
                return;
            };
            let width = area.allocated_width().max(1) as f64;
            let height = area.allocated_height() as f64;
            if y < 6.0 || y > height - 6.0 {
                return;
            }
            let index =
                {
                    let mut runtime = session.borrow_mut();
                    let duration = runtime.motion.duration.max(0.001);
                    let Some(index) = runtime.motion.text_segments.iter().enumerate().find_map(
                        |(index, segment)| {
                            let x0 = (segment.start / duration).clamp(0.0, 1.0) * width;
                            let x1 = (segment.end / duration).clamp(0.0, 1.0) * width;
                            (x >= x0 && x <= x0 + (x1 - x0).max(22.0)).then_some(index)
                        },
                    ) else {
                        return;
                    };
                    runtime.motion.selected_text = Some(index);
                    runtime.motion.selected = None;
                    runtime.source_selected = false;
                    index
                };
            redraw();
            show_clip_menu(
                &area,
                ClipMenuTarget::Text(index),
                x,
                y,
                session.clone(),
                redraw.clone(),
            );
        }
    });
    parts.timeline.text_track.add_controller(text_menu);

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

    // Hover is tracked on the whole board: only the capsule head expands into
    // the clock pill, so the stem and lanes below it stay inert. Hit-testing
    // uses the drawn head's model-space x, never a widget allocation.
    if let Some(board) = parts
        .timeline
        .playhead_overlay
        .ancestor(Overlay::static_type())
    {
        let hover = EventControllerMotion::new();
        hover.set_propagation_phase(gtk4::PropagationPhase::Capture);
        hover.connect_motion({
            let session = session.runtime.clone();
            let hovered = parts.timeline.playhead_hovered.clone();
            let hover_playhead = parts.timeline.hover_playhead.clone();
            let motion_track = parts.timeline.motion_track.clone();
            let text_track = parts.timeline.text_track.clone();
            let drag_kind = drag_kind.clone();
            let text_drag_kind = text_drag_kind.clone();
            move |controller, x, y| {
                if drag_kind.get().is_some() || text_drag_kind.get().is_some() {
                    let (had_hover, was_previewing) = {
                        let mut runtime = session.borrow_mut();
                        let was = super::super::preview::hover_preview_frame(
                            &runtime.motion,
                            runtime.hover_time,
                            runtime.hover_track,
                            runtime.playing,
                        )
                        .is_some();
                        let had = runtime.hover_time.is_some() || runtime.hover_track.is_some();
                        runtime.hover_time = None;
                        runtime.hover_track = None;
                        (had, was)
                    };
                    if had_hover {
                        hover_playhead.queue_draw();
                        motion_track.queue_draw();
                        text_track.queue_draw();
                        if was_previewing {
                            session.borrow_mut().preview_dirty = true;
                        }
                    }
                    return;
                }
                let board = controller.widget();
                let width = board
                    .as_ref()
                    .map(|widget| widget.allocated_width().max(1) as f64)
                    .unwrap_or(1.0);
                // Hover time is directionless: scrubbing right-to-left drives
                // the same hover frame as left-to-right, so clips also play
                // backwards under the red line.
                let (near, was_previewing, is_previewing, prev_track, next_track) = {
                    let mut runtime = session.borrow_mut();
                    let was = super::super::preview::hover_preview_frame(
                        &runtime.motion,
                        runtime.hover_time,
                        runtime.hover_track,
                        runtime.playing,
                    )
                    .is_some();
                    let prev_track = runtime.hover_track;
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
                    let next_track = runtime.hover_track;
                    let is = super::super::preview::hover_preview_frame(
                        &runtime.motion,
                        runtime.hover_time,
                        runtime.hover_track,
                        runtime.playing,
                    )
                    .is_some();
                    (
                        super::super::super::motion_timeline::playhead_head_hit(x, y, line_x),
                        was,
                        is,
                        prev_track,
                        next_track,
                    )
                };
                // Like the video editor card: the hover line repaints every
                // motion, but each lane only repaints while the pointer is
                // over it (the add ghost follows the line) or on the change
                // that shows/clears it. Hovering the ruler repaints one
                // overlay instead of the whole timeline.
                hover_playhead.queue_draw();
                if prev_track != next_track {
                    if prev_track == Some(MotionHoverTrack::Motion)
                        || next_track == Some(MotionHoverTrack::Motion)
                    {
                        motion_track.queue_draw();
                    }
                    if prev_track == Some(MotionHoverTrack::Text)
                        || next_track == Some(MotionHoverTrack::Text)
                    {
                        text_track.queue_draw();
                    }
                } else if next_track == Some(MotionHoverTrack::Motion) {
                    motion_track.queue_draw();
                } else if next_track == Some(MotionHoverTrack::Text) {
                    text_track.queue_draw();
                }
                if was_previewing || is_previewing {
                    session.borrow_mut().preview_dirty = true;
                }
                if hovered.replace(near) != near {
                    // The cursor follows the drawn head, so grabbing it works
                    // from the ruler without a positioned handle widget.
                    if let Some(widget) = controller.widget() {
                        let cursor = near
                            .then(|| gdk::Cursor::from_name("ew-resize", None))
                            .flatten();
                        widget.set_cursor(cursor.as_ref());
                    }
                }
            }
        });
        hover.connect_leave({
            let session = session.runtime.clone();
            let hovered = parts.timeline.playhead_hovered.clone();
            let hover_playhead = parts.timeline.hover_playhead.clone();
            let motion_track = parts.timeline.motion_track.clone();
            let text_track = parts.timeline.text_track.clone();
            move |controller| {
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
                    session.borrow_mut().preview_dirty = true;
                }
                if let Some(widget) = controller.widget() {
                    widget.set_cursor(None);
                }
                hovered.set(false);
            }
        });
        board.add_controller(hover);
    }

    parts.timeline.add_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let request_transition_preview = request_transition_preview.clone();
        move |_| {
            let new_clip_start = {
                let mut runtime = session.borrow_mut();
                let playhead = runtime.motion.playhead;
                runtime.source_selected = false;
                runtime.begin_motion_edit();
                runtime
                    .motion
                    .add_segment_at(playhead)
                    .and_then(|index| runtime.motion.segments.get(index).map(|s| s.start))
            };
            redraw();
            if let Some(start) = new_clip_start {
                request_transition_preview(start);
            }
        }
    });

    parts.timeline.add_text_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let request_text_transition_preview = request_text_transition_preview.clone();
        move |_| {
            let new_text = {
                let mut runtime = session.borrow_mut();
                let playhead = runtime.motion.playhead;
                runtime.source_selected = false;
                runtime.begin_motion_edit();
                runtime.motion.add_text_at(playhead).and_then(|index| {
                    runtime
                        .motion
                        .text_segments
                        .get(index)
                        .map(|s| (s.start, s.entrance_seconds()))
                })
            };
            redraw();
            if let Some((start, entrance_seconds)) = new_text {
                request_text_transition_preview(start, entrance_seconds);
            }
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
    let last_cursor = Rc::new(Cell::new(None::<&'static str>));
    let motion_cursor = last_cursor.clone();
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
            let edge_at = |start: f64, end: f64| {
                if (time - end).abs() <= edge_seconds {
                    Some("e-resize")
                } else if (time - start).abs() <= edge_seconds {
                    Some("w-resize")
                } else {
                    None
                }
            };
            if text_track {
                runtime
                    .motion
                    .text_segments
                    .iter()
                    .find_map(|segment| edge_at(segment.start, segment.end))
            } else {
                runtime
                    .motion
                    .segments
                    .iter()
                    .find_map(|segment| edge_at(segment.start, segment.end))
            }
        };
        let cursor_name = cursor_name.unwrap_or("default");
        if motion_cursor.get() != Some(cursor_name) {
            if let Some(widget) = controller.widget() {
                widget.set_cursor(gdk::Cursor::from_name(cursor_name, None).as_ref());
            }
            motion_cursor.set(Some(cursor_name));
        }
    });
    pointer.connect_leave({
        let last_cursor = last_cursor.clone();
        move |controller| {
            if let Some(widget) = controller.widget() {
                widget.set_cursor(None);
            }
            last_cursor.set(None);
        }
    });
    track.add_controller(pointer);
}
