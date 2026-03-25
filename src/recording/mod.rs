use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use tokio::sync::oneshot;
use zbus::zvariant::OwnedValue;

mod stop_overlay;
pub use stop_overlay::{run_recording_stop_overlay, StopOverlayError};

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
    // VP8 (WebM) - Prioritized fallback over H.264 if missing, and better than Theora
    EncoderProfile {
        name: "VP8",
        encoder: "vp8enc",
        props: "deadline=1",
        muxer: "webmmux",
        extension: "webm",
    },
    // VP9 (WebM)
    EncoderProfile {
        name: "VP9",
        encoder: "vp9enc",
        props: "deadline=1",
        muxer: "webmmux",
        extension: "webm",
    },
    // Standard H.264
    EncoderProfile {
        name: "H.264 (x264)",
        encoder: "x264enc",
        props: "speed-preset=ultrafast tune=zerolatency",
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
    start_recording_with_optional_stop(config, None).await
}

/// Start a recording session and stop when `stop_rx` resolves (in addition to Ctrl+C).
pub async fn start_recording_with_stop(
    config: RecordingConfig,
    stop_rx: oneshot::Receiver<()>,
) -> RecordResult<PathBuf> {
    start_recording_with_optional_stop(config, Some(stop_rx)).await
}

async fn start_recording_with_optional_stop(
    config: RecordingConfig,
    stop_rx: Option<oneshot::Receiver<()>>,
) -> RecordResult<PathBuf> {
    // 1. Initialize GStreamer
    gst::init().map_err(|e| RecordError::InitError(e.to_string()))?;

    // Check if GIF requested
    if config.output_path.extension().is_some_and(|e| e == "gif") {
        return record_gif_rust_with_optional_stop(config, stop_rx).await;
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
    let pipeline_str = build_pipeline(&config, profile, final_path.as_path()).await?;
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

    // 6. Watch for messages and Ctrl+C
    let bus = pipeline
        .bus()
        .ok_or_else(|| RecordError::GStreamerError("Pipeline has no bus".into()))?;

    println!("Recording... Press Ctrl+C to stop.");

    // Handle Ctrl+C
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    let stop_fut = async {
        if let Some(rx) = stop_rx {
            let _ = rx.await;
        } else {
            futures_util::future::pending::<()>().await;
        }
    };
    tokio::pin!(stop_fut);

    // Phase 1: Recording until Ctrl+C or Error
    let mut stopping = false;
    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("\nStopping recording... Finalizing file...");
                pipeline.send_event(gst::event::Eos::new());
                stopping = true;
                break;
            }
            _ = &mut stop_fut => {
                println!("\nStopping recording... Finalizing file...");
                pipeline.send_event(gst::event::Eos::new());
                stopping = true;
                break;
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

    println!("Recording saved to {:?}", final_path);
    if let Ok(metadata) = std::fs::metadata(&final_path) {
        println!(
            "File size: {:.2} MB",
            metadata.len() as f64 / 1024.0 / 1024.0
        );
    }

    Ok(final_path)
}

pub fn copy_to_clipboard(path: &PathBuf) -> RecordResult<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    println!("Copying to clipboard...");

    // Convert path to file:// URI for better compatibility with chat apps (Discord, Slack, etc.)
    // They often fail to handle raw image/gif bytes but handle text/uri-list correctly.
    let uri = url::Url::from_file_path(path)
        .map_err(|_| RecordError::GStreamerError("Failed to convert path to URI".into()))?
        .to_string();

    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        // Wayland: use wl-copy with text/uri-list
        let mut child = Command::new("wl-copy")
            .arg("--type")
            .arg("text/uri-list")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| {
                RecordError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "wl-copy not found. Install wl-clipboard.",
                ))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(uri.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(RecordError::GStreamerError("wl-copy failed".into()));
        }
    } else {
        // X11: use xclip with text/uri-list
        let mut child = Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .arg("-t")
            .arg("text/uri-list")
            .arg("-i")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| {
                RecordError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "xclip not found. Install xclip.",
                ))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(uri.as_bytes())?;
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(RecordError::GStreamerError("xclip failed".into()));
        }
    }

    println!("Copied GIF URI to clipboard!");
    Ok(())
}

fn select_encoder(
    requested_path: &std::path::Path,
) -> RecordResult<(&'static EncoderProfile, PathBuf)> {
    // Check for x264enc first to warn user if missing
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

async fn build_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &std::path::Path,
) -> RecordResult<String> {
    let output_str = output_path.to_string_lossy();

    // Get video source
    let video_source = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        get_wayland_source(config.cursor).await?
    } else {
        get_x11_source(config)?
    };

    // HiDPI: downscale to logical resolution (2x)
    let hidpi_filter = if config.hidpi {
        " ! videoscale"
    } else {
        ""
    };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale
                format!(" ! videoscale ! video/x-raw,width={},height={}", max_w, max_h)
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // TODO: Audio pipeline support
    // When audio is implemented, the pipeline should include audio sources:
    //
    // Video: {video_source} ! videoconvert ! videorate ! ... -> muxer
    // Audio: pulsesrc(speaker) ! audioconvert ! audioresample
    //        pulsesrc(mic) ! audioconvert ! audioresample
    //        -> audiomixer -> encoder -> muxer
    //
    // Mono audio (config.mono_audio):
    //   Add caps filter after audioconvert to force mono output:
    //   let mono_filter = if config.mono_audio {
    //       " ! audio/x-raw,channels=1"
    //   } else {
    //       ""
    //   };
    //   Example: pulsesrc ! audioconvert{} ! audioresample ! ...
    //
    // The muxer (mp4mux/webmmux) will then combine video and audio streams.

    Ok(format!(
        "{} ! videoconvert{}{} ! videorate ! video/x-raw,framerate={}/1 ! queue ! {} {} ! {} ! filesink location=\"{}\"",
        video_source, hidpi_filter, resolution_filter, config.fps,
        profile.encoder, profile.props, profile.muxer, output_str
    ))
}

async fn get_wayland_source(cursor: bool) -> RecordResult<String> {
    use ashpd::desktop::screencast::Screencast;
    use zbus::zvariant::Value;

    println!("Requesting Wayland ScreenCast session...");

    let proxy = Screencast::new()
        .await
        .map_err(|e| RecordError::PortalError(e.to_string()))?;

    let session = proxy
        .create_session()
        .await
        .map_err(|e| RecordError::PortalError(e.to_string()))?;

    let connection = proxy.connection();

    let cursor_mode = if cursor {
        ashpd::desktop::screencast::CursorMode::Embedded
    } else {
        ashpd::desktop::screencast::CursorMode::Hidden
    };

    // 1. Select Sources
    proxy
        .select_sources(
            &session,
            cursor_mode,
            ashpd::desktop::screencast::SourceType::Monitor
                | ashpd::desktop::screencast::SourceType::Window,
            false, // multiple
            None,
            ashpd::desktop::PersistMode::DoNot,
        )
        .await
        .map_err(|e| RecordError::PortalError(e.to_string()))?;

    println!("Please select a screen or window to record...");

    // 2. Start
    let msg = connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.ScreenCast"),
            "Start",
            &(&session, "", HashMap::<String, Value>::new()),
        )
        .await
        .map_err(|e| RecordError::PortalError(format!("Start call failed: {}", e)))?;

    let request_path: zbus::zvariant::OwnedObjectPath = msg
        .body()
        .deserialize()
        .map_err(|e| RecordError::PortalError(format!("Failed to parse Start response: {}", e)))?;

    let results: HashMap<String, OwnedValue> = wait_for_response(connection, &request_path).await?;

    let streams_value = results
        .get("streams")
        .ok_or_else(|| RecordError::PortalError("No streams in portal response".into()))?;

    let streams: Vec<(u32, HashMap<String, OwnedValue>)> = streams_value
        .try_clone()
        .unwrap()
        .try_into()
        .map_err(|e| RecordError::PortalError(format!("Invalid streams format: {}", e)))?;

    let stream = streams
        .first()
        .ok_or_else(|| RecordError::PortalError("No streams returned".into()))?;

    let node_id = stream.0;
    println!("Got PipeWire Node ID: {}", node_id);

    Ok(format!("pipewiresrc path={} do-timestamp=true", node_id))
}

async fn wait_for_response(
    connection: &zbus::Connection,
    path: &zbus::zvariant::ObjectPath<'_>,
) -> RecordResult<HashMap<String, OwnedValue>> {
    use futures_util::StreamExt;

    let match_rule = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{}'",
        path
    );

    let rule: zbus::MatchRule = match_rule
        .as_str()
        .try_into()
        .map_err(|e| RecordError::PortalError(format!("Invalid match rule: {}", e)))?;

    let mut stream = zbus::MessageStream::for_match_rule(rule, connection, Some(1))
        .await
        .map_err(|e| RecordError::PortalError(format!("Failed to create message stream: {}", e)))?;

    let message = stream
        .next()
        .await
        .ok_or_else(|| RecordError::PortalError("No response from portal".into()))?
        .map_err(|e| RecordError::PortalError(format!("Signal error: {}", e)))?;

    // Response signal signature: (ua{sv})
    let (status, results): (u32, HashMap<String, OwnedValue>) =
        message.body().deserialize().map_err(|e| {
            RecordError::PortalError(format!("Failed to deserialize portal response: {}", e))
        })?;

    if status != 0 {
        return Err(RecordError::Cancelled);
    }

    Ok(results)
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

async fn record_gif_rust_with_optional_stop(
    config: RecordingConfig,
    stop_rx: Option<oneshot::Receiver<()>>,
) -> RecordResult<PathBuf> {
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
    let source_str = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        get_wayland_source(config.cursor).await?
    } else {
        get_x11_source(&config)?
    };

    // HiDPI: downscale to logical resolution (2x)
    let hidpi_filter = if config.hidpi {
        " ! videoscale"
    } else {
        ""
    };

    // Max resolution: downscale if needed
    let resolution_filter = if let Some((max_w, max_h)) = config.max_resolution {
        if let (Some(w), Some(h)) = (config.width, config.height) {
            if w > max_w || h > max_h {
                // Only downscale, never upscale
                format!(" ! videoscale ! video/x-raw,width={},height={}", max_w, max_h)
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
        "{} ! videoconvert{}{} ! videorate ! video/x-raw,format=RGBA,framerate={}/1 ! appsink name=sink emit-signals=true sync=false drop=false max-buffers=200",
        source_str, hidpi_filter, resolution_filter, gif_fps
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

    println!("Recording GIF... Press Ctrl+C to stop.");

    // Handle Ctrl+C
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    let stop_fut = async {
        if let Some(rx) = stop_rx {
            let _ = rx.await;
        } else {
            futures_util::future::pending::<()>().await;
        }
    };
    tokio::pin!(stop_fut);

    let mut stopping = false;
    let mut ffmpeg_child: Option<std::process::Child> = None;

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("\nStopping recording...");
                stopping = true;
            }
            _ = &mut stop_fut => {
                println!("\nStopping recording...");
                stopping = true;
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

                            let child = Command::new("ffmpeg")
                                .arg("-y") // Overwrite
                                .arg("-loglevel").arg("warning")
                                .arg("-nostats")
                                .arg("-f").arg("rawvideo")
                                .arg("-pix_fmt").arg("rgba")
                                .arg("-s").arg(format!("{}x{}", width, height))
                                .arg("-r").arg(gif_fps.to_string())
                                .arg("-i").arg("pipe:0")
                                // High quality GIF palette generation
                                .arg("-vf").arg("split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse")
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

    println!("GIF saved to {:?}", config.output_path);
    Ok(config.output_path)
}
