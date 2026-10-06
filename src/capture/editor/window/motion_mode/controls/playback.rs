use gtk4::{glib, prelude::*, DrawingArea, GestureDrag};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use super::super::session::AutoPreviewLane;
use super::super::{MotionModeParts, MotionSession};
use super::Redraw;

pub(super) fn install_primary(parts: &MotionModeParts, session: &MotionSession, redraw: Redraw) {
    parts.timeline.play_btn.connect_clicked({
        let session = session.runtime.clone();
        let hover_playhead = parts.timeline.hover_playhead.clone();
        let motion_track = parts.timeline.motion_track.clone();
        let text_track = parts.timeline.text_track.clone();
        let redraw = redraw.clone();
        move |_| {
            {
                let mut runtime = session.borrow_mut();
                runtime.cancel_auto_preview();
                runtime.playing = !runtime.playing;
                runtime.last_tick = if runtime.playing {
                    Some(Instant::now())
                } else {
                    None
                };
                // Playback must win over a parked hover scrub; otherwise a
                // pointer resting over a lane could keep the preview on its
                // frozen hover frame while the playhead advances alone.
                if runtime.playing {
                    runtime.preview_end = None;
                    runtime.hover_time = None;
                    runtime.hover_track = None;
                }
                if runtime.playing && runtime.motion.playhead >= runtime.motion.duration {
                    runtime.motion.playhead = 0.0;
                }
            }
            hover_playhead.queue_draw();
            motion_track.queue_draw();
            text_track.queue_draw();
            redraw();
        }
    });
    parts.timeline.skip_back.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            runtime.cancel_auto_preview();
            runtime.motion.playhead = (runtime.motion.playhead - 1.0).max(0.0);
            drop(runtime);
            redraw();
        }
    });
}

pub(super) fn install_timer(
    parts: &MotionModeParts,
    session: &MotionSession,
    redraw: Redraw,
    in_motion: Rc<Cell<bool>>,
) {
    parts.timeline.skip_forward.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            let mut runtime = session.borrow_mut();
            runtime.cancel_auto_preview();
            let duration = runtime.motion.duration;
            runtime.motion.playhead = (runtime.motion.playhead + 1.0).min(duration);
            drop(runtime);
            redraw();
        }
    });

    parts.shell.preview.add_tick_callback({
        let session_runtime = session.runtime.clone();
        let redraw = redraw.clone();
        let playhead_overlay = parts.timeline.playhead_overlay.clone();
        let playhead_clock = parts.timeline.playhead_clock.clone();
        let undo_btn = parts.timeline.undo_btn.clone();
        let redo_btn = parts.timeline.redo_btn.clone();
        let drag_widgets = vec![
            parts.transform.scale_slider.widget(),
            parts.transform.intensity_slider.widget(),
            parts.transform.anchor_pad.widget(),
            parts.transform.yaw_slider.widget(),
            parts.transform.pitch_slider.widget(),
            parts.transform.roll_slider.widget(),
            parts.transform.perspective_slider.widget(),
            parts.transform.position_pad.widget(),
            parts.transform.pos_x_slider.widget(),
            parts.transform.pos_y_slider.widget(),
            parts.transform.ease_slider.widget(),
            parts.transform.spring_bounce_slider.widget(),
            parts.transform.easing_x1_slider.widget(),
            parts.transform.easing_y1_slider.widget(),
            parts.transform.easing_x2_slider.widget(),
            parts.transform.easing_y2_slider.widget(),
            parts.text.text_opacity_slider.widget(),
            parts.text.text_width_slider.widget(),
            parts.text.text_size_slider.widget(),
            parts.text.text_line_spacing_slider.widget(),
            parts.text.text_letter_spacing_slider.widget(),
            parts.text.text_rotation_slider.widget(),
            parts.text.text_outline_slider.widget(),
            parts.text.text_transition_slider.widget(),
            parts.text.text_typewriter_slider.widget(),
            parts.text.text_pos_pad.widget(),
            parts.shell.preview.clone(),
            parts.timeline.ruler.clone(),
            parts.timeline.motion_track.clone(),
            parts.timeline.text_track.clone(),
        ];
        let last_clock = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let in_motion = in_motion.clone();
        let prefers_dark = session.prefers_dark;
        move |preview, _| {
            if !in_motion.get() {
                session_runtime.borrow_mut().cancel_auto_preview();
                return glib::ControlFlow::Continue;
            }
            let (can_undo, can_redo) = session_runtime.borrow().motion_history_availability();
            if undo_btn.is_sensitive() != can_undo {
                undo_btn.set_sensitive(can_undo);
            }
            if redo_btn.is_sensitive() != can_redo {
                redo_btn.set_sensitive(can_redo);
            }
            // Finished background composites land here (hover, scrub, and
            // playback all schedule through the same slot).
            super::super::preview::poll_preview_results(&session_runtime, preview, prefers_dark);
            let now = Instant::now();
            let pending_ready = session_runtime
                .borrow()
                .pending_auto_preview
                .is_some_and(|pending| now >= pending.ready_at);
            let pending = if pending_ready {
                let drag_active = drag_widgets.iter().any(gesture_drag_is_active);
                session_runtime
                    .borrow_mut()
                    .take_ready_auto_preview(now, drag_active)
            } else {
                session_runtime.borrow_mut().cancel_stale_auto_preview();
                None
            };
            if let Some(pending) = pending {
                let mut runtime = session_runtime.borrow_mut();
                match pending.lane {
                    AutoPreviewLane::Motion => {
                        let clip_end = runtime
                            .motion
                            .selected_segment()
                            .map(|segment| segment.end)
                            .unwrap_or(pending.start);
                        runtime.motion.playhead = pending.start;
                        super::start_motion_transition_preview(&mut runtime, pending.start);
                        let full_clip_end = runtime.preview_end.unwrap_or(pending.end);
                        runtime.preview_end = Some(pending.end.min(full_clip_end).min(clip_end));
                    }
                    AutoPreviewLane::Text => {
                        let clip_end = runtime
                            .motion
                            .selected_text_segment()
                            .map(|segment| segment.end)
                            .unwrap_or(pending.start);
                        runtime.motion.playhead = pending.start;
                        runtime.playing = true;
                        runtime.last_tick = Some(Instant::now());
                        runtime.preview_end = Some(pending.end.min(clip_end));
                    }
                }
                drop(runtime);
                redraw();
            }
            let playing = session_runtime.borrow().playing;
            if playing {
                let mut runtime = session_runtime.borrow_mut();
                if runtime.playing {
                    let now = Instant::now();
                    let dt = runtime
                        .last_tick
                        .map(|last| now.duration_since(last).as_secs_f64())
                        .unwrap_or(0.0);
                    runtime.last_tick = Some(now);
                    runtime.motion.playhead += dt;
                    let preview_done = runtime
                        .preview_end
                        .is_some_and(|end| runtime.motion.playhead >= end);
                    if preview_done {
                        runtime.motion.playhead = runtime.preview_end.take().unwrap_or(0.0);
                        runtime.playing = false;
                        runtime.last_tick = None;
                    } else if runtime.motion.playhead >= runtime.motion.duration {
                        runtime.motion.playhead = 0.0;
                        runtime.playing = false;
                        runtime.last_tick = None;
                        runtime.preview_end = None;
                    }
                }
                let playhead = runtime.motion.playhead;
                let stopped = !runtime.playing;
                drop(runtime);
                if stopped {
                    redraw();
                } else {
                    let clock = super::super::super::motion_timeline::format_clock(playhead);
                    if *last_clock.borrow() != clock {
                        *last_clock.borrow_mut() = clock.clone();
                        playhead_clock.set_text(&clock);
                    }
                    preview.queue_draw();
                    playhead_overlay.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        }
    });
}

fn gesture_drag_is_active(widget: &DrawingArea) -> bool {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items()).any(|index| {
        controllers
            .item(index)
            .and_then(|controller| controller.downcast::<GestureDrag>().ok())
            .is_some_and(|gesture| gesture.is_active())
    })
}
