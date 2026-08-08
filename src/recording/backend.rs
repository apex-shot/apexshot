use super::*;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CropMargins {
    left: u32,
    right: u32,
    top: u32,
    bottom: u32,
}

pub(super) type RecordingPortalSession =
    ashpd::desktop::Session<'static, ashpd::desktop::screencast::Screencast<'static>>;

/// Backend that owns the lifetime of a Wayland capture stream.
pub(super) enum WaylandCaptureSession {
    /// XDG ScreenCast portal session (GNOME and generic fallback).
    Portal(#[allow(dead_code)] RecordingPortalSession),
    /// KWin `zkde_screencast_unstable_v1` (Spectacle-style, no portal dialog).
    /// Boxed so the portal variant stays small (clippy `large_enum_variant`).
    KdeNative(Box<crate::backend::kde_screencast::KdeScreencastHandle>),
}

impl std::fmt::Debug for WaylandCaptureSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Portal(_) => f.write_str("Portal(..)"),
            Self::KdeNative(handle) => f
                .debug_struct("KdeNative")
                .field("node_id", &handle.node_id())
                .finish(),
        }
    }
}

#[derive(Debug)]
pub(super) struct WaylandSource {
    node_id: u32,
    /// Portal remote FD. `None` for KDE-native streams that publish on the
    /// default session PipeWire socket (same as Spectacle / KPipeWire).
    pipewire_fd: Option<OwnedFd>,
    #[allow(dead_code)]
    stream_width: u32,
    #[allow(dead_code)]
    stream_height: u32,
    #[allow(dead_code)]
    crop: Option<CropMargins>,
    _session: WaylandCaptureSession,
}

#[derive(Debug)]
pub(super) struct BuiltPipeline {
    wayland_source: Option<WaylandSource>,
    profile: &'static EncoderProfile,
    encoder_name: String,
    encoder_props: String,
    final_path: PathBuf,
    config: super::RecordingConfig,
}

pub(super) struct PreparedGifWaylandRecording {
    final_path: PathBuf,
    temp_path: PathBuf,
    config: super::RecordingConfig,
    backend: BuiltPipeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordingScreenCastTarget {
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

pub(super) fn recording_restore_token_path(target: RecordingScreenCastTarget) -> Option<PathBuf> {
    let mut path = dirs::cache_dir()?;
    path.push("apexshot");
    path.push(target.token_file_name());
    Some(path)
}

pub(super) fn load_recording_restore_token(target: RecordingScreenCastTarget) -> Option<String> {
    let path = recording_restore_token_path(target)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub(super) fn save_recording_restore_token(target: RecordingScreenCastTarget, token: &str) {
    let Some(path) = recording_restore_token_path(target) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, token);
}

pub(super) fn clear_recording_restore_token(target: RecordingScreenCastTarget) {
    if let Some(path) = recording_restore_token_path(target) {
        let _ = std::fs::remove_file(path);
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

/// Resolve the stream's top-left in global coordinates.
///
/// KDE's ScreenCast portal often returns `position=None` even for monitor
/// streams. Infer from the monitor that contains the selection when possible,
/// otherwise fall back to `(0, 0)` so area recording does not hard-crash.
fn resolve_wayland_stream_position(
    reported: Option<(i32, i32)>,
    stream_size: (i32, i32),
    selection: (i32, i32, u32, u32),
) -> (i32, i32) {
    if let Some(pos) = reported {
        return pos;
    }

    let (sel_x, sel_y, sel_w, sel_h) = selection;
    let cx = sel_x + (sel_w as i32) / 2;
    let cy = sel_y + (sel_h as i32) / 2;
    let (stream_w, stream_h) = stream_size;

    // Prefer a monitor that contains the selection center and matches the
    // stream size (typical when the portal returns a single-monitor stream).
    for (mx, my, mw, mh) in iter_gdk_monitor_geometries() {
        if cx >= mx
            && cy >= my
            && cx < mx + mw
            && cy < my + mh
            && (stream_w <= 0 || stream_h <= 0 || (mw == stream_w && mh == stream_h))
        {
            eprintln!(
                "[recording] Stream missing position metadata; using monitor origin ({mx},{my}) for crop"
            );
            return (mx, my);
        }
    }

    eprintln!("[recording] Stream missing position metadata; assuming (0,0) for crop");
    (0, 0)
}

pub(super) fn iter_gdk_monitor_geometries() -> Vec<(i32, i32, i32, i32)> {
    use gtk4::gdk::prelude::*;
    use gtk4::glib::object::Cast;
    use gtk4::prelude::ListModelExt;

    let Some(display) = gtk4::gdk::Display::default() else {
        return Vec::new();
    };
    let monitors = display.monitors();
    let mut out = Vec::new();
    for i in 0..monitors.n_items() {
        let Some(item) = monitors.item(i) else {
            continue;
        };
        let Ok(monitor) = item.downcast::<gtk4::gdk::Monitor>() else {
            continue;
        };
        let g = monitor.geometry();
        out.push((g.x(), g.y(), g.width(), g.height()));
    }
    out
}

/// Build a client-side crop for a pre-selected area, or `None` to record the
/// whole stream when crop math cannot be resolved (never fail the session).
fn wayland_area_crop_or_full(
    stream_position: Option<(i32, i32)>,
    stream_size: (i32, i32),
    selection: (i32, i32, u32, u32),
) -> Option<CropMargins> {
    let position = resolve_wayland_stream_position(stream_position, stream_size, selection);
    match compute_wayland_crop(position, stream_size, selection) {
        Ok(crop) => Some(crop),
        Err(err) => {
            eprintln!(
                "[recording] Could not crop to selected region ({err}); recording the full stream instead"
            );
            None
        }
    }
}

#[derive(Debug)]
struct EncoderProfile {
    name: &'static str,
    encoder: &'static str,        // GStreamer element name (used by X11 path)
    ffmpeg_encoder: &'static str, // ffmpeg -c:v name (used by Wayland path)
    muxer: &'static str,
    extension: &'static str,
}

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

pub(super) async fn prepare_recording_backend(
    config: super::RecordingConfig,
) -> super::RecordResult<BuiltPipeline> {
    if super::wf_recorder::is_wlroots_session() {
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording must be prepared through its dedicated backend".into(),
        ));
    }

    if config.output_path.extension().is_some_and(|e| e == "gif") {
        return Err(RecordError::UnsupportedBackend(
            "GIF recording must be prepared through its dedicated backend".into(),
        ));
    }

    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        return Err(RecordError::NoEncoderFound);
    }

    let (profile, final_path) = select_encoder(config.output_path.as_path())?;
    let effective_config = normalize_recording_config_for_profile(profile, &config);
    println!("Using Encoder: {} ({})", profile.name, profile.encoder);

    if final_path != config.output_path {
        println!(
            "Note: Output filename changed to match format: {:?}",
            final_path
        );
    }

    build_pipeline(&effective_config, profile, final_path.as_path()).await
}

pub(super) async fn start_recording_with_prepared_backend(
    built: BuiltPipeline,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    if let Some(wayland_source) = built.wayland_source {
        let final_path = built.final_path.clone();
        let encoder_name = built.encoder_name.clone();
        let encoder_props = built.encoder_props.clone();
        let config = built.config.clone();
        return tokio::task::spawn_blocking(move || {
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
    }

    record_x11_with_gstreamer(&built.config, built.profile, &built.final_path, command_rx).await
}

/// Cached set of encoder names reported by `ffmpeg -encoders`.
/// Fedora ships `ffmpeg-free` without `libx264`; `libopenh264` is the usual
/// H.264 path. Ubuntu/etc. typically have `libx264`. Probe once per process.
fn ffmpeg_available_encoders() -> &'static std::collections::HashSet<String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut set = std::collections::HashSet::new();
        let Ok(output) = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        else {
            return set;
        };
        // ffmpeg prints the encoder table to stdout (sometimes mixed with
        // banner noise on stderr). Parse both.
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for line in text.lines() {
            // Lines look like: " V....D libopenh264          OpenH264 ..."
            let trimmed = line.trim();
            if trimmed.len() < 10 {
                continue;
            }
            // Flag field is typically 6 chars then space then name.
            let rest = if trimmed.starts_with([' ', 'V', 'A', 'S']) && trimmed.len() > 8 {
                trimmed.get(7..).unwrap_or(trimmed).trim_start()
            } else {
                continue;
            };
            let name = rest.split_whitespace().next().unwrap_or("");
            if name.starts_with("lib")
                || name.contains("264")
                || name.contains("265")
                || name.contains("vp8")
                || name.contains("vp9")
                || name.contains("theora")
                || name.contains("av1")
            {
                set.insert(name.to_string());
            }
        }
        set
    })
}

pub(super) fn ffmpeg_encoder_available(name: &str) -> bool {
    ffmpeg_available_encoders().contains(name)
}

/// Whether this profile can be used on the current session.
/// Wayland recording encodes with ffmpeg; X11 uses GStreamer elements.
fn encoder_profile_available(profile: &EncoderProfile) -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return ffmpeg_encoder_available(profile.ffmpeg_encoder);
    }
    // X11 / GStreamer path — best-effort; init is cheap if already done.
    if gst::init().is_err() {
        // Fall back to ffmpeg availability if GST is unavailable.
        return ffmpeg_encoder_available(profile.ffmpeg_encoder);
    }
    gst::ElementFactory::find(profile.encoder).is_some()
        || ffmpeg_encoder_available(profile.ffmpeg_encoder)
}

fn select_encoder(
    requested_path: &std::path::Path,
) -> super::RecordResult<(&'static EncoderProfile, PathBuf)> {
    // Prefer the requested container when an actually-installed encoder supports it.
    // Important where ffmpeg-free lacks libx264: pick OpenH264 or another format
    // instead of hard-failing mid-encode.
    if let Some(ext) = requested_path.extension().and_then(|s| s.to_str()) {
        let mut matched_ext = false;
        for profile in PROFILES {
            if profile.extension != ext {
                continue;
            }
            matched_ext = true;
            if encoder_profile_available(profile) {
                return Ok((profile, requested_path.to_path_buf()));
            }
        }
        if matched_ext {
            println!("Warning: no installed encoder for '.{ext}'; trying another format.");
        } else {
            println!("Warning: Requested format '{ext}' not in profile list; using default.");
        }
    }

    // Fall back: first available profile in priority order (VP9 → VP8 → x264 → OpenH264 → Theora).
    for profile in PROFILES {
        if encoder_profile_available(profile) {
            let mut new_path = requested_path.to_path_buf();
            new_path.set_extension(profile.extension);
            if new_path != requested_path {
                println!(
                    "Note: using {} ({}) → {}",
                    profile.name,
                    profile.ffmpeg_encoder,
                    new_path.display()
                );
            }
            return Ok((profile, new_path));
        }
    }

    Err(RecordError::NoEncoderFound)
}

/// Wayland recording: native PipeWire frame capture + ffmpeg pipe for encoding.
pub(super) fn record_wayland_with_ffmpeg_sync(
    wayland_source: WaylandSource,
    final_path: &std::path::Path,
    encoder_name: &str,
    encoder_props: &str,
    config: &super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let final_path = final_path.to_path_buf();

    // Open PipeWire capture stream (continuous).
    // Portal path: connect via the remote FD from OpenPipeWireRemote.
    // KDE-native path: node lives on the default session socket.
    let capture = match wayland_source.pipewire_fd {
        Some(fd) => crate::pipewire_engine::PipeWireCapture::connect(
            fd,
            wayland_source.node_id,
            None, // continuous — no max frame limit
            config.width,
            config.height,
        ),
        None => crate::pipewire_engine::PipeWireCapture::connect_default(
            wayland_source.node_id,
            None,
            config.width,
            config.height,
        ),
    }
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
    let use_vaapi = super::wf_recorder::should_use_vaapi();
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats");

    if use_vaapi {
        let (vaapi_width, vaapi_height) =
            fit_within_max_resolution(input_width, input_height, config.max_resolution);
        let vaapi_args = super::wf_recorder::ffmpeg_vaapi_args(vaapi_width, vaapi_height);
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
        } else if encoder_name == "libopenh264" {
            // Fedora ffmpeg-free ships OpenH264 (no libx264). CRF is not
            // supported; use a solid CBR-ish bitrate for desktop capture.
            ffmpeg_cmd.arg("-b:v").arg("8M");
            ffmpeg_cmd.arg("-maxrate").arg("10M");
            ffmpeg_cmd.arg("-bufsize").arg("16M");
            ffmpeg_cmd.arg("-allow_skip_frames").arg("0");
        } else if encoder_name == "libvpx-vp9" || encoder_name == "libvpx" {
            ffmpeg_cmd.arg("-b:v").arg("0");
            ffmpeg_cmd.arg("-crf").arg("32");
            ffmpeg_cmd.arg("-deadline").arg("realtime");
            ffmpeg_cmd.arg("-cpu-used").arg("6");
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
        super::audio::ensure_pipewire_pulse_running();

        if config.mic_enabled {
            let mic_dev = config
                .mic_source
                .clone()
                .unwrap_or_else(super::audio::get_pulse_default_source);
            eprintln!("[recording] Audio: mic device={mic_dev}");
            ffmpeg_cmd.arg("-f").arg("pulse");
            ffmpeg_cmd.arg("-i").arg(&mic_dev);
        }

        if config.speaker_enabled {
            let spk_dev = config
                .speaker_source
                .clone()
                .unwrap_or_else(super::audio::get_pulse_speaker_monitor);
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
    let mut stop_action = super::RecordingTerminalAction::Save;
    let mut frames_written = 0u64;
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_frame_at: Option<std::time::Instant> = None;
    let mut last_pixels: Option<Vec<u8>> = None;
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
                    stop_action = super::RecordingTerminalAction::Restart;
                    break;
                }
                RecordingControlCommand::StopSave => {
                    println!("\nStopping recording...");
                    break;
                }
                RecordingControlCommand::StopDiscard => {
                    stop_action = super::RecordingTerminalAction::Discard;
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

        // Keep the latest PipeWire frame, but write to ffmpeg on our own clock.
        // Some compositors only deliver changed frames; without duplicates a
        // 16s mostly-static recording can encode as a 4s video at 30fps.
        match capture.try_recv_frame() {
            Ok(Some(frame)) => {
                last_pixels = Some(if let Some(crop) = wayland_source.crop {
                    crop_rgba_frame(&frame, crop)?
                } else {
                    frame.pixels
                });
                if next_frame_at.is_none() {
                    next_frame_at = Some(std::time::Instant::now());
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("PipeWire frame error: {e}");
                break;
            }
        }

        let Some(pixels) = last_pixels.as_ref() else {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        };

        let now = std::time::Instant::now();
        let Some(deadline) = next_frame_at else {
            next_frame_at = Some(now);
            continue;
        };
        if now < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        next_frame_at = Some(deadline + frame_interval);

        // Write frame to ffmpeg stdin
        if let Err(e) = stdin.write_all(pixels) {
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

    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&final_path);
        return Ok((final_path, stop_action));
    }

    if !status.success() {
        let _ = std::fs::remove_file(&final_path);
        return Err(RecordError::GStreamerError(format!(
            "ffmpeg failed to encode the recording (exit {status}). \
             On Fedora, install a codec pack or ensure libopenh264 is available \
             (ffmpeg-free typically provides it)."
        )));
    }

    // Guard against zero-byte / missing outputs that used to be reported as saved.
    match std::fs::metadata(&final_path) {
        Ok(metadata) if metadata.len() > 0 => {
            println!("Recording saved to {:?}", final_path);
            println!(
                "File size: {:.2} MB",
                metadata.len() as f64 / 1024.0 / 1024.0
            );
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&final_path);
            return Err(RecordError::GStreamerError(
                "Recording finished but the output file is empty (encoder failed).".into(),
            ));
        }
        Err(err) => {
            return Err(RecordError::GStreamerError(format!(
                "Recording finished but output file is missing: {err}"
            )));
        }
    }

    Ok((final_path, stop_action))
}

pub(super) fn fit_within_max_resolution(
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

pub(super) fn wayland_video_filter(max_resolution: Option<(u32, u32)>) -> String {
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
) -> super::RecordResult<Vec<u8>> {
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
    config: &super::RecordingConfig,
    profile: &EncoderProfile,
    final_path: &std::path::Path,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
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
    let mut stop_action = super::RecordingTerminalAction::Save;
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
                        stop_action = super::RecordingTerminalAction::Restart;
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = super::RecordingTerminalAction::Save;
                        pipeline.send_event(gst::event::Eos::new());
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = super::RecordingTerminalAction::Discard;
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

    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(final_path);
    }

    Ok((final_path.to_path_buf(), stop_action))
}

/// Build a GStreamer pipeline string for X11 capture (preserved from old code).
fn build_x11_gstreamer_pipeline(
    config: &super::RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> super::RecordResult<String> {
    let output_str = output_path.to_string_lossy();
    let video_source = get_x11_source(config)?;
    let video_raw_caps = format!("video/x-raw,framerate={}/1", config.fps);

    Ok(format!(
        "{} ! videoconvert ! {}videorate ! {} ! {} ! {} ! filesink location=\"{}\"",
        video_source, video_raw_caps, "queue", profile.encoder, profile.muxer, output_str
    ))
}

#[cfg(test)]
fn video_encoder_props(profile: &EncoderProfile, config: &super::RecordingConfig) -> String {
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

fn normalize_recording_config_for_profile(
    profile: &EncoderProfile,
    config: &super::RecordingConfig,
) -> super::RecordingConfig {
    let mut normalized = config.clone();

    // H.264 encoders (x264 / OpenH264) need even dimensions for yuv420p.
    if !matches!(profile.encoder, "x264enc" | "openh264enc") {
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

async fn build_pipeline(
    config: &super::RecordingConfig,
    profile: &'static EncoderProfile,
    output_path: &std::path::Path,
) -> super::RecordResult<BuiltPipeline> {
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
        profile,
        encoder_name: profile.ffmpeg_encoder.to_string(),
        encoder_props,
        final_path: output_path.to_path_buf(),
        config: config.clone(),
    })
}

/// On KDE Plasma, the reliable capture path is the desktop portal UI
/// (same chooser Spectacle uses under the hood). KDE-native
/// `zkde_screencast` is opt-in only — it needs compositor authorization that
/// often fails for third-party apps and caused confusing dual-UI / crashes.
pub(super) fn prefer_kde_native_screencast() -> bool {
    std::env::var_os("APEXSHOT_KDE_NATIVE_SCREENCAST").is_some()
        && crate::backend::kde_screencast::is_kde_native_screencast_preferred()
}

pub(super) async fn get_wayland_source(
    config: &super::RecordingConfig,
) -> super::RecordResult<WaylandSource> {
    // Optional experimental path only (APEXSHOT_KDE_NATIVE_SCREENCAST=1).
    if prefer_kde_native_screencast() {
        match get_kde_wayland_source(config) {
            Ok(source) => {
                println!("Using KDE-native zkde_screencast (APEXSHOT_KDE_NATIVE_SCREENCAST).");
                return Ok(source);
            }
            Err(err) => {
                eprintln!(
                    "[recording] KDE-native screencast failed ({err}); using ScreenCast portal."
                );
            }
        }
    }

    use ashpd::desktop::{
        screencast::{CursorMode, Screencast, SourceType},
        PersistMode,
    };

    // Fedora / KDE / GNOME: system portal picks the source; ApexShot settings
    // still control countdown, audio, shortcuts, notifications, and save path.
    println!("Requesting Wayland ScreenCast session (system share UI)…");

    let wants_area_crop = matches!(
        (config.x, config.y, config.width, config.height),
        (Some(_), Some(_), Some(w), Some(h)) if w > 0 && h > 0
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
    ) -> super::RecordResult<(
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
        let size = (stream_width as i32, stream_height as i32);
        let selection = (
            config.x.expect("checked above"),
            config.y.expect("checked above"),
            config.width.expect("checked above"),
            config.height.expect("checked above"),
        );
        // KDE portal frequently omits stream position. Infer it or fall back to
        // full-stream capture instead of aborting after the user already confirmed.
        wayland_area_crop_or_full(stream.position(), size, selection)
    } else {
        None
    };

    Ok(WaylandSource {
        node_id,
        pipewire_fd: Some(pipewire_fd),
        stream_width,
        stream_height,
        crop,
        _session: WaylandCaptureSession::Portal(session),
    })
}

/// Spectacle-style KWin screencast: `zkde_screencast_unstable_v1` → PipeWire node
/// on the default session socket. No xdg-desktop-portal dialog.
pub(super) fn get_kde_wayland_source(
    config: &super::RecordingConfig,
) -> super::RecordResult<WaylandSource> {
    use crate::backend::kde_screencast::{start_stream, KdeScreencastTarget};

    let target = match (config.x, config.y, config.width, config.height) {
        (Some(x), Some(y), Some(w), Some(h)) if w > 0 && h > 0 => KdeScreencastTarget::Region {
            x,
            y,
            width: w,
            height: h,
        },
        _ => KdeScreencastTarget::Output,
    };

    let handle = start_stream(target, config.cursor)
        .map_err(|e| RecordError::PortalError(format!("KDE-native screencast failed: {e}")))?;

    let stream_width = handle.width();
    let stream_height = handle.height();
    let node_id = handle.node_id();

    // Region streams are already cropped by KWin — no client-side crop.
    Ok(WaylandSource {
        node_id,
        pipewire_fd: None,
        stream_width,
        stream_height,
        crop: None,
        _session: WaylandCaptureSession::KdeNative(Box::new(handle)),
    })
}

pub(super) fn get_x11_source(config: &super::RecordingConfig) -> super::RecordResult<String> {
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

pub(super) async fn record_gif_rust_with_commands(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
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

/// GIF recording on Wayland: record with the same PipeWire -> video path used by
/// normal video recording, then convert the temporary video to GIF. This avoids
/// the GNOME/portal raw-frame path producing a visually static GIF while video
/// recording works correctly on the same system.
pub(super) async fn record_gif_wayland_native(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    let prepared = prepare_gif_wayland_recording(config).await?;
    record_prepared_gif_wayland_native(prepared, command_rx).await
}

pub(super) async fn prepare_gif_wayland_recording(
    config: super::RecordingConfig,
) -> super::RecordResult<PreparedGifWaylandRecording> {
    let final_path = config.output_path.clone();
    let temp_path = std::env::temp_dir().join(format!(
        "apexshot-gif-source-{}-{}.mp4",
        std::process::id(),
        chrono::Local::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    ));

    let mut video_config = config.clone();
    video_config.output_path = temp_path.clone();
    // GIFs have no audio track. Keeping audio out also avoids audio-device
    // setup failures from breaking GIF capture.
    video_config.mic_source = None;
    video_config.speaker_source = None;

    let backend = prepare_recording_backend(video_config).await?;
    Ok(PreparedGifWaylandRecording {
        final_path,
        temp_path,
        config,
        backend,
    })
}

pub(super) async fn record_prepared_gif_wayland_native(
    prepared: PreparedGifWaylandRecording,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    use std::process::Command;

    let PreparedGifWaylandRecording {
        final_path,
        temp_path,
        config,
        backend,
    } = prepared;

    let (_recorded_path, stop_action) =
        start_recording_with_prepared_backend(backend, command_rx).await?;

    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(&final_path);
        return Ok((final_path, stop_action));
    }

    if stop_action == super::RecordingTerminalAction::Restart {
        let _ = std::fs::remove_file(&temp_path);
        return Ok((final_path, stop_action));
    }

    let max_colors = ((32.0 + 224.0 * config.gif_quality) as u32).clamp(32, 256);
    let dither = if config.gif_quality >= 0.5 {
        "floyd_steinberg"
    } else {
        "bayer:bayer_scale=5"
    };
    let stats_mode = if config.gif_optimize { "diff" } else { "full" };
    let scale_prefix = match config.gif_max_width {
        Some(target_w) => format!("scale={}:-2:flags=lanczos,", target_w),
        None => String::new(),
    };
    let vf_filter = format!(
        "fps={},{}format=rgb24,split[s0][s1];[s0]palettegen=max_colors={}:reserve_transparent=0:stats_mode={}[p];[s1][p]paletteuse=dither={}",
        config.fps, scale_prefix, max_colors, stats_mode, dither
    );

    // The visible recording session is over once the temporary video stops.
    // GIF conversion can take a while; clear masks/controls immediately instead
    // of keeping the dim background up until ffmpeg finishes.
    crate::gnome_shell::hide_recording_mask_best_effort();
    super::notify_daemon_event("recording_session_ended");

    println!("Converting temporary video to GIF...");
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats")
        .arg("-i")
        .arg(&temp_path)
        .arg("-filter_complex")
        .arg(&vf_filter)
        .arg(&final_path)
        .status()
        .map_err(RecordError::IoError)?;

    let _ = std::fs::remove_file(&temp_path);

    if !status.success() {
        let _ = std::fs::remove_file(&final_path);
        return Err(RecordError::GifError(format!(
            "FFmpeg GIF conversion failed: {status}"
        )));
    }

    println!("GIF saved to {:?}", final_path);
    Ok((final_path, stop_action))
}

/// GIF recording on X11 using GStreamer pipeline (preserved from old code).
#[allow(unused_imports)]
pub(super) async fn record_gif_x11_gstreamer(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
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
    let mut stop_action = super::RecordingTerminalAction::Save;
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
                        super::notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        pipeline
                            .set_state(gst::State::Playing)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to resume GIF pipeline: {e}")))?;
                        paused = false;
                        super::notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = super::RecordingTerminalAction::Restart;
                        println!("\nRestarting recording...");
                        stopping = true;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = super::RecordingTerminalAction::Save;
                        println!("\nStopping recording...");
                        stopping = true;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = super::RecordingTerminalAction::Discard;
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
                                "{}format=rgb24,split[s0][s1];[s0]palettegen=max_colors={}:reserve_transparent=0:stats_mode={}[p];[s1][p]paletteuse=dither={}",
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
        super::RecordingTerminalAction::Save | super::RecordingTerminalAction::Discard
    ) {
        crate::gnome_shell::hide_recording_mask_best_effort();
        super::notify_daemon_event("recording_session_ended");
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

    if stop_action == super::RecordingTerminalAction::Save {
        println!("GIF saved to {:?}", config.output_path);
    }
    let _ = wayland_source;
    Ok((config.output_path, stop_action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
    fn normalize_openh264_forces_even_dimensions() {
        let mut config = x11_recording_config();
        config.width = Some(641);
        config.height = Some(481);
        let normalized =
            normalize_recording_config_for_profile(profile_by_encoder("openh264enc"), &config);
        assert_eq!(normalized.width, Some(640));
        assert_eq!(normalized.height, Some(480));
    }

    #[test]
    fn ffmpeg_encoder_probe_sees_common_fedora_or_ubuntu_codecs() {
        // Skip cleanly when ffmpeg is not installed in the test environment.
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let encoders = ffmpeg_available_encoders();
        // At least one of the H.264 or VP* software encoders should exist on
        // any distro that can run ApexShot recording (Fedora: openh264/vpx,
        // Ubuntu: often libx264 + vpx).
        assert!(
            encoders.contains("libopenh264")
                || encoders.contains("libx264")
                || encoders.contains("libvpx-vp9")
                || encoders.contains("libvpx"),
            "expected a usable ffmpeg video encoder, got: {encoders:?}"
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
    fn missing_stream_position_defaults_to_origin() {
        let pos = resolve_wayland_stream_position(None, (1920, 1080), (100, 100, 200, 200));
        // Without GDK monitors in unit tests, falls back to (0, 0).
        assert_eq!(pos, (0, 0));
        assert_eq!(
            resolve_wayland_stream_position(Some((1920, 0)), (2560, 1440), (2000, 10, 100, 100)),
            (1920, 0)
        );
    }

    #[test]
    fn area_crop_soft_fails_instead_of_erroring() {
        // Selection completely outside a (0,0) 100x100 stream → soft fail to None.
        assert!(wayland_area_crop_or_full(None, (100, 100), (500, 500, 50, 50)).is_none());
        // Valid selection on assumed (0,0) origin.
        let crop = wayland_area_crop_or_full(None, (1920, 1080), (100, 100, 200, 200))
            .expect("in-bounds selection should crop");
        assert_eq!(crop.left, 100);
        assert_eq!(crop.top, 100);
    }

    #[test]
    fn compute_wayland_crop_rejects_selection_outside_monitor() {
        let err = compute_wayland_crop((1920, 0), (2560, 1440), (1800, 100, 400, 300))
            .expect_err("selection should be rejected");

        assert!(err.contains("outside the selected monitor"));
    }
}
