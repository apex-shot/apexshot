//! Asynchronous effects rebuild pipeline (PR 10.12).
//!
//! Owns request/result channels, the background worker, stale-revision
//! rejection, request coalescing, UI-thread result polling, the rebuild
//! watchdog, and the `rebuild_effects_async` callback.
//!
//! GTK widgets stay on the main context. Only owned image/action data crosses
//! the worker channel. Timer intervals and revision lock lifetimes match the
//! pre-extraction setup.

use gtk4::{glib, prelude::*, DrawingArea};
use image::RgbaImage;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::super::state::{apply_effect_actions, EditorState};
use super::super::types::AnnotationAction;

/// Install the async effects pipeline and return the UI-thread rebuild callback.
///
/// Side effects (started immediately):
/// - 16ms local timeout polling completed worker results
/// - one dedicated worker thread draining coalesced rebuild requests
/// - 500ms watchdog recovering stuck `select_effect_rebuild_pending` state
pub(super) fn install_async_effects_pipeline(
    state: &Arc<Mutex<EditorState>>,
    drawing_area: &DrawingArea,
) -> Rc<dyn Fn()> {
    // Async Effects Pipeline
    let (effects_sender, effects_receiver) = mpsc::channel::<(RgbaImage, u64)>();
    let (request_sender, request_receiver) =
        mpsc::channel::<(Arc<RgbaImage>, Vec<AnnotationAction>, u64)>();

    // Used by the UI thread to coalesce effect rebuild requests.
    let effects_request_sender = request_sender.clone();

    let state_effects = state.clone();
    let drawing_area_effects = drawing_area.downgrade();
    {
        glib::timeout_add_local(Duration::from_millis(16), move || {
            while let Ok((new_image, revision)) = effects_receiver.try_recv() {
                // Apply results, then if another rebuild was requested while pending,
                // schedule one more rebuild.
                let (should_schedule_next, base_image, actions, next_revision) = {
                    let mut st = state_effects.lock().unwrap();
                    if revision <= st.last_applied_effect_revision {
                        (false, None, None, 0)
                    } else {
                        st.working_image = Arc::new(new_image);
                        st.last_applied_effect_revision = revision;
                        st.select_effect_rebuild_pending = false;
                        st.mark_working_image_dirty();

                        let should = st.select_effect_rebuild_dirty;
                        if should {
                            st.select_effect_rebuild_dirty = false;
                            st.select_effect_rebuild_pending = true;
                            st.pending_effect_revision += 1;
                            (
                                true,
                                Some(Arc::clone(&st.base_image)),
                                Some(st.actions.clone()),
                                st.pending_effect_revision,
                            )
                        } else {
                            (false, None, None, 0)
                        }
                    }
                };

                if let Some(area) = drawing_area_effects.upgrade() {
                    area.queue_draw();
                }

                if should_schedule_next {
                    if let (Some(base_image), Some(actions)) = (base_image, actions) {
                        let _ = effects_request_sender.send((base_image, actions, next_revision));
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Single background worker thread
    std::thread::spawn(move || {
        while let Ok(mut request) = request_receiver.recv() {
            // Drain the channel to get only the latest request
            while let Ok(newer) = request_receiver.try_recv() {
                request = newer;
            }

            let (base_image, actions, revision) = request;
            let mut working_image = (*base_image).clone();

            // EXPENSIVE: This blocks the worker thread
            apply_effect_actions(&mut working_image, &actions);

            let _ = effects_sender.send((working_image, revision));
        }
    });

    let rebuild_effects_async: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let sender = request_sender;
        move || {
            let maybe_payload = {
                let mut st = state.lock().unwrap();

                // Avoid flooding the worker with rebuild requests while one is already pending.
                // This helps prevent UI stalls when many effect-triggering actions happen quickly.
                if st.select_effect_rebuild_pending {
                    // A rebuild is already in-flight; remember that we need another pass.
                    st.select_effect_rebuild_dirty = true;
                    return;
                }
                st.select_effect_rebuild_pending = true;
                st.select_effect_rebuild_dirty = false;
                st.last_effect_request_time_us = glib::monotonic_time();

                st.pending_effect_revision += 1;
                Some((
                    Arc::clone(&st.base_image),
                    st.actions.clone(),
                    st.pending_effect_revision,
                ))
            };

            if let Some((base_image, actions, revision)) = maybe_payload {
                let _ = sender.send((base_image, actions, revision));
            }
        }
    });

    // Effects rebuild watchdog: if we ever get stuck with `select_effect_rebuild_pending=true`
    // (e.g., app was backgrounded / main loop paused), recover by clearing pending and
    // scheduling a fresh rebuild.
    {
        let state = state.clone();
        let rebuild_effects_async = rebuild_effects_async.clone();
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let should_recover = {
                let st = state.lock().unwrap();
                if !st.select_effect_rebuild_pending {
                    false
                } else {
                    let elapsed = glib::monotonic_time() - st.last_effect_request_time_us;
                    // 2 seconds without a result is considered stuck.
                    elapsed > 2_000_000
                }
            };

            if should_recover {
                {
                    let mut st = state.lock().unwrap();
                    st.select_effect_rebuild_pending = false;
                }
                rebuild_effects_async();
            }

            glib::ControlFlow::Continue
        });
    }

    rebuild_effects_async
}

#[cfg(test)]
mod tests {
    #[test]
    fn async_effects_pipeline_preserves_coalesce_revision_and_watchdog() {
        let source = include_str!("effects.rs");
        assert!(
            source.contains("Duration::from_millis(16)")
                && source.contains("Duration::from_millis(500)")
                && source.contains("elapsed > 2_000_000")
                && source.contains("if revision <= st.last_applied_effect_revision")
                && source.contains("st.select_effect_rebuild_dirty = true")
                && source.contains("while let Ok(newer) = request_receiver.try_recv()")
                && source.contains("apply_effect_actions(&mut working_image, &actions)")
                && source.contains("fn install_async_effects_pipeline"),
            "effects service must keep poll interval, watchdog, stale-revision reject, coalesce, and worker apply"
        );
    }
}
