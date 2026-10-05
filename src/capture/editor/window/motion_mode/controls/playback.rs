use gtk4::{glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

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
        let last_clock = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let in_motion = in_motion.clone();
        let prefers_dark = session.prefers_dark;
        move |preview, _| {
            if !in_motion.get() {
                return glib::ControlFlow::Continue;
            }
            // Finished background composites land here (hover, scrub, and
            // playback all schedule through the same slot).
            super::super::preview::poll_preview_results(&session_runtime, preview, prefers_dark);
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
