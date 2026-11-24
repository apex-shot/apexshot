use std::path::{Path, PathBuf};
// No atomic imports needed here anymore
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::config::AppConfig;

mod control_session;
pub mod editor;
mod indicator_notify;
// The countdown overlay and the X11 recording controls bar share this module.
#[allow(dead_code)]
mod stop_overlay;
#[cfg(test)]
mod stop_overlay_tests;
use control_session::RecordingControlServer;
pub use control_session::{
    has_active_recording_control, send_active_recording_command, send_external_recording_command,
    send_external_toggle_pause, toggle_active_recording_pause, RecordingControlCommand,
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
mod gst_audio;
mod kwin_clicks;
mod pointer_track;
mod recording_manifest;
mod timeline;
mod wf_recorder;
mod x11_controls_mask;
mod x11_pointer_track;
mod x11_window_target;

pub(crate) use backend::normalize_window_still_frame;

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
}

/// A recording failure that the controls outcome handler already reported to
/// the user. Callers can tell it apart from a failure they still need to
/// notify about.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct RecordingFailureNotified(pub anyhow::Error);

pub(crate) use audio::app_stream_serials;
pub use audio::{list_audio_applications, AudioApplication};
pub use audio::{list_audio_inputs, list_audio_outputs, AudioInputDevice, AudioOutputDevice};

/// Whether a single app's streams can be captured on this session: the
/// PipeWire socket must be reachable, and the wlroots and Flatpak paths only
/// record the default output.
pub(crate) fn app_audio_capture_available() -> bool {
    if wf_recorder::is_wlroots_session() || crate::app_identity::portal_only() {
        return false;
    }
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    std::path::Path::new(&runtime).join("pipewire-0").exists()
}

/// GStreamer pulsesrc level meter used by the daemon/overlay instead of a
/// second native PipeWire capture (which races recording + screen capture).
pub(crate) fn run_audio_level_monitor(
    label: &'static str,
    device: Option<String>,
    capture_sink: bool,
    level: &'static std::sync::atomic::AtomicU64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    gst_audio::run_pulse_level_monitor(label, device, capture_sink, level, stop);
}

/// Level meter on one PipeWire node, used while the Speaker item watches a
/// single app's streams.
pub(crate) fn run_app_audio_level_monitor(
    label: &'static str,
    serial: u32,
    level: &'static std::sync::atomic::AtomicU64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    gst_audio::run_pipewire_level_monitor(label, serial, level, stop);
}
pub use controls::{
    persist_overlay_recording_request_state, prepare_overlay_recording_request,
    run_overlay_recording_request, run_overlay_recording_request_with_gtk,
    run_recording_with_controls, run_recording_with_native_controls, run_standalone_countdown,
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
            // One-shot stop-instruction notification (Stop button / shortcut).
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
            // Stop the GNOME pointer hook now so global hotkeys work while
            // ffmpeg still muxes. Sidecar samples are stashed for finish_save.
            controls::harvest_pointer_track_on_session_end();
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

struct RecordedSession {
    path: PathBuf,
    action: RecordingTerminalAction,
    timeline: Option<timeline::RecordingTimeline>,
}

#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub output_path: PathBuf,
    /// Portal or backend source selected for this recording.
    pub target: RecordingTarget,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub cursor: bool,
    pub pointer_track: bool,
    /// The window the recording follows on X11, when the target was picked
    /// outside GNOME. The source captures the screen and crops it to the
    /// window's current visible rectangle.
    pub x11_window: Option<crate::backend::x11_window_pick::X11WindowSource>,
    pub hidpi: bool,
    // Video tab settings
    pub max_resolution: Option<(u32, u32)>,
    pub fps: u32,
    /// x264 CRF from Settings → quality tier (16–23 recording range).
    pub crf: u32,
    pub mono_audio: bool,
    pub mic_enabled: bool,
    pub speaker_enabled: bool,
    pub mic_source: Option<String>,
    pub speaker_source: Option<String>,
    /// One app to record instead of every app on the default output. Linux
    /// only; `speaker_source` stays unset on these paths.
    pub speaker_app: Option<SpeakerApp>,
    /// webrtcdsp noise suppression on the mic branch (GStreamer audio path).
    pub noise_suppression: bool,
}

/// The app a recording captures alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerApp {
    pub name: String,
    pub pid: u32,
}

/// Capture source requested for a recording session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecordingTarget {
    /// Record the full screen or selected monitor.
    #[default]
    Screen,
    /// Record a selected region of a monitor.
    Area,
    /// Record a window selected by the ScreenCast portal.
    Window,
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
            target: RecordingTarget::Screen,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: true,
            pointer_track: false,
            x11_window: None,
            hidpi: true,
            max_resolution: None,
            fps: 30,
            crf: 20,
            mono_audio: false,
            mic_enabled: false,
            speaker_enabled: false,
            mic_source: None,
            speaker_source: None,
            speaker_app: None,
            noise_suppression: false,
        }
    }
}

impl RecordingConfig {
    /// Whether this target should record pointer motion and click metadata.
    pub(crate) fn pointer_track_enabled(&self) -> bool {
        self.pointer_track && self.target != RecordingTarget::Window
    }

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
        let max_resolution = max_resolution_for_setting(app_config.rec_video_max_res);

        Self {
            output_path,
            target: RecordingTarget::Screen,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: app_config.rec_cursor,
            pointer_track: self::pointer_track::should_use(),
            x11_window: None,
            hidpi: app_config.rec_hidpi,
            max_resolution,
            fps,
            crf: crf_for_quality(app_config.rec_video_quality),
            mono_audio: app_config.rec_video_mono,
            mic_enabled: false,
            speaker_enabled: false,
            // Hotkey recordings use the microphone the Quick Capture menus
            // saved. They always record every app: the saved speaker choice is
            // an app, whose pid is unknown here.
            mic_source: (!app_config.rec_mic_device.is_empty())
                .then(|| app_config.rec_mic_device.clone()),
            speaker_source: None,
            speaker_app: None,
            noise_suppression: app_config.rec_noise_suppression,
        }
    }
}

/// Number of options in Settings → "Maximum resolution". The index is the value
/// stored in `rec_video_max_res`, so the list order must match
/// [`max_resolution_for_setting`]. Settings is the only place that offers the
/// list; the capture overlay carries the configured value through untouched.
pub const VIDEO_MAX_RES_OPTION_COUNT: usize = 7;

/// Settings → "Maximum resolution" as a capture cap.
///
/// Index 0 is Original (native size, no rescaling at all). Every other entry is
/// a ceiling: a target at or above the capture size leaves the frame untouched
/// (no rescale = no quality loss); smaller targets are downscaled with lanczos
/// in RGB space plus CRF compensation. Nothing here upscales. The three larger
/// targets were appended after the original three so saved configs keep their
/// meaning.
pub fn max_resolution_for_setting(value: u8) -> Option<(u32, u32)> {
    match value {
        1 => Some((1920, 1080)),
        2 => Some((1280, 720)),
        3 => Some((2560, 1440)),
        4 => Some((1600, 900)),
        5 => Some((854, 480)),
        6 => Some((3840, 2160)),
        _ => None,
    }
}

/// Number of options in Settings → "Frame rate". The index is the value
/// stored in `rec_video_fps`, so the list order must match the frame-rate
/// match in `RecordingConfig::from_app_config_at` (24 / 30 / 50 / 60).
/// Settings is the only place that offers the list; `AppConfig::sanitized()`
/// rejects anything outside it before an overlay or the encoder sees it.
pub const VIDEO_FPS_OPTION_COUNT: usize = 4;

/// x264 CRF for a Settings quality tier, inside the recommended 16–23
/// recording range (sharpest tier = 16). Lower is sharper at file-size cost.
/// The video editor's export picker maps through these same tiers.
pub fn crf_for_quality(tier: u8) -> u32 {
    match tier {
        0 => 23, // Balanced
        2 => 16, // Ultra
        _ => 20, // High (default)
    }
}

/// Resolution compensation for CRF: smaller outputs get a lower CRF so they
/// don't turn to mush — up to -10 below 2000px diagonal. Applied to the tier
/// base at encode time.
pub fn crf_resolution_reduction(width: u32, height: u32) -> u32 {
    const CUTOFF: f64 = 2000.0;
    let diagonal = ((u64::from(width) * u64::from(width) + u64::from(height) * u64::from(height))
        as f64)
        .sqrt();
    ((1.0 - diagonal.min(CUTOFF) / CUTOFF) * 10.0) as u32
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
        .map(|session| session.path)
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
        .map(|session| {
            let stop_action = match session.action {
                RecordingTerminalAction::Save => StopAction::Save,
                RecordingTerminalAction::Discard => StopAction::Discard,
                RecordingTerminalAction::Restart => StopAction::Discard,
            };
            (session.path, stop_action)
        })
}

async fn start_recording_with_commands(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<RecordedSession> {
    if wf_recorder::is_wlroots_session() && config.target != RecordingTarget::Window {
        if wf_recorder::should_use_wf_recorder(&config) {
            return wf_recorder::record_with_wf_recorder(config, command_rx)
                .await
                .map(|(path, action)| RecordedSession {
                    path,
                    action,
                    timeline: None,
                });
        }
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording with this output format is not supported".into(),
        ));
    }

    let built = backend::prepare_recording_backend(config.clone()).await?;
    let stream_cursor_metadata = built.stream_cursor_metadata_enabled();
    let target = config.target;
    let fps = config.fps;
    // KDE's compositor gives no global button stream, so a temporary script
    // effect supplies the clicks the sidecar needs. It stays `None` on every
    // other session and on compositor failures.
    let click_bridge = if stream_cursor_metadata {
        tokio::task::spawn_blocking(kwin_clicks::KwinClickBridge::start)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let session = backend::start_recording_with_prepared_backend(built, command_rx).await?;
    let raw_clicks = match click_bridge {
        Some(bridge) => tokio::task::spawn_blocking(move || bridge.stop())
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    // Paths without recording controls (CLI screen recordings, API callers)
    // still have to leave the sidecar the editor reads. Without it a
    // metadata-mode capture has no cursor at all, because the compositor left
    // it out of the video.
    if stream_cursor_metadata && session.action == RecordingTerminalAction::Save {
        if let Some(timeline) = session.timeline.as_ref() {
            let (clicks, presses) = kwin_clicks::clicks_and_presses(&raw_clicks, timeline);
            controls::write_stream_cursor_metadata_sidecar(
                &session.path,
                target,
                timeline,
                f64::from(fps),
                clicks,
                presses,
            );
        }
    }
    Ok(session)
}

pub fn copy_to_clipboard(path: &Path) -> RecordResult<()> {
    println!("Copying to clipboard...");

    crate::utils::clipboard::copy_uri_to_clipboard(path).map_err(RecordError::GStreamerError)?;

    println!("Copied to clipboard!");
    Ok(())
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
    fn quality_tier_maps_to_crf_range() {
        assert_eq!(crf_for_quality(0), 23);
        assert_eq!(crf_for_quality(1), 20);
        assert_eq!(crf_for_quality(2), 16);
        assert_eq!(crf_for_quality(9), 20);
        // Full-HD and above keep the base CRF; smaller outputs get a bonus.
        assert_eq!(crf_resolution_reduction(1920, 1080), 0);
        assert_eq!(crf_resolution_reduction(1920, 1200), 0);
        assert_eq!(crf_resolution_reduction(1280, 720), 2);
        assert_eq!(crf_resolution_reduction(640, 480), 6);
        let app = AppConfig {
            rec_video_quality: 2,
            ..AppConfig::default()
        };
        let config = RecordingConfig::from_app_config_at(
            &app,
            "mp4",
            chrono::Utc
                .with_ymd_and_hms(2026, 7, 12, 11, 0, 49)
                .unwrap(),
        );
        assert_eq!(config.crf, 16);
    }

    #[test]
    fn resolution_setting_maps_to_every_option_and_never_upscales() {
        let at = chrono::Utc
            .with_ymd_and_hms(2026, 7, 12, 11, 0, 49)
            .unwrap();
        let resolution_for = |idx: u8| {
            let app = AppConfig {
                rec_video_max_res: idx,
                ..AppConfig::default()
            };
            RecordingConfig::from_app_config_at(&app, "mp4", at).max_resolution
        };
        // Saved configs from before the extra options keep their meaning.
        assert_eq!(resolution_for(0), None);
        assert_eq!(resolution_for(1), Some((1920, 1080)));
        assert_eq!(resolution_for(2), Some((1280, 720)));
        assert_eq!(resolution_for(3), Some((2560, 1440)));
        assert_eq!(resolution_for(4), Some((1600, 900)));
        assert_eq!(resolution_for(5), Some((854, 480)));
        assert_eq!(resolution_for(6), Some((3840, 2160)));
        assert_eq!(resolution_for(99), None);
        // A target at or above the capture size must leave frames untouched —
        // choosing a bigger option never costs quality.
        assert_eq!(
            super::backend::fit_within_max_resolution(1920, 1200, resolution_for(3)),
            (1920, 1200)
        );
        assert_eq!(
            super::backend::fit_within_max_resolution(1920, 1200, resolution_for(6)),
            (1920, 1200)
        );
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
