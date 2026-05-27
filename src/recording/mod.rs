use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use serde::Serialize;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Stdio;
// No atomic imports needed here anymore
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    capture_overlay::{RecordingRequest, RecordingType},
    config::{save_config, AppConfig},
};

mod control_session;
pub mod editor;
mod stop_overlay;
use control_session::RecordingControlServer;
pub use control_session::{
    has_active_recording_control, send_active_recording_command, toggle_active_recording_pause,
    RecordingControlCommand,
};
pub use stop_overlay::{
    run_recording_controls, run_recording_countdown_bar, run_recording_stop_overlay,
    run_recording_ui, RecordingControlsParams, StopAction, StopOverlayError,
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

fn notify_recording_session_ended_best_effort() {
    crate::gnome_shell::hide_recording_mask_best_effort();
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
    pub open_editor: bool,
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
    node_id: u32,
    pipewire_fd: OwnedFd,
    stream_width: u32,
    stream_height: u32,
    #[allow(dead_code)]
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
    wayland_source: Option<WaylandSource>,
    encoder_name: String,
    encoder_props: String,
    final_path: PathBuf,
    config: RecordingConfig,
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

#[allow(dead_code)]
fn pipewire_source_pipeline(node_id: u32, fd: i32) -> String {
    let copy_buffers = gst::ElementFactory::make("pipewiresrc")
        .build()
        .map(|element| element.has_property("always-copy"))
        .unwrap_or(false);
    let copy_buffers_prop = if copy_buffers {
        " always-copy=true"
    } else {
        ""
    };

    format!("pipewiresrc fd={fd} path={node_id} do-timestamp=true{copy_buffers_prop}")
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

struct EncoderProfile {
    name: &'static str,
    encoder: &'static str,        // GStreamer element name (used by X11 path)
    ffmpeg_encoder: &'static str, // ffmpeg -c:v name (used by Wayland path)
    muxer: &'static str,
    extension: &'static str,
}

// Priority list of encoders
const PROFILES: &[EncoderProfile] = &[
    // VP9 (WebM)
    EncoderProfile {
        name: "VP9",
        encoder: "vp9enc",
        ffmpeg_encoder: "libvpx-vp9",
        muxer: "webmmux",
        extension: "webm",
    },
    // VP8 (WebM) - fallback when VP9 is unavailable
    EncoderProfile {
        name: "VP8",
        encoder: "vp8enc",
        ffmpeg_encoder: "libvpx",
        muxer: "webmmux",
        extension: "webm",
    },
    // Standard H.264
    EncoderProfile {
        name: "H.264 (x264)",
        encoder: "x264enc",
        ffmpeg_encoder: "libx264",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Cisco OpenH264
    EncoderProfile {
        name: "H.264 (OpenH264)",
        encoder: "openh264enc",
        ffmpeg_encoder: "libopenh264",
        muxer: "mp4mux",
        extension: "mp4",
    },
    // Theora (Ogg) - Last resort
    EncoderProfile {
        name: "Theora",
        encoder: "theoraenc",
        ffmpeg_encoder: "libtheora",
        muxer: "oggmux",
        extension: "ogv",
    },
];

fn is_wlroots_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_default()
        .to_lowercase();
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some();

    // Compositor-specific env vars (set by the compositor itself).
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var_os("SWAYSOCK").is_some()
    {
        return true;
    }

    // String match on desktop session id (works for labwc when used standalone,
    // but NOT when labwc is embedded inside XFCE/Wayland where the session
    // reports as "XFCE").
    if desktop.contains("hyprland")
        || desktop.contains("sway")
        || desktop.contains("river")
        || desktop.contains("wayfire")
        || desktop.contains("labwc")
        || desktop.contains("niri")
    {
        return true;
    }

    // labwc running under XFCE/Wayland: no unique env var, so detect by
    // checking for a running labwc process on the same Wayland display.
    if wayland_display && command_exists("labwc") {
        // pgrep -x matches the exact process name.
        if let Ok(output) = std::process::Command::new("pgrep")
            .args(["-x", "labwc"])
            .output()
        {
            if output.status.success() {
                return true;
            }
        }
    }

    false
}

fn command_exists(name: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn should_use_wf_recorder(config: &RecordingConfig) -> bool {
    is_wlroots_session() && config.output_path.extension().is_none_or(|e| e != "gif")
}

// ---------------------------------------------------------------------------
// Hardware encoder detection (VAAPI)
// ---------------------------------------------------------------------------

fn detect_vaapi_device() -> Option<String> {
    // Try the standard render node paths.
    for path in &["/dev/dri/renderD128", "/dev/dri/renderD129"] {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

fn should_use_vaapi() -> bool {
    if std::env::var_os("APEXSHOT_HW_ENCODER")
        .map(|v| v == "vaapi")
        .unwrap_or(false)
    {
        return detect_vaapi_device().is_some();
    }
    if let Ok(val) = std::env::var("APEXSHOT_HW_ENCODER") {
        return val == "vaapi" && detect_vaapi_device().is_some();
    }
    false
}

fn ffmpeg_vaapi_args(width: u32, height: u32) -> Vec<String> {
    let device = detect_vaapi_device().unwrap_or_else(|| "/dev/dri/renderD128".into());
    vec![
        "-vaapi_device".into(),
        device,
        "-vf".into(),
        format!("format=nv12,hwupload,scale_vaapi=w={width}:h={height}"),
        "-c:v".into(),
        "h264_vaapi".into(),
        "-qp".into(),
        "24".into(),
        "-profile".into(),
        "main".into(),
    ]
}

async fn record_with_wf_recorder(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    if !command_exists("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording requires wf-recorder. Install it with: sudo pacman -S wf-recorder"
                .into(),
        ));
    }

    let final_path = config.output_path.clone();
    let mut args: Vec<String> = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    }

    args.push("-r".into());
    args.push(config.fps.max(1).to_string());

    // wf-recorder records the cursor by default on current wlroots setups.
    // Older packaged versions do not recognize `--show-cursor`, and passing it
    // can make recording startup noisy or fail. There is no portable positive
    // "show cursor" flag, so only omit cursor customization here.

    if config.mic_enabled || config.speaker_enabled {
        args.push("-a".into());
        let source = if config.speaker_enabled && !config.mic_enabled {
            config
                .speaker_source
                .clone()
                .unwrap_or_else(get_pulse_speaker_monitor)
        } else {
            // wf-recorder accepts a single Pulse source. For mic-only use the
            // default mic. If both mic + speaker are requested, prefer the mic
            // here; the GStreamer backend can mix both, but wf-recorder cannot
            // portably mix two Pulse sources without an external filter graph.
            config
                .mic_source
                .clone()
                .unwrap_or_else(get_pulse_default_source)
        };
        if !source.is_empty() {
            args.push(source);
        }
    }

    args.push("-f".into());
    args.push(final_path.to_string_lossy().to_string());

    println!("Starting wlroots recording to: {:?}", final_path);
    println!("wf-recorder {}", args.join(" "));

    let mut child = tokio::process::Command::new("wf-recorder")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(RecordError::IoError)?;

    notify_daemon_event("recording_session_started");
    let mut command_rx = command_rx;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut paused = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RecordError::IoError)?;
                if !status.success() && stop_action == RecordingTerminalAction::Save {
                    return Err(RecordError::GStreamerError(format!("wf-recorder exited with {status}")));
                }
                break;
            }
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
                        if let Some(pid) = child.id() {
                            // Some wf-recorder builds treat SIGUSR1 as fatal (observed as
                            // exit by signal 10). Use SIGSTOP/SIGCONT for a compositor-agnostic
                            // process pause instead of crashing the recorder.
                            let _ = std::process::Command::new("kill").args(["-STOP", &pid.to_string()]).status();
                        }
                        paused = true;
                        notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        if let Some(pid) = child.id() {
                            let _ = std::process::Command::new("kill").args(["-CONT", &pid.to_string()]).status();
                        }
                        paused = false;
                        notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = RecordingTerminalAction::Restart;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = RecordingTerminalAction::Save;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = RecordingTerminalAction::Discard;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;

    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&final_path);
    }
    if let Some(event) = daemon_event_for_terminal_action(stop_action) {
        notify_daemon_event(event);
    }
    Ok((final_path, stop_action))
}

/// GIF recording on wlroots: record via wf-recorder to a temp MP4 file, then
/// convert to GIF with ffmpeg (palettegen + paletteuse).
async fn record_gif_with_wf_recorder(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::process::{Command, Stdio};

    if !command_exists("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(
            "wlroots GIF recording requires wf-recorder. Install it with: sudo pacman -S wf-recorder"
                .into(),
        ));
    }
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err(RecordError::NoEncoderFound);
    }

    let final_path = config.output_path.clone();

    // Build a temp .mp4 path in the same directory as the target GIF (or /tmp).
    let temp_dir = final_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("/tmp"));
    let temp_path = {
        let stem = final_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("apexshot-gif-temp");
        let mut p = temp_dir.join(format!("{}.temp.mp4", stem));
        // Avoid collisions if a previous temp file still exists.
        let mut counter = 1_u32;
        while p.exists() {
            p = temp_dir.join(format!("{}.temp-{}.mp4", stem, counter));
            counter += 1;
        }
        p
    };

    // ---- Phase 1: record video with wf-recorder ----
    let mut args: Vec<String> = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    }

    args.push("-r".into());
    args.push(config.fps.max(1).to_string());

    if config.mic_enabled || config.speaker_enabled {
        args.push("-a".into());
        let source = if config.speaker_enabled && !config.mic_enabled {
            config
                .speaker_source
                .clone()
                .unwrap_or_else(get_pulse_speaker_monitor)
        } else {
            config
                .mic_source
                .clone()
                .unwrap_or_else(get_pulse_default_source)
        };
        if !source.is_empty() {
            args.push(source);
        }
    }

    args.push("-f".into());
    args.push(temp_path.to_string_lossy().to_string());

    println!("Recording GIF via wf-recorder (temp: {:?})", temp_path);
    println!("wf-recorder {}", args.join(" "));

    let mut child = tokio::process::Command::new("wf-recorder")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(RecordError::IoError)?;

    notify_daemon_event("recording_session_started");
    let mut command_rx = command_rx;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut paused = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RecordError::IoError)?;
                if !status.success() && stop_action == RecordingTerminalAction::Save {
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(RecordError::GStreamerError(format!("wf-recorder exited with {status}")));
                }
                break;
            }
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
                        if let Some(pid) = child.id() {
                            let _ = std::process::Command::new("kill").args(["-STOP", &pid.to_string()]).status();
                        }
                        paused = true;
                        notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        if let Some(pid) = child.id() {
                            let _ = std::process::Command::new("kill").args(["-CONT", &pid.to_string()]).status();
                        }
                        paused = false;
                        notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = RecordingTerminalAction::Restart;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = RecordingTerminalAction::Save;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = RecordingTerminalAction::Discard;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;

    // ---- Handle stop actions ----
    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&temp_path);
        if let Some(event) = daemon_event_for_terminal_action(stop_action) {
            notify_daemon_event(event);
        }
        return Ok((final_path, stop_action));
    }

    if stop_action == RecordingTerminalAction::Restart {
        let _ = std::fs::remove_file(&temp_path);
        if let Some(event) = daemon_event_for_terminal_action(stop_action) {
            notify_daemon_event(event);
        }
        return Ok((final_path, stop_action));
    }

    // ---- Phase 2: convert MP4 to GIF with ffmpeg ----
    let max_colors = ((32.0 + 224.0 * config.gif_quality) as u32).clamp(32, 256);
    let dither = if config.gif_quality >= 0.5 {
        "floyd_steinberg"
    } else {
        "bayer:bayer_scale=5"
    };
    let stats_mode = if config.gif_optimize { "diff" } else { "full" };
    let scale_prefix = match config.gif_max_width {
        Some(w) => format!("scale={}:-2:flags=lanczos,", w),
        None => String::new(),
    };
    let vf_filter = format!(
        "{}fps={},split[s0][s1];[s0]palettegen=max_colors={}:stats_mode={}[p];[s1][p]paletteuse=dither={}",
        scale_prefix, config.fps, max_colors, stats_mode, dither
    );

    println!("Converting to GIF with ffmpeg...");
    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats")
        .arg("-i")
        .arg(&temp_path)
        .arg("-vf")
        .arg(&vf_filter)
        .arg(&final_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(RecordError::IoError)?;

    let _ = std::fs::remove_file(&temp_path);

    if !status.success() {
        let _ = std::fs::remove_file(&final_path);
        return Err(RecordError::GifError(format!(
            "FFmpeg GIF conversion failed with status: {status}"
        )));
    }

    if let Some(event) = daemon_event_for_terminal_action(stop_action) {
        notify_daemon_event(event);
    }

    println!("GIF saved to {:?}", final_path);
    Ok((final_path, stop_action))
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
    if is_wlroots_session() {
        if should_use_wf_recorder(&config) {
            return record_with_wf_recorder(config, command_rx).await;
        }
        // GIF on wlroots: record via wf-recorder to a temp file, then convert to GIF
        if config.output_path.extension().is_some_and(|e| e == "gif") {
            return record_gif_with_wf_recorder(config, command_rx).await;
        }
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording with this output format is not supported".into(),
        ));
    }

    // Check if GIF requested
    if config.output_path.extension().is_some_and(|e| e == "gif") {
        return record_gif_rust_with_commands(config, command_rx).await;
    }

    // Check ffmpeg availability (needed for encoding)
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        return Err(RecordError::NoEncoderFound);
    }

    // Select encoder based on output extension
    let (profile, final_path) = select_encoder(config.output_path.as_path())?;
    let effective_config = normalize_recording_config_for_profile(profile, &config);
    println!("Using Encoder: {} ({})", profile.name, profile.encoder);

    if final_path != config.output_path {
        println!(
            "Note: Output filename changed to match format: {:?}",
            final_path
        );
    }

    // Build pipeline (portal session + PipeWire fd for Wayland)
    let built = build_pipeline(&effective_config, profile, final_path.as_path()).await?;

    // For Wayland: use native PipeWire capture + ffmpeg pipe
    if let Some(wayland_source) = built.wayland_source {
        // Clone what we need before the move
        let final_path = built.final_path.clone();
        let encoder_name = built.encoder_name.clone();
        let encoder_props = built.encoder_props.clone();
        let config = built.config.clone();
        let result = tokio::task::spawn_blocking(move || {
            record_wayland_with_ffmpeg_sync(
                wayland_source,
                &final_path,
                &encoder_name,
                &encoder_props,
                &config,
                command_rx,
            )
        })
        .await
        .map_err(|e| RecordError::GStreamerError(format!("Join error: {e}")))?;
        return result;
    }

    // For X11: keep GStreamer ximagesrc path
    record_x11_with_gstreamer(&effective_config, profile, &final_path, command_rx).await
}

pub fn copy_to_clipboard(path: &Path) -> RecordResult<()> {
    println!("Copying to clipboard...");

    crate::utils::clipboard::copy_uri_to_clipboard(path).map_err(RecordError::GStreamerError)?;

    println!("Copied to clipboard!");
    Ok(())
}

fn select_encoder(
    requested_path: &std::path::Path,
) -> RecordResult<(&'static EncoderProfile, PathBuf)> {
    if let Some(ext) = requested_path.extension().and_then(|s| s.to_str()) {
        for profile in PROFILES {
            if profile.extension == ext {
                return Ok((profile, requested_path.to_path_buf()));
            }
        }
        println!(
            "Warning: Requested format '{}' not in profile list; using default.",
            ext
        );
    }

    // Default to VP9/WebM
    let profile = PROFILES.first().ok_or(RecordError::NoEncoderFound)?;
    let mut new_path = requested_path.to_path_buf();
    new_path.set_extension(profile.extension);
    Ok((profile, new_path))
}

/// Wayland recording: native PipeWire frame capture + ffmpeg pipe for encoding.
fn record_wayland_with_ffmpeg_sync(
    wayland_source: WaylandSource,
    final_path: &std::path::Path,
    encoder_name: &str,
    encoder_props: &str,
    config: &RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let final_path = final_path.to_path_buf();

    // Open PipeWire capture stream (continuous)
    let capture = crate::pipewire_engine::PipeWireCapture::connect(
        wayland_source.pipewire_fd,
        wayland_source.node_id,
        None, // continuous — no max frame limit
        config.width,
        config.height,
    )
    .map_err(|e| RecordError::GStreamerError(format!("PipeWire capture failed: {e}")))?;

    let format = capture.format().ok_or_else(|| {
        RecordError::GStreamerError("No format negotiated before recording".into())
    })?;

    // Raw frame dimensions sent into ffmpeg after our manual area crop. Video
    // settings such as max resolution are applied as ffmpeg filters, not here.
    let mut input_width = format.width;
    let mut input_height = format.height;
    if let Some(crop) = wayland_source.crop {
        input_width = input_width
            .checked_sub(crop.left + crop.right)
            .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop width".into()))?;
        input_height = input_height
            .checked_sub(crop.top + crop.bottom)
            .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop height".into()))?;
        eprintln!(
            "[recording] Applying Wayland area crop: left={} top={} right={} bottom={} => {}x{}",
            crop.left, crop.top, crop.right, crop.bottom, input_width, input_height
        );
    }
    let fps = config.fps.max(1);

    // Build ffmpeg command
    let use_vaapi = should_use_vaapi();
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats");

    if use_vaapi {
        let (vaapi_width, vaapi_height) =
            fit_within_max_resolution(input_width, input_height, config.max_resolution);
        let vaapi_args = ffmpeg_vaapi_args(vaapi_width, vaapi_height);
        for arg in &vaapi_args {
            ffmpeg_cmd.arg(arg);
        }
    }

    ffmpeg_cmd
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-s")
        .arg(format!("{}x{}", input_width, input_height))
        .arg("-r")
        .arg(fps.to_string())
        .arg("-i")
        .arg("pipe:0");

    if !use_vaapi {
        // Convert desktop RGBA (full-range RGB) to standard limited-range
        // YUV420P for broad MP4/player compatibility. Tagging H.264 as full
        // range can make some Linux players display lifted blacks / a washed
        // layer, so use normal video range while preserving correct RGB input.
        let filter = wayland_video_filter(config.max_resolution);
        ffmpeg_cmd
            .arg("-vf")
            .arg(filter)
            .arg("-color_range")
            .arg("tv")
            .arg("-colorspace")
            .arg("bt709")
            .arg("-color_primaries")
            .arg("bt709")
            .arg("-color_trc")
            .arg("iec61966-2-1");
        ffmpeg_cmd.arg("-c:v").arg(encoder_name);
        // Sane defaults for screen recording.
        if encoder_name == "libx264" {
            ffmpeg_cmd.arg("-preset").arg("veryfast");
            ffmpeg_cmd.arg("-crf").arg("23");
        }
        if !encoder_props.is_empty() {
            for prop in encoder_props.split_whitespace() {
                if let Some((key, val)) = prop.split_once('=') {
                    ffmpeg_cmd.arg(format!("-{key}")).arg(val);
                }
            }
        }
    }

    // Add audio inputs when mic/speaker are enabled.
    // ffmpeg captures from PulseAudio directly with -f pulse. On modern GNOME
    // this is normally provided by pipewire-pulse, so start it if the user
    // session has not already activated it.
    if config.mic_enabled || config.speaker_enabled {
        ensure_pipewire_pulse_running();

        if config.mic_enabled {
            let mic_dev = config
                .mic_source
                .clone()
                .unwrap_or_else(get_pulse_default_source);
            eprintln!("[recording] Audio: mic device={mic_dev}");
            ffmpeg_cmd.arg("-f").arg("pulse");
            ffmpeg_cmd.arg("-i").arg(&mic_dev);
        }

        if config.speaker_enabled {
            let spk_dev = config
                .speaker_source
                .clone()
                .unwrap_or_else(get_pulse_speaker_monitor);
            eprintln!("[recording] Audio: speaker monitor={spk_dev}");
            ffmpeg_cmd.arg("-f").arg("pulse");
            ffmpeg_cmd.arg("-i").arg(&spk_dev);
        }

        // Mix multiple audio streams if both enabled.
        if config.mic_enabled && config.speaker_enabled {
            ffmpeg_cmd.arg("-filter_complex");
            ffmpeg_cmd.arg("[1:a][2:a]amix=inputs=2:duration=first[aout]");
            ffmpeg_cmd.arg("-map").arg("0:v");
            ffmpeg_cmd.arg("-map").arg("[aout]");
        } else {
            ffmpeg_cmd.arg("-map").arg("0:v");
            ffmpeg_cmd.arg("-map").arg("1:a");
        }

        if config.mono_audio {
            ffmpeg_cmd.arg("-ac").arg("1");
        }
    }

    ffmpeg_cmd.arg(&final_path);
    ffmpeg_cmd.stdin(Stdio::piped());
    ffmpeg_cmd.stdout(Stdio::null());
    ffmpeg_cmd.stderr(Stdio::inherit());

    let mut child = ffmpeg_cmd
        .spawn()
        .map_err(|e| RecordError::GStreamerError(format!("Failed to spawn ffmpeg: {e}")))?;

    let mut stdin = child.stdin.take().expect("stdin should be piped");

    println!("Recording (native PipeWire + ffmpeg) to {:?}", final_path);

    // Recording loop
    let mut command_rx = command_rx;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut frames_written = 0u64;
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_frame_at: Option<std::time::Instant> = None;
    let mut paused = false;

    loop {
        // Check for control commands
        let command = match &mut command_rx {
            Some(rx) => match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
                Err(_) => {
                    command_rx = None;
                    None
                }
            },
            None => None,
        };

        if let Some(command) = command {
            match command {
                RecordingControlCommand::Restart => {
                    stop_action = RecordingTerminalAction::Restart;
                    break;
                }
                RecordingControlCommand::StopSave => {
                    println!("\nStopping recording...");
                    break;
                }
                RecordingControlCommand::StopDiscard => {
                    stop_action = RecordingTerminalAction::Discard;
                    println!("\nDiscarding recording...");
                    break;
                }
                RecordingControlCommand::Pause if !paused => {
                    println!("Recording paused");
                    paused = true;
                }
                RecordingControlCommand::Resume if paused => {
                    println!("Recording resumed");
                    paused = false;
                    next_frame_at = None; // don't skip the first frame
                }
                _ => {}
            }
        }

        // While paused, spin briefly and check for commands instead
        // of capturing frames.
        if paused {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }

        // Try to get a frame from PipeWire
        let frame = match capture.try_recv_frame() {
            Ok(Some(f)) => f,
            Ok(None) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            Err(e) => {
                eprintln!("PipeWire frame error: {e}");
                break;
            }
        };

        let now = std::time::Instant::now();
        if let Some(deadline) = next_frame_at {
            if now < deadline {
                continue;
            }
        }
        next_frame_at = Some(now + frame_interval);

        let pixels = if let Some(crop) = wayland_source.crop {
            crop_rgba_frame(&frame, crop)?
        } else {
            frame.pixels
        };

        // Write frame to ffmpeg stdin
        if let Err(e) = stdin.write_all(&pixels) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                eprintln!("ffmpeg pipe broken (likely exited)");
            } else {
                eprintln!("Failed to write to ffmpeg: {e}");
            }
            break;
        }

        frames_written += 1;
        if frames_written == 1 {
            eprintln!(
                "[recording] First frame written to ffmpeg ({} bytes)",
                pixels.len()
            );
        }
        if frames_written.is_multiple_of(30) {
            eprintln!("[recording] {} frames written", frames_written);
        }
    }

    // Close stdin to signal ffmpeg EOF
    drop(stdin);

    // Wait for ffmpeg to finish
    let status = child
        .wait()
        .map_err(|e| RecordError::GStreamerError(format!("Failed to wait for ffmpeg: {e}")))?;

    if !status.success() && stop_action == RecordingTerminalAction::Save {
        eprintln!("ffmpeg exited with non-zero status: {status}");
    }

    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&final_path);
    }

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

fn fit_within_max_resolution(
    width: u32,
    height: u32,
    max_resolution: Option<(u32, u32)>,
) -> (u32, u32) {
    let Some((max_w, max_h)) = max_resolution else {
        return (width, height);
    };
    if width <= max_w && height <= max_h {
        return (width, height);
    }

    let scale = (max_w as f64 / width as f64).min(max_h as f64 / height as f64);
    let mut out_w = (width as f64 * scale).round().max(2.0) as u32;
    let mut out_h = (height as f64 * scale).round().max(2.0) as u32;
    out_w -= out_w % 2;
    out_h -= out_h % 2;
    (out_w.max(2), out_h.max(2))
}

fn wayland_video_filter(max_resolution: Option<(u32, u32)>) -> String {
    let scale = if let Some((max_w, max_h)) = max_resolution {
        format!(
            "scale=w='min(iw,{max_w})':h='min(ih,{max_h})':force_original_aspect_ratio=decrease:force_divisible_by=2:in_range=pc:out_range=tv"
        )
    } else {
        // Keep original size, but make dimensions encoder-safe for yuv420p.
        "scale=w='trunc(iw/2)*2':h='trunc(ih/2)*2':in_range=pc:out_range=tv".to_string()
    };
    format!("{scale},format=yuv420p")
}

fn crop_rgba_frame(
    frame: &crate::pipewire_engine::PipeWireFrame,
    crop: CropMargins,
) -> RecordResult<Vec<u8>> {
    let out_width = frame
        .width
        .checked_sub(crop.left + crop.right)
        .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop width".into()))?;
    let out_height = frame
        .height
        .checked_sub(crop.top + crop.bottom)
        .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop height".into()))?;

    let src_stride = frame.stride as usize;
    let row_bytes = out_width as usize * 4;
    let start_x = crop.left as usize * 4;
    let start_y = crop.top as usize;
    let mut cropped = Vec::with_capacity(row_bytes * out_height as usize);

    for y in 0..out_height as usize {
        let src_start = (start_y + y) * src_stride + start_x;
        let src_end = src_start + row_bytes;
        let row = frame.pixels.get(src_start..src_end).ok_or_else(|| {
            RecordError::GStreamerError("Wayland crop exceeded frame bounds".into())
        })?;
        cropped.extend_from_slice(row);
    }

    Ok(cropped)
}

/// X11 fallback recording using GStreamer ximagesrc.
/// Preserved from the previous implementation for backward compatibility.
#[allow(unused_assignments)]
async fn record_x11_with_gstreamer(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    final_path: &std::path::Path,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    gst::init().map_err(|e| RecordError::InitError(e.to_string()))?;

    let pipeline_str = build_x11_gstreamer_pipeline(config, profile, final_path)?;
    println!("Starting recording (GStreamer X11) to: {:?}", final_path);
    println!("Pipeline: {}", pipeline_str);

    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to parse pipeline: {}", e)))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| RecordError::GStreamerError("Cast to Pipeline failed".into()))?;

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to start pipeline: {}", e)))?;

    let bus = pipeline
        .bus()
        .ok_or_else(|| RecordError::GStreamerError("Pipeline has no bus".into()))?;

    let mut command_rx = command_rx;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut stopping = false;

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
                    RecordingControlCommand::Restart => {
                        stop_action = RecordingTerminalAction::Restart;
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = RecordingTerminalAction::Save;
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = RecordingTerminalAction::Discard;
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                for msg in bus.iter_timed(gst::ClockTime::ZERO) {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Eos(..) => { stopping = true; break; }
                        MessageView::Error(err) => {
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

    pipeline
        .set_state(gst::State::Null)
        .map_err(|e| RecordError::GStreamerError(format!("Cleanup failed: {}", e)))?;

    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(final_path);
    }

    Ok((final_path.to_path_buf(), stop_action))
}

/// Build a GStreamer pipeline string for X11 capture (preserved from old code).
fn build_x11_gstreamer_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<String> {
    let output_str = output_path.to_string_lossy();
    let video_source = get_x11_source(config)?;
    let video_raw_caps = format!("video/x-raw,framerate={}/1", config.fps);

    Ok(format!(
        "{} ! videoconvert ! {}videorate ! {} ! {} ! {} ! filesink location=\"{}\"",
        video_source, video_raw_caps, "queue", profile.encoder, profile.muxer, output_str
    ))
}

fn ensure_pipewire_pulse_running() {
    if !command_exists("systemctl") {
        return;
    }

    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "pipewire-pulse.service"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if active {
        return;
    }

    eprintln!("[recording] pipewire-pulse is not active; attempting to start it for audio capture");
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", "pipewire-pulse.service"])
        .status();
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

/// List all PulseAudio/PipeWire input sources (microphones).
pub fn list_audio_inputs() -> Vec<(String, String)> {
    // name, description
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    output
        .lines()
        .filter(|line| !line.contains(".monitor")) // exclude monitor sources
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let desc = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                // Filter out "auto_null" and other virtual sources
                if !name.contains("auto_null") && !desc.is_empty() {
                    Some((name, desc))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// List all PulseAudio/PipeWire monitor sources (speaker output capture).
pub fn list_audio_outputs() -> Vec<(String, String)> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    output
        .lines()
        .filter(|line| line.contains(".monitor"))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let desc = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                Some((name, desc))
            } else {
                None
            }
        })
        .collect()
}

#[allow(dead_code)]
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

#[cfg(test)]
fn video_encoder_props(profile: &EncoderProfile, config: &RecordingConfig) -> String {
    let key_int_max = config.fps.saturating_mul(2).max(1);

    // Presets informed by OBS's obs-ffmpeg-video-encoders.c, adapted for
    // file recording (prioritize quality over streaming latency).

    if profile.encoder == "x264enc" {
        // OBS default: veryfast, CRF 23, main profile.
        // For screen recording we bump quality slightly but keep the fast preset.
        return format!("preset=veryfast crf=22 profile=main key-int-max={key_int_max}",);
    }

    if profile.encoder == "vp9enc" {
        // OBS default: CQ 30, deadline good, cpu-used 0.
        // For local recording we use slightly higher quality.
        return format!(
            "deadline=good end-usage=cq cq-level=20 target-bitrate=0 cpu-used=2 row-mt=true threads=8 keyframe-max-dist={key_int_max} lag-in-frames=0",
        );
    }

    if profile.encoder == "vp8enc" {
        return format!(
            "deadline=good end-usage=cq cq-level=10 target-bitrate=0 cpu-used=2 threads=8 keyframe-max-dist={key_int_max} lag-in-frames=0",
        );
    }

    if profile.encoder == "openh264enc" {
        return "bitrate=8000000 complexity=medium".to_string();
    }

    String::new()
}

#[allow(dead_code)]
fn video_raw_caps(profile: &EncoderProfile, config: &RecordingConfig) -> String {
    if matches!(profile.encoder, "x264enc" | "openh264enc") {
        return format!("video/x-raw,framerate={}/1,format=I420", config.fps);
    }

    format!("video/x-raw,framerate={}/1", config.fps)
}

fn normalize_recording_config_for_profile(
    profile: &EncoderProfile,
    config: &RecordingConfig,
) -> RecordingConfig {
    let mut normalized = config.clone();

    if profile.encoder != "x264enc" {
        return normalized;
    }

    if let Some(width) = normalized.width {
        if width > 1 && width % 2 != 0 {
            normalized.width = Some(width - 1);
        }
    }

    if let Some(height) = normalized.height {
        if height > 1 && height % 2 != 0 {
            normalized.height = Some(height - 1);
        }
    }

    normalized
}

#[allow(dead_code)]
fn video_queue_props(profile: &EncoderProfile) -> &'static str {
    if profile.encoder == "x264enc" {
        // x264enc can accumulate latency; use a larger queue budget than the
        // default live-pipeline settings to avoid visible stutter under motion.
        "queue max-size-time=5000000000 max-size-bytes=0 max-size-buffers=0"
    } else {
        "queue"
    }
}

#[allow(dead_code)]
fn video_post_encoder_caps(profile: &EncoderProfile) -> &'static str {
    if profile.encoder == "x264enc" {
        " ! h264parse config-interval=-1 ! video/x-h264,stream-format=avc,alignment=au,profile=high"
    } else if profile.encoder == "openh264enc" {
        " ! h264parse config-interval=-1 ! video/x-h264,stream-format=avc,alignment=au"
    } else {
        ""
    }
}

async fn build_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<BuiltPipeline> {
    // Get video source (Portal session + PipeWire fd for Wayland)
    let wayland_source = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        Some(get_wayland_source(config).await?)
    } else {
        None
    };

    // Encoder props are GStreamer-specific; ffmpeg has its own defaults.
    // Only used by the X11 GStreamer fallback path.
    let encoder_props = String::new();

    Ok(BuiltPipeline {
        wayland_source,
        encoder_name: profile.ffmpeg_encoder.to_string(),
        encoder_props,
        final_path: output_path.to_path_buf(),
        config: config.clone(),
    })
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
        // Ask the portal/compositor to embed the cursor in the video stream.
        // Metadata mode is optional and GNOME often does not provide usable
        // cursor bitmap metadata for ScreenCast streams, which made the
        // "show cursor" setting appear ignored in PipeWire recordings.
        CursorMode::Embedded
    } else {
        CursorMode::Hidden
    };

    async fn request_screencast(
        cursor_mode: CursorMode,
        wants_area_crop: bool,
        restore_token: Option<&str>,
        persist_mode: PersistMode,
    ) -> RecordResult<(
        ashpd::desktop::screencast::Streams,
        RecordingPortalSession,
        OwnedFd,
    )> {
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
            SourceType::Monitor | SourceType::Window
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

        let pipewire_fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .map_err(|e| RecordError::PortalError(e.to_string()))?;

        Ok((response, session, pipewire_fd))
    }

    let (response, session, pipewire_fd) = if let Some(token) = load_recording_restore_token(target)
    {
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

    let (stream_width, stream_height) = stream
        .size()
        .map(|(w, h)| (w as u32, h as u32))
        .unwrap_or((0, 0));

    let crop = if wants_area_crop {
        let position = stream.position().ok_or_else(|| {
            RecordError::PortalError(
                "The selected Wayland stream did not expose monitor position metadata".into(),
            )
        })?;
        let size = (stream_width as i32, stream_height as i32);
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
        node_id,
        pipewire_fd,
        stream_width,
        stream_height,
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
    use std::process::Command;

    println!("Starting GIF recording (via FFmpeg Pipe)...");

    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err(RecordError::NoEncoderFound);
    }

    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return record_gif_wayland_native(config, command_rx).await;
    }

    // X11: keep GStreamer pipeline
    record_gif_x11_gstreamer(config, command_rx).await
}

/// GIF recording on Wayland using native PipeWire frame capture + ffmpeg.
async fn record_gif_wayland_native(
    config: RecordingConfig,
    mut command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let source = get_wayland_source(&config).await?;

    // Use stream dimensions from the portal metadata to determine the
    // frame size going into ffmpeg (after applying area crop when present).
    let (raw_width, raw_height) = if let Some(ref c) = source.crop {
        let w = source
            .stream_width
            .checked_sub(c.left + c.right)
            .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop width".into()))?;
        let h = source
            .stream_height
            .checked_sub(c.top + c.bottom)
            .ok_or_else(|| RecordError::GStreamerError("Invalid Wayland crop height".into()))?;
        eprintln!(
            "[recording] GIF wayland area crop: left={} top={} right={} bottom={} => {}x{}",
            c.left, c.top, c.right, c.bottom, w, h
        );
        (w, h)
    } else {
        (source.stream_width, source.stream_height)
    };

    if raw_width == 0 || raw_height == 0 {
        return Err(RecordError::GStreamerError(
            "Could not determine stream dimensions for GIF recording".into(),
        ));
    }

    println!("Detected stream: {}x{} (after crop)", raw_width, raw_height);

    let gif_fps = config.fps;
    let max_colors = ((32.0 + 224.0 * config.gif_quality) as u32).clamp(32, 256);
    let dither = if config.gif_quality >= 0.5 {
        "floyd_steinberg"
    } else {
        "bayer:bayer_scale=5"
    };
    let stats_mode = if config.gif_optimize { "diff" } else { "full" };
    let output_w = config.gif_max_width.unwrap_or(raw_width);
    let scale_prefix = if output_w != raw_width {
        format!("scale={}:-2:flags=lanczos,", output_w)
    } else {
        String::new()
    };
    let vf_filter = format!(
        "{}split[s0][s1];[s0]palettegen=max_colors={}:stats_mode={}[p];[s1][p]paletteuse=dither={}",
        scale_prefix, max_colors, stats_mode, dither
    );

    let mut child = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-s")
        .arg(format!("{}x{}", raw_width, raw_height))
        .arg("-r")
        .arg(gif_fps.to_string())
        .arg("-i")
        .arg("pipe:0")
        .arg("-vf")
        .arg(&vf_filter)
        .arg(&config.output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(RecordError::IoError)?;

    let mut stdin = child.stdin.take().expect("stdin");

    // Spawn a worker thread that reads frames from PipeWire (applying the
    // area crop when necessary) and sends them through a channel.
    // PipeWireCapture is constructed *inside* the thread because it contains
    // Rc types that are not Send.
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
    let (error_tx, error_rx) = std::sync::mpsc::channel::<String>();

    let node_id = source.node_id;
    let pipewire_fd = source.pipewire_fd;
    let pw_width = config.width;
    let pw_height = config.height;
    let crop = source.crop;

    let pw_thread = std::thread::spawn(move || {
        let capture = match crate::pipewire_engine::PipeWireCapture::connect(
            pipewire_fd,
            node_id,
            None,
            pw_width,
            pw_height,
        ) {
            Ok(c) => c,
            Err(e) => {
                let _ = error_tx.send(format!("PipeWire: {e}"));
                return;
            }
        };
        loop {
            match capture.try_recv_frame() {
                Ok(Some(frame)) => {
                    let pixels = if let Some(ref c) = crop {
                        match crop_rgba_frame(&frame, *c) {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("GIF crop error: {e}");
                                continue;
                            }
                        }
                    } else {
                        frame.pixels
                    };
                    if frame_tx.send(pixels).is_err() {
                        break;
                    }
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
                Err(e) => {
                    let _ = error_tx.send(format!("PipeWire: {e}"));
                    break;
                }
            }
            if capture.has_error() {
                if let Some(e) = capture.error_message() {
                    let _ = error_tx.send(e);
                }
                break;
            }
        }
    });

    println!("Recording GIF...");

    let mut stop_action = RecordingTerminalAction::Save;
    loop {
        let command = match &mut command_rx {
            Some(rx) => match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
                Err(_) => {
                    command_rx = None;
                    None
                }
            },
            None => None,
        };
        if let Some(cmd) = command {
            match cmd {
                RecordingControlCommand::Restart => {
                    stop_action = RecordingTerminalAction::Restart;
                    break;
                }
                RecordingControlCommand::StopSave => {
                    stop_action = RecordingTerminalAction::Save;
                    println!("\nStopping...");
                    break;
                }
                RecordingControlCommand::StopDiscard => {
                    stop_action = RecordingTerminalAction::Discard;
                    println!("\nDiscarding...");
                    break;
                }
                _ => {}
            }
        }
        match frame_rx.try_recv() {
            Ok(data) => {
                if stdin.write_all(&data).is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if let Ok(err) = error_rx.try_recv() {
                    eprintln!("PipeWire error: {err}");
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }

    if matches!(
        stop_action,
        RecordingTerminalAction::Save | RecordingTerminalAction::Discard
    ) {
        crate::gnome_shell::hide_recording_mask_best_effort();
        notify_daemon_event("recording_session_ended");
    }

    drop(stdin);
    drop(frame_rx);
    let _ = pw_thread.join();
    let status = child.wait().map_err(RecordError::IoError)?;

    if !status.success() && stop_action == RecordingTerminalAction::Save {
        let code = status.code();
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            status.signal()
        };
        #[cfg(not(unix))]
        let signal: Option<i32> = None;
        let is_expected = signal == Some(2) || code == Some(255) || code == Some(130);
        if !is_expected {
            return Err(RecordError::GifError(format!("FFmpeg: {status}")));
        }
    }
    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&config.output_path);
    }
    if stop_action == RecordingTerminalAction::Save {
        println!("GIF saved to {:?}", config.output_path);
    }
    Ok((config.output_path, stop_action))
}

/// GIF recording on X11 using GStreamer pipeline (preserved from old code).
#[allow(unused_imports)]
async fn record_gif_x11_gstreamer(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Build X11 GIF pipeline: ximagesrc -> videoconvert -> rgba -> appsink
    let source_str = get_x11_source(&config)?;
    let crop_filter = "";
    let wayland_source: Option<()> = None;

    // HiDPI:
    //   ON  -> keep the native source resolution (physical pixels on HiDPI displays).
    //          Sharper, larger files. This is the default to match historical behavior.
    //   OFF -> downscale to the user's logical selection size with Lanczos. Smaller
    //          files, output matches the rectangle the user drew on screen.
    //   Fullscreen (no width/height) is always a no-op since we have no logical target.
    let hidpi_filter = if !config.hidpi {
        match (config.width, config.height) {
            (Some(w), Some(h)) => format!(
                " ! videoscale method=lanczos ! video/x-raw,width={},height={}",
                w, h
            ),
            _ => String::new(),
        }
    } else {
        String::new()
    };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale; lanczos keeps text/UI edges sharp.
                format!(
                    " ! videoscale method=lanczos ! video/x-raw,width={},height={}",
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
                            // GIF size dropdown: when a width is set we always scale to it
                            // (matches Kap/ScreenToGif/GIPHY Capture semantics — the dropdown is a
                            // target, not a cap). `None` means "Original" (no resize).
                            // `-2` keeps aspect ratio while ensuring the height is divisible by 2,
                            // which is required by ffmpeg's GIF encoder for some palette filters.
                            let scale_prefix = match config.gif_max_width {
                                Some(target_w) if target_w != width => {
                                    format!("scale={}:-2:flags=lanczos,", target_w)
                                }
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

    // Eagerly tear down the recording UI before the (potentially long) ffmpeg
    // finalization step so the user sees the dim mask and tray state clear
    // immediately, matching the non-GIF stop UX. ffmpeg can take many seconds
    // to run palettegen/paletteuse on the buffered frames; we don't want the
    // overlay/tray hanging around for that. The outer recording loop will also
    // emit `recording_session_ended` once we return — that is idempotent.
    if matches!(
        stop_action,
        RecordingTerminalAction::Save | RecordingTerminalAction::Discard
    ) {
        crate::gnome_shell::hide_recording_mask_best_effort();
        notify_daemon_event("recording_session_ended");
    }

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
    let _ = wayland_source;
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
    app_config.rec_display_time = true; // always show recording time in top bar
    app_config.rec_hidpi = request.hidpi;
    app_config.rec_notifications = request.notifications;
    app_config.rec_cursor = true;
    app_config.rec_remember_selection = request.remember_selection;
    app_config.rec_dim_screen = request.dim_screen;
    app_config.rec_countdown = request.countdown;
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
    let filename = app_config
        .rec_filename_pattern
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
    });

    let controls_params = Some(RecordingControlsParams {
        capture_x: request.x,
        capture_y: request.y,
        capture_w: request.width,
        capture_h: request.height,
        is_fullscreen: request.fullscreen,
        show_timer: true,
        use_shell_mask,
        show_webcam: request.webcam,
        webcam_device: request.webcam_device,
        webcam_size: request.webcam_size as usize,
        webcam_shape: request.webcam_shape as usize,
        webcam_rel_x: request.webcam_rel_x,
        webcam_rel_y: request.webcam_rel_y,
        webcam_flip: request.webcam_flip,
        countdown_enabled: request.countdown,
        countdown_seconds: 3, // Default to 3s for now, or fetch from config
        session_id: None,     // Will be set when controls are launched
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
        open_editor: matches!(request.record_type, RecordingType::Video) && request.open_editor,
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
    // If GNOME Shell extension is available, use it (premium experience)
    if crate::gnome_shell::current_session_supports_gnome_shell_overlay() {
        return run_recording_with_shell_controls(
            config,
            params.clone(),
            runtime_overlay_snapshot,
            visibility_policy
                .unwrap_or_else(|| shell_controls_visibility_policy_for_params(&params)),
        )
        .await;
    }

    // Fallback for non-GNOME (Hyprland, Sway, Niri, River, etc.)
    // We use our native GTK+LayerShell overlay which implements the same mask/webcam logic.
    eprintln!("[recording] GNOME Shell not detected; using native recording overlay.");
    run_recording_with_native_controls(config, params).await
}

pub async fn run_recording_with_native_controls(
    config: RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id.clone()).await?;

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
    let mut ui_params = params.clone();
    ui_params.session_id = Some(session_id);
    let params_json = serde_json::to_string(&ui_params)?;

    let mut child = std::process::Command::new(&exe)
        .arg("recording-ui-internal")
        .arg(&params_json)
        .arg(params.countdown_seconds.to_string())
        .stdout(Stdio::piped())
        .spawn()?;

    let child_stdout = child.stdout.take().expect("failed to take child stdout");

    let (ui_line_tx, mut ui_line_rx) = mpsc::unbounded_channel::<String>();
    let stdout_task = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(child_stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if ui_line_tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let timeout_secs = if params.countdown_enabled {
        params.countdown_seconds + 5
    } else {
        5
    };
    let ready = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs as u64), async {
        while let Some(line) = ui_line_rx.recv().await {
            match line.trim() {
                "ready" => return Some(true),
                "discard" => return Some(false),
                _ => {}
            }
        }
        None
    })
    .await;

    match ready {
        Ok(Some(true)) => {}
        Ok(Some(false)) => {
            let _ = child.kill();
            let _ = child.wait();
            notify_recording_session_ended_best_effort();
            return Ok((config.output_path.clone(), StopAction::Discard));
        }
        Ok(None) | Err(_) => {
            if let Ok(Some(status)) = child.try_wait() {
                if status.code() == Some(2) {
                    notify_recording_session_ended_best_effort();
                    return Ok((config.output_path.clone(), StopAction::Discard));
                }
            }
        }
    }

    notify_daemon_event("recording_session_started");

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

    let ui_tx = recording_command_tx.clone();
    let ui_forward = tokio::spawn(async move {
        while let Some(line) = ui_line_rx.recv().await {
            let cmd = match line.trim() {
                "save" => RecordingControlCommand::StopSave,
                "discard" => RecordingControlCommand::StopDiscard,
                "pause" => RecordingControlCommand::Pause,
                "resume" => RecordingControlCommand::Resume,
                "restart" => RecordingControlCommand::Restart,
                _ => continue,
            };
            if ui_tx.send(cmd).is_err() {
                break;
            }
        }
    });

    control_server.set_command_sender(control_command_tx);
    let outcome = start_recording_with_commands(config.clone(), Some(command_rx)).await;
    control_server.clear_command_sender();

    dbus_forward.abort();
    ui_forward.abort();

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            notify_recording_session_ended_best_effort();
            return Err(err.into());
        }
    };

    let final_outcome = match outcome {
        (path, RecordingTerminalAction::Restart) => {
            if let Some(event) = daemon_event_for_terminal_action(RecordingTerminalAction::Restart)
            {
                notify_daemon_event(event);
            }
            let _ = std::fs::remove_file(&path);
            stdout_task.abort();
            let _ = child.kill();
            let _ = child.wait();
            return Box::pin(run_recording_with_native_controls(config, params)).await;
        }
        (path, RecordingTerminalAction::Save) => {
            if let Some(event) = daemon_event_for_terminal_action(RecordingTerminalAction::Save) {
                notify_daemon_event(event);
            }
            (path, StopAction::Save)
        }
        (path, RecordingTerminalAction::Discard) => {
            if let Some(event) = daemon_event_for_terminal_action(RecordingTerminalAction::Discard)
            {
                notify_daemon_event(event);
            }
            (path, StopAction::Discard)
        }
    };

    stdout_task.abort();
    let _ = child.wait();

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

    notify_daemon_event("recording_session_started");
    let final_outcome = loop {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        control_server.set_command_sender(command_tx);
        let outcome = start_recording_with_commands(config.clone(), Some(command_rx)).await;
        control_server.clear_command_sender();
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(err) => {
                drop(controls_handle);
                drop(control_server);
                drop(webcam_preview);
                notify_recording_session_ended_best_effort();
                return Err(err.into());
            }
        };

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
            if prepared.open_editor {
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
    use crate::capture_overlay::{RecordingRequest, RecordingType};
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
                show_webcam: false,
                webcam_device: -1,
                webcam_size: 1,
                webcam_shape: 3,
                webcam_rel_x: 0.0,
                webcam_rel_y: 0.0,
                webcam_flip: false,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, false);
        assert_eq!(
            prepared.use_shell_controls,
            crate::gnome_shell::current_session_supports_gnome_shell_overlay()
        );
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
                show_webcam: false,
                webcam_device: -1,
                webcam_size: 1,
                webcam_shape: 3,
                webcam_rel_x: 0.0,
                webcam_rel_y: 0.0,
                webcam_flip: false,
                countdown_enabled: true,
                countdown_seconds: 3,
                session_id: None,
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

        // OBS-based preset: veryfast + crf 22 + main profile
        assert!(props.contains("preset=veryfast"));
        assert!(props.contains("crf=22"));
        assert!(props.contains("profile=main"));
        assert!(props.contains("key-int-max=120"));
    }

    #[test]
    fn video_encoder_props_uses_quality_focused_webm_settings() {
        let config = RecordingConfig {
            fps: 60,
            ..x11_recording_config()
        };

        let vp9_props = video_encoder_props(profile_by_encoder("vp9enc"), &config);
        assert!(vp9_props.contains("end-usage=cq"));
        assert!(vp9_props.contains("cq-level=20"));
        assert!(vp9_props.contains("target-bitrate=0"));
        assert!(vp9_props.contains("cpu-used=2"));
        assert!(vp9_props.contains("keyframe-max-dist=120"));
        assert!(vp9_props.contains("deadline=good"));

        let vp8_props = video_encoder_props(profile_by_encoder("vp8enc"), &config);
        assert!(vp8_props.contains("end-usage=cq"));
        assert!(vp8_props.contains("target-bitrate=0"));
        assert!(vp8_props.contains("cpu-used=2"));
        assert!(vp8_props.contains("keyframe-max-dist=120"));
        assert!(vp8_props.contains("deadline=good"));

        let openh264_props = video_encoder_props(profile_by_encoder("openh264enc"), &config);
        assert!(openh264_props.contains("bitrate=8000000"));
        assert!(openh264_props.contains("complexity=medium"));
    }

    // Tests removed: GStreamer pipeline assertions and encoder availability checks
    // are no longer applicable with native PipeWire recording.
    #[test]
    fn normalize_recording_config_for_x264_makes_area_dimensions_even() {
        let config = RecordingConfig {
            width: Some(801),
            height: Some(599),
            ..x11_recording_config()
        };

        let normalized =
            normalize_recording_config_for_profile(profile_by_encoder("x264enc"), &config);

        assert_eq!(normalized.width, Some(800));
        assert_eq!(normalized.height, Some(598));
        assert_eq!(normalized.x, config.x);
        assert_eq!(normalized.y, config.y);
    }

    #[test]
    fn normalize_recording_config_for_vp9_preserves_area_dimensions() {
        let config = RecordingConfig {
            width: Some(801),
            height: Some(599),
            ..x11_recording_config()
        };

        let normalized =
            normalize_recording_config_for_profile(profile_by_encoder("vp9enc"), &config);

        assert_eq!(normalized.width, Some(801));
        assert_eq!(normalized.height, Some(599));
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
            webcam: true,
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
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
        let shell_supported = crate::gnome_shell::current_session_supports_gnome_shell_overlay();
        assert_eq!(prepared.runtime_overlay_snapshot.is_some(), shell_supported);
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
            webcam: true,
            webcam_rel_x: 0.61,
            webcam_rel_y: 0.17,
            webcam_size: 2,
            webcam_shape: 1,
            webcam_flip: true,
            webcam_device: 7,
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
                show_webcam: false,
                webcam_device: -1,
                webcam_size: 1,
                webcam_shape: 3,
                webcam_rel_x: 0.0,
                webcam_rel_y: 0.0,
                webcam_flip: false,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, shell_supported);
        assert_eq!(prepared.use_shell_controls, shell_supported);
        assert_eq!(
            prepared.shell_controls_visibility_policy,
            shell_supported
                .then_some(crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden)
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
            shell_supported
                .then_some(crate::gnome_shell::RecordingControlsVisibilityPolicy::Hidden)
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
                show_webcam: false,
                webcam_device: -1,
                webcam_size: 1,
                webcam_shape: 3,
                webcam_rel_x: 0.0,
                webcam_rel_y: 0.0,
                webcam_flip: false,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
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
