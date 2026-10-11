use crate::{config::load_config, tray::ApexShotTray};

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
    handle_recording_target(crate::recording::RecordingTarget::Screen).await;
}

pub(super) async fn handle_record_window(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    handle_recording_target(crate::recording::RecordingTarget::Window).await;
}

async fn handle_recording_target(target: crate::recording::RecordingTarget) {
    use crate::recording::{
        run_recording_with_controls, RecordingConfig, RecordingControlsParams, StopAction,
    };

    // Fedora: video recording intentionally unsupported.
    if crate::recording::is_fedora_recording_unsupported() {
        let _ = crate::recording::refuse_fedora_recording();
        return;
    }

    let label = match target {
        crate::recording::RecordingTarget::Window => "window",
        crate::recording::RecordingTarget::Screen => "screen",
        crate::recording::RecordingTarget::Area => "area",
    };
    eprintln!("[daemon] Starting {label} recording…");

    let app_config = load_config().sanitized();
    let mut config = RecordingConfig::from_app_config(&app_config, "mp4");
    config.target = target;
    eprintln!(
        "[daemon] Recording output path: {}",
        config.output_path.display()
    );
    let params = RecordingControlsParams {
        capture_x: 0,
        capture_y: 0,
        capture_w: 0,
        capture_h: 0,
        is_fullscreen: target == crate::recording::RecordingTarget::Screen,
        show_timer: true,
        use_shell_mask: false,
        dim_screen: false,
        countdown_enabled: app_config.rec_countdown,
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
        Err(e) => {
            eprintln!("[daemon] Recording error: {e}");
            if target == crate::recording::RecordingTarget::Window
                && !e
                    .downcast_ref::<crate::recording::RecordError>()
                    .is_some_and(|error| matches!(error, crate::recording::RecordError::Cancelled))
            {
                crate::utils::notify::desktop_notification(
                    &crate::i18n::t("Window recording unavailable"),
                    &e.to_string(),
                );
            }
        }
    }
}

pub(super) async fn handle_open_recording_ui(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    // Recording UI is discontinued — do not fall back to fullscreen recording,
    // which surprises users who pressed a leftover recording shortcut.
    eprintln!("[daemon] Recording UI is discontinued. Use fullscreen recording.");
}

pub(super) async fn handle_record_area(_tx: std::sync::mpsc::Sender<DaemonAction>) {
    // Area recording is discontinued — do not fall back to fullscreen recording,
    // which surprises users who pressed a leftover area shortcut.
    eprintln!("[daemon] Area recording is discontinued. Use fullscreen recording.");
}
