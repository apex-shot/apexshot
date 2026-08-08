use std::path::{Path, PathBuf};
// No atomic imports needed here anymore
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::config::AppConfig;

mod control_session;
pub mod editor;
mod indicator_notify;
// ponytail: controls are removed; this module still owns the countdown overlay.
#[allow(dead_code)]
mod stop_overlay;
#[cfg(test)]
mod stop_overlay_tests;
use control_session::RecordingControlServer;
pub use control_session::{
    has_active_recording_control, send_active_recording_command, toggle_active_recording_pause,
    RecordingControlCommand,
};
pub use stop_overlay::{
    run_recording_countdown_bar, RecordingControlsParams, StopAction, StopOverlayError,
};

pub mod countdown_overlay;
pub mod dim_overlay;
pub mod dnd;

mod audio;
mod backend;
mod controls;
mod wf_recorder;

#[derive(Debug, Error)]
pub enum RecordError {
    #[error("GStreamer initialization failed: {0}")]
    InitError(String),

    #[error("GStreamer error: {0}")]
    GStreamerError(String),

    #[error("Wayland portal error: {0}")]
    PortalError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unsupported backend: {0}")]
    UnsupportedBackend(String),

    #[error("Cancelled by user")]
    Cancelled,

    #[error("No suitable video encoder found. Please install gst-plugins-good/ugly/bad.")]
    NoEncoderFound,

    #[error("GIF encoding error: {0}")]
    GifError(String),
}

pub use audio::{list_audio_inputs, list_audio_outputs};
pub use controls::{
    persist_overlay_recording_request_state, prepare_overlay_recording_request,
    run_overlay_recording_request, run_overlay_recording_request_with_gtk,
    run_recording_with_controls, run_recording_with_native_controls,
    PreparedOverlayRecordingRequest,
};

pub type RecordResult<T> = Result<T, RecordError>;

fn daemon_event_for_terminal_action(action: RecordingTerminalAction) -> Option<&'static str> {
    match action {
        RecordingTerminalAction::Restart => Some("recording_session_restarted"),
        RecordingTerminalAction::Save | RecordingTerminalAction::Discard => {
            Some("recording_session_ended")
        }
    }
}

fn notify_daemon_event(event: &str) {
    match event {
        "recording_session_started" => {
            let _ = crate::daemon::notify_daemon_recording_started();
            // Persistent blinking red-circle notification (click → stop). This
            // is the stop affordance that works everywhere: no tray host and
            // no shell extension required.
            indicator_notify::show_recording_indicator();
        }
        "recording_session_paused" => {
            let _ = crate::daemon::notify_daemon_recording_paused();
            indicator_notify::set_recording_indicator_paused(true);
        }
        "recording_session_resumed" => {
            let _ = crate::daemon::notify_daemon_recording_resumed();
            indicator_notify::set_recording_indicator_paused(false);
        }
        "recording_session_restarted" => {
            let _ = crate::daemon::notify_daemon_recording_restarted();
            indicator_notify::show_recording_indicator();
        }
        "recording_session_ended" => {
            let _ = crate::daemon::notify_daemon_recording_ended();
            // Tear shell chrome down immediately on stop so the area dim/mask
            // does not linger while encode / after-capture work continues.
            crate::gnome_shell::hide_recording_mask_best_effort();
            indicator_notify::hide_recording_indicator();
        }
        _ => {}
    }
}

fn notify_recording_session_ended_best_effort() {
    notify_daemon_event("recording_session_ended");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingTerminalAction {
    Save,
    Discard,
    Restart,
}

#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub output_path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub cursor: bool,
    pub hidpi: bool,
    // Video tab settings
    pub max_resolution: Option<(u32, u32)>,
    pub fps: u32,
    pub mono_audio: bool,
    pub mic_enabled: bool,
    pub speaker_enabled: bool,
    pub mic_source: Option<String>,
    pub speaker_source: Option<String>,
    // GIF-specific settings
    pub gif_quality: f64,
    pub gif_optimize: bool,
    pub gif_max_width: Option<u32>,
}

/// Directory for new recordings: Settings `video_export_location`, else XDG Videos.
pub fn recording_output_dir(app_config: &AppConfig) -> PathBuf {
    if !app_config.video_export_location.is_empty() {
        PathBuf::from(&app_config.video_export_location)
    } else {
        dirs::video_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Expand `rec_filename_pattern` (`{Date}`, `{Time}`) and join under the export dir.
pub fn recording_output_path(
    app_config: &AppConfig,
    extension: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> PathBuf {
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();
    let pattern = app_config.rec_filename_pattern.trim();
    let pattern = if pattern.is_empty() {
        "ApexShot Recording {Date} at {Time}"
    } else {
        pattern
    };
    let filename = pattern
        .replace("{Date}", &date_str)
        .replace("{Time}", &time_str)
        // Patterns must stay single-path-segment filenames.
        .replace(['/', '\\'], "-");
    let ext = extension.trim_start_matches('.');
    recording_output_dir(app_config).join(format!("{filename}.{ext}"))
}

fn ensure_recording_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "[recording] Warning: could not create output directory {}: {e}",
                parent.display()
            );
        }
    }
}

impl Default for RecordingConfig {
    fn default() -> Self {
        // Fallback only for tests/ad-hoc callers. Prefer `from_app_config`.
        let mut path = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("output.mp4");
        Self {
            output_path: path,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: true,
            hidpi: true,
            max_resolution: None,
            fps: 30,
            mono_audio: false,
            mic_enabled: false,
            speaker_enabled: false,
            mic_source: None,
            speaker_source: None,
            gif_quality: 0.75,
            gif_optimize: true,
            gif_max_width: Some(800),
        }
    }
}

impl RecordingConfig {
    /// Build a recording config from Settings (export folder, filename pattern,
    /// fps / resolution / cursor / HiDPI / mono). Used by daemon screen/area
    /// hotkeys so they match the overlay recording path.
    pub fn from_app_config(app_config: &AppConfig, extension: &str) -> Self {
        Self::from_app_config_at(app_config, extension, chrono::Utc::now())
    }

    pub fn from_app_config_at(
        app_config: &AppConfig,
        extension: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let output_path = recording_output_path(app_config, extension, now);
        ensure_recording_parent_dir(&output_path);

        let fps = match app_config.rec_video_fps {
            0 => 24,
            1 => 30,
            2 => 50,
            3 => 60,
            _ => 30,
        };
        let max_resolution = match app_config.rec_video_max_res {
            0 => None,
            1 => Some((1920, 1080)),
            2 => Some((1280, 720)),
            _ => None,
        };

        Self {
            output_path,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: app_config.rec_cursor,
            hidpi: app_config.rec_hidpi,
            max_resolution,
            fps,
            mono_audio: app_config.rec_video_mono,
            mic_enabled: false,
            speaker_enabled: false,
            mic_source: None,
            speaker_source: None,
            gif_quality: app_config.rec_gif_quality,
            gif_optimize: app_config.rec_gif_optimize,
            gif_max_width: match app_config.rec_gif_size_idx {
                0 => Some(800),
                1 => Some(640),
                2 => Some(480),
                _ => None,
            },
        }
    }
}

fn command_exists(name: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Start a recording session
pub async fn start_recording(config: RecordingConfig) -> RecordResult<PathBuf> {
    start_recording_with_commands(config, None)
        .await
        .map(|(path, _)| path)
}

/// Start a recording session and stop when `stop_rx` resolves (in addition to Ctrl+C).
pub async fn start_recording_with_stop(
    config: RecordingConfig,
    stop_rx: oneshot::Receiver<StopAction>,
) -> RecordResult<(PathBuf, StopAction)> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let command = match stop_rx.await {
            Ok(StopAction::Save) => RecordingControlCommand::StopSave,
            Ok(StopAction::Discard) => RecordingControlCommand::StopDiscard,
            Err(_) => return,
        };
        let _ = command_tx.send(command);
    });

    start_recording_with_commands(config, Some(command_rx))
        .await
        .map(|(path, action)| {
            let stop_action = match action {
                RecordingTerminalAction::Save => StopAction::Save,
                RecordingTerminalAction::Discard => StopAction::Discard,
                RecordingTerminalAction::Restart => StopAction::Discard,
            };
            (path, stop_action)
        })
}

async fn start_recording_with_commands(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    if wf_recorder::is_wlroots_session() {
        if wf_recorder::should_use_wf_recorder(&config) {
            return wf_recorder::record_with_wf_recorder(config, command_rx).await;
        }
        if config.output_path.extension().is_some_and(|e| e == "gif") {
            return wf_recorder::record_gif_with_wf_recorder(config, command_rx).await;
        }
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording with this output format is not supported".into(),
        ));
    }

    if config.output_path.extension().is_some_and(|e| e == "gif") {
        return backend::record_gif_rust_with_commands(config, command_rx).await;
    }

    let built = backend::prepare_recording_backend(config).await?;
    backend::start_recording_with_prepared_backend(built, command_rx).await
}

pub fn copy_to_clipboard(path: &Path) -> RecordResult<()> {
    println!("Copying to clipboard...");

    crate::utils::clipboard::copy_uri_to_clipboard(path).map_err(RecordError::GStreamerError)?;

    println!("Copied to clipboard!");
    Ok(())
}

/// User-facing copy when recording is attempted on Fedora.
pub const FEDORA_RECORDING_UNSUPPORTED_MSG: &str = "Video recording is not supported on Fedora. \
Screenshots still work. For screen recording, use Spectacle or Kooha.";

/// Video recording is intentionally **unsupported** on Fedora.
///
/// Screenshots, preview, settings, tray, and shortcuts for capture remain
/// supported. Other distros keep their full recording UX unchanged.
pub fn is_fedora_recording_unsupported() -> bool {
    crate::distro::DistroInfo::detect()
        .map(|d| d.is_fedora())
        .unwrap_or(false)
}

/// Backward-compatible alias used by older call sites.
#[inline]
pub fn is_fedora_portal_only_recording() -> bool {
    is_fedora_recording_unsupported()
}

/// Notify the user and fail. Never starts a recording session on Fedora.
pub fn refuse_fedora_recording() -> anyhow::Result<()> {
    if !is_fedora_recording_unsupported() {
        return Ok(());
    }
    eprintln!("[recording] {FEDORA_RECORDING_UNSUPPORTED_MSG}");
    // Notify on a plain OS thread so zbus blocking does not nest inside Tokio
    // (daemon hotkeys run on the async runtime).
    let summary = "Recording not supported".to_string();
    let body = FEDORA_RECORDING_UNSUPPORTED_MSG.to_string();
    let _ = std::thread::spawn(move || {
        crate::utils::notify::desktop_notification(&summary, &body);
    })
    .join();
    anyhow::bail!("{FEDORA_RECORDING_UNSUPPORTED_MSG}")
}

/// Former Fedora portal entry — now always refuses.
pub async fn run_fedora_portal_recording() -> anyhow::Result<(PathBuf, StopAction)> {
    refuse_fedora_recording()?;
    unreachable!("refuse_fedora_recording always errors on Fedora")
}

#[cfg(test)]
fn reap_child_if_exited(child: &mut Option<std::process::Child>) -> bool {
    if let Some(c) = child {
        if let Ok(Some(_)) = c.try_wait() {
            *child = None;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    #[test]
    fn reap_child_if_exited_clears_completed_child() {
        let mut child = Some(
            std::process::Command::new("sh")
                .arg("-c")
                .arg("exit 0")
                .spawn()
                .expect("child should spawn"),
        );
        std::thread::sleep(std::time::Duration::from_millis(50));

        assert!(reap_child_if_exited(&mut child));
        assert!(child.is_none());
    }

    #[test]
    fn daemon_event_for_terminal_action_keeps_restart_distinct_from_end() {
        assert_eq!(
            daemon_event_for_terminal_action(RecordingTerminalAction::Restart),
            Some("recording_session_restarted")
        );
        assert_eq!(
            daemon_event_for_terminal_action(RecordingTerminalAction::Save),
            Some("recording_session_ended")
        );
        assert_eq!(
            daemon_event_for_terminal_action(RecordingTerminalAction::Discard),
            Some("recording_session_ended")
        );
    }

    #[test]
    fn recording_output_path_uses_export_location_and_filename_pattern() {
        let app = AppConfig {
            video_export_location: "/mnt/media/apexshot".into(),
            rec_filename_pattern: "Clip {Date} {Time}".into(),
            ..AppConfig::default()
        };
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 7, 12, 11, 0, 49)
            .unwrap();
        let path = recording_output_path(&app, "mp4", now);
        assert_eq!(
            path,
            PathBuf::from("/mnt/media/apexshot/Clip 2026-07-12 11-00-49.mp4")
        );
        assert!(!path.to_string_lossy().contains("output.mp4"));
    }

    #[test]
    fn from_app_config_does_not_use_hardcoded_output_mp4() {
        let app = AppConfig {
            video_export_location: "/tmp/apexshot-daemon-recs".into(),
            rec_filename_pattern: "ApexShot Recording {Date} at {Time}".into(),
            rec_video_fps: 3, // 60
            rec_cursor: false,
            rec_hidpi: false,
            ..AppConfig::default()
        };
        let now = chrono::Utc
            .with_ymd_and_hms(2026, 7, 12, 15, 30, 0)
            .unwrap();
        let config = RecordingConfig::from_app_config_at(&app, "mp4", now);
        assert_eq!(
            config.output_path,
            PathBuf::from(
                "/tmp/apexshot-daemon-recs/ApexShot Recording 2026-07-12 at 15-30-00.mp4"
            )
        );
        assert_eq!(config.fps, 60);
        assert!(!config.cursor);
        assert!(!config.hidpi);
    }
}
