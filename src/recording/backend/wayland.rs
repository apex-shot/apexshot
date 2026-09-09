use super::*;

/// Try to start the GStreamer audio capture (mic + speaker monitor mixed and
/// encoded in-process). `None` means the caller should keep the legacy
/// ffmpeg-pulse audio inputs — only when GStreamer plugins are missing or
/// the pipeline cannot produce samples.
fn start_gst_audio_or_fallback(
    config: &super::RecordingConfig,
    muxer: &str,
) -> Option<super::gst_audio::ActiveGstAudio> {
    use super::gst_audio::{audio_available, ActiveGstAudio, AudioTermination, GstAudioSetup};

    let setup = GstAudioSetup::from_recording(config)?;
    if !audio_available(muxer, AudioTermination::AppSink, setup.noise_suppression) {
        eprintln!("[recording] GStreamer audio unavailable; using ffmpeg pulse inputs");
        return None;
    }
    match ActiveGstAudio::start(&setup, muxer) {
        Ok(audio) => {
            println!("Recording audio via GStreamer (mic/monitor mix, unified pipeline)");
            Some(audio)
        }
        Err(err) => {
            eprintln!("[recording] GStreamer audio first start failed ({err}); retrying");
            std::thread::sleep(std::time::Duration::from_millis(250));
            match ActiveGstAudio::start(&setup, muxer) {
                Ok(audio) => {
                    println!("Recording audio via GStreamer (mic/monitor mix, unified pipeline)");
                    Some(audio)
                }
                Err(err) => {
                    eprintln!(
                        "[recording] GStreamer audio failed to start ({err}); using ffmpeg pulse inputs"
                    );
                    None
                }
            }
        }
    }
}

/// Wayland recording: native PipeWire frame capture + ffmpeg pipe for encoding.
pub(in crate::recording) fn record_wayland_with_ffmpeg_sync(
    mut wayland_source: WaylandSource,
    final_path: &std::path::Path,
    encoder_name: &str,
    encoder_props: &str,
    audio_muxer: &str,
    config: &super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let final_path = final_path.to_path_buf();

    // Initialize GStreamer before PipeWire so the libraries do not fight over
    // spa/libpipewire startup. Do not start audio yet: starting it here lets
    // audio run while the portal/PipeWire stream is still negotiating, which
    // makes the saved audio track longer than the actual video timeline.
    let _ = super::gst_audio::ensure_gst_initialized();
    let mut audio_exclusive = Some(RecordingAudioExclusiveGuard::acquire(
        config.mic_enabled || config.speaker_enabled,
    ));
    if config.mic_enabled || config.speaker_enabled {
        super::audio::ensure_pipewire_pulse_running();
    }

    // Open PipeWire capture stream (continuous).
    // Portal path: connect via the remote FD from OpenPipeWireRemote.
    // KDE-native path: node lives on the default session socket.
    // Negotiate against the portal/KWin stream size, not the area crop.
    // Feeding the crop as VideoSize makes GNOME emit a buffer that does not
    // match the negotiated format, which used to panic in convert_to_rgba_frame.
    let hint_w = (wayland_source.stream_width > 0).then_some(wayland_source.stream_width);
    let hint_h = (wayland_source.stream_height > 0).then_some(wayland_source.stream_height);
    let capture = match match wayland_source.pipewire_fd {
        Some(fd) => crate::pipewire_engine::PipeWireCapture::connect(
            fd,
            wayland_source.node_id,
            None, // continuous — no max frame limit
            hint_w,
            hint_h,
        ),
        None => crate::pipewire_engine::PipeWireCapture::connect_default(
            wayland_source.node_id,
            None,
            hint_w,
            hint_h,
        ),
    } {
        Ok(capture) => capture,
        Err(e) => {
            return Err(RecordError::GStreamerError(format!(
                "PipeWire capture failed: {e}"
            )));
        }
    };
    // Declared after `capture` so unwind/error cleanup closes the exclusive
    // portal session before attempting native PipeWire teardown.
    let mut capture_session = wayland_source.session.take();

    let format = capture.format().ok_or_else(|| {
        RecordError::GStreamerError("No format negotiated before recording".into())
    })?;

    let crop = wayland_source.crop.and_then(|crop| {
        scale_crop_to_frame(
            crop,
            wayland_source.stream_width,
            wayland_source.stream_height,
            format.width,
            format.height,
        )
    });
    let (input_width, input_height) = match crop {
        Some(crop) => {
            let (width, height) = even_crop_output(crop, format.width, format.height);
            eprintln!(
                "[recording] Applying Wayland area crop: left={} top={} => {}x{}",
                crop.left, crop.top, width, height
            );
            (width, height)
        }
        None => (format.width.max(2) & !1, format.height.max(2) & !1),
    };
    let fps = config.fps.max(1);

    // Start audio only after the video capture stream is negotiated. This
    // keeps the encoded audio timeline aligned with the first video frame.
    let mut active_audio = if config.mic_enabled || config.speaker_enabled {
        start_gst_audio_or_fallback(config, audio_muxer)
    } else {
        None
    };

    // Build ffmpeg command
    let use_vaapi = super::wf_recorder::should_use_vaapi();
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats");

    ffmpeg_cmd
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-s")
        .arg(format!("{}x{}", input_width, input_height))
        .arg("-framerate")
        .arg(fps.to_string())
        // Rawvideo normally numbers submitted frames consecutively. At large
        // resolutions encoder backpressure may allow only a few submissions
        // per second, producing a sub-second video inside a much longer audio
        // file. Timestamp frames when FFmpeg receives them so sparse writes
        // retain the real recording duration.
        .arg("-use_wallclock_as_timestamps")
        .arg("1")
        .arg("-i")
        .arg("pipe:0");

    // Add every input before filters, codecs, maps, and other output options.
    // FFmpeg otherwise applies an option such as -vf to the following audio
    // input and exits with AVERROR(EINVAL) (234).
    let gst_audio_fd = active_audio.as_mut().and_then(|audio| {
        let format = audio.input_format();
        audio.take_read_fd().map(|fd| (format, fd))
    });
    if let Some((format, read_fd)) = gst_audio_fd {
        // GStreamer path: encoded audio arrives on an inherited fd as a second
        // input. Both inputs EOF deterministically (pipe writer closes on EOS),
        // so no -shortest hack is needed.
        ffmpeg_cmd.arg("-f").arg(format);
        ffmpeg_cmd.arg("-i").arg("pipe:3");
        attach_audio_pipe_as_fd3(&mut ffmpeg_cmd, read_fd);
    } else if config.mic_enabled || config.speaker_enabled {
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
    }

    if use_vaapi {
        let (vaapi_width, vaapi_height) =
            fit_within_max_resolution(input_width, input_height, config.max_resolution);
        let vaapi_args = super::wf_recorder::ffmpeg_vaapi_args(vaapi_width, vaapi_height);
        for arg in &vaapi_args {
            ffmpeg_cmd.arg(arg);
        }
    } else {
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

    // Map audio after all inputs have been declared.
    if config.mic_enabled || config.speaker_enabled {
        if active_audio.is_some() {
            // Encoded audio is muxed as-is; mono is handled by the GStreamer
            // branch caps, so no channel conversion happens here.
            ffmpeg_cmd.arg("-map").arg("0:v");
            ffmpeg_cmd.arg("-map").arg("1:a");
            ffmpeg_cmd.arg("-c:a").arg("copy");
        } else {
            // Legacy pulse inputs: ffmpeg captures from PulseAudio directly;
            // on modern GNOME this is provided by pipewire-pulse.
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
            // Pulse sources never EOF. Without this, closing the video pipe
            // leaves ffmpeg blocked on mic/speaker input and Stop never
            // finishes. (The GStreamer pipe path EOFs deterministically.)
            ffmpeg_cmd.arg("-shortest");
        }
    }

    // Convert wall-clock-spaced input timestamps to the configured constant
    // frame rate inside FFmpeg. This duplicates the latest decoded frame
    // without pushing hundreds of MiB/s of repeated RGBA data through stdin.
    ffmpeg_cmd
        .arg("-fps_mode")
        .arg("cfr")
        .arg("-r")
        .arg(fps.to_string());

    ffmpeg_cmd.arg(&final_path);
    ffmpeg_cmd.stdin(Stdio::piped());
    ffmpeg_cmd.stdout(Stdio::null());
    ffmpeg_cmd.stderr(Stdio::piped());

    let mut child = match ffmpeg_cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            if let Some(mut audio) = active_audio.take() {
                audio.abort();
            }
            return Err(RecordError::GStreamerError(format!(
                "Failed to spawn ffmpeg: {e}"
            )));
        }
    };

    let mut stdin = child.stdin.take().expect("stdin should be piped");
    set_child_stdin_nonblocking(&stdin).map_err(|e| {
        let _ = child.kill();
        let _ = child.wait();
        RecordError::GStreamerError(format!("Failed to make ffmpeg input cancellable: {e}"))
    })?;
    let mut stderr = child.stderr.take().expect("stderr should be piped");
    let stderr_reader = std::thread::spawn(move || {
        const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    output.extend_from_slice(&buffer[..read]);
                    if output.len() > MAX_DIAGNOSTIC_BYTES {
                        let excess = output.len() - MAX_DIAGNOSTIC_BYTES;
                        output.drain(..excess);
                    }
                }
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    });

    println!("Recording (native PipeWire + ffmpeg) to {:?}", final_path);

    // Recording loop
    let mut command_rx = command_rx;
    let mut stop_action = super::RecordingTerminalAction::Save;
    let mut frames_written = 0u64;
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_frame_at: Option<std::time::Instant> = None;
    let mut first_written_at: Option<std::time::Instant> = None;
    let mut last_pixels: Option<Vec<u8>> = None;
    let mut paused = false;
    let first_frame_deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);

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
                    if let Some(audio) = active_audio.as_mut() {
                        audio.set_paused(true);
                    }
                    paused = true;
                    super::notify_daemon_event("recording_session_paused");
                }
                RecordingControlCommand::Resume if paused => {
                    println!("Recording resumed");
                    if let Some(audio) = active_audio.as_mut() {
                        audio.set_paused(false);
                    }
                    paused = false;
                    next_frame_at = None; // don't skip the first frame
                    super::notify_daemon_event("recording_session_resumed");
                }
                _ => {}
            }
        }

        // An audio pipeline error truncates the audio track but must not kill
        // an otherwise healthy video recording.
        if let Some(err) = active_audio.as_ref().and_then(|a| a.poll_bus_error()) {
            eprintln!("[recording] GStreamer audio error: {err}; continuing without audio");
            if let Some(mut audio) = active_audio.take() {
                audio.abort();
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
        // Take at most one buffer per tick — spinning the queue can recycle a
        // PipeWire buffer into a black frame.
        match capture.try_recv_frame() {
            Ok(Some(frame)) => {
                let expected = input_width as usize * input_height as usize * 4;
                let pixels = if let Some(crop) = crop {
                    crop_rgba_frame(&frame, crop, input_width, input_height).ok()
                } else if frame.pixels.len() == expected {
                    Some(frame.pixels)
                } else {
                    None
                };
                match pixels {
                    Some(pixels) if pixels.len() == expected => last_pixels = Some(pixels),
                    Some(pixels) => {
                        eprintln!(
                            "[recording] Skipping frame with {} bytes (expected {expected})",
                            pixels.len()
                        );
                    }
                    None => {
                        eprintln!("[recording] Skipping frame that failed area crop");
                    }
                }
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
            if std::time::Instant::now() > first_frame_deadline {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                if let Some(mut audio) = active_audio.take() {
                    audio.abort();
                }
                return Err(RecordError::GStreamerError(
                    "Recording produced no frames. The screen share ended before capture started."
                        .into(),
                ));
            }
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

        // A nonblocking pipe keeps stop/discard responsive even if ffmpeg
        // stalls and stops consuming raw frames.
        match write_ffmpeg_frame_interruptible(
            &mut stdin,
            pixels,
            &mut command_rx,
            &mut stop_action,
            &mut paused,
            &mut active_audio,
        ) {
            Ok(keep_recording) => {
                frames_written += 1;
                if first_written_at.is_none() {
                    first_written_at = Some(std::time::Instant::now());
                }
                next_frame_at = Some(deadline + frame_interval);
                if frames_written == 1 {
                    let max_rgb = pixels
                        .chunks_exact(4)
                        .map(|px| px[0].max(px[1]).max(px[2]))
                        .max()
                        .unwrap_or(0);
                    eprintln!(
                        "[recording] First frame written to ffmpeg ({} bytes, max_rgb={max_rgb})",
                        pixels.len()
                    );
                    if max_rgb == 0 {
                        eprintln!(
                            "[recording] First frame is fully black — capture buffer is empty"
                        );
                    }
                }
                if frames_written.is_multiple_of(30) {
                    eprintln!("[recording] {} frames written", frames_written);
                }
                if !keep_recording {
                    break;
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::BrokenPipe {
                    eprintln!("ffmpeg pipe broken (likely exited)");
                } else {
                    eprintln!("Failed to write to ffmpeg: {e}");
                }
                break;
            }
        }
    }

    let stopped_at = std::time::Instant::now();

    // Release the exclusive GNOME ScreenCast session before audio drain or
    // ffmpeg finalization. Those can block for seconds; the compositor must
    // not stay captured while they finish. Drop it *before* unlocking the
    // busy flag so a new recording cannot open a second portal on top of this
    // one.
    drop(capture_session.take());
    drop(capture);

    if matches!(
        stop_action,
        super::RecordingTerminalAction::Save | super::RecordingTerminalAction::Discard
    ) {
        // Tray / hotkeys / screenshots become available immediately. ffmpeg
        // keeps writing the file in this worker.
        super::control_session::release_recording_busy();
        super::notify_daemon_event("recording_session_ended");
    }

    // Stop audio at the same wall-clock instant as video capture. Do not keep
    // the mic open while ffmpeg drains — that would grow the audio track past
    // the session.
    if let Some(mut audio) = active_audio.take() {
        if stop_action == super::RecordingTerminalAction::Discard {
            audio.abort();
        } else {
            audio.stop();
        }
    }
    drop(audio_exclusive.take());

    if let (Some(start), true) = (
        first_written_at,
        stop_action == super::RecordingTerminalAction::Save,
    ) {
        let wall = stopped_at.saturating_duration_since(start).as_secs_f64();
        eprintln!(
            "[recording] duration wall={wall:.3}s submitted_frames={frames_written} output_fps={fps}"
        );
    }

    // Deterministic encoder stop: close video stdin so ffmpeg finalizes once
    // both inputs are at EOF (audio already EOFed above).
    drop(stdin);

    let status = wait_for_ffmpeg_child(&mut child)?;
    let ffmpeg_stderr = stderr_reader.join().unwrap_or_default();

    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&final_path);
        return Ok((final_path, stop_action));
    }

    if frames_written == 0 {
        let _ = std::fs::remove_file(&final_path);
        return Err(RecordError::GStreamerError(
            "Recording produced no frames. The screen share ended before capture started.".into(),
        ));
    }

    if !status.success() {
        let keep_output = std::fs::metadata(&final_path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false);
        if !keep_output {
            let _ = std::fs::remove_file(&final_path);
            let detail = ffmpeg_error_detail(&ffmpeg_stderr);
            return Err(RecordError::GStreamerError(format!(
                "ffmpeg failed to encode the recording with {encoder_name} (exit {status}).{detail}"
            )));
        }
        eprintln!("[recording] ffmpeg exited {status}; keeping output because it already has data");
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
