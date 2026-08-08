use super::*;
use crate::{
    capture_overlay::{RecordingRequest, RecordingType},
    config::{save_config, AppConfig},
};
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct PreparedOverlayRecordingRequest {
    pub updated_app_config: AppConfig,
    pub output_path: PathBuf,
    pub recording_config: super::RecordingConfig,
    pub controls_params: Option<RecordingControlsParams>,
    pub use_shell_mask: bool,
    pub open_editor: bool,
}

enum PreparedShellRecording {
    Video(super::backend::BuiltPipeline),
    Gif(super::backend::PreparedGifWaylandRecording),
}

fn should_use_shell_mask_for_request(
    request: &RecordingRequest,
    shell_mask_available: bool,
) -> bool {
    shell_mask_available
        && request.dim_screen
        && !request.fullscreen
        && request.width > 0
        && request.height > 0
}

pub fn prepare_overlay_recording_request(
    mut app_config: AppConfig,
    request: &RecordingRequest,
    now: chrono::DateTime<chrono::Utc>,
) -> PreparedOverlayRecordingRequest {
    let shell_overlay_available =
        crate::gnome_shell::current_session_supports_gnome_shell_overlay();
    let use_shell_mask = should_use_shell_mask_for_request(request, shell_overlay_available);
    app_config.rec_controls = request.controls;
    app_config.rec_display_time = true; // always show recording time in top bar
    app_config.rec_hidpi = request.hidpi;
    app_config.rec_notifications = request.notifications;
    app_config.rec_cursor = true;
    app_config.rec_remember_selection = request.remember_selection;
    app_config.rec_dim_screen = request.dim_screen;
    app_config.rec_countdown = request.countdown;
    app_config.rec_mic = request.mic;
    app_config.rec_speaker = request.speaker;
    app_config.rec_video_format = 0;
    app_config.rec_video_max_res = request.video_max_res;
    app_config.rec_video_fps = request.video_fps;
    app_config.rec_video_mono = request.record_mono;
    app_config.rec_video_open_editor = request.open_editor;
    app_config.rec_gif_fps = request.gif_fps;
    app_config.rec_gif_quality = request.gif_quality;
    app_config.rec_gif_size_idx = request.gif_size_idx;
    app_config.rec_gif_optimize = request.optimize_gif;

    if request.remember_selection {
        app_config.last_selection_x = Some(request.x);
        app_config.last_selection_y = Some(request.y);
        app_config.last_selection_w = Some(request.width);
        app_config.last_selection_h = Some(request.height);
    }

    let extension = match request.record_type {
        RecordingType::Video => "mp4",
        RecordingType::Gif => "gif",
    };
    let output_path = super::recording_output_path(&app_config, extension, now);
    super::ensure_recording_parent_dir(&output_path);

    let max_resolution = match request.video_max_res {
        0 => None,
        1 => Some((1920, 1080)),
        2 => Some((1280, 720)),
        _ => None,
    };

    let video_fps = match request.video_fps {
        0 => 24,
        1 => 30,
        2 => 50,
        3 => 60,
        _ => 30,
    };

    let (fps, gif_quality, gif_optimize, gif_max_width) =
        if matches!(request.record_type, RecordingType::Gif) {
            let max_width = match request.gif_size_idx {
                0 => Some(800),
                1 => Some(640),
                2 => Some(480),
                _ => None,
            };
            (
                request.gif_fps as u32,
                request.gif_quality,
                request.optimize_gif,
                max_width,
            )
        } else {
            (video_fps, 0.75, true, Some(800))
        };

    let (capture_x, capture_y, capture_width, capture_height) = if request.fullscreen {
        (None, None, None, None)
    } else {
        (
            Some(request.x),
            Some(request.y),
            Some(request.width as u32),
            Some(request.height as u32),
        )
    };

    let recording_config = super::RecordingConfig {
        output_path: output_path.clone(),
        width: capture_width,
        height: capture_height,
        x: capture_x,
        y: capture_y,
        cursor: true,
        hidpi: request.hidpi,
        max_resolution,
        fps,
        mono_audio: request.record_mono,
        mic_enabled: request.mic,
        speaker_enabled: request.speaker,
        mic_source: None,
        speaker_source: None,
        gif_quality,
        gif_optimize,
        gif_max_width,
    };

    let controls_params = Some(RecordingControlsParams {
        capture_x: request.x,
        capture_y: request.y,
        capture_w: request.width,
        capture_h: request.height,
        is_fullscreen: request.fullscreen,
        show_timer: true,
        use_shell_mask,
        dim_screen: request.dim_screen,
        countdown_enabled: request.countdown,
        countdown_seconds: 3, // Default to 3s for now, or fetch from config
        session_id: None,     // Will be set when controls are launched
    });

    PreparedOverlayRecordingRequest {
        updated_app_config: app_config,
        output_path,
        recording_config,
        controls_params,
        use_shell_mask,
        open_editor: matches!(request.record_type, RecordingType::Video) && request.open_editor,
    }
}

pub async fn run_recording_with_controls(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Fedora: video recording is not supported.
    super::refuse_fedora_recording()?;

    // On GNOME the shell extension dims the area outside the capture rect.
    if crate::gnome_shell::current_session_supports_gnome_shell_overlay() {
        return run_recording_with_shell_mask(config, params).await;
    }

    // Fallback for non-GNOME (Fedora KDE, Hyprland, Sway, Niri, River, etc.).
    // No floating control bar — shortcuts, tray, and desktop notifications only.
    eprintln!(
        "[recording] GNOME Shell not detected; using native recording path (shortcuts + notifications)."
    );
    run_recording_with_native_controls(config, params).await
}

async fn run_recording_countdown_subprocess(
    params: RecordingControlsParams,
) -> anyhow::Result<bool> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
    let seconds = params.countdown_seconds;
    let params_json = serde_json::to_string(&params)?;

    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&exe)
            .arg("recording-countdown-internal")
            .arg(params_json)
            .arg(seconds.to_string())
            .status()
    })
    .await??;

    if status.success() {
        Ok(true)
    } else if status.code() == Some(2) {
        Ok(false)
    } else {
        Err(anyhow::anyhow!(
            "recording countdown exited with status {status}"
        ))
    }
}

async fn run_shell_recording_countdown(params: RecordingControlsParams) -> anyhow::Result<bool> {
    let geometry = crate::gnome_shell::RecordingMaskGeometry {
        x: params.capture_x,
        y: params.capture_y,
        width: params.capture_w,
        height: params.capture_h,
    };

    match crate::gnome_shell::show_recording_countdown(geometry, params.countdown_seconds) {
        Ok(countdown) => {
            tokio::time::sleep(std::time::Duration::from_secs(
                params.countdown_seconds.into(),
            ))
            .await;
            drop(countdown);
            Ok(true)
        }
        Err(err) => {
            eprintln!("[recording] Failed to show GNOME Shell countdown ({err}); using fallback countdown.");
            run_recording_countdown_subprocess(params).await
        }
    }
}

async fn prepare_shell_recording(
    config: super::RecordingConfig,
) -> super::RecordResult<Option<PreparedShellRecording>> {
    if super::wf_recorder::is_wlroots_session() {
        return Ok(None);
    }

    if config
        .output_path
        .extension()
        .is_some_and(|extension| extension == "gif")
    {
        return super::backend::prepare_gif_wayland_recording(config)
            .await
            .map(PreparedShellRecording::Gif)
            .map(Some);
    }

    super::backend::prepare_recording_backend(config)
        .await
        .map(PreparedShellRecording::Video)
        .map(Some)
}

async fn start_shell_recording(
    config: super::RecordingConfig,
    prepared: Option<PreparedShellRecording>,
    command_rx: mpsc::UnboundedReceiver<RecordingControlCommand>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    match prepared {
        Some(PreparedShellRecording::Video(backend)) => {
            super::backend::start_recording_with_prepared_backend(backend, Some(command_rx)).await
        }
        Some(PreparedShellRecording::Gif(backend)) => {
            super::backend::record_prepared_gif_wayland_native(backend, Some(command_rx)).await
        }
        None => super::start_recording_with_commands(config, Some(command_rx)).await,
    }
}

/// Non-GNOME recording path (Fedora KDE, wlroots, etc.).
///
/// No floating control bar — users control the session with configured
/// global shortcuts and the tray. Lifecycle feedback is via desktop
/// notifications (same idea as Spectacle / system recorders).
pub async fn run_recording_with_native_controls(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Belt-and-suspenders: Fedora never records (entry points refuse first).
    if super::is_fedora_recording_unsupported() {
        super::refuse_fedora_recording()?;
    }

    // Prepare the capture backend *before* countdown so any portal / permission
    // UI appears first. Countdown then leads straight into recording without a
    // second chooser after "3-2-1".
    let prepared_backend = if super::wf_recorder::is_wlroots_session()
        || config.output_path.extension().is_some_and(|e| e == "gif")
    {
        None
    } else {
        match super::backend::prepare_recording_backend(config.clone()).await {
            Ok(prepared) => Some(prepared),
            Err(err) => {
                super::notify_recording_session_ended_best_effort();
                return Err(err.into());
            }
        }
    };

    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id).await?;

    // Optional pre-roll — never spawn the floating controls UI on native path.
    if params.countdown_enabled {
        match run_recording_countdown_subprocess(params.clone()).await {
            Ok(true) => {}
            Ok(false) => {
                super::notify_recording_session_ended_best_effort();
                return Ok((config.output_path.clone(), StopAction::Discard));
            }
            Err(err) => {
                super::notify_recording_session_ended_best_effort();
                return Err(err);
            }
        }
    }

    let (recording_command_tx, command_rx) = mpsc::unbounded_channel();
    let (control_command_tx, mut control_command_rx) =
        mpsc::unbounded_channel::<RecordingControlCommand>();

    let dbus_tx = recording_command_tx.clone();
    let dbus_forward = tokio::spawn(async move {
        while let Some(cmd) = control_command_rx.recv().await {
            if dbus_tx.send(cmd).is_err() {
                break;
            }
        }
    });

    control_server.set_command_sender(control_command_tx);
    // `start_recording_*` emits recording_session_started (tray + desktop toast).
    let outcome = if let Some(prepared_backend) = prepared_backend {
        super::backend::start_recording_with_prepared_backend(prepared_backend, Some(command_rx))
            .await
    } else {
        super::start_recording_with_commands(config.clone(), Some(command_rx)).await
    };
    control_server.clear_command_sender();
    dbus_forward.abort();

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            super::notify_recording_session_ended_best_effort();
            return Err(err.into());
        }
    };

    match outcome {
        (path, super::RecordingTerminalAction::Restart) => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Restart)
            {
                super::notify_daemon_event(event);
            }
            let _ = std::fs::remove_file(&path);
            Box::pin(run_recording_with_native_controls(config, params)).await
        }
        (path, super::RecordingTerminalAction::Save) => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Save)
            {
                super::notify_daemon_event(event);
            }
            Ok((path, StopAction::Save))
        }
        (path, super::RecordingTerminalAction::Discard) => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Discard)
            {
                super::notify_daemon_event(event);
            }
            Ok((path, StopAction::Discard))
        }
    }
}

/// GNOME recording path: identical to the native path except the shell
/// extension dims the area outside the capture rect while we record.
async fn run_recording_with_shell_mask(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Same ordering as the native path: portal / backend first, then countdown.
    let mut prepared_backend = prepare_shell_recording(config.clone()).await?;

    let _shell_mask = if params.use_shell_mask {
        match crate::gnome_shell::show_recording_mask(crate::gnome_shell::RecordingMaskGeometry {
            x: params.capture_x,
            y: params.capture_y,
            width: params.capture_w,
            height: params.capture_h,
        }) {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!("[recording] Failed to show GNOME shell recording mask ({err}); continuing without shell mask.");
                None
            }
        }
    } else {
        None
    };

    if params.countdown_enabled && !run_shell_recording_countdown(params.clone()).await? {
        super::notify_recording_session_ended_best_effort();
        return Ok((config.output_path.clone(), StopAction::Discard));
    }

    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id).await?;

    super::notify_daemon_event("recording_session_started");
    let final_outcome = loop {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        control_server.set_command_sender(command_tx);
        let outcome =
            start_shell_recording(config.clone(), prepared_backend.take(), command_rx).await;
        control_server.clear_command_sender();
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(err) => {
                drop(control_server);
                super::notify_recording_session_ended_best_effort();
                return Err(err.into());
            }
        };

        match outcome {
            (path, action @ super::RecordingTerminalAction::Restart) => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                let _ = std::fs::remove_file(&path);
                prepared_backend = prepare_shell_recording(config.clone()).await?;
                continue;
            }
            (path, action @ super::RecordingTerminalAction::Save) => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                break (path, StopAction::Save);
            }
            (path, action @ super::RecordingTerminalAction::Discard) => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                break (path, StopAction::Discard);
            }
        }
    };

    drop(control_server);

    Ok(final_outcome)
}

pub fn run_overlay_recording_request(request: RecordingRequest) -> anyhow::Result<PathBuf> {
    run_overlay_recording_request_with_gtk(request, None)
}

pub fn persist_overlay_recording_request_state(request: &RecordingRequest) -> anyhow::Result<()> {
    let prepared = prepare_overlay_recording_request(
        crate::config::load_config(),
        request,
        chrono::Utc::now(),
    );
    save_config(&prepared.updated_app_config)?;
    Ok(())
}

pub fn run_overlay_recording_request_with_gtk(
    request: RecordingRequest,
    _gtk_tx: Option<std::sync::mpsc::Sender<crate::daemon::GtkWork>>,
) -> anyhow::Result<PathBuf> {
    // Fedora: video recording is not supported.
    super::refuse_fedora_recording()?;

    let prepared = prepare_overlay_recording_request(
        crate::config::load_config(),
        &request,
        chrono::Utc::now(),
    );

    if let Some(parent) = prepared.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    save_config(&prepared.updated_app_config)?;

    let _dnd_guard = if request.notifications {
        crate::recording::dnd::DndGuard::enable()
    } else {
        None
    };

    eprintln!("Starting recording to {:?}...", prepared.output_path);

    let open_editor = prepared.open_editor;
    let recording_config = prepared.recording_config.clone();
    let controls_params = prepared.controls_params.clone();
    let outcome = std::thread::spawn(move || -> anyhow::Result<(PathBuf, StopAction)> {
        let runtime = tokio::runtime::Runtime::new()?;
        if let Some(params) = controls_params {
            runtime
                .block_on(run_recording_with_controls(recording_config, params))
                .map_err(|err| anyhow::anyhow!("failed to run recording controls: {err}"))
        } else {
            runtime
                .block_on(super::start_recording(recording_config))
                .map(|path| (path, StopAction::Save))
                .map_err(|err| anyhow::anyhow!("Recording failed: {err}"))
        }
    })
    .join()
    .map_err(|_| anyhow::anyhow!("recording worker panicked"))?;

    match outcome {
        Ok((path, StopAction::Discard)) => {
            eprintln!("Recording discarded — deleting {:?}", path);
            let _ = std::fs::remove_file(&path);
            Ok(path)
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("Recording saved to {:?}", path);
            let config = crate::config::load_config().sanitized();

            if config.rec_after_capture_copy_to_clipboard {
                if let Err(e) = crate::utils::clipboard::copy_uri_to_clipboard(&path) {
                    eprintln!("[recording] Failed to copy recording to clipboard: {e}");
                }
            }

            if !config.rec_after_capture_save {
                let _ = std::fs::remove_file(&path);
                eprintln!(
                    "[recording] Recording discarded because Save is disabled in after-capture settings"
                );
                crate::utils::notify::desktop_notification(
                    "Recording not saved",
                    "Save is disabled in After capture settings",
                );
                return Ok(path);
            }

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("recording");
            crate::utils::notify::desktop_notification("Recording saved", file_name);

            if open_editor {
                spawn_recording_editor_subprocess(path.clone());
            }
            Ok(path)
        }
        Err(err) => Err(anyhow::anyhow!("Recording failed: {err}")),
    }
}

/// Spawn the recording editor as a **subprocess** so it gets its own GTK
/// event loop and doesn't conflict with the recording worker thread.
fn spawn_recording_editor_subprocess(path: std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    if let Err(e) = std::process::Command::new(&exe)
        .arg("video-editor")
        .arg(&path)
        .spawn()
    {
        eprintln!("[recording] Failed to spawn recording editor: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    #[test]
    fn prepare_overlay_recording_request_maps_video_settings() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: true,
            speaker: false,
            display_rec_time: true,
            hidpi: true,
            notifications: false,
            cursor: false,
            remember_selection: true,
            dim_screen: false,
            countdown: false,
            video_format: 0,
            video_max_res: 2,
            video_fps: 3,
            record_mono: true,
            open_editor: true,
            gif_fps: 12,
            gif_quality: 0.4,
            gif_size_idx: 2,
            optimize_gif: false,
            fullscreen: false,
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig {
                video_export_location: "/tmp/apexshot-recordings".into(),
                ..AppConfig::default()
            },
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 25, 12, 0, 0).unwrap(),
        );

        assert_eq!(
            prepared.output_path,
            PathBuf::from("/tmp/apexshot-recordings/ApexShot Recording 2026-03-25 at 12-00-00.mp4")
        );
        assert_eq!(prepared.updated_app_config.rec_controls, true);
        assert_eq!(prepared.updated_app_config.rec_display_time, true);
        assert_eq!(prepared.updated_app_config.rec_hidpi, true);
        assert_eq!(prepared.updated_app_config.rec_notifications, false);
        assert_eq!(prepared.updated_app_config.rec_cursor, true);
        assert_eq!(prepared.updated_app_config.rec_remember_selection, true);
        assert_eq!(prepared.updated_app_config.last_selection_x, Some(10));
        assert_eq!(prepared.updated_app_config.last_selection_y, Some(20));
        assert_eq!(prepared.updated_app_config.last_selection_w, Some(640));
        assert_eq!(prepared.updated_app_config.last_selection_h, Some(480));
        assert_eq!(prepared.updated_app_config.rec_video_format, 0);
        assert_eq!(prepared.updated_app_config.rec_video_max_res, 2);
        assert_eq!(prepared.updated_app_config.rec_video_fps, 3);
        assert_eq!(prepared.updated_app_config.rec_video_mono, true);
        assert_eq!(prepared.updated_app_config.rec_video_open_editor, true);
        assert_eq!(prepared.open_editor, true);
        assert_eq!(prepared.recording_config.output_path, prepared.output_path);
        assert_eq!(prepared.recording_config.width, Some(640));
        assert_eq!(prepared.recording_config.height, Some(480));
        assert_eq!(prepared.recording_config.x, Some(10));
        assert_eq!(prepared.recording_config.y, Some(20));
        assert_eq!(prepared.recording_config.cursor, true);
        assert_eq!(prepared.recording_config.hidpi, true);
        assert_eq!(prepared.recording_config.max_resolution, Some((1280, 720)));
        assert_eq!(prepared.recording_config.fps, 60);
        assert_eq!(prepared.recording_config.mono_audio, true);
        assert_eq!(prepared.recording_config.mic_enabled, true);
        assert_eq!(prepared.recording_config.speaker_enabled, false);
        assert_eq!(prepared.recording_config.gif_quality, 0.75);
        assert_eq!(prepared.recording_config.gif_optimize, true);
        assert_eq!(prepared.recording_config.gif_max_width, Some(800));
        assert_eq!(
            prepared.controls_params,
            Some(RecordingControlsParams {
                capture_x: 10,
                capture_y: 20,
                capture_w: 640,
                capture_h: 480,
                is_fullscreen: false,
                show_timer: true,
                use_shell_mask: false,
                dim_screen: false,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, false);
    }

    #[test]
    fn prepare_overlay_recording_request_sets_open_editor_for_video() {
        let request = RecordingRequest {
            record_type: RecordingType::Video,
            open_editor: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 5, 8, 12, 0, 0).unwrap(),
        );

        assert!(prepared.open_editor);
    }

    #[test]
    fn prepare_overlay_recording_request_does_not_set_open_editor_for_gif() {
        let request = RecordingRequest {
            record_type: RecordingType::Gif,
            open_editor: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 5, 8, 12, 0, 0).unwrap(),
        );

        assert!(!prepared.open_editor);
    }

    #[test]
    fn prepare_overlay_recording_request_maps_video_setting_variants() {
        let cases = [
            (0, 0, None, 24_u32, "mp4"),
            (1, 1, Some((1920, 1080)), 30_u32, "mp4"),
            (0, 2, Some((1280, 720)), 50_u32, "mp4"),
            (1, 0, None, 60_u32, "mp4"),
        ];

        for (index, (video_format, video_max_res, expected_max_res, expected_fps, extension)) in
            cases.into_iter().enumerate()
        {
            let request = RecordingRequest {
                x: 5,
                y: 10,
                width: 800,
                height: 600,
                record_type: RecordingType::Video,
                video_format,
                video_max_res,
                video_fps: index as u8,
                ..RecordingRequest::default()
            };

            let prepared = prepare_overlay_recording_request(
                AppConfig::default(),
                &request,
                chrono::Utc
                    .with_ymd_and_hms(2026, 4, 2, 10, 0, index as u32)
                    .unwrap(),
            );

            assert_eq!(prepared.recording_config.max_resolution, expected_max_res);
            assert_eq!(prepared.recording_config.fps, expected_fps);
            assert_eq!(prepared.updated_app_config.rec_video_format, 0);
            assert_eq!(
                prepared
                    .output_path
                    .extension()
                    .and_then(|ext| ext.to_str()),
                Some(extension)
            );
        }
    }

    #[test]
    fn prepare_overlay_recording_request_uses_full_monitor_bounds_for_fullscreen_capture() {
        let request = RecordingRequest {
            x: 0,
            y: 32,
            width: 1920,
            height: 1048,
            record_type: RecordingType::Video,
            fullscreen: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 4, 2, 10, 0, 0).unwrap(),
        );

        assert_eq!(prepared.recording_config.x, None);
        assert_eq!(prepared.recording_config.y, None);
        assert_eq!(prepared.recording_config.width, None);
        assert_eq!(prepared.recording_config.height, None);
        assert_eq!(
            prepared.controls_params,
            Some(RecordingControlsParams {
                capture_x: 0,
                capture_y: 32,
                capture_w: 1920,
                capture_h: 1048,
                is_fullscreen: true,
                show_timer: true,
                use_shell_mask: false,
                dim_screen: true,
                countdown_enabled: true,
                countdown_seconds: 3,
                session_id: None,
            })
        );
    }

    #[test]
    fn prepare_overlay_recording_request_maps_gif_settings_without_controls() {
        let request = RecordingRequest {
            x: 1,
            y: 2,
            width: 300,
            height: 200,
            record_type: RecordingType::Gif,
            controls: false,
            mic: false,
            speaker: true,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
            video_format: 0,
            video_max_res: 1,
            video_fps: 2,
            record_mono: false,
            open_editor: false,
            gif_fps: 18,
            gif_quality: 0.6,
            gif_size_idx: 1,
            optimize_gif: false,
            fullscreen: true,
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig {
                video_export_location: "/var/tmp/apexshot-gifs".into(),
                ..AppConfig::default()
            },
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 25, 12, 0, 1).unwrap(),
        );

        assert_eq!(
            prepared.output_path,
            PathBuf::from("/var/tmp/apexshot-gifs/ApexShot Recording 2026-03-25 at 12-00-01.gif")
        );
        assert_eq!(prepared.updated_app_config.rec_remember_selection, false);
        assert_eq!(prepared.updated_app_config.last_selection_x, None);
        assert_eq!(prepared.updated_app_config.last_selection_y, None);
        assert_eq!(prepared.updated_app_config.last_selection_w, None);
        assert_eq!(prepared.updated_app_config.last_selection_h, None);
        assert_eq!(prepared.recording_config.max_resolution, Some((1920, 1080)));
        assert_eq!(prepared.recording_config.fps, 18);
        assert_eq!(prepared.recording_config.gif_quality, 0.6);
        assert_eq!(prepared.recording_config.gif_optimize, false);
        assert_eq!(prepared.recording_config.gif_max_width, Some(640));
        assert_eq!(prepared.recording_config.speaker_enabled, true);
        assert!(prepared.controls_params.is_some());
        assert_eq!(prepared.use_shell_mask, false);
    }

    #[test]
    fn prepare_overlay_recording_request_sets_shell_mask_for_gnome_wayland_area_recording() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: false,
            video_max_res: 0,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 12,
            gif_quality: 0.75,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: false,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 26, 1, 30, 0).unwrap(),
        );

        let shell_supported = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
        assert_eq!(
            prepared.controls_params,
            Some(RecordingControlsParams {
                capture_x: 10,
                capture_y: 20,
                capture_w: 640,
                capture_h: 480,
                is_fullscreen: false,
                show_timer: true,
                use_shell_mask: shell_supported,
                dim_screen: true,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, shell_supported);
    }

    #[test]
    fn prepare_overlay_recording_request_skips_mask_for_gnome_wayland_fullscreen_recording() {
        let request = RecordingRequest {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            display_rec_time: true,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: false,
            video_max_res: 0,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 12,
            gif_quality: 0.75,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 26, 1, 35, 0).unwrap(),
        );

        assert_eq!(prepared.use_shell_mask, false); // fullscreen never uses mask
        assert_eq!(
            prepared.controls_params,
            Some(RecordingControlsParams {
                capture_x: 0,
                capture_y: 0,
                capture_w: 1920,
                capture_h: 1080,
                is_fullscreen: true,
                show_timer: true,
                use_shell_mask: false, // fullscreen never uses mask
                dim_screen: true,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
    }

    #[test]
    fn shell_mask_follows_gnome_wayland_support() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: false,
            video_max_res: 0,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 12,
            gif_quality: 0.75,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: false,
            ..RecordingRequest::default()
        };

        assert!(should_use_shell_mask_for_request(&request, true));
        assert!(!should_use_shell_mask_for_request(&request, false));
    }

    #[test]
    fn prepare_overlay_recording_request_keeps_shortcut_controls_when_toggle_is_off() {
        let request = RecordingRequest {
            controls: false,
            fullscreen: false,
            x: 30,
            y: 40,
            width: 320,
            height: 200,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 4, 2, 8, 0, 0).unwrap(),
        );

        assert!(prepared.controls_params.is_some());
    }
}
