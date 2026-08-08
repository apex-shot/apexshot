use crate::{
    capture_overlay::{open_recording_ui_via_cpp, AreaCapturePathResult},
    config::load_config,
    recording::run_overlay_recording_request_with_gtk,
    tray::ApexShotTray,
};

use super::*;

#[derive(Debug, Clone)]
pub(super) struct RecordingTrayState;

impl RecordingTrayState {
    pub(super) fn started() -> Self {
        Self
    }

    pub(super) fn pause(&mut self) {}

    pub(super) fn resume(&mut self) {}

    pub(super) fn restart(&mut self) {}
}

pub fn notify_daemon_recording_started() -> bool {
    super::trigger_daemon_action_blocking("recording_session_started")
}

pub fn notify_daemon_recording_paused() -> bool {
    super::trigger_daemon_action_blocking("recording_session_paused")
}

pub fn notify_daemon_recording_resumed() -> bool {
    super::trigger_daemon_action_blocking("recording_session_resumed")
}

pub fn notify_daemon_recording_restarted() -> bool {
    super::trigger_daemon_action_blocking("recording_session_restarted")
}

pub fn notify_daemon_recording_ended() -> bool {
    super::trigger_daemon_action_blocking("recording_session_ended")
}

pub(super) fn update_tray_recording_state(
    tray: &Option<ksni::Handle<ApexShotTray>>,
    recording_state: Option<&RecordingTrayState>,
) {
    if let Some(handle) = tray {
        handle.update(|tray| tray.set_recording(recording_state.is_some()));
    }
}

pub(super) fn update_recording_tray(
    tray: &Option<ksni::Handle<ApexShotTray>>,
    recording_state: Option<&RecordingTrayState>,
) {
    update_tray_recording_state(tray, recording_state);
}

pub(super) async fn handle_record_screen(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    // Fedora: video recording intentionally unsupported.
    if crate::recording::is_fedora_recording_unsupported() {
        let _ = crate::recording::refuse_fedora_recording();
        return;
    }

    eprintln!("[daemon] Starting screen recording…");

    let app_config = load_config().sanitized();
    let config = RecordingConfig::from_app_config(&app_config, "mp4");
    eprintln!(
        "[daemon] Recording output path: {}",
        config.output_path.display()
    );
    let params = RecordingControlsParams {
        capture_x: 0,
        capture_y: 0,
        capture_w: 0,
        capture_h: 0,
        is_fullscreen: true,
        show_timer: true,
        use_shell_mask: false,
        dim_screen: false,
        countdown_enabled: false,
        countdown_seconds: 3,
        session_id: None,
    };

    match run_recording_with_controls(config, params).await {
        Ok((path, StopAction::Discard)) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Recording discarded.");
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("[daemon] Recording saved: {}", path.display());
            crate::usage_telemetry::record_recording();
        }
        Err(e) => eprintln!("[daemon] Recording error: {e}"),
    }
}

pub(super) async fn handle_open_recording_ui(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    // Fedora: video recording intentionally unsupported.
    if crate::recording::is_fedora_recording_unsupported() {
        let _ = crate::recording::refuse_fedora_recording();
        return;
    }

    match tokio::task::spawn_blocking(open_recording_ui_via_cpp).await {
        Ok(Ok(AreaCapturePathResult::RecordingRequested(request))) => {
            if let Err(err) = run_overlay_recording_request_with_gtk(request, None) {
                eprintln!("[daemon] Recording UI failed: {err}");
                // Show notification for GNOME extension not installed
                if err
                    .to_string()
                    .contains("GNOME Shell extension is not installed")
                {
                    super::capture_handlers::send_desktop_notification(
                        "Recording failed",
                        "GNOME Shell extension is not installed. Please install the ApexShot GNOME extension first.",
                    );
                }
            }
        }
        Ok(Ok(AreaCapturePathResult::RecordingConfigUpdated)) => {
            eprintln!("[daemon] Recording UI updated settings only.");
        }
        Ok(Ok(AreaCapturePathResult::Cancelled)) => {
            eprintln!("[daemon] Recording UI cancelled.");
        }
        Ok(Ok(other)) => {
            eprintln!("[daemon] Unexpected recording UI result: {:?}", other);
        }
        Ok(Err(err)) => eprintln!("[daemon] Failed to open recording UI: {err}"),
        Err(err) => eprintln!("[daemon] Recording UI task panicked: {err}"),
    }
}

pub(super) async fn handle_record_area(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    use crate::capture_overlay::run_capture_overlay;
    use crate::overlay::OverlaySelection;
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    // Fedora: video recording intentionally unsupported.
    if crate::recording::is_fedora_recording_unsupported() {
        let _ = crate::recording::refuse_fedora_recording();
        return;
    }

    eprintln!("[daemon] Selecting area for recording…");

    // Show C++ overlay on a blocking thread.
    let selection = tokio::task::spawn_blocking(|| run_capture_overlay(None)).await;

    let cpp_area = match selection {
        Ok(Ok(OverlaySelection::Area(Some(a)))) => a,
        Ok(Ok(OverlaySelection::Area(None))) | Ok(Ok(OverlaySelection::Recording(_))) => {
            eprintln!("[daemon] Area selection cancelled.");
            return;
        }
        Ok(Err(e)) => {
            eprintln!("[daemon] Area selection error: {e}");
            return;
        }
        Err(e) => {
            eprintln!("[daemon] Area selection task panicked: {e}");
            return;
        }
    };
    let area = crate::overlay::SelectionArea {
        x: cpp_area.x,
        y: cpp_area.y,
        width: cpp_area.width,
        height: cpp_area.height,
    };

    let app_config = load_config().sanitized();
    let mut config = RecordingConfig::from_app_config(&app_config, "mp4");
    config.x = Some(area.x);
    config.y = Some(area.y);
    config.width = Some(area.width as u32);
    config.height = Some(area.height as u32);

    eprintln!(
        "[daemon] Starting area recording ({},{} {}x{}) → {}…",
        area.x,
        area.y,
        area.width,
        area.height,
        config.output_path.display()
    );

    let params = RecordingControlsParams {
        capture_x: area.x,
        capture_y: area.y,
        capture_w: area.width,
        capture_h: area.height,
        is_fullscreen: false,
        show_timer: true,
        use_shell_mask: false,
        dim_screen: true,
        countdown_enabled: false,
        countdown_seconds: 3,
        session_id: None,
    };
    match run_recording_with_controls(config, params).await {
        Ok((path, StopAction::Discard)) => {
            let _ = std::fs::remove_file(&path);
            eprintln!("[daemon] Recording discarded.");
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("[daemon] Recording saved: {}", path.display());
            crate::usage_telemetry::record_recording();
        }
        Err(e) => eprintln!("[daemon] Recording error: {e}"),
    }
}
