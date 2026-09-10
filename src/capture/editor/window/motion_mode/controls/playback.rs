use gtk4::{glib, prelude::*};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use super::super::{MotionModeParts, MotionSession};
use super::Redraw;

/// Motion is an interactive camera tool, so target a display-refresh cadence
/// instead of the old 30 Hz edit-preview timer. `Instant` still supplies the
/// elapsed time, which keeps the animation duration independent of frames
/// that GTK may skip under load.
const MOTION_PREVIEW_FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

pub(super) fn install_primary(parts: &MotionModeParts, session: &MotionSession, redraw: Redraw) {
    parts.timeline.play_btn.connect_clicked({
        let session = session.runtime.clone();
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
                // Manual playback always runs the whole composition; only an
                // edit-triggered preview stops early.
                if runtime.playing {
                    runtime.preview_end = None;
                }
                if runtime.playing && runtime.motion.playhead >= runtime.motion.duration {
                    runtime.motion.playhead = 0.0;
                }
            }
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

    glib::timeout_add_local(MOTION_PREVIEW_FRAME_INTERVAL, {
        let session_runtime = session.runtime.clone();
        let redraw = redraw.clone();
        let preview = parts.shell.preview.clone();
        let playhead_overlay = parts.timeline.playhead_overlay.clone();
        let playhead_clock = parts.timeline.playhead_clock.clone();
        let in_motion = in_motion.clone();
        move || {
            if !in_motion.get() {
                return glib::ControlFlow::Continue;
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
                    playhead_clock.set_text(&super::super::super::motion_timeline::format_clock(
                        playhead,
                    ));
                    preview.queue_draw();
                    playhead_overlay.queue_draw();
                }
            }
            glib::ControlFlow::Continue
        }
    });
}
