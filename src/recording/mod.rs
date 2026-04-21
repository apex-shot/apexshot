use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    capture_overlay::{RecordingRequest, RecordingType},
    config::{save_config, AppConfig},
};

mod control_session;
mod runtime_keystrokes;
mod stop_overlay;
use control_session::RecordingControlServer;
pub use control_session::{
    has_active_recording_control, send_active_recording_command, toggle_active_recording_pause,
    RecordingControlCommand,
};
pub use stop_overlay::{
    run_recording_controls, run_recording_countdown_bar, run_recording_stop_overlay,
    RecordingControlsParams, StopAction, StopOverlayError,
};

pub mod countdown_overlay;
pub mod dim_overlay;
pub mod dnd;

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

pub type RecordResult<T> = Result<T, RecordError>;

fn reap_child_if_exited(child: &mut Option<std::process::Child>) -> bool {
    let Some(process) = child.as_mut() else {
        return true;
    };

    match process.try_wait() {
        Ok(Some(_status)) => {
            *child = None;
            true
        }
        Ok(None) => false,
        Err(_) => false,
    }
}

fn daemon_event_for_terminal_action(action: RecordingTerminalAction) -> Option<&'static str> {
    match action {
        RecordingTerminalAction::Restart => Some("recording_session_restarted"),
        RecordingTerminalAction::Save | RecordingTerminalAction::Discard => {
            Some("recording_session_ended")
        }
    }
}

fn should_end_recording_ui_for_terminal_action(action: RecordingTerminalAction) -> bool {
    matches!(
        action,
        RecordingTerminalAction::Save | RecordingTerminalAction::Discard
    )
}

fn notify_daemon_event(event: &str) {
    match event {
        "recording_session_started" => {
            let _ = crate::daemon::notify_daemon_recording_started();
        }
        "recording_session_paused" => {
            let _ = crate::daemon::notify_daemon_recording_paused();
        }
        "recording_session_resumed" => {
            let _ = crate::daemon::notify_daemon_recording_resumed();
        }
        "recording_session_restarted" => {
            let _ = crate::daemon::notify_daemon_recording_restarted();
        }
        "recording_session_ended" => {
            let _ = crate::daemon::notify_daemon_recording_ended();
        }
        _ => {}
    }
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

#[derive(Debug)]
pub struct PreparedOverlayRecordingRequest {
    pub updated_app_config: AppConfig,
    pub output_path: PathBuf,
    pub recording_config: RecordingConfig,
    pub controls_params: Option<RecordingControlsParams>,
    pub shell_controls_visibility_policy:
        Option<crate::gnome_shell::RecordingControlsVisibilityPolicy>,
    pub runtime_overlay_snapshot: Option<RuntimeOverlaySnapshot>,
    pub use_shell_mask: bool,
    pub use_shell_controls: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuntimeOverlaySnapshot {
    pub mic_visible: bool,
    pub speaker_visible: bool,
    pub webcam_enabled: bool,
    pub webcam_preview_manifest_path: String,
    pub webcam_rel_x: f64,
    pub webcam_rel_y: f64,
    pub webcam_size: u8,
    pub webcam_shape: u8,
    pub webcam_flip: bool,
    pub webcam_device: i32,
    pub clicks_enabled: bool,
    pub click_size: f64,
    pub click_color: u8,
    pub click_style: u8,
    pub click_animate: bool,
    pub keystrokes_enabled: bool,
    pub keystrokes_supported: bool,
    pub keystrokes_support_message: String,
    pub key_size: f64,
    pub key_position: u8,
    pub key_appearance: u8,
    pub key_blur_bg: bool,
    pub key_filter: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CropMargins {
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
}

type RecordingPortalSession =
    ashpd::desktop::Session<'static, ashpd::desktop::screencast::Screencast<'static>>;

#[derive(Debug)]
struct WaylandSource {
    pipeline_source: String,
    crop: Option<CropMargins>,
    _session: RecordingPortalSession,
}

#[derive(Debug, Clone, Serialize)]
struct WebcamPreviewManifest {
    session_id: String,
    sequence: u64,
    frame_path: String,
    width: u32,
    height: u32,
    format: String,
}

#[derive(Debug)]
struct WebcamPreviewTransport {
    manifest_path: PathBuf,
    frame_dir: PathBuf,
}

#[derive(Debug)]
struct WebcamPreviewHandle {
    stop_tx: std::sync::mpsc::Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
    transport: WebcamPreviewTransport,
}

#[derive(Debug)]
struct BuiltPipeline {
    pipeline_str: String,
    wayland_source: Option<WaylandSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingScreenCastTarget {
    Screen,
    Area,
}

impl RecordingScreenCastTarget {
    fn token_file_name(self) -> &'static str {
        match self {
            Self::Screen => "wayland-record-screen.token",
            Self::Area => "wayland-record-area.token",
        }
    }
}

fn recording_restore_token_path(target: RecordingScreenCastTarget) -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push(target.token_file_name());
    Some(path)
}

fn load_recording_restore_token(target: RecordingScreenCastTarget) -> Option<String> {
    let path = recording_restore_token_path(target)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn save_recording_restore_token(target: RecordingScreenCastTarget, token: &str) {
    let Some(path) = recording_restore_token_path(target) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, token);
}

fn clear_recording_restore_token(target: RecordingScreenCastTarget) {
    if let Some(path) = recording_restore_token_path(target) {
        let _ = std::fs::remove_file(path);
    }
}

fn overlay_recording_output_dir(app_config: &AppConfig) -> PathBuf {
    if !app_config.video_export_location.is_empty() {
        PathBuf::from(&app_config.video_export_location)
    } else {
        dirs::video_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

fn webcam_preview_runtime_root() -> PathBuf {
    std::env::temp_dir().join("apexshot-gnome-webcam-preview")
}

fn webcam_preview_transport(session_id: &str) -> WebcamPreviewTransport {
    let frame_dir = webcam_preview_runtime_root().join(session_id);
    WebcamPreviewTransport {
        manifest_path: frame_dir.join("manifest.json"),
        frame_dir,
    }
}

fn webcam_preview_device_path(device: i32) -> Option<String> {
    (device >= 0).then(|| format!("/dev/video{device}"))
}

fn webcam_preview_pipeline(snapshot: &RuntimeOverlaySnapshot) -> Option<String> {
    let device = webcam_preview_device_path(snapshot.webcam_device)?;
    let flip_filter = if snapshot.webcam_flip {
        " ! videoflip method=horizontal-flip"
    } else {
        ""
    };

    Some(format!(
        "v4l2src device=\"{device}\" ! video/x-raw,width=640,height=480,framerate=30/1 ! videoconvert{flip_filter} ! video/x-raw,format=BGRA ! appsink name=sink emit-signals=true sync=false drop=true max-buffers=1"
    ))
}

fn write_webcam_preview_manifest(
    transport: &WebcamPreviewTransport,
    manifest: &WebcamPreviewManifest,
) -> RecordResult<()> {
    if let Some(parent) = transport.manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_manifest = transport.manifest_path.with_extension("json.tmp");
    let payload = serde_json::to_vec(manifest).map_err(|err| {
        RecordError::GStreamerError(format!("Failed to encode webcam preview manifest: {err}"))
    })?;
    std::fs::write(&tmp_manifest, payload)?;
    std::fs::rename(tmp_manifest, &transport.manifest_path)?;
    Ok(())
}

fn encode_webcam_preview_frame(
    sample: &gst::Sample,
    frame_path: &std::path::Path,
) -> RecordResult<(u32, u32)> {
    let buffer = sample
        .buffer()
        .ok_or_else(|| RecordError::GStreamerError("No buffer in webcam preview sample".into()))?;
    let map = buffer
        .map_readable()
        .map_err(|_| RecordError::GStreamerError("Failed to map webcam preview buffer".into()))?;
    let caps = sample
        .caps()
        .ok_or_else(|| RecordError::GStreamerError("No caps in webcam preview sample".into()))?;
    let structure = caps
        .structure(0)
        .ok_or_else(|| RecordError::GStreamerError("No structure in webcam preview caps".into()))?;
    let format = structure
        .get::<&str>("format")
        .map_err(|_| RecordError::GStreamerError("Missing webcam preview format".into()))?;
    let width = structure
        .get::<i32>("width")
        .map_err(|_| RecordError::GStreamerError("Missing webcam preview width".into()))?
        as u32;
    let height = structure
        .get::<i32>("height")
        .map_err(|_| RecordError::GStreamerError("Missing webcam preview height".into()))?
        as u32;

    // Convert BGRA to RGBA
    let mut rgba = map.as_slice().to_vec();
    if format == "BGRA" {
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    } else if format != "RGBA" {
        return Err(RecordError::GStreamerError(format!(
            "Unsupported webcam preview format: {format}"
        )));
    }

    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        RecordError::GStreamerError("Unexpected webcam preview frame shape".into())
    })?;

    // Use JPEG for faster encoding (much faster than PNG)
    let mut output = std::fs::File::create(frame_path).map_err(|err| {
        RecordError::GStreamerError(format!("Failed to create webcam preview frame: {err}"))
    })?;
    image
        .write_to(
            &mut std::io::BufWriter::new(&mut output),
            image::ImageFormat::Jpeg,
        )
        .map_err(|err| {
            RecordError::GStreamerError(format!("Failed to write webcam preview frame: {err}"))
        })?;
    Ok((width, height))
}

fn start_webcam_preview_transport(
    session_id: &str,
    snapshot: &RuntimeOverlaySnapshot,
) -> Option<WebcamPreviewHandle> {
    if !snapshot.webcam_enabled {
        return None;
    }

    let pipeline_str = webcam_preview_pipeline(snapshot)?;
    let transport = webcam_preview_transport(session_id);
    let _ = std::fs::create_dir_all(&transport.frame_dir);
    let _ = std::fs::remove_file(&transport.manifest_path);
    eprintln!(
        "[recording] webcam preview transport starting: session={} manifest={} pipeline={}",
        session_id,
        transport.manifest_path.display(),
        pipeline_str
    );

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let session_id = session_id.to_string();
    let frame_dir = transport.frame_dir.clone();
    let manifest_path = transport.manifest_path.clone();

    let join = std::thread::spawn(move || {
        if let Err(err) = gst::init() {
            eprintln!("[recording] webcam preview gst init failed: {err}");
            return;
        }

        let pipeline = match gst::parse::launch(&pipeline_str)
            .map_err(|e| {
                RecordError::GStreamerError(format!("Failed to parse webcam preview pipeline: {e}"))
            })
            .and_then(|element| {
                element.downcast::<gst::Pipeline>().map_err(|_| {
                    RecordError::GStreamerError("Failed to cast webcam preview pipeline".into())
                })
            }) {
            Ok(pipeline) => pipeline,
            Err(err) => {
                eprintln!("[recording] webcam preview pipeline setup failed: {err}");
                return;
            }
        };

        let appsink = match pipeline
            .by_name("sink")
            .ok_or_else(|| RecordError::GStreamerError("Webcam preview appsink not found".into()))
            .and_then(|element| {
                element.downcast::<gst_app::AppSink>().map_err(|_| {
                    RecordError::GStreamerError("Failed to cast webcam preview appsink".into())
                })
            }) {
            Ok(appsink) => appsink,
            Err(err) => {
                eprintln!("[recording] webcam preview sink setup failed: {err}");
                return;
            }
        };

        if let Err(err) = pipeline.set_state(gst::State::Playing) {
            eprintln!("[recording] webcam preview failed to start: {err}");
            let _ = pipeline.set_state(gst::State::Null);
            return;
        }

        let transport = WebcamPreviewTransport {
            manifest_path,
            frame_dir,
        };
        let bus = pipeline.bus();
        let mut sequence = 0_u64;
        let mut logged_first_frame = false;
        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
                sequence += 1;
                let frame_path = transport.frame_dir.join(format!("frame-{sequence}.jpg"));
                match encode_webcam_preview_frame(&sample, &frame_path) {
                    Ok((width, height)) => {
                        if let Err(err) = write_webcam_preview_manifest(
                            &transport,
                            &WebcamPreviewManifest {
                                session_id: session_id.clone(),
                                sequence,
                                frame_path: frame_path.to_string_lossy().into_owned(),
                                width,
                                height,
                                format: "jpeg".to_string(),
                            },
                        ) {
                            eprintln!("[recording] webcam preview manifest write failed: {err}");
                        }
                        if !logged_first_frame {
                            eprintln!(
                                "[recording] webcam preview first frame published: session={} frame={} manifest={}",
                                session_id,
                                frame_path.display(),
                                transport.manifest_path.display()
                            );
                            logged_first_frame = true;
                        }
                        if sequence > 2 {
                            let old_path = transport
                                .frame_dir
                                .join(format!("frame-{}.jpg", sequence - 2));
                            let _ = std::fs::remove_file(old_path);
                        }
                    }
                    Err(err) => {
                        eprintln!("[recording] webcam preview frame publish failed: {err}");
                    }
                }
            } else if let Some(bus) = &bus {
                for msg in bus.iter_timed(gst::ClockTime::ZERO) {
                    use gst::MessageView;
                    if let MessageView::Error(err) = msg.view() {
                        eprintln!(
                            "[recording] webcam preview bus error from {:?}: {}",
                            err.src().map(|s| s.name()),
                            err.error()
                        );
                        if let Some(debug) = err.debug() {
                            eprintln!("[recording] webcam preview debug: {debug}");
                        }
                    }
                }
            }
        }

        let _ = pipeline.set_state(gst::State::Null);
        eprintln!(
            "[recording] webcam preview transport stopped: session={} manifest={}",
            session_id,
            transport.manifest_path.display()
        );
    });

    Some(WebcamPreviewHandle {
        stop_tx,
        join: Some(join),
        transport,
    })
}

impl WebcamPreviewHandle {
    fn manifest_path(&self) -> &std::path::Path {
        &self.transport.manifest_path
    }
}

impl Drop for WebcamPreviewHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn compute_wayland_crop(
    stream_position: (i32, i32),
    stream_size: (i32, i32),
    selection: (i32, i32, u32, u32),
) -> Result<CropMargins, String> {
    let (stream_x, stream_y) = stream_position;
    let (stream_w, stream_h) = stream_size;
    let (sel_x, sel_y, sel_w, sel_h) = selection;

    if stream_w <= 0 || stream_h <= 0 || sel_w == 0 || sel_h == 0 {
        return Err("invalid stream or selection size".into());
    }

    let left = sel_x - stream_x;
    let top = sel_y - stream_y;
    let right = stream_w - left - sel_w as i32;
    let bottom = stream_h - top - sel_h as i32;

    if left < 0 || top < 0 || right < 0 || bottom < 0 {
        return Err("selection falls outside the selected monitor stream".into());
    }

    Ok(CropMargins {
        left: left as u32,
        right: right as u32,
        top: top as u32,
        bottom: bottom as u32,
    })
}

impl Default for RecordingConfig {
    fn default() -> Self {
        let mut path = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("output.mp4");
        Self {
            output_path: path,
            width: None,
            height: None,
            x: None,
            y: None,
            cursor: true,
            hidpi: false,
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

struct EncoderProfile {
    name: &'static str,
    encoder: &'static str,
    props: &'static str,
    muxer: &'static str,
    extension: &'static str,
}

// Priority list of encoders
const PROFILES: &[EncoderProfile] = &[
    // VP9 (WebM)
    EncoderProfile {
        name: "VP9",
        encoder: "vp9enc",
        props: "",
        muxer: "webmmux",
        extension: "webm",
    },
    // VP8 (WebM) - fallback when VP9 is unavailable
    EncoderProfile {
        name: "VP8",
        encoder: "vp8enc",
        props: "",
        muxer: "webmmux",
        extension: "webm",
    },
    // Standard H.264
    EncoderProfile {
        name: "H.264 (x264)",
        encoder: "x264enc",
        props: "",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Cisco OpenH264
    EncoderProfile {
        name: "H.264 (OpenH264)",
        encoder: "openh264enc",
        props: "",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Theora (Ogg) - Last resort
    EncoderProfile {
        name: "Theora",
        encoder: "theoraenc",
        props: "",
        muxer: "oggmux",
        extension: "ogv",
    },
];

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
    // 1. Initialize GStreamer
    gst::init().map_err(|e| RecordError::InitError(e.to_string()))?;

    // Check if GIF requested
    if config.output_path.extension().is_some_and(|e| e == "gif") {
        return record_gif_rust_with_commands(config, command_rx).await;
    }

    // 2. Select Encoder Profile
    let (profile, final_path) = select_encoder(config.output_path.as_path())?;
    println!("Using Encoder: {} ({})", profile.name, profile.encoder);

    if final_path != config.output_path {
        println!(
            "Note: Output filename changed to match format: {:?}",
            final_path
        );
    }

    // 3. Build pipeline description
    let built_pipeline = build_pipeline(&config, profile, final_path.as_path()).await?;
    let _wayland_source = built_pipeline.wayland_source;
    let pipeline_str = built_pipeline.pipeline_str;
    println!("Starting recording to: {:?}", final_path);

    // 4. Create pipeline
    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to parse pipeline: {}", e)))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| RecordError::GStreamerError("Cast to Pipeline failed".into()))?;

    // 5. Start playing
    if let Err(err) = pipeline.set_state(gst::State::Playing) {
        eprintln!("Failed to set pipeline to Playing: {}", err);
        if let Some(bus) = pipeline.bus() {
            while let Some(msg) = bus.pop() {
                if let gst::MessageView::Error(err) = msg.view() {
                    eprintln!(
                        "Detailed Error from {}: {}",
                        err.src().map(|s| s.name()).unwrap_or("unknown".into()),
                        err.error()
                    );
                    if let Some(debug) = err.debug() {
                        eprintln!("Debug Info: {}", debug);
                    }
                }
            }
        }
        let _ = pipeline.set_state(gst::State::Null);
        return Err(RecordError::GStreamerError(format!(
            "State change failed: {}",
            err
        )));
    }

    // 6. Watch for messages and recording control commands
    let bus = pipeline
        .bus()
        .ok_or_else(|| RecordError::GStreamerError("Pipeline has no bus".into()))?;

    println!("Recording...");

    let mut command_rx = command_rx;

    // Phase 1: Recording until explicit recording stop/restart/discard or Error
    let mut stopping = false;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut paused = false;
    loop {
        tokio::select! {
            command = async {
                match &mut command_rx {
                    Some(rx) => rx.recv().await,
                    None => futures_util::future::pending::<Option<RecordingControlCommand>>().await,
                }
            } => {
                let Some(command) = command else {
                    command_rx = None;
                    continue;
                };

                match command {
                    RecordingControlCommand::Pause if !paused => {
                        pipeline
                            .set_state(gst::State::Paused)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to pause pipeline: {e}")))?;
                        paused = true;
                        notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        pipeline
                            .set_state(gst::State::Playing)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to resume pipeline: {e}")))?;
                        paused = false;
                        notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = RecordingTerminalAction::Restart;
                        println!("\nRestarting recording... Finalizing current file...");
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = RecordingTerminalAction::Save;
                        println!("\nStopping recording... Finalizing file...");
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = RecordingTerminalAction::Discard;
                        println!("\nStopping recording... Finalizing file...");
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                // Poll bus
                for msg in bus.iter_timed(gst::ClockTime::ZERO) {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Eos(..) => {
                            println!("End of stream reached (unexpected).");
                            stopping = true;
                            break;
                        }
                        MessageView::Error(err) => {
                            eprintln!("Error from element {:?}: {}", err.src().map(|s| s.name()), err.error());
                            let _ = pipeline.set_state(gst::State::Null);
                            return Err(RecordError::GStreamerError(err.error().to_string()));
                        }
                        _ => (),
                    }
                }
                if stopping { break; }
            }
        }
    }

    // Phase 2: Wait for EOS if we initiated stop
    if stopping {
        let start_wait = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5); // 5s timeout for finalization

        loop {
            if start_wait.elapsed() > timeout {
                eprintln!("Timeout waiting for EOS. Forcing stop.");
                break;
            }

            // Check bus
            let mut eos_received = false;
            for msg in bus.iter_timed(gst::ClockTime::from_mseconds(100)) {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Eos(..) => {
                        println!("File finalized successfully.");
                        eos_received = true;
                        break;
                    }
                    MessageView::Error(err) => {
                        eprintln!("Error during finalization: {}", err.error());
                        eos_received = true; // Stop waiting
                        break;
                    }
                    _ => (),
                }
            }
            if eos_received {
                break;
            }
        }
    }

    // 7. Cleanup
    pipeline
        .set_state(gst::State::Null)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to set state to Null: {}", e)))?;

    if stop_action == RecordingTerminalAction::Save {
        println!("Recording saved to {:?}", final_path);
        if let Ok(metadata) = std::fs::metadata(&final_path) {
            println!(
                "File size: {:.2} MB",
                metadata.len() as f64 / 1024.0 / 1024.0
            );
        }
    }

    Ok((final_path, stop_action))
}

pub fn copy_to_clipboard(path: &PathBuf) -> RecordResult<()> {
    println!("Copying to clipboard...");

    crate::utils::clipboard::copy_uri_to_clipboard(path)
        .map_err(|e| RecordError::GStreamerError(e))?;

    println!("Copied to clipboard!");
    Ok(())
}

fn select_encoder(
    requested_path: &std::path::Path,
) -> RecordResult<(&'static EncoderProfile, PathBuf)> {
    // Check for x264enc first to warn user if missing. We can fall back, but motion-heavy
    // captures will usually look worse on the alternative encoders.
    if gst::ElementFactory::find("x264enc").is_none() {
        println!("\n\x1b[33mWARNING: H.264 encoder (x264enc) not found!\x1b[0m");
        println!("Falling back to inferior encoders (Theora/VP8). For high-quality MP4 recording, please install:");
        println!("  Ubuntu/Debian: \x1b[1msudo apt install gstreamer1.0-plugins-ugly\x1b[0m");
        println!("  Arch:          \x1b[1msudo pacman -S gst-plugins-ugly\x1b[0m");
        println!("  Fedora:        \x1b[1msudo dnf install gstreamer1-plugins-ugly-free\x1b[0m\n");
    }

    if let Some(ext) = requested_path.extension().and_then(|s| s.to_str()) {
        for profile in PROFILES {
            if profile.extension == ext
                && gst::ElementFactory::find(profile.encoder).is_some()
                && gst::ElementFactory::find(profile.muxer).is_some()
            {
                return Ok((profile, requested_path.to_path_buf()));
            }
        }
        println!(
            "Warning: Requested format '{}' not supported or encoder missing.",
            ext
        );
    }

    for profile in PROFILES {
        if gst::ElementFactory::find(profile.encoder).is_some()
            && gst::ElementFactory::find(profile.muxer).is_some()
        {
            let mut new_path = requested_path.to_path_buf();
            new_path.set_extension(profile.extension);
            return Ok((profile, new_path));
        }
    }

    Err(RecordError::NoEncoderFound)
}

fn get_pulse_default_source() -> String {
    std::process::Command::new("pactl")
        .arg("get-default-source")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

fn get_pulse_speaker_monitor() -> String {
    std::process::Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| format!("{}.monitor", s.trim()))
        .filter(|s| s != ".monitor")
        .unwrap_or_else(|| "default.monitor".to_string())
}

fn select_audio_encoder(muxer: &str) -> Option<&'static str> {
    let candidates: &[&str] = match muxer {
        "webmmux" => &["opusenc", "vorbisenc"],
        "mp4mux" => &["fdkaacenc", "avenc_aac", "lamemp3enc"],
        "oggmux" => &["vorbisenc"],
        _ => &[],
    };
    candidates
        .iter()
        .find(|&&enc| gst::ElementFactory::find(enc).is_some())
        .copied()
}

fn video_encoder_props(profile: &EncoderProfile, config: &RecordingConfig) -> String {
    let key_int_max = config.fps.saturating_mul(2).max(1);

    if profile.encoder == "x264enc" {
        // Screen recordings favor detail retention over ultra-low-latency delivery.
        // Bias toward structural detail so scrolling text and thumbnails stay sharper
        // in both area and fullscreen recordings.
        return format!(
            "speed-preset=medium pass=qual quantizer=14 bitrate=8000 key-int-max={} rc-lookahead=20 ref=4 subme=6 psy-tune=ssim",
            key_int_max
        );
    }

    if profile.encoder == "vp9enc" {
        // The libvpx defaults target just 256 kbps, which is far too low for desktop
        // capture and leads to obvious grain on moving text and thumbnails. Keep a
        // higher quality target, but avoid realtime settings that turn desktop motion
        // into visible blockiness.
        return format!(
            "deadline=10000 end-usage=cq cq-level=10 target-bitrate=0 bits-per-pixel=0.12 cpu-used=4 row-mt=true threads=8 keyframe-max-dist={} lag-in-frames=0",
            key_int_max
        );
    }

    if profile.encoder == "vp8enc" {
        return format!(
            "deadline=8 end-usage=cq cq-level=10 target-bitrate=0 bits-per-pixel=0.12 cpu-used=4 threads=8 keyframe-max-dist={} lag-in-frames=0",
            key_int_max
        );
    }

    profile.props.to_string()
}

fn video_raw_caps(profile: &EncoderProfile, config: &RecordingConfig) -> String {
    if profile.encoder == "x264enc" {
        return format!("video/x-raw,framerate={}/1,format=I420", config.fps);
    }

    format!("video/x-raw,framerate={}/1", config.fps)
}

fn video_queue_props(profile: &EncoderProfile) -> &'static str {
    if profile.encoder == "x264enc" {
        // x264enc can accumulate latency; use a larger queue budget than the
        // default live-pipeline settings to avoid visible stutter under motion.
        "queue max-size-time=5000000000 max-size-bytes=0 max-size-buffers=0"
    } else {
        "queue"
    }
}

fn video_post_encoder_caps(profile: &EncoderProfile) -> &'static str {
    if profile.encoder == "x264enc" {
        " ! video/x-h264,profile=high"
    } else {
        ""
    }
}

async fn build_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<BuiltPipeline> {
    let output_str = output_path.to_string_lossy();

    // Get video source
    let wayland_source = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        Some(get_wayland_source(config).await?)
    } else {
        None
    };
    let (video_source, crop_filter) = if let Some(source) = &wayland_source {
        let crop_filter = source.crop.map_or_else(String::new, |crop| {
            format!(
                " ! videocrop left={} right={} top={} bottom={}",
                crop.left, crop.right, crop.top, crop.bottom
            )
        });
        (source.pipeline_source.clone(), crop_filter)
    } else {
        (get_x11_source(config)?, String::new())
    };

    // HiDPI: downscale to logical resolution (2x)
    let hidpi_filter = if config.hidpi { " ! videoscale" } else { "" };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale
                format!(
                    " ! videoscale ! video/x-raw,width={},height={}",
                    max_w, max_h
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let want_audio = config.mic_enabled || config.speaker_enabled;

    let audio_encoder = if want_audio {
        if gst::ElementFactory::find("pulsesrc").is_none() {
            eprintln!(
                "[recording] pulsesrc not found (gst-plugins-good missing?); recording without audio."
            );
            None
        } else {
            let enc = select_audio_encoder(profile.muxer);
            if enc.is_none() {
                eprintln!(
                    "[recording] No audio encoder found for muxer {}; recording without audio.",
                    profile.muxer
                );
            }
            enc
        }
    } else {
        None
    };

    let mono_caps = if config.mono_audio {
        " ! audio/x-raw,channels=1"
    } else {
        ""
    };

    let mic_dev = config
        .mic_source
        .clone()
        .unwrap_or_else(get_pulse_default_source);
    let spk_dev = config
        .speaker_source
        .clone()
        .unwrap_or_else(get_pulse_speaker_monitor);
    let video_encoder = {
        let props = video_encoder_props(profile, config);
        if props.is_empty() {
            profile.encoder.to_string()
        } else {
            format!("{} {}", profile.encoder, props)
        }
    };
    let video_raw_caps = video_raw_caps(profile, config);
    let video_queue = video_queue_props(profile);
    let video_post_caps = video_post_encoder_caps(profile);

    if let Some(aenc) = audio_encoder {
        let muxer_named = format!("{} name=mux", profile.muxer);

        let video_leg = format!(
            "{}{} ! videoconvert{}{} ! videorate ! {} ! {} ! {}{} ! mux.",
            video_source,
            crop_filter,
            hidpi_filter,
            resolution_filter,
            video_raw_caps,
            video_queue,
            video_encoder,
            video_post_caps
        );

        let filesink_leg = format!("{} ! filesink location=\"{}\"", muxer_named, output_str);

        let audio_legs = if config.mic_enabled && config.speaker_enabled {
            vec![
                format!("audiomixer name=amix ! {} ! mux.", aenc),
                format!(
                    "pulsesrc device=\"{}\" ! audioconvert ! audioresample{} ! queue ! amix.",
                    mic_dev, mono_caps
                ),
                format!(
                    "pulsesrc device=\"{}\" ! audioconvert ! audioresample{} ! queue ! amix.",
                    spk_dev, mono_caps
                ),
            ]
        } else {
            let dev = if config.mic_enabled {
                &mic_dev
            } else {
                &spk_dev
            };
            vec![format!(
                "pulsesrc device=\"{}\" ! audioconvert ! audioresample{} ! queue ! {} ! mux.",
                dev, mono_caps, aenc
            )]
        };

        let full = std::iter::once(video_leg)
            .chain(std::iter::once(filesink_leg))
            .chain(audio_legs)
            .collect::<Vec<_>>()
            .join("  ");

        Ok(BuiltPipeline {
            pipeline_str: full,
            wayland_source,
        })
    } else {
        Ok(BuiltPipeline {
            pipeline_str: format!(
            "{}{} ! videoconvert{}{} ! videorate ! {} ! {} ! {}{} ! {} ! filesink location=\"{}\"",
            video_source, crop_filter, hidpi_filter, resolution_filter, video_raw_caps,
            video_queue, video_encoder, video_post_caps, profile.muxer, output_str
            ),
            wayland_source,
        })
    }
}

async fn get_wayland_source(config: &RecordingConfig) -> RecordResult<WaylandSource> {
    use ashpd::desktop::{
        screencast::{CursorMode, Screencast, SourceType},
        PersistMode,
    };

    println!("Requesting Wayland ScreenCast session...");

    let wants_area_crop = matches!(
        (config.x, config.y, config.width, config.height),
        (Some(_), Some(_), Some(_), Some(_))
    );
    let target = if wants_area_crop {
        RecordingScreenCastTarget::Area
    } else {
        RecordingScreenCastTarget::Screen
    };
    let cursor_mode = if config.cursor {
        CursorMode::Embedded
    } else {
        CursorMode::Hidden
    };

    async fn request_screencast(
        cursor_mode: CursorMode,
        wants_area_crop: bool,
        restore_token: Option<&str>,
        persist_mode: PersistMode,
    ) -> RecordResult<(ashpd::desktop::screencast::Streams, RecordingPortalSession)> {
        let _portal_identity = crate::utils::desktop_env::scoped_portal_capture_identity();

        let proxy = Screencast::new()
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        let session = proxy
            .create_session()
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        let source_types = if wants_area_crop {
            SourceType::Monitor.into()
        } else {
            (SourceType::Monitor | SourceType::Window).into()
        };

        proxy
            .select_sources(
                &session,
                cursor_mode,
                source_types,
                false,
                restore_token,
                persist_mode,
            )
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?
            .response()
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        if restore_token.is_none() {
            if wants_area_crop {
                println!("Please select the monitor containing the recording area...");
            } else {
                println!("Please select a screen or window to record...");
            }
        }

        let response = proxy
            .start(&session, None)
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?
            .response()
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        Ok((response, session))
    }

    let (response, session) = if let Some(token) = load_recording_restore_token(target) {
        match request_screencast(
            cursor_mode,
            wants_area_crop,
            Some(token.as_str()),
            PersistMode::ExplicitlyRevoked,
        )
        .await
        {
            Ok(response) => response,
            Err(err) => {
                eprintln!(
                    "[recording] ScreenCast restore token failed for {:?}: {err}; retrying interactively.",
                    target
                );
                clear_recording_restore_token(target);
                let response = request_screencast(
                    cursor_mode,
                    wants_area_crop,
                    None,
                    PersistMode::ExplicitlyRevoked,
                )
                .await?;
                if let Some(token) = response.0.restore_token() {
                    if !token.trim().is_empty() {
                        save_recording_restore_token(target, token);
                    }
                }
                response
            }
        }
    } else {
        let response = request_screencast(
            cursor_mode,
            wants_area_crop,
            None,
            PersistMode::ExplicitlyRevoked,
        )
        .await?;
        if let Some(token) = response.0.restore_token() {
            if !token.trim().is_empty() {
                save_recording_restore_token(target, token);
            }
        }
        response
    };

    let stream = response
        .streams()
        .first()
        .ok_or_else(|| RecordError::PortalError("No streams returned".into()))?;

    let node_id = stream.pipe_wire_node_id();
    println!("Got PipeWire Node ID: {}", node_id);
    println!(
        "Wayland stream metadata: position={:?} size={:?} type={:?}",
        stream.position(),
        stream.size(),
        stream.source_type()
    );

    let crop = if wants_area_crop {
        let position = stream.position().ok_or_else(|| {
            RecordError::PortalError(
                "The selected Wayland stream did not expose monitor position metadata".into(),
            )
        })?;
        let size = stream.size().ok_or_else(|| {
            RecordError::PortalError(
                "The selected Wayland stream did not expose monitor size metadata".into(),
            )
        })?;
        let selection = (
            config.x.expect("checked above"),
            config.y.expect("checked above"),
            config.width.expect("checked above"),
            config.height.expect("checked above"),
        );
        Some(compute_wayland_crop(position, size, selection).map_err(RecordError::PortalError)?)
    } else {
        None
    };

    Ok(WaylandSource {
        pipeline_source: format!("pipewiresrc path={} do-timestamp=true", node_id),
        crop,
        _session: session,
    })
}

fn get_x11_source(config: &RecordingConfig) -> RecordResult<String> {
    let show_pointer = if config.cursor { "true" } else { "false" };
    let mut source = format!("ximagesrc show-pointer={} use-damage=false", show_pointer);

    if let (Some(x), Some(y), Some(w), Some(h)) = (config.x, config.y, config.width, config.height)
    {
        source.push_str(&format!(
            " startx={} starty={} endx={} endy={}",
            x,
            y,
            x + w as i32 - 1,
            y + h as i32 - 1
        ));
    }

    Ok(source)
}

async fn record_gif_rust_with_commands(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    println!("Starting GIF recording (via FFmpeg Pipe)...");

    // Check if ffmpeg is available
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("Error: ffmpeg not found!");
        eprintln!("Please install ffmpeg to record GIFs:");
        eprintln!("  sudo pacman -S ffmpeg");
        eprintln!("  sudo apt install ffmpeg");
        return Err(RecordError::NoEncoderFound);
    }

    // Build pipeline: Source -> videoconvert -> rgba -> appsink
    let mut wayland_source = None;
    let (source_str, crop_filter) = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        let source = get_wayland_source(&config).await?;
        let crop_filter = source.crop.map_or_else(String::new, |crop| {
            format!(
                " ! videocrop left={} right={} top={} bottom={}",
                crop.left, crop.right, crop.top, crop.bottom
            )
        });
        let pipeline_source = source.pipeline_source.clone();
        wayland_source = Some(source);
        (pipeline_source, crop_filter)
    } else {
        (get_x11_source(&config)?, String::new())
    };

    // HiDPI: downscale to logical resolution (2x)
    let hidpi_filter = if config.hidpi { " ! videoscale" } else { "" };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale
                format!(
                    " ! videoscale ! video/x-raw,width={},height={}",
                    max_w, max_h
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Use configured FPS for GIF recording
    let gif_fps = config.fps;

    let pipeline_str = format!(
        "{}{} ! videoconvert{}{} ! videorate ! video/x-raw,format=RGBA,framerate={}/1 ! appsink name=sink emit-signals=true sync=false drop=false max-buffers=200",
        source_str, crop_filter, hidpi_filter, resolution_filter, gif_fps
    );

    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to parse pipeline: {}", e)))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| RecordError::GStreamerError("Cast to Pipeline failed".into()))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| RecordError::GStreamerError("AppSink not found".into()))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| RecordError::GStreamerError("Cast to AppSink failed".into()))?;

    // Start pipeline
    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to start pipeline: {}", e)))?;

    println!("Recording GIF...");

    let mut command_rx = command_rx;

    let mut stopping = false;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut ffmpeg_child: Option<std::process::Child> = None;
    let mut paused = false;

    loop {
        tokio::select! {
            command = async {
                match &mut command_rx {
                    Some(rx) => rx.recv().await,
                    None => futures_util::future::pending::<Option<RecordingControlCommand>>().await,
                }
            } => {
                let Some(command) = command else {
                    command_rx = None;
                    continue;
                };

                match command {
                    RecordingControlCommand::Pause if !paused => {
                        pipeline
                            .set_state(gst::State::Paused)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to pause GIF pipeline: {e}")))?;
                        paused = true;
                        notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        pipeline
                            .set_state(gst::State::Playing)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to resume GIF pipeline: {e}")))?;
                        paused = false;
                        notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = RecordingTerminalAction::Restart;
                        println!("\nRestarting recording...");
                        stopping = true;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = RecordingTerminalAction::Save;
                        println!("\nStopping recording...");
                        stopping = true;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = RecordingTerminalAction::Discard;
                        println!("\nStopping recording...");
                        stopping = true;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {
                // Pull sample
                match appsink.try_pull_sample(gst::ClockTime::from_mseconds(5)) {
                    Some(sample) => {
                        let buffer = sample.buffer().ok_or_else(|| RecordError::GStreamerError("No buffer in sample".into()))?;
                        let map = buffer.map_readable().map_err(|_| RecordError::GStreamerError("Failed to map buffer".into()))?;

                        // Initialize FFmpeg on first frame
                        if ffmpeg_child.is_none() {
                            let caps = sample.caps().ok_or_else(|| RecordError::GStreamerError("No caps".into()))?;
                            let structure = caps.structure(0).ok_or_else(|| RecordError::GStreamerError("No structure".into()))?;
                            let width = structure.get::<i32>("width").map_err(|_| RecordError::GStreamerError("No width".into()))? as u32;
                            let height = structure.get::<i32>("height").map_err(|_| RecordError::GStreamerError("No height".into()))? as u32;

                            println!("Detected stream: {}x{}", width, height);

                            let max_colors = ((32.0 + 224.0 * config.gif_quality) as u32).clamp(32, 256);
                            let dither = if config.gif_quality >= 0.5 {
                                "floyd_steinberg"
                            } else {
                                "bayer:bayer_scale=5"
                            };
                            let stats_mode = if config.gif_optimize { "diff" } else { "full" };
                            let scale_prefix = match config.gif_max_width {
                                Some(max_w) if width > max_w => format!("scale={}:-1,", max_w),
                                _ => String::new(),
                            };
                            let vf_filter = format!(
                                "{}split[s0][s1];[s0]palettegen=max_colors={}:stats_mode={}[p];[s1][p]paletteuse=dither={}",
                                scale_prefix, max_colors, stats_mode, dither
                            );

                            let child = Command::new("ffmpeg")
                                .arg("-y") // Overwrite
                                .arg("-loglevel").arg("warning")
                                .arg("-nostats")
                                .arg("-f").arg("rawvideo")
                                .arg("-pix_fmt").arg("rgba")
                                .arg("-s").arg(format!("{}x{}", width, height))
                                .arg("-r").arg(gif_fps.to_string())
                                .arg("-i").arg("pipe:0")
                                .arg("-vf").arg(&vf_filter)
                                .arg(&config.output_path)
                                .stdin(Stdio::piped())
                                .stdout(Stdio::null())
                                .stderr(Stdio::inherit())
                                .spawn()
                                .map_err(RecordError::IoError)?;

                            ffmpeg_child = Some(child);
                        }

                        // Write to FFmpeg stdin
                        if let Some(child) = &mut ffmpeg_child {
                            if let Some(stdin) = &mut child.stdin {
                                if let Err(e) = stdin.write_all(map.as_slice()) {
                                    // Broken pipe usually means ffmpeg exited
                                    if e.kind() != std::io::ErrorKind::BrokenPipe {
                                        eprintln!("Failed to write to ffmpeg: {}", e);
                                    }
                                    stopping = true;
                                }
                            }
                        }
                    }
                    None => {
                        // No data yet
                    }
                }
            }
        }
        if stopping {
            break;
        }
    }

    // Stop pipeline
    pipeline
        .set_state(gst::State::Null)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to stop pipeline: {}", e)))?;

    // Close stdin to signal EOF to ffmpeg
    if let Some(mut child) = ffmpeg_child {
        drop(child.stdin.take()); // Close stdin
        println!("Finalizing GIF (FFmpeg processing)...");
        let status = child.wait().map_err(RecordError::IoError)?;

        if !status.success() {
            let code = status.code();
            #[cfg(unix)]
            let signal = {
                use std::os::unix::process::ExitStatusExt;
                status.signal()
            };
            #[cfg(not(unix))]
            let signal = None;

            // Signal 2 (SIGINT) is expected because Ctrl+C hits the whole process group.
            // Some FFmpeg versions/filters return 255 or 130 on interruption.
            let is_expected_interruption =
                signal == Some(2) || code == Some(255) || code == Some(130);

            if !is_expected_interruption {
                return Err(RecordError::GifError(format!(
                    "FFmpeg failed with status: {}",
                    status
                )));
            }
        }
    } else {
        return Err(RecordError::GifError("No frames captured".into()));
    }

    if stop_action == RecordingTerminalAction::Save {
        println!("GIF saved to {:?}", config.output_path);
    }
    drop(wayland_source);
    Ok((config.output_path, stop_action))
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

fn should_use_shell_controls_for_request(
    _request: &RecordingRequest,
    shell_overlay_available: bool,
) -> bool {
    shell_overlay_available
}

fn shell_controls_visibility_policy_for_request(
    _request: &RecordingRequest,
) -> crate::gnome_shell::RecordingControlsVisibilityPolicy {
    crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden
}

fn shell_controls_visibility_policy_for_params(
    _params: &RecordingControlsParams,
) -> crate::gnome_shell::RecordingControlsVisibilityPolicy {
    crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden
}

fn should_use_legacy_pre_record_dim(request: &RecordingRequest, use_shell_mask: bool) -> bool {
    request.dim_screen && request.countdown && !use_shell_mask
}

pub fn prepare_overlay_recording_request(
    mut app_config: AppConfig,
    request: &RecordingRequest,
    now: chrono::DateTime<chrono::Utc>,
) -> PreparedOverlayRecordingRequest {
    let shell_overlay_available =
        crate::gnome_shell::current_session_supports_gnome_shell_overlay();
    let use_shell_mask = should_use_shell_mask_for_request(request, shell_overlay_available);
    let use_shell_controls =
        should_use_shell_controls_for_request(request, shell_overlay_available);
    let shell_controls_visibility_policy =
        use_shell_controls.then(|| shell_controls_visibility_policy_for_request(request));
    app_config.rec_controls = request.controls;
    app_config.rec_display_time = request.display_rec_time;
    app_config.rec_hidpi = request.hidpi;
    app_config.rec_notifications = request.notifications;
    app_config.rec_cursor = request.cursor;
    app_config.rec_clicks = request.clicks;
    app_config.rec_keystrokes = request.keystrokes;
    app_config.rec_remember_selection = request.remember_selection;
    app_config.rec_dim_screen = request.dim_screen;
    app_config.rec_countdown = request.countdown;
    app_config.rec_click_size = request.click_size;
    app_config.rec_click_color = request.click_color;
    app_config.rec_click_style = request.click_style;
    app_config.rec_click_animate = request.click_animate;
    app_config.rec_key_size = request.key_size;
    app_config.rec_key_position = request.key_position;
    app_config.rec_key_appearance = request.key_appearance;
    app_config.rec_key_blur_bg = request.key_blur_bg;
    app_config.rec_key_filter = request.key_filter;
    app_config.rec_webcam_enabled = request.webcam;
    app_config.rec_webcam_size = request.webcam_size;
    app_config.rec_webcam_shape = request.webcam_shape;
    app_config.rec_webcam_flip = request.webcam_flip;
    app_config.rec_webcam_device = request.webcam_device;
    app_config.rec_webcam_rel_x = request.webcam_rel_x;
    app_config.rec_webcam_rel_y = request.webcam_rel_y;
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
    let output_dir = overlay_recording_output_dir(&app_config);

    // Generate filename using pattern from config
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H-%M-%S").to_string();
    let filename = app_config.rec_filename_pattern
        .replace("{Date}", &date_str)
        .replace("{Time}", &time_str);
    let output_path = output_dir.join(format!("{}.{}", filename, extension));

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

    let recording_config = RecordingConfig {
        output_path: output_path.clone(),
        width: capture_width,
        height: capture_height,
        x: capture_x,
        y: capture_y,
        cursor: request.cursor,
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

    let runtime_overlay_snapshot = use_shell_controls.then_some(RuntimeOverlaySnapshot {
        mic_visible: request.mic,
        speaker_visible: request.speaker,
        webcam_enabled: request.webcam,
        webcam_preview_manifest_path: String::new(),
        webcam_rel_x: request.webcam_rel_x,
        webcam_rel_y: request.webcam_rel_y,
        webcam_size: request.webcam_size,
        webcam_shape: request.webcam_shape,
        webcam_flip: request.webcam_flip,
        webcam_device: request.webcam_device,
        clicks_enabled: request.clicks,
        click_size: request.click_size,
        click_color: request.click_color,
        click_style: request.click_style,
        click_animate: request.click_animate,
        keystrokes_enabled: request.keystrokes,
        keystrokes_supported: false,
        keystrokes_support_message: "Not supported on GNOME Wayland yet".to_string(),
        key_size: request.key_size,
        key_position: request.key_position,
        key_appearance: request.key_appearance,
        key_blur_bg: request.key_blur_bg,
        key_filter: request.key_filter,
    });

    let controls_params = Some(RecordingControlsParams {
        capture_x: request.x,
        capture_y: request.y,
        capture_w: request.width,
        capture_h: request.height,
        is_fullscreen: request.fullscreen,
        show_timer: true,
        use_shell_mask,
    });

    PreparedOverlayRecordingRequest {
        updated_app_config: app_config,
        output_path,
        recording_config,
        controls_params,
        shell_controls_visibility_policy,
        runtime_overlay_snapshot,
        use_shell_mask,
        use_shell_controls,
    }
}

pub async fn run_recording_with_controls(
    config: RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    run_recording_with_controls_with_runtime_overlay(config, params, None, None).await
}

async fn run_recording_with_controls_with_runtime_overlay(
    config: RecordingConfig,
    params: RecordingControlsParams,
    runtime_overlay_snapshot: Option<RuntimeOverlaySnapshot>,
    visibility_policy: Option<crate::gnome_shell::RecordingControlsVisibilityPolicy>,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Check if GNOME Shell extension is available before starting recording
    if !crate::gnome_shell::current_session_supports_gnome_shell_overlay() {
        return Err(anyhow::anyhow!(
            "GNOME Shell extension is not installed. Please install the ApexShot GNOME Shell extension first.\n\n\
            You can install it from the onboarding window or by running:\n\
            gnome-extensions install apexshot-gnome-integration@apexshot.github.io"
        ));
    }

    // Always use GNOME Shell recording controls (no fallback to headless)
    run_recording_with_shell_controls(
        config,
        params,
        runtime_overlay_snapshot,
        visibility_policy
            .unwrap_or_else(|| shell_controls_visibility_policy_for_params(&params)),
    )
    .await
}

pub async fn run_recording_with_cpp_controls(
    config: RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id).await?;

    let controls_child = Arc::new(Mutex::new(Some(
        crate::capture_overlay::spawn_recording_controls_via_cpp(
            control_server.bus_name(),
            control_server.session_id(),
            params,
        )?,
    )));

    let controls_child_reaper = Arc::clone(&controls_child);
    let reaper_task = tokio::spawn(async move {
        loop {
            let finished = {
                if let Ok(mut child) = controls_child_reaper.lock() {
                    reap_child_if_exited(&mut child)
                } else {
                    true
                }
            };
            if finished {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    notify_daemon_event("recording_session_started");
    let final_outcome = loop {
        let (recording_command_tx, command_rx) = mpsc::unbounded_channel();
        let (control_command_tx, mut control_command_rx) =
            mpsc::unbounded_channel::<RecordingControlCommand>();
        let controls_child_for_task = Arc::clone(&controls_child);
        let forward_commands = tokio::spawn(async move {
            while let Some(command) = control_command_rx.recv().await {
                if command.ends_session() {
                    if let Ok(mut child) = controls_child_for_task.lock() {
                        if let Some(process) = child.as_mut() {
                            let _ = process.kill();
                            let _ = process.wait();
                        }
                        *child = None;
                    }
                }

                if recording_command_tx.send(command).is_err() {
                    break;
                }
            }
        });

        control_server.set_command_sender(control_command_tx);
        let outcome = start_recording_with_commands(config.clone(), Some(command_rx)).await;
        control_server.clear_command_sender();
        forward_commands.abort();
        let outcome = outcome?;
        if should_end_recording_ui_for_terminal_action(outcome.1) {
            control_server.end_recording_ui();
        }

        match outcome {
            (path, action @ RecordingTerminalAction::Restart) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                let _ = std::fs::remove_file(&path);
                continue;
            }
            (path, action @ RecordingTerminalAction::Save) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                break (path, StopAction::Save);
            }
            (path, action @ RecordingTerminalAction::Discard) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                break (path, StopAction::Discard);
            }
        }
    };

    reaper_task.abort();
    if let Ok(mut child) = controls_child.lock() {
        if let Some(process) = child.as_mut() {
            let _ = process.kill();
            let _ = process.wait();
        }
        *child = None;
    }
    drop(control_server);

    Ok(final_outcome)
}

async fn run_recording_with_shell_controls(
    config: RecordingConfig,
    params: RecordingControlsParams,
    runtime_overlay_snapshot: Option<RuntimeOverlaySnapshot>,
    visibility_policy: crate::gnome_shell::RecordingControlsVisibilityPolicy,
) -> anyhow::Result<(PathBuf, StopAction)> {
    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id.clone()).await?;
    let mut runtime_overlay_snapshot = runtime_overlay_snapshot;
    let webcam_preview = runtime_overlay_snapshot.as_mut().and_then(|snapshot| {
        let handle = start_webcam_preview_transport(&session_id, snapshot)?;
        snapshot.webcam_preview_manifest_path =
            handle.manifest_path().to_string_lossy().into_owned();
        Some(handle)
    });
    let controls_handle =
        crate::gnome_shell::show_recording_controls(&crate::gnome_shell::RecordingControlsSpec {
            dbus_dest: control_server.bus_name().to_string(),
            session_id: session_id.clone(),
            geometry: crate::gnome_shell::RecordingMaskGeometry {
                x: params.capture_x,
                y: params.capture_y,
                width: params.capture_w,
                height: params.capture_h,
            },
            is_fullscreen: params.is_fullscreen,
            show_timer: params.show_timer,
            visibility_policy,
            runtime_overlay_snapshot: runtime_overlay_snapshot.clone(),
        })?;
    let keystroke_forwarder = runtime_overlay_snapshot
        .filter(|snapshot| snapshot.keystrokes_enabled && snapshot.keystrokes_supported)
        .map(|snapshot| {
            runtime_keystrokes::spawn_runtime_keystroke_forwarder(
                session_id.clone(),
                snapshot.key_filter.min(1),
            )
        });

    notify_daemon_event("recording_session_started");
    let final_outcome = loop {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        control_server.set_command_sender(command_tx);
        let outcome = start_recording_with_commands(config.clone(), Some(command_rx)).await;
        control_server.clear_command_sender();
        let outcome = outcome?;
        if should_end_recording_ui_for_terminal_action(outcome.1) {
            control_server.end_recording_ui();
        }

        match outcome {
            (path, action @ RecordingTerminalAction::Restart) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                let _ = std::fs::remove_file(&path);
                continue;
            }
            (path, action @ RecordingTerminalAction::Save) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                break (path, StopAction::Save);
            }
            (path, action @ RecordingTerminalAction::Discard) => {
                if let Some(event) = daemon_event_for_terminal_action(action) {
                    notify_daemon_event(event);
                }
                break (path, StopAction::Discard);
            }
        }
    };

    drop(controls_handle);
    drop(control_server);
    drop(webcam_preview);
    if let Some(forwarder) = keystroke_forwarder {
        forwarder.stop();
    }

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

    let use_legacy_pre_record_dim =
        should_use_legacy_pre_record_dim(&request, prepared.use_shell_mask);
    let dim_close = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let dim_handle = if use_legacy_pre_record_dim {
        let flag = dim_close.clone();
        Some(std::thread::spawn(move || {
            crate::recording::dim_overlay::run_dim_overlay(flag);
        }))
    } else {
        None
    };

    dim_close.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(handle) = dim_handle {
        let _ = handle.join();
    }

    let _shell_mask = if prepared.use_shell_mask {
        match crate::gnome_shell::show_recording_mask(crate::gnome_shell::geometry_from_request(
            &request,
        )) {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!("[recording] Failed to show GNOME shell recording mask ({err}); continuing without shell mask.");
                None
            }
        }
    } else {
        None
    };

    eprintln!("Starting recording to {:?}...", prepared.output_path);

    let outcome = tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::current();
        if let Some(params) = prepared.controls_params {
            handle
                .block_on(run_recording_with_controls_with_runtime_overlay(
                    prepared.recording_config.clone(),
                    params,
                    prepared.runtime_overlay_snapshot,
                    prepared.shell_controls_visibility_policy,
                ))
                .map_err(|err| anyhow::anyhow!("failed to run recording controls: {err}"))
        } else {
            handle
                .block_on(start_recording(prepared.recording_config.clone()))
                .map(|path| (path, StopAction::Save))
                .map_err(|err| anyhow::anyhow!("Recording failed: {err}"))
        }
    });

    match outcome {
        Ok((path, StopAction::Discard)) => {
            eprintln!("Recording discarded — deleting {:?}", path);
            let _ = std::fs::remove_file(&path);
            Ok(path)
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("Recording saved to {:?}", path);
            Ok(path)
        }
        Err(err) => Err(anyhow::anyhow!("Recording failed: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_overlay::{RecordingRequest, RecordingType};
    use crate::config::AppConfig;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;
    use std::path::Path;

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
    fn restart_does_not_request_recording_ui_teardown() {
        assert!(!should_end_recording_ui_for_terminal_action(
            RecordingTerminalAction::Restart
        ));
        assert!(should_end_recording_ui_for_terminal_action(
            RecordingTerminalAction::Save
        ));
        assert!(should_end_recording_ui_for_terminal_action(
            RecordingTerminalAction::Discard
        ));
    }

    fn x11_recording_config() -> RecordingConfig {
        RecordingConfig {
            output_path: PathBuf::from("/tmp/apexshot-test.mp4"),
            width: Some(2560),
            height: Some(1440),
            x: Some(120),
            y: Some(80),
            cursor: true,
            hidpi: false,
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

    fn profile_by_encoder(encoder: &str) -> &'static EncoderProfile {
        PROFILES
            .iter()
            .find(|profile| profile.encoder == encoder)
            .expect("expected encoder profile to exist")
    }

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
            clicks: true,
            keystrokes: false,
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
            ..RecordingRequest::default()
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
        assert_eq!(prepared.updated_app_config.rec_cursor, false);
        assert_eq!(prepared.updated_app_config.rec_clicks, true);
        assert_eq!(prepared.updated_app_config.rec_keystrokes, false);
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
        assert_eq!(prepared.recording_config.output_path, prepared.output_path);
        assert_eq!(prepared.recording_config.width, Some(640));
        assert_eq!(prepared.recording_config.height, Some(480));
        assert_eq!(prepared.recording_config.x, Some(10));
        assert_eq!(prepared.recording_config.y, Some(20));
        assert_eq!(prepared.recording_config.cursor, false);
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
            })
        );
        assert_eq!(prepared.use_shell_mask, false);
        assert_eq!(
            prepared.use_shell_controls,
            crate::gnome_shell::current_session_supports_gnome_shell_overlay()
        );
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
            })
        );
    }

    #[test]
    fn video_encoder_props_uses_quality_focused_x264_settings() {
        let config = RecordingConfig {
            fps: 60,
            ..x11_recording_config()
        };

        let props = video_encoder_props(profile_by_encoder("x264enc"), &config);

        assert!(props.contains("speed-preset=medium"));
        assert!(props.contains("quantizer=14"));
        assert!(props.contains("bitrate=8000"));
        assert!(props.contains("key-int-max=120"));
        assert!(props.contains("rc-lookahead=20"));
        assert!(props.contains("ref=4"));
        assert!(props.contains("subme=6"));
        assert!(props.contains("psy-tune=ssim"));
        assert!(!props.contains("tune=zerolatency"));
        assert!(!props.contains("speed-preset=fast "));
        assert!(!props.contains("speed-preset=ultrafast"));
    }

    #[test]
    fn video_encoder_props_uses_quality_focused_webm_settings() {
        let config = RecordingConfig {
            fps: 60,
            ..x11_recording_config()
        };

        let vp9_props = video_encoder_props(profile_by_encoder("vp9enc"), &config);
        assert!(vp9_props.contains("end-usage=cq"));
        assert!(vp9_props.contains("cq-level=10"));
        assert!(vp9_props.contains("target-bitrate=0"));
        assert!(vp9_props.contains("bits-per-pixel=0.12"));
        assert!(vp9_props.contains("cpu-used=4"));
        assert!(vp9_props.contains("keyframe-max-dist=120"));
        assert!(vp9_props.contains("deadline=10000"));

        let vp8_props = video_encoder_props(profile_by_encoder("vp8enc"), &config);
        assert!(vp8_props.contains("end-usage=cq"));
        assert!(vp8_props.contains("target-bitrate=0"));
        assert!(vp8_props.contains("bits-per-pixel=0.12"));
        assert!(vp8_props.contains("cpu-used=4"));
        assert!(vp8_props.contains("keyframe-max-dist=120"));
        assert!(vp8_props.contains("deadline=8"));

        assert_eq!(
            video_encoder_props(profile_by_encoder("openh264enc"), &config),
            ""
        );
    }

    #[test]
    fn select_encoder_prefers_vp9_for_webm_when_available() {
        gst::init().expect("gstreamer should initialize for encoder selection");

        let (profile, path) = select_encoder(Path::new("/tmp/out.webm"))
            .expect("webm encoder selection should succeed");

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("webm"));

        if gst::ElementFactory::find("vp9enc").is_some()
            && gst::ElementFactory::find("webmmux").is_some()
        {
            assert_eq!(profile.encoder, "vp9enc");
        } else if gst::ElementFactory::find("vp8enc").is_some()
            && gst::ElementFactory::find("webmmux").is_some()
        {
            assert_eq!(profile.encoder, "vp8enc");
        } else {
            panic!("expected either vp9enc or vp8enc to be available for webm");
        }
    }

    #[tokio::test]
    async fn build_pipeline_includes_requested_frame_rate_and_quality_encoder_props() {
        std::env::remove_var("WAYLAND_DISPLAY");
        let config = RecordingConfig {
            fps: 60,
            ..x11_recording_config()
        };

        let built = build_pipeline(
            &config,
            profile_by_encoder("x264enc"),
            Path::new("/tmp/out.mp4"),
        )
        .await
        .expect("pipeline should build");

        assert!(built
            .pipeline_str
            .contains("videorate ! video/x-raw,framerate=60/1"));
        assert!(built.pipeline_str.contains("x264enc"));
        assert!(built.pipeline_str.contains("quantizer=14"));
        assert!(built.pipeline_str.contains("bitrate=8000"));
        assert!(built.pipeline_str.contains("speed-preset=medium"));
        assert!(built.pipeline_str.contains("rc-lookahead=20"));
        assert!(built.pipeline_str.contains("ref=4"));
        assert!(built.pipeline_str.contains("subme=6"));
        assert!(built.pipeline_str.contains("psy-tune=ssim"));
        assert!(built
            .pipeline_str
            .contains("video/x-raw,framerate=60/1,format=I420"));
        assert!(built
            .pipeline_str
            .contains("queue max-size-time=5000000000 max-size-bytes=0 max-size-buffers=0"));
        assert!(built.pipeline_str.contains("video/x-h264,profile=high"));
        assert!(!built.pipeline_str.contains("tune=zerolatency"));
        assert!(!built.pipeline_str.contains("speed-preset=ultrafast"));
    }

    #[tokio::test]
    async fn build_pipeline_only_downscales_when_max_resolution_requires_it() {
        std::env::remove_var("WAYLAND_DISPLAY");

        let no_downscale = RecordingConfig {
            max_resolution: Some((1920, 1080)),
            width: Some(1280),
            height: Some(720),
            ..x11_recording_config()
        };
        let no_downscale_pipeline = build_pipeline(
            &no_downscale,
            profile_by_encoder("vp9enc"),
            Path::new("/tmp/no-downscale.webm"),
        )
        .await
        .expect("pipeline should build");
        assert!(!no_downscale_pipeline
            .pipeline_str
            .contains("video/x-raw,width=1920,height=1080"));

        let downscale = RecordingConfig {
            max_resolution: Some((1280, 720)),
            ..x11_recording_config()
        };
        let downscale_pipeline = build_pipeline(
            &downscale,
            profile_by_encoder("vp9enc"),
            Path::new("/tmp/downscale.webm"),
        )
        .await
        .expect("pipeline should build");
        assert!(downscale_pipeline
            .pipeline_str
            .contains("! videoscale ! video/x-raw,width=1280,height=720"));
    }

    #[tokio::test]
    async fn build_pipeline_keeps_hidpi_and_audio_settings_authoritative() {
        std::env::remove_var("WAYLAND_DISPLAY");
        gst::init().expect("gstreamer should initialize for pipeline inspection");

        let config = RecordingConfig {
            hidpi: true,
            mic_enabled: true,
            speaker_enabled: true,
            mono_audio: true,
            mic_source: Some("mic.test".into()),
            speaker_source: Some("speaker.test.monitor".into()),
            ..x11_recording_config()
        };

        let built = build_pipeline(
            &config,
            profile_by_encoder("vp9enc"),
            Path::new("/tmp/audio.webm"),
        )
        .await
        .expect("pipeline should build");

        assert!(built.pipeline_str.contains("videoconvert ! videoscale"));

        if gst::ElementFactory::find("pulsesrc").is_some()
            && select_audio_encoder(PROFILES[0].muxer).is_some()
        {
            assert!(built.pipeline_str.contains("pulsesrc device=\"mic.test\""));
            assert!(built
                .pipeline_str
                .contains("pulsesrc device=\"speaker.test.monitor\""));
            assert!(built.pipeline_str.contains("audio/x-raw,channels=1"));
        } else {
            assert!(!built.pipeline_str.contains("pulsesrc device="));
        }
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
            clicks: false,
            keystrokes: true,
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
            ..RecordingRequest::default()
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
        assert_eq!(
            prepared.use_shell_controls,
            crate::gnome_shell::current_session_supports_gnome_shell_overlay()
        );
    }

    #[test]
    fn prepare_overlay_recording_request_maps_runtime_overlay_snapshot() {
        let request = RecordingRequest {
            x: 42,
            y: 24,
            width: 1280,
            height: 720,
            record_type: RecordingType::Video,
            controls: true,
            mic: true,
            speaker: true,
            clicks: true,
            keystrokes: true,
            webcam: true,
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
            click_size: 0.45,
            click_color: 3,
            click_style: 2,
            click_animate: false,
            key_size: 0.5,
            key_position: 2,
            key_appearance: 1,
            key_blur_bg: false,
            key_filter: 4,
            display_rec_time: true,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
            video_format: 0,
            video_max_res: 1,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 30,
            gif_quality: 0.8,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: true,
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 26, 9, 15, 0).unwrap(),
        );

        assert_eq!(prepared.updated_app_config.rec_mic, true);
        assert_eq!(prepared.updated_app_config.rec_speaker, true);
        assert_eq!(prepared.updated_app_config.rec_webcam_enabled, true);
        assert_eq!(prepared.updated_app_config.rec_webcam_rel_x, 0.61);
        assert_eq!(prepared.updated_app_config.rec_webcam_rel_y, 0.17);
        assert_eq!(prepared.updated_app_config.rec_webcam_size, 2);
        assert_eq!(prepared.updated_app_config.rec_webcam_shape, 1);
        assert_eq!(prepared.updated_app_config.rec_webcam_flip, true);
        assert_eq!(prepared.updated_app_config.rec_webcam_device, 7);
        assert_eq!(prepared.updated_app_config.rec_click_size, 0.45);
        assert_eq!(prepared.updated_app_config.rec_click_color, 3);
        assert_eq!(prepared.updated_app_config.rec_click_style, 2);
        assert_eq!(prepared.updated_app_config.rec_click_animate, false);
        assert_eq!(prepared.updated_app_config.rec_key_size, 0.5);
        assert_eq!(prepared.updated_app_config.rec_key_position, 2);
        assert_eq!(prepared.updated_app_config.rec_key_appearance, 1);
        assert_eq!(prepared.updated_app_config.rec_key_blur_bg, false);
        assert_eq!(prepared.updated_app_config.rec_key_filter, 4);
        let shell_supported = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
        assert_eq!(
            prepared.runtime_overlay_snapshot.is_some(),
            shell_supported
        );
        if shell_supported {
            let snap = prepared.runtime_overlay_snapshot.unwrap();
            assert_eq!(snap.mic_visible, true);
            assert_eq!(snap.speaker_visible, true);
            assert_eq!(snap.webcam_enabled, true);
            assert_eq!(snap.webcam_rel_x, 0.61);
            assert_eq!(snap.webcam_rel_y, 0.17);
            assert_eq!(snap.webcam_size, 2);
            assert_eq!(snap.webcam_shape, 1);
            assert_eq!(snap.webcam_flip, true);
            assert_eq!(snap.webcam_device, 7);
            assert_eq!(snap.clicks_enabled, true);
            assert_eq!(snap.click_size, 0.45);
            assert_eq!(snap.click_color, 3);
            assert_eq!(snap.click_style, 2);
            assert_eq!(snap.click_animate, false);
            assert_eq!(snap.keystrokes_enabled, true);
            assert_eq!(snap.keystrokes_supported, false);
            assert_eq!(snap.key_size, 0.5);
            assert_eq!(snap.key_position, 2);
            assert_eq!(snap.key_appearance, 1);
            assert_eq!(snap.key_blur_bg, false);
            assert_eq!(snap.key_filter, 4);
        }
    }

    #[test]
    fn prepare_overlay_recording_request_tracks_runtime_snapshot_based_on_shell_support() {
        let request = RecordingRequest {
            x: 42,
            y: 24,
            width: 1280,
            height: 720,
            record_type: RecordingType::Video,
            controls: false,
            mic: true,
            speaker: true,
            clicks: true,
            keystrokes: true,
            webcam: true,
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
            click_size: 0.45,
            click_color: 3,
            click_style: 2,
            click_animate: false,
            key_size: 0.5,
            key_position: 2,
            key_appearance: 1,
            key_blur_bg: false,
            key_filter: 4,
            display_rec_time: true,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
            video_format: 0,
            video_max_res: 1,
            video_fps: 1,
            record_mono: false,
            open_editor: false,
            gif_fps: 30,
            gif_quality: 0.8,
            gif_size_idx: 0,
            optimize_gif: true,
            fullscreen: true,
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 27, 9, 15, 0).unwrap(),
        );

        let shell_supported = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
        assert_eq!(prepared.use_shell_controls, shell_supported);
        assert!(prepared.controls_params.is_some());
        assert_eq!(prepared.runtime_overlay_snapshot.is_some(), shell_supported);
    }

    #[test]
    fn compute_wayland_crop_within_selected_monitor() {
        let crop = compute_wayland_crop((1920, 0), (2560, 1440), (2100, 200, 600, 744))
            .expect("crop should be valid");

        assert_eq!(
            crop,
            CropMargins {
                left: 180,
                right: 1780,
                top: 200,
                bottom: 496,
            }
        );
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
            clicks: false,
            keystrokes: false,
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
            })
        );
        assert_eq!(prepared.use_shell_mask, shell_supported);
        assert_eq!(prepared.use_shell_controls, shell_supported);
        assert_eq!(
            prepared.shell_controls_visibility_policy,
            shell_supported.then_some(crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden)
        );
    }

    #[test]
    fn prepare_overlay_recording_request_uses_shell_controls_for_gnome_wayland_fullscreen_recording(
    ) {
        let request = RecordingRequest {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            clicks: false,
            keystrokes: false,
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

        let shell_supported = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
        assert_eq!(prepared.use_shell_mask, false); // fullscreen never uses mask
        assert_eq!(prepared.use_shell_controls, shell_supported);
        assert_eq!(
            prepared.shell_controls_visibility_policy,
            shell_supported.then_some(crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden)
        );
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
            })
        );
    }

    #[test]
    fn legacy_pre_record_dim_disabled_when_shell_mask_is_active() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            clicks: false,
            keystrokes: false,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
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

        assert!(!should_use_legacy_pre_record_dim(&request, true));
    }

    #[test]
    fn legacy_pre_record_dim_enabled_without_shell_mask() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            clicks: false,
            keystrokes: false,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: true,
            countdown: true,
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

        assert!(should_use_legacy_pre_record_dim(&request, false));
    }

    #[test]
    fn shell_controls_follow_gnome_wayland_support() {
        let request = RecordingRequest {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
            record_type: RecordingType::Video,
            controls: true,
            mic: false,
            speaker: false,
            clicks: false,
            keystrokes: false,
            display_rec_time: false,
            hidpi: false,
            notifications: true,
            cursor: true,
            remember_selection: false,
            dim_screen: false,
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

        assert!(should_use_shell_controls_for_request(&request, true));
        assert!(!should_use_shell_controls_for_request(&request, false));
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

    #[test]
    fn compute_wayland_crop_rejects_selection_outside_monitor() {
        let err = compute_wayland_crop((1920, 0), (2560, 1440), (1800, 100, 400, 300))
            .expect_err("selection should be rejected");

        assert!(err.contains("outside the selected monitor"));
    }
}
