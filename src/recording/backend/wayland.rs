use super::*;
use gstreamer::glib;

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
) -> super::RecordResult<super::RecordedSession> {
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
    // Native SPA bytes — no channel swap (see PipeWireCapture::pix_fmt).
    let pix_fmt = capture.pix_fmt();
    let stream_header = super::timestamped_video::header(input_width, input_height, fps, pix_fmt)?;

    // Start audio only after the video capture stream is negotiated. This
    // keeps the encoded audio timeline aligned with the first video frame.
    let mut active_audio = if config.mic_enabled || config.speaker_enabled {
        start_gst_audio_or_fallback(config, audio_muxer)
    } else {
        None
    };

    // Build ffmpeg command
    // Encoder preference: NVENC CQP > VAAPI QP > x264 CRF. All auto when HW present.
    let use_nvenc = super::wf_recorder::should_use_nvenc()
        && encoder_name != "libvpx-vp9"
        && encoder_name != "libvpx"
        && encoder_name != "libtheora";
    let use_vaapi = !use_nvenc && super::wf_recorder::should_use_vaapi();
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats");

    ffmpeg_cmd.arg("-f").arg("matroska").arg("-i").arg("pipe:0");

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

    // One line describing the actual video path (encoder + quality + sizes).
    // Without this every "blurry recording" report is blind guessing.
    let mut video_desc = String::from("unknown");

    if use_nvenc {
        // NVENC recording settings: RC=CQP, profile high.
        let filter = wayland_video_filter(config.max_resolution);
        let (fit_w, fit_h) =
            super::fit_within_max_resolution(input_width, input_height, config.max_resolution);
        let cqp = config
            .crf
            .saturating_sub(super::crf_resolution_reduction(fit_w, fit_h));
        ffmpeg_cmd
            .arg("-vf")
            .arg(super::timestamped_video::cfr_filter(fps, &filter))
            .arg("-c:v")
            .arg("h264_nvenc")
            .arg("-rc")
            .arg("constqp")
            .arg("-qp")
            .arg(cqp.to_string())
            .arg("-profile:v")
            .arg("high")
            .arg("-preset")
            .arg("p5")
            .arg("-tune")
            .arg("hq")
            .arg("-multipass")
            .arg("qres")
            // Lookahead (8 frames) with adaptive I/B frames; without it motion
            // re-allocates bits less smoothly.
            .arg("-rc-lookahead")
            .arg("8")
            .arg("-spatial-aq")
            .arg("1")
            .arg("-temporal-aq")
            .arg("1")
            .arg("-bf")
            .arg("2")
            .arg("-g")
            .arg((fps * 2).to_string())
            // Match the bt709 conversion in the filter; untagged NVENC output
            // makes players guess, which can wash the image out.
            .arg("-color_range")
            .arg("tv")
            .arg("-colorspace")
            .arg("bt709")
            .arg("-color_primaries")
            .arg("bt709")
            .arg("-color_trc")
            .arg("bt709")
            // ffmpeg's nvenc wrapper drops transfer/primaries from the VUI.
            // Players then guess wrong and lift blacks (cloudy playback), so
            // rewrite the H.264 VUI so all four fields are tagged bt709/tv.
            .arg("-bsf:v")
            .arg("h264_metadata=video_full_range_flag=0:colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1");
        video_desc = format!("h264_nvenc CQP{cqp} preset=p5 tune=hq multipass=qres AQ");
    } else if use_vaapi {
        let (vaapi_width, vaapi_height) =
            fit_within_max_resolution(input_width, input_height, config.max_resolution);
        // Tier-derived QP like the other encoders (was a fixed 20/24).
        let qp = config
            .crf
            .saturating_sub(super::crf_resolution_reduction(vaapi_width, vaapi_height));
        let vaapi_args = super::wf_recorder::ffmpeg_vaapi_args(vaapi_width, vaapi_height, qp);
        video_desc = format!("h264_vaapi CQP qp{qp} {vaapi_width}x{vaapi_height} profile=high");
        for (index, arg) in vaapi_args.iter().enumerate() {
            if index > 0 && vaapi_args[index - 1] == "-vf" {
                ffmpeg_cmd.arg(super::timestamped_video::cfr_filter(fps, arg));
            } else {
                ffmpeg_cmd.arg(arg);
            }
        }
        ffmpeg_cmd.arg("-g").arg((fps * 2).to_string());
        ffmpeg_cmd
            .arg("-color_range")
            .arg("tv")
            .arg("-colorspace")
            .arg("bt709")
            .arg("-color_primaries")
            .arg("bt709")
            .arg("-color_trc")
            .arg("bt709")
            // Same VUI fix as NVENC: vaapi does not emit transfer/primaries.
            .arg("-bsf:v")
            .arg("h264_metadata=video_full_range_flag=0:colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1");
    } else {
        // Convert desktop RGBA (full-range RGB) to standard limited-range
        // YUV420P for broad MP4/player compatibility. Tagging H.264 as full
        // range can make some Linux players display lifted blacks / a washed
        // layer, so use normal video range while preserving correct RGB input.
        let filter = wayland_video_filter(config.max_resolution);
        ffmpeg_cmd
            .arg("-vf")
            .arg(super::timestamped_video::cfr_filter(fps, &filter))
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
            ffmpeg_cmd.arg("-profile:v").arg("high");
            ffmpeg_cmd.arg("-g").arg((fps * 2).to_string());
            ffmpeg_cmd.arg("-bf").arg("2");
            // Resolution-compensated CRF: the tier base minus the output-size
            // reduction, so capped resolutions stay sharp.
            let (fit_w, fit_h) =
                super::fit_within_max_resolution(input_width, input_height, config.max_resolution);
            let crf = config
                .crf
                .saturating_sub(super::crf_resolution_reduction(fit_w, fit_h));
            ffmpeg_cmd.arg("-crf").arg(crf.to_string());
            video_desc = format!("libx264 CRF{crf} preset=veryfast profile=high");
            // x264 sets VUI color metadata from its own params, ignoring
            // ffmpeg's -color_* output options — pass them explicitly so the
            // transfer/primaries tags actually land in the file. Strict
            // players otherwise guess, which can wash the image out.
            ffmpeg_cmd
                .arg("-x264-params")
                .arg("colorprim=bt709:transfer=iec61966-2-1:colormatrix=bt709");
        } else if encoder_name == "libopenh264" {
            // Fedora ffmpeg-free ships OpenH264 (no libx264). CRF is not
            // supported; use a solid CBR-ish bitrate for desktop capture.
            ffmpeg_cmd.arg("-b:v").arg("8M");
            ffmpeg_cmd.arg("-maxrate").arg("10M");
            ffmpeg_cmd.arg("-bufsize").arg("16M");
            ffmpeg_cmd.arg("-allow_skip_frames").arg("0");
            video_desc = String::from("libopenh264 CBR 8M");
        } else if encoder_name == "libvpx-vp9" || encoder_name == "libvpx" {
            // File-recording quality, not streaming realtime: deadline good,
            // cpu-used 2, CQ 20 (was realtime/6/CRF 32 = blurry).
            ffmpeg_cmd.arg("-b:v").arg("0");
            ffmpeg_cmd.arg("-crf").arg("20");
            ffmpeg_cmd.arg("-deadline").arg("good");
            ffmpeg_cmd.arg("-cpu-used").arg("2");
            video_desc = format!("{encoder_name} CQ20 deadline=good cpu-used=2");
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

    ffmpeg_cmd
        .arg("-fps_mode")
        .arg("cfr")
        .arg("-r")
        .arg(fps.to_string());

    if final_path.extension().is_some_and(|e| e == "mp4") {
        ffmpeg_cmd.arg("-movflags").arg("+faststart");
    }

    eprintln!(
        "[recording] video: {video_desc} | pipe {input_width}x{input_height} {pix_fmt} @ {fps}fps -> {}",
        final_path.display()
    );

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
    let mut dropped_frames = 0u64;
    let mut last_drop_log = std::time::Instant::now();
    let mut first_written_at: Option<std::time::Instant> = None;
    let mut last_pixels: Option<std::sync::Arc<Vec<u8>>> = None;
    let mut last_encoded_pixels = None;
    let mut pending_frame_at_us = None;
    let mut paused = false;
    let mut timeline = super::timeline::RecordingTimeline::default();
    if active_audio.is_none() && (config.mic_enabled || config.speaker_enabled) {
        timeline.retain_live_audio_pauses();
        eprintln!("[recording] Live Pulse audio fallback retains frozen pause intervals on the media clock");
    }
    let mut last_frame_at_us = None;
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
                    timeline.pause(glib::monotonic_time());
                    println!("Recording paused");
                    if let Some(audio) = active_audio.as_mut() {
                        audio.set_paused(true);
                    }
                    paused = true;
                    super::notify_daemon_event("recording_session_paused");
                }
                RecordingControlCommand::Resume if paused => {
                    timeline.resume(glib::monotonic_time());
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

        // Gate acquisition on cadence (one frame per tick): acquiring
        // and converting a full RGBA frame on every 1ms spin burns hundreds
        // of MiB/s of copies for frames that are overwritten before use.
        let now = std::time::Instant::now();
        if let Some(deadline) = next_frame_at {
            if now < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
        }

        match capture.try_recv_frame() {
            Ok(Some(frame)) => {
                let captured_at_us = frame.captured_at_us;
                let expected = input_width as usize * input_height as usize * 4;
                let pixels = if let Some(crop) = crop {
                    crop_rgba_frame(&frame, crop, input_width, input_height).ok()
                } else if frame.pixels.len() == expected {
                    Some(frame.pixels)
                } else {
                    None
                };
                match pixels {
                    Some(pixels) if pixels.len() == expected => {
                        last_pixels = Some(std::sync::Arc::new(pixels));
                        pending_frame_at_us = Some(captured_at_us);
                    }
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
        let Some(frame_at_us) = pending_frame_at_us else {
            next_frame_at = Some(std::time::Instant::now() + frame_interval);
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

        if now.saturating_duration_since(deadline) > frame_interval * 2 {
            dropped_frames += 1;
            if last_drop_log.elapsed() > std::time::Duration::from_secs(5) {
                eprintln!(
                    "[recording] encoder lagging; dropped {dropped_frames} backlog frame(s) to stay realtime"
                );
                dropped_frames = 0;
                last_drop_log = std::time::Instant::now();
            }
            next_frame_at = Some(now + frame_interval);
            continue;
        }

        // Debug: APEXSHOT_DUMP_FRAME=/tmp/frame keeps the first fed frame for
        // capture-vs-encode diagnosis (raw bytes + .info with WxH/pixfmt).
        if frames_written == 0 {
            if let Ok(path) = std::env::var("APEXSHOT_DUMP_FRAME") {
                if !path.is_empty() {
                    let _ = std::fs::write(&path, pixels.as_slice());
                    let _ = std::fs::write(
                        format!("{path}.info"),
                        format!("{input_width}x{input_height} {pix_fmt} fps={fps} {video_desc}"),
                    );
                    eprintln!("[recording] dumped first frame to {path}");
                }
            }
        }

        if last_frame_at_us.is_some_and(|previous| frame_at_us <= previous) {
            pending_frame_at_us = None;
            next_frame_at = Some(now + frame_interval);
            continue;
        }
        let pts_us = timeline.frame_time_us(frame_at_us);
        let frame_header = super::timestamped_video::frame_header(pts_us, pixels.len());
        let initial_header = if frames_written == 0 {
            stream_header.as_slice()
        } else {
            &[]
        };
        match write_ffmpeg_frame_interruptible(
            &mut stdin,
            &[initial_header, &frame_header, pixels],
            &mut FfmpegFrameControl {
                command_rx: &mut command_rx,
                stop_action: &mut stop_action,
                paused: &mut paused,
                active_audio: &mut active_audio,
                timeline: &mut timeline,
                notify: super::notify_daemon_event,
            },
        ) {
            Ok(keep_recording) => {
                if !keep_recording && stop_action != super::RecordingTerminalAction::Save {
                    break;
                }
                frames_written += 1;
                last_frame_at_us = Some(frame_at_us);
                last_encoded_pixels = Some(std::sync::Arc::clone(pixels));
                pending_frame_at_us = None;
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
                    eprintln!(
                        "[recording] media origin monotonic_us={frame_at_us} pts_us={pts_us}"
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
    let stopped_at_us = glib::monotonic_time();
    timeline.finish(stopped_at_us);

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
        if stop_action != super::RecordingTerminalAction::Save {
            audio.abort();
        } else {
            audio.stop();
        }
    }
    drop(audio_exclusive.take());

    if stop_action == super::RecordingTerminalAction::Save {
        if let (Some(pixels), Some(last_frame_at_us)) = (&last_encoded_pixels, last_frame_at_us) {
            let end_pts = timeline.frame_time_us(stopped_at_us);
            let tail_pts = end_pts.saturating_sub(1_000_000 / u64::from(fps));
            let last_pts = timeline.frame_time_us(last_frame_at_us);
            if tail_pts > last_pts {
                let tail_header = super::timestamped_video::frame_header(tail_pts, pixels.len());
                if let Err(err) = write_ffmpeg_frame_interruptible(
                    &mut stdin,
                    &[&tail_header, pixels],
                    &mut FfmpegFrameControl {
                        command_rx: &mut command_rx,
                        stop_action: &mut stop_action,
                        paused: &mut paused,
                        active_audio: &mut active_audio,
                        timeline: &mut timeline,
                        notify: super::notify_daemon_event,
                    },
                ) {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stderr_reader.join();
                    return Err(err.into());
                }
            }
        }
    }

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
    if stop_action != super::RecordingTerminalAction::Save {
        let _ = child.kill();
    }

    let status = wait_for_ffmpeg_child(&mut child)?;
    let ffmpeg_stderr = stderr_reader.join().unwrap_or_default();

    if stop_action != super::RecordingTerminalAction::Save {
        let _ = std::fs::remove_file(&final_path);
        return Ok(super::RecordedSession {
            path: final_path,
            action: stop_action,
            timeline: Some(timeline),
        });
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

    Ok(super::RecordedSession {
        path: final_path,
        action: stop_action,
        timeline: Some(timeline),
    })
}
