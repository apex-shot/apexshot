use super::super::{
    notify_daemon_event, RecordError, RecordResult, RecordingConfig, RecordingControlCommand,
    RecordingTerminalAction,
};
use super::{prepare_recording_backend, start_recording_with_prepared_backend, BuiltPipeline};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use std::path::PathBuf;
use tokio::sync::mpsc;

pub(in crate::recording) struct PreparedGifWaylandRecording {
    final_path: PathBuf,
    temp_path: PathBuf,
    config: RecordingConfig,
    backend: BuiltPipeline,
}

pub(in crate::recording) async fn record_gif_rust_with_commands(
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

/// GIF recording on Wayland: record with the same PipeWire -> video path used by
/// normal video recording, then convert the temporary video to GIF. This avoids
/// the GNOME/portal raw-frame path producing a visually static GIF while video
/// recording works correctly on the same system.
async fn record_gif_wayland_native(
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    let prepared = prepare_gif_wayland_recording(config).await?;
    record_prepared_gif_wayland_native(prepared, command_rx).await
}

pub(in crate::recording) async fn prepare_gif_wayland_recording(
    config: RecordingConfig,
) -> RecordResult<PreparedGifWaylandRecording> {
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

pub(in crate::recording) async fn record_prepared_gif_wayland_native(
    prepared: PreparedGifWaylandRecording,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::process::Command;

    let PreparedGifWaylandRecording {
        final_path,
        temp_path,
        config,
        backend,
    } = prepared;

    let (_recorded_path, stop_action) =
        start_recording_with_prepared_backend(backend, command_rx).await?;

    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(&final_path);
        return Ok((final_path, stop_action));
    }

    if stop_action == RecordingTerminalAction::Restart {
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
    notify_daemon_event("recording_session_ended");

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
    config: RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Build X11 GIF pipeline: ximagesrc -> videoconvert -> rgba -> appsink
    let source_str = super::get_x11_source(&config)?;
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
