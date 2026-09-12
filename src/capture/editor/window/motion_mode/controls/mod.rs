use gtk4::{gdk, glib, prelude::*, EventControllerKey};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use crate::i18n::t;
use crate::recording::editor::model::MotionState;

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
    // Playhead changes do not alter the inspector or track geometry. Updating
    // only the moving pieces avoids running every slider sync and a full
    // timeline repaint for each scrub event or animation frame.
    // Fast path: this runs per pointer event during a scrub, so it must not
    // allocate, hit i18n, or invalidate layout unless something visible
    // actually changed. `queue_draw` coalesces to one frame; label/icon and
    // ruler work are guarded to ~1Hz.
    let redraw_playhead: Redraw = {
        let session = session.runtime.clone();
        let preview = parts.shell.preview.clone();
        let ruler = parts.timeline.ruler.clone();
        let playhead_overlay = parts.timeline.playhead_overlay.clone();
        let playhead_clock = parts.timeline.playhead_clock.clone();
        let play_btn = parts.timeline.play_btn.clone();
        let handle = parts.timeline.playhead_handle.clone();
        let dragging = parts.timeline.playhead_dragging.clone();
        let hovered = parts.timeline.playhead_hovered.clone();
        let pause_text = t("Pause");
        let play_text = t("Play");
        let last_clock = Rc::new(RefCell::new(String::new()));
        let last_playing = Rc::new(Cell::new(None::<bool>));
        Rc::new(move || {
            let (playhead, playing, is_dragging, is_hovered) = {
                let runtime = session.borrow();
                (
                    runtime.motion.playhead,
                    runtime.playing,
                    dragging.get(),
                    hovered.get(),
                )
            };
            // Clock + ruler tick at 1Hz; the playhead line itself moves every
            // event via the overlay draw below.
            let clock = super::super::motion_timeline::format_clock(playhead);
            if *last_clock.borrow() != clock {
                *last_clock.borrow_mut() = clock.clone();
                playhead_clock.set_text(&clock);
                ruler.queue_draw();
            }
            if last_playing.get() != Some(playing) {
                last_playing.set(Some(playing));
                play_btn
                    .set_tooltip_text(Some(if playing { &pause_text } else { &play_text }));
                if let Some(image) = play_btn
                    .child()
                    .and_then(|child| child.downcast::<gtk4::Image>().ok())
                {
                    image.set_icon_name(Some(if playing {
                        "media-playback-pause-symbolic"
                    } else {
                        "media-playback-start-symbolic"
                    }));
                }
            }
            if !is_dragging {
                let board_w = playhead_overlay.allocated_width().max(1) as f64;
                super::super::motion_timeline::sync_playhead_handle(
                    &handle,
                    &session,
                    board_w,
                    is_dragging || is_hovered,
                );
            }
            preview.queue_draw();
            playhead_overlay.queue_draw();
        })
    };

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

    // Editing a timed clip parameter has to be judged in motion. Play through
    // the complete clip so the automatic preview and its timeline block have
    // the same visible duration.
    let request_transition_preview = {
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
        Rc::new(move |segment_start: f64| {
            let mut runtime = session.borrow_mut();
            let (start, end) = motion_transition_preview_range(&runtime.motion, segment_start);
            let inside = runtime.motion.playhead >= start && runtime.motion.playhead <= end;
            if !inside {
                runtime.motion.playhead = start;
            }
            runtime.playing = true;
            runtime.last_tick = Some(Instant::now());
            runtime.preview_end = Some(end);
            drop(runtime);
            redraw_playhead();
        })
    };

    // Text entrances run on their own clock (typewriter reveal or the shared
    // 0.28s slide), so picking an animation replays the title's entrance
    // instead of leaving the static preview on its invisible first frame.
    let request_text_transition_preview = {
        let session = session.runtime.clone();
        let redraw_playhead = redraw_playhead.clone();
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
            redraw_playhead();
        })
    };

    sync::install_shared(parts, session, redraw.clone(), request_live_preview.clone());

    playback::install_primary(parts, session, redraw.clone());

    timeline::install(
        parts,
        session,
        redraw.clone(),
        redraw_playhead,
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
            let mut runtime = session.borrow_mut();
            runtime.begin_motion_edit();
            runtime.motion.remove_selected();
            drop(runtime);
            redraw();
        }
    });

    // Timeline history: the same stacks power the dock buttons and the
    // Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) shortcuts.
    parts.timeline.undo_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            if session.borrow_mut().undo_motion() {
                redraw();
            }
        }
    });
    parts.timeline.redo_btn.connect_clicked({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        move |_| {
            if session.borrow_mut().redo_motion() {
                redraw();
            }
        }
    });

    let delete_keys = EventControllerKey::new();
    delete_keys.connect_key_pressed({
        let session = session.runtime.clone();
        let redraw = redraw.clone();
        let in_motion = in_motion.clone();
        move |_, key, _, state| {
            if !in_motion.get() {
                return glib::Propagation::Proceed;
            }
            if key == gdk::Key::z && state.contains(gdk::ModifierType::CONTROL_MASK) {
                let changed = if state.contains(gdk::ModifierType::SHIFT_MASK) {
                    session.borrow_mut().redo_motion()
                } else {
                    session.borrow_mut().undo_motion()
                };
                if changed {
                    redraw();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::y && state.contains(gdk::ModifierType::CONTROL_MASK) {
                let changed = session.borrow_mut().redo_motion();
                if changed {
                    redraw();
                }
                return glib::Propagation::Stop;
            }
            if key != gdk::Key::Delete && key != gdk::Key::BackSpace {
                return glib::Propagation::Proceed;
            }
            let mut runtime = session.borrow_mut();
            runtime.begin_motion_edit();
            let changed = runtime.motion.remove_selected();
            drop(runtime);
            if changed {
                redraw();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    parts.shell.page.add_controller(delete_keys);

    playback::install_timer(parts, session, redraw, in_motion);
}

fn motion_transition_preview_range(motion: &MotionState, segment_start: f64) -> (f64, f64) {
    let start = segment_start.clamp(0.0, motion.duration.max(0.0));
    let end = motion
        .segments
        .iter()
        .find(|segment| (segment.start - start).abs() < 1e-6)
        .map(|segment| segment.end)
        .unwrap_or(start)
        .clamp(start, motion.duration.max(start));
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_preview_uses_the_complete_motion_clip() {
        let mut motion = MotionState::default();
        motion.add_segment_at(0.0).expect("motion clip");
        motion.set_selected_transition_ms(300);

        let (start, end) = motion_transition_preview_range(&motion, 0.0);
        assert!((start - motion.segments[0].start).abs() < f64::EPSILON);
        assert!((end - motion.segments[0].end).abs() < f64::EPSILON);
        assert!(end > motion.transform_timing.transition_duration);
    }
}
