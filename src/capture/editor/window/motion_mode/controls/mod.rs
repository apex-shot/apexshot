use gtk4::{gdk, glib, prelude::*, EventControllerKey};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use super::{MotionModeChrome, MotionModeParts, MotionSession};

mod playback;
mod sync;
mod text;
mod timeline;
mod transform;

pub(super) type Redraw = Rc<dyn Fn()>;
pub(super) type RequestLivePreview = Rc<dyn Fn()>;
pub(super) type RequestTransitionPreview = Rc<dyn Fn(f64)>;
pub(super) type RequestTextTransitionPreview = Rc<dyn Fn(f64, f64)>;

pub(in crate::capture::editor::window) fn wire_motion_controls(
    parts: &MotionModeParts,
    session: &MotionSession,
    _chrome: Rc<MotionModeChrome>,
    _last_inspector: Rc<RefCell<String>>,
    in_motion: Rc<Cell<bool>>,
) {
    let redraw = sync::make_redraw(parts, session);

    // Segment trimming and movement can generate far more pointer updates than
    // the expensive perspective preview can render. Keep the direct-manipulation
    // path limited to the lane being dragged; the full preview and inspector
    // catch up once the pointer is released.
    let redraw_motion_track = {
        let motion_track = parts.timeline.motion_track.clone();
        Rc::new(move || motion_track.queue_draw())
    };
    let redraw_text_track = {
        let text_track = parts.timeline.text_track.clone();
        Rc::new(move || text_track.queue_draw())
    };

    let request_live_preview = {
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let gen = Rc::new(Cell::new(0u32));
        Rc::new(move || {
            session.borrow_mut().live_preview = true;
            preview.queue_draw();
            let token = gen.get().wrapping_add(1);
            gen.set(token);
            let session = session.clone();
            let preview = preview.clone();
            let gen = gen.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
                if gen.get() != token {
                    return glib::ControlFlow::Break;
                }
                session.borrow_mut().live_preview = false;
                preview.queue_draw();
                glib::ControlFlow::Break
            });
        })
    };

    // Editing a timed clip parameter has to be judged in motion: a static
    // frame at the playhead is identical before and after most edits. Like
    // Shotbase, play the affected transition once. If the playhead is already
    // inside the transition window the preview continues from there instead
    // of restarting.
    let request_transition_preview = {
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        Rc::new(move |segment_start: f64| {
            let mut runtime = session.borrow_mut();
            let transition = runtime
                .motion
                .transform_timing
                .clamped()
                .transition_duration;
            let start = segment_start.max(0.0).min(runtime.motion.duration);
            let end = (start + transition + 0.4).min(runtime.motion.duration);
            let inside = runtime.motion.playhead >= start && runtime.motion.playhead <= end;
            if !inside {
                runtime.motion.playhead = start;
            }
            runtime.playing = true;
            runtime.last_tick = Some(Instant::now());
            runtime.preview_end = Some(end);
            drop(runtime);
            redraw();
        })
    };

    // Text entrances run on their own clock (typewriter reveal or the shared
    // 0.28s slide), so picking an animation replays the title's entrance
    // instead of leaving the static preview on its invisible first frame.
    let request_text_transition_preview = {
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        Rc::new(move |text_start: f64, typewriter_time: f64| {
            let mut runtime = session.borrow_mut();
            let span = typewriter_time.max(0.28) + 0.25;
            let start = text_start.max(0.0).min(runtime.motion.duration);
            let end = (start + span).min(runtime.motion.duration);
            let inside = runtime.motion.playhead >= start && runtime.motion.playhead <= end;
            if !inside {
                runtime.motion.playhead = start;
            }
            runtime.playing = true;
            runtime.last_tick = Some(Instant::now());
            runtime.preview_end = Some(end);
            drop(runtime);
            redraw();
        })
    };

    sync::install_shared(parts, session, redraw.clone(), request_live_preview.clone());

    playback::install_primary(parts, session, redraw.clone());

    timeline::install(
        parts,
        session,
        redraw.clone(),
        redraw_motion_track,
        redraw_text_track,
    );

    text::install(
        parts,
        session,
        request_live_preview.clone(),
        request_text_transition_preview.clone(),
    );

    transform::install(
        parts,
        session,
        redraw.clone(),
        request_live_preview.clone(),
        request_transition_preview.clone(),
    );

    parts.shared.delete_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            session.borrow_mut().motion.remove_selected();
            redraw();
        }
    });

    let delete_keys = EventControllerKey::new();
    delete_keys.connect_key_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let in_motion = in_motion.clone();
        move |_, key, _, _| {
            if !in_motion.get() {
                return glib::Propagation::Proceed;
            }
            if key != gdk::Key::Delete && key != gdk::Key::BackSpace {
                return glib::Propagation::Proceed;
            }
            if session.borrow_mut().motion.remove_selected() {
                redraw();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    parts.shell.page.add_controller(delete_keys);

    playback::install_timer(parts, session, redraw, in_motion);
}
