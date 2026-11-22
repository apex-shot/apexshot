use super::capture_handlers;
use super::recording_handlers;
use super::{spawn_daemon_tray, ApexShotTray, DaemonAction, DaemonState, HOTKEY_SUPPRESSED};
use std::sync::{Arc, Mutex};

/// Dispatch one daemon action. Returns `false` when the daemon should quit.
pub(super) fn dispatch_daemon_action(
    action: DaemonAction,
    state: &Arc<Mutex<DaemonState>>,
    action_tx: &std::sync::mpsc::Sender<DaemonAction>,
    tray_handle: &mut Option<ksni::Handle<ApexShotTray>>,
    recording_tray_state: &mut Option<recording_handlers::RecordingTrayState>,
) -> bool {
    let state_clone = state.clone();
    let action_tx_clone = action_tx.clone();
    match action {
        DaemonAction::UpdateAvailable(update) => {
            if let Some(handle) = tray_handle {
                handle.update(|tray| tray.set_update_available(update.clone()));
            }
            // The window owns the prompt and its snooze. When it cannot start
            // at all, the desktop notification remains the quiet fallback.
            if capture_handlers::spawn_update_window_subprocess(false) {
                // A release-highlights window due in this same start stays out
                // of the way of the update prompt.
                super::note_update_window_opened();
            } else {
                crate::utils::notify::desktop_notification(
                    &crate::i18n::t("ApexShot update available"),
                    &crate::i18n::tfmt(
                        "Version {version} is ready. Open the ApexShot menu to view it.",
                        &[("version", &update.version)],
                    ),
                );
            }
        }
        DaemonAction::OpenUpdate => {
            if capture_handlers::spawn_update_window_subprocess(true) {
                super::note_update_window_opened();
            } else {
                eprintln!("[update] Could not open the update window");
            }
        }
        DaemonAction::QuickCapture => {
            if let Some(session_guard) = capture_handlers::acquire_capture_session_guard() {
                tokio::task::spawn_blocking(move || {
                    capture_handlers::handle_quick_capture(state_clone, session_guard)
                });
            }
        }
        DaemonAction::CaptureArea => {
            if let Some(session_guard) = capture_handlers::acquire_capture_session_guard() {
                tokio::task::spawn_blocking(move || {
                    capture_handlers::handle_capture_area(state_clone, session_guard)
                });
            }
        }
        DaemonAction::CaptureCrosshair => {
            if let Some(session_guard) = capture_handlers::acquire_capture_session_guard() {
                tokio::task::spawn_blocking(move || {
                    capture_handlers::handle_capture_crosshair(state_clone, session_guard)
                });
            }
        }
        DaemonAction::CaptureScreen => {
            if let Some(session_guard) = capture_handlers::acquire_capture_session_guard() {
                tokio::task::spawn_blocking(move || {
                    capture_handlers::handle_capture_screen(state_clone, session_guard)
                });
            }
        }
        DaemonAction::CaptureWindow => {
            tokio::task::spawn_blocking(move || {
                capture_handlers::handle_capture_window(state_clone)
            });
        }
        DaemonAction::OpenFile => {
            if let Some(path) = capture_handlers::last_capture_target(
                capture_handlers::last_capture_path(&state_clone).as_deref(),
            ) {
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = capture_handlers::open_file(path) {
                        eprintln!("[daemon] {e}");
                    }
                });
            } else {
                eprintln!("[daemon] No capture yet.");
            }
        }
        DaemonAction::OpenFromClipboard => {
            tokio::task::spawn_blocking(move || {
                match capture_handlers::import_clipboard_image_to_temp_png() {
                    Ok(path) => capture_handlers::save_existing_png_and_open(path, state_clone),
                    Err(err) => {
                        eprintln!("[daemon] Clipboard import failed: {err}");
                        let (summary, body) =
                            capture_handlers::clipboard_missing_image_notification();
                        capture_handlers::send_desktop_notification(&summary, &body);
                    }
                }
            });
        }
        DaemonAction::RestoreRecentlyClosed => {
            if let Some(path) = capture_handlers::restore_recently_closed_target(
                capture_handlers::last_capture_path(&state_clone).as_deref(),
            ) {
                tokio::task::spawn_blocking(move || {
                    let _ = capture_handlers::show_preview_for_path(path, &state_clone);
                });
            } else {
                eprintln!("[daemon] No capture available to restore.");
            }
        }
        DaemonAction::ToggleOverlays => {
            tokio::task::spawn_blocking(move || {
                if !capture_handlers::toggle_preview_overlay(&state_clone) {
                    eprintln!("[daemon] No preview overlay available to toggle.");
                }
            });
        }
        DaemonAction::RecordScreen => {
            tokio::spawn(recording_handlers::handle_record_screen(action_tx_clone));
        }
        DaemonAction::RecordWindow => {
            tokio::spawn(recording_handlers::handle_record_window(action_tx_clone));
        }
        DaemonAction::RecordArea => {
            tokio::spawn(recording_handlers::handle_record_area(action_tx_clone));
        }
        DaemonAction::OpenRecordingUi => {
            tokio::spawn(recording_handlers::handle_open_recording_ui(
                action_tx_clone,
            ));
        }
        DaemonAction::OpenVideoEditor => {
            tokio::task::spawn_blocking(capture_handlers::spawn_empty_video_editor_subprocess);
        }
        DaemonAction::OpenImageEditor => {
            tokio::task::spawn_blocking(capture_handlers::spawn_empty_image_editor_subprocess);
        }
        DaemonAction::StopRecordingSave => {
            let command = crate::recording::RecordingControlCommand::StopSave;
            let stopped = crate::recording::send_active_recording_command(command)
                || crate::recording::send_external_recording_command(command);
            if !stopped {
                if let Ok(mut daemon_state) = state.lock() {
                    daemon_state.recording_active = false;
                }
                eprintln!("[daemon] No active recording available for stop/save.");
                idle_recording_tray(tray_handle, recording_tray_state);
            }
            std::thread::spawn(crate::gnome_shell::hide_recording_mask_best_effort);
        }
        DaemonAction::PauseRecording => {
            dispatch_recording_control(
                crate::recording::RecordingControlCommand::Pause,
                "pause",
                state,
                tray_handle,
                recording_tray_state,
            );
        }
        DaemonAction::ResumeRecording => {
            dispatch_recording_control(
                crate::recording::RecordingControlCommand::Resume,
                "resume",
                state,
                tray_handle,
                recording_tray_state,
            );
        }
        DaemonAction::ToggleRecordingPause => {
            dispatch_toggle_recording_pause(state, tray_handle, recording_tray_state);
        }
        DaemonAction::RestartRecording => {
            dispatch_recording_control(
                crate::recording::RecordingControlCommand::Restart,
                "restart",
                state,
                tray_handle,
                recording_tray_state,
            );
        }
        DaemonAction::DiscardRecording => {
            dispatch_recording_control(
                crate::recording::RecordingControlCommand::StopDiscard,
                "discard",
                state,
                tray_handle,
                recording_tray_state,
            );
        }

        DaemonAction::SaveLastCapture => {
            tokio::task::spawn_blocking(move || {
                capture_handlers::handle_save_last_capture(state_clone)
            });
        }
        DaemonAction::ShowLastPreview => {
            let path = state.lock().unwrap().last_capture_path.clone();
            if let Some(p) = path {
                tokio::task::spawn_blocking(move || {
                    let _ = capture_handlers::show_preview_for_path(p, &state_clone);
                });
            } else {
                eprintln!("[daemon] No capture yet.");
            }
        }
        DaemonAction::ShowPreviewForPath(path) => {
            tokio::task::spawn_blocking(move || {
                let _ = capture_handlers::show_preview_for_path(path, &state_clone);
            });
        }
        DaemonAction::OpenLastCapture => {
            let path = state.lock().unwrap().last_capture_path.clone();
            if let Some(p) = path {
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = capture_handlers::open_file(p) {
                        eprintln!("[daemon] {e}");
                    }
                });
            } else {
                eprintln!("[daemon] No capture yet.");
            }
        }
        DaemonAction::OpenHistory => {
            tokio::task::spawn_blocking(capture_handlers::spawn_history_subprocess);
        }
        DaemonAction::WhatsNew => {
            tokio::task::spawn_blocking(|| {
                let _ = capture_handlers::spawn_whats_new_subprocess();
            });
        }
        DaemonAction::OpenSettings => {
            tokio::task::spawn_blocking(capture_handlers::show_settings_subprocess);
        }
        DaemonAction::SetTrayVisible(visible) => {
            if visible {
                if tray_handle.is_none() {
                    match spawn_daemon_tray(action_tx) {
                        Ok(handle) => {
                            *tray_handle = Some(handle);
                            recording_handlers::update_tray_recording_state(
                                tray_handle,
                                recording_tray_state.as_ref(),
                            );
                            eprintln!("[daemon] Tray icon enabled live.");
                        }
                        Err(e) => {
                            eprintln!("[daemon] Failed to enable tray icon live: {e}");
                        }
                    }
                }
            } else if recording_tray_state.is_none() {
                if let Some(handle) = tray_handle.take() {
                    handle.shutdown();
                    eprintln!("[daemon] Tray icon disabled live.");
                }
            }
        }
        DaemonAction::RecordingSessionStarted => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = true;
            }
            *recording_tray_state = Some(recording_handlers::RecordingTrayState::started());
            recording_handlers::update_recording_tray(tray_handle, recording_tray_state.as_ref());
        }
        DaemonAction::RecordingSessionPaused => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = true;
            }
            if recording_tray_state.as_mut().map(|s| s.pause()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionResumed => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = true;
            }
            if recording_tray_state.as_mut().map(|s| s.resume()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionRestarted => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = true;
            }
            if recording_tray_state.as_mut().map(|s| s.restart()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionEnded => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = false;
            }
            idle_recording_tray(tray_handle, recording_tray_state);
        }
        DaemonAction::SetHotkeySuppressed(suppressed) => {
            HOTKEY_SUPPRESSED.store(suppressed, std::sync::atomic::Ordering::Relaxed);
            eprintln!(
                "[daemon] Hotkey suppression {}.",
                if suppressed { "enabled" } else { "disabled" }
            );
        }
        DaemonAction::Quit => {
            eprintln!("[daemon] Quit requested.");
            return false;
        }
    }

    true
}

fn idle_recording_tray(
    tray_handle: &mut Option<ksni::Handle<ApexShotTray>>,
    recording_tray_state: &mut Option<recording_handlers::RecordingTrayState>,
) {
    // Never shutdown/respawn the StatusNotifier item here. Stop used to fire
    // session_ended twice (capture end, then mux end); tearing the tray down
    // in the middle of a re-register left the icon gone.
    *recording_tray_state = None;
    recording_handlers::update_tray_recording_state(tray_handle, None);
}

fn dispatch_recording_control(
    command: crate::recording::RecordingControlCommand,
    label: &str,
    state: &Arc<Mutex<DaemonState>>,
    tray_handle: &mut Option<ksni::Handle<ApexShotTray>>,
    recording_tray_state: &mut Option<recording_handlers::RecordingTrayState>,
) {
    let sent = crate::recording::send_active_recording_command(command)
        || crate::recording::send_external_recording_command(command);
    if !sent {
        if let Ok(mut daemon_state) = state.lock() {
            daemon_state.recording_active = false;
        }
        eprintln!("[daemon] No active recording available for {label}.");
        idle_recording_tray(tray_handle, recording_tray_state);
        return;
    }

    match command {
        crate::recording::RecordingControlCommand::Pause => {
            sync_tray_paused_state(true, tray_handle, recording_tray_state);
        }
        crate::recording::RecordingControlCommand::Resume
        | crate::recording::RecordingControlCommand::Restart => {
            sync_tray_paused_state(false, tray_handle, recording_tray_state);
        }
        crate::recording::RecordingControlCommand::StopDiscard => {
            if let Ok(mut daemon_state) = state.lock() {
                daemon_state.recording_active = false;
            }
            idle_recording_tray(tray_handle, recording_tray_state);
        }
        crate::recording::RecordingControlCommand::StopSave => {}
    }
}

fn dispatch_toggle_recording_pause(
    state: &Arc<Mutex<DaemonState>>,
    tray_handle: &mut Option<ksni::Handle<ApexShotTray>>,
    recording_tray_state: &mut Option<recording_handlers::RecordingTrayState>,
) {
    let toggled = crate::recording::toggle_active_recording_pause()
        || crate::recording::send_external_toggle_pause();
    if !toggled {
        if let Ok(mut daemon_state) = state.lock() {
            daemon_state.recording_active = false;
        }
        eprintln!("[daemon] No active recording available for toggle-pause.");
        idle_recording_tray(tray_handle, recording_tray_state);
        return;
    }

    let was_paused = recording_tray_state
        .as_ref()
        .is_some_and(|state| state.is_paused());
    sync_tray_paused_state(!was_paused, tray_handle, recording_tray_state);
}

fn sync_tray_paused_state(
    paused: bool,
    tray_handle: &mut Option<ksni::Handle<ApexShotTray>>,
    recording_tray_state: &mut Option<recording_handlers::RecordingTrayState>,
) {
    {
        let Some(recording_state) = recording_tray_state.as_mut() else {
            return;
        };
        if paused {
            recording_state.pause();
        } else {
            recording_state.resume();
        }
    }
    recording_handlers::update_recording_tray(tray_handle, recording_tray_state.as_ref());
}
