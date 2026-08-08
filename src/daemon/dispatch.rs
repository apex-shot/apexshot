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
        DaemonAction::CaptureArea => {
            tokio::task::spawn_blocking(move || capture_handlers::handle_capture_area(state_clone));
        }
        DaemonAction::CaptureCrosshair => {
            tokio::task::spawn_blocking(move || {
                capture_handlers::handle_capture_crosshair(state_clone)
            });
        }
        DaemonAction::CaptureScreen => {
            tokio::task::spawn_blocking(move || {
                capture_handlers::handle_capture_screen(state_clone)
            });
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
                        capture_handlers::send_desktop_notification(summary, body);
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
            crate::gnome_shell::hide_recording_mask_best_effort();
            if !crate::recording::send_active_recording_command(
                crate::recording::RecordingControlCommand::StopSave,
            ) {
                eprintln!("[daemon] No active recording available for stop/save.");
            }
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
            *recording_tray_state = Some(recording_handlers::RecordingTrayState::started());
            if tray_handle.is_none() {
                match spawn_daemon_tray(action_tx) {
                    Ok(handle) => *tray_handle = Some(handle),
                    Err(e) => {
                        eprintln!("[daemon] Failed to show recording tray: {e}");
                    }
                }
            }
            recording_handlers::update_recording_tray(tray_handle, recording_tray_state.as_ref());
        }
        DaemonAction::RecordingSessionPaused => {
            if recording_tray_state.as_mut().map(|s| s.pause()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionResumed => {
            if recording_tray_state.as_mut().map(|s| s.resume()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionRestarted => {
            if recording_tray_state.as_mut().map(|s| s.restart()).is_some() {
                recording_handlers::update_recording_tray(
                    tray_handle,
                    recording_tray_state.as_ref(),
                );
            }
        }
        DaemonAction::RecordingSessionEnded => {
            *recording_tray_state = None;
            recording_handlers::update_tray_recording_state(tray_handle, None);
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
