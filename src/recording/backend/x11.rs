use super::super::timeline::RecordingTimeline;
use super::super::{
    gst_audio, notify_daemon_event, RecordError, RecordResult, RecordingConfig,
    RecordingControlCommand, RecordingTerminalAction,
};
use super::fit_within_max_resolution;
use super::profile::{video_encoder_props, EncoderProfile};
use super::session::RecordingAudioExclusiveGuard;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer::glib;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use x11rb::connection::Connection;

use crate::backend::x11_window_pick::{crop_for_rect, X11WindowFollower, X11WindowSource};

/// Bound on handing EOS to the pipeline. A wedged source streaming thread can
/// hold the pipeline's stream lock, so a synchronous `send_event` may never
/// return; the send runs on a blocking thread and is abandoned after this.
const STOP_EOS_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Bound on the final NULL transition, which can block the same way.
const TEARDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// How often a window recording re-reads the window's visible rectangle.
const WINDOW_CROP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Best-effort audio for the X11 path: the shared GStreamer audio bin linked
/// into the video muxer. Audio is a new capability here — any setup failure
/// falls back to the historical video-only recording instead of failing.
fn build_x11_audio_bin(
    config: &mut RecordingConfig,
    profile: &EncoderProfile,
) -> Option<gst_audio::GstAudioBin> {
    use gst_audio::{
        audio_available, build_audio_bin, encoder_for_muxer, AudioTermination, GstAudioSetup,
    };

    let inputs = super::super::audio::resolve_available_audio_inputs(config);
    let setup = GstAudioSetup::from_recording(config, inputs)?;
    if !audio_available(
        profile.muxer,
        AudioTermination::GhostPad,
        setup.noise_suppression,
        !setup.app_streams.is_empty(),
    ) {
        eprintln!("[recording] X11 audio unavailable: GStreamer audio elements missing");
        return None;
    }
    let encoder = encoder_for_muxer(profile.muxer)?;
    match build_audio_bin(&setup, encoder, AudioTermination::GhostPad) {
        Ok(bin) => {
            println!("Recording audio via GStreamer (X11, first-class audio track)");
            Some(bin)
        }
        Err(err) => {
            eprintln!("[recording] X11 audio skipped: {err}");
            None
        }
    }
}

/// Polls the picked window's visible rectangle and re-crops the live capture.
/// The crop starts at the pick-time rectangle; a window that goes away keeps
/// the last one.
struct WindowCropWatcher {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl WindowCropWatcher {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for WindowCropWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The crop to apply for a newly seen window rectangle, or `None` when it
/// matches the crop already set.
fn crop_update(
    last: (u32, u32, u32, u32),
    rect: (i32, i32, u32, u32),
    screen: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let next = crop_for_rect(rect, screen);
    (next != last).then_some(next)
}

fn start_window_crop_watcher(
    source: X11WindowSource,
    pipeline: &gst::Pipeline,
) -> Option<WindowCropWatcher> {
    let follower = match X11WindowFollower::new(&source) {
        Ok(follower) => follower,
        Err(err) => {
            eprintln!("[recording] X11 window resize tracking unavailable: {err}");
            return None;
        }
    };
    let screen = follower.screen_size();
    let pipeline = pipeline.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let join = std::thread::Builder::new()
        .name("apexshot-x11-crop".into())
        .spawn(move || {
            // Start from the crop the pipeline was built with.
            let mut last = crop_for_rect(
                (source.x, source.y, source.width, source.height),
                (source.screen_width, source.screen_height),
            );
            while !stop_flag.load(Ordering::SeqCst) {
                std::thread::sleep(WINDOW_CROP_POLL);
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                let Some(rect) = follower.visible_rect() else {
                    continue;
                };
                let Some(next) = crop_update(last, rect, screen) else {
                    continue;
                };
                let Some(wcrop) = pipeline.by_name("wcrop") else {
                    continue;
                };
                wcrop.set_property("left", next.0 as i32);
                wcrop.set_property("top", next.1 as i32);
                wcrop.set_property("right", next.2 as i32);
                wcrop.set_property("bottom", next.3 as i32);
                last = next;
            }
        });
    match join {
        Ok(join) => Some(WindowCropWatcher {
            stop,
            join: Some(join),
        }),
        Err(err) => {
            eprintln!("[recording] failed to spawn the X11 window crop watcher: {err}");
            None
        }
    }
}

/// X11 fallback recording using GStreamer ximagesrc.
/// Preserved from the previous implementation for backward compatibility,
/// now with optional audio from the shared GStreamer audio bin.
#[allow(unused_assignments)]
pub(super) async fn record_x11_with_gstreamer(
    config: &mut RecordingConfig,
    profile: &EncoderProfile,
    final_path: &Path,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction, Option<RecordingTimeline>)> {
    let _audio_exclusive =
        RecordingAudioExclusiveGuard::acquire(config.mic_enabled || config.speaker_enabled);
    gst::init().map_err(|e| RecordError::InitError(e.to_string()))?;

    let audio_bin = build_x11_audio_bin(config, profile);
    let pipeline_str = build_x11_gstreamer_pipeline(
        config,
        profile,
        final_path,
        audio_bin.is_some(),
        x11_source_size(config),
    )?;
    println!("Starting recording (GStreamer X11) to: {:?}", final_path);
    println!("Pipeline: {}", pipeline_str);

    let pipeline = gst::parse::launch(&pipeline_str)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to parse pipeline: {}", e)))?
        .downcast::<gst::Pipeline>()
        .map_err(|_| RecordError::GStreamerError("Cast to Pipeline failed".into()))?;

    // A one-app branch's pipewiresrc offers the PipeWire clock; Pulse sources
    // are told not to. Keep the recording on the system clock either way.
    pipeline.use_clock(Some(&gst::SystemClock::obtain()));

    if let Some(audio) = audio_bin {
        let muxer = pipeline
            .by_name("mux")
            .ok_or_else(|| RecordError::GStreamerError("named muxer not found".into()))?;
        pipeline
            .add(&audio.bin)
            .map_err(|e| RecordError::GStreamerError(format!("Failed to add audio bin: {e}")))?;
        let audio_pad = muxer
            .request_pad_simple("audio_%u")
            .ok_or_else(|| RecordError::GStreamerError("no audio pad on muxer".into()))?;
        let ghost = audio
            .bin
            .static_pad("src")
            .ok_or_else(|| RecordError::GStreamerError("audio bin has no ghost pad".into()))?;
        ghost.link(&audio_pad).map_err(|e| {
            RecordError::GStreamerError(format!("Failed to link audio into muxer: {e:?}"))
        })?;
    }

    let mut disk_guard = super::disk_space::DiskSpaceGuard::new(final_path);
    if disk_guard.refuse_start() {
        let _ = pipeline.set_state(gst::State::Null);
        return Err(RecordError::GStreamerError(
            "Not enough free disk space to start recording (512 MB required)".into(),
        ));
    }

    // Stamp the media clock when the first buffer leaves the capture source.
    // The pointer sidecar is rebased onto this origin afterwards.
    let frame_origin_us = Arc::new(AtomicI64::new(0));
    if let Some(source) = pipeline.by_name("vsrc") {
        if let Some(pad) = source.static_pad("src") {
            let origin = Arc::clone(&frame_origin_us);
            let _ = pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                if origin.load(Ordering::SeqCst) == 0 {
                    origin.store(glib::monotonic_time(), Ordering::SeqCst);
                }
                gst::PadProbeReturn::Remove
            });
        }
    }

    // The controls bar is on screen over the captured area; the probe rebuilds
    // the pixels underneath it before the frame is converted, so the bar never
    // reaches the encoder.
    if super::super::x11_controls_mask::controls_window() != 0 {
        let capture_origin = match (config.x, config.y, config.width, config.height) {
            (Some(x), Some(y), Some(_), Some(_)) if config.x11_window.is_none() => (x, y),
            _ => (0, 0),
        };
        if let Some(masker) =
            super::super::x11_controls_mask::X11ControlsMasker::connect(capture_origin)
        {
            if let Some(source) = pipeline.by_name("vsrc") {
                if let Some(pad) = source.static_pad("src") {
                    // Pad probes are `Fn`, so the per-frame state lives behind
                    // a lock; the probe itself only runs on the streaming thread.
                    let masker = std::sync::Mutex::new(masker);
                    pad.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
                        if let Ok(mut masker) = masker.lock() {
                            masker.mask(pad, info);
                        }
                        gst::PadProbeReturn::Ok
                    });
                }
            }
        } else {
            eprintln!("[recording] X11 controls mask unavailable: no display connection");
        }
    }

    // Window recordings re-crop the screen as the window moves or resizes.
    // The watcher polls on its own X connection and stops before teardown.
    let mut crop_watcher = config
        .x11_window
        .and_then(|source| start_window_crop_watcher(source, &pipeline));

    pipeline
        .set_state(gst::State::Playing)
        .map_err(|e| RecordError::GStreamerError(format!("Failed to start pipeline: {}", e)))?;

    let bus = pipeline
        .bus()
        .ok_or_else(|| RecordError::GStreamerError("Pipeline has no bus".into()))?;

    let mut command_rx = command_rx;
    let mut stop_action = RecordingTerminalAction::Save;
    let mut stopping = false;
    let mut paused = false;
    let mut eos_received = false;
    let mut stopped_low_disk = false;
    let mut stop_us: Option<i64> = None;
    let mut timeline = RecordingTimeline::default();

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
                        stop_us = Some(glib::monotonic_time());
                        stop_action = RecordingTerminalAction::Restart;
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_us = Some(glib::monotonic_time());
                        stop_action = RecordingTerminalAction::Save;
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_us = Some(glib::monotonic_time());
                        stop_action = RecordingTerminalAction::Discard;
                        stopping = true;
                        break;
                    }
                    RecordingControlCommand::Pause if !paused => {
                        pipeline
                            .set_state(gst::State::Paused)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to pause pipeline: {e}")))?;
                        timeline.pause(glib::monotonic_time());
                        paused = true;
                        notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        if disk_guard.low() {
                            eprintln!(
                                "[recording] refusing to resume: disk space is almost full"
                            );
                            stopped_low_disk = true;
                            stop_action = RecordingTerminalAction::Save;
                            stopping = true;
                            break;
                        }
                        pipeline
                            .set_state(gst::State::Playing)
                            .map_err(|e| RecordError::GStreamerError(format!("Failed to resume pipeline: {e}")))?;
                        timeline.resume(glib::monotonic_time());
                        paused = false;
                        notify_daemon_event("recording_session_resumed");
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                for msg in bus.iter_timed(gst::ClockTime::ZERO) {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Eos(..) => {
                            eos_received = true;
                            stopping = true;
                            break;
                        }
                        MessageView::Error(err) => {
                            if let Some(watcher) = crop_watcher.as_mut() {
                                watcher.stop();
                            }
                            let _ = pipeline.set_state(gst::State::Null);
                            return Err(RecordError::GStreamerError(err.error().to_string()));
                        }
                        _ => (),
                    }
                }
                if stopping { break; }
                if let Some(true) = disk_guard.poll() {
                    eprintln!("[recording] stopping: disk space is almost full");
                    stopped_low_disk = true;
                    stop_action = RecordingTerminalAction::Save;
                    stopping = true;
                    break;
                }
            }
        }
    }

    // The watcher is stopped before any teardown so no property set races the
    // pipeline shutdown.
    if let Some(mut watcher) = crop_watcher.take() {
        watcher.stop();
    }
    if paused {
        let _ = pipeline.set_state(gst::State::Playing);
    }
    let stop_us = stop_us.unwrap_or_else(glib::monotonic_time);
    timeline.finish(stop_us);
    let origin_us = frame_origin_us.load(Ordering::SeqCst);
    let timeline = if origin_us > 0 {
        timeline.frame_time_us(origin_us);
        Some(timeline)
    } else {
        None
    };
    // EOS is handed to the pipeline off the async task, bounded by a timeout.
    let mut eos_send_failed = false;
    if stopping && !eos_received {
        eos_send_failed = !send_eos_bounded(&pipeline, STOP_EOS_SEND_TIMEOUT).await;
    }
    let mut stop_error: Option<String> = None;
    if eos_send_failed {
        // Without the EOS the file cannot be finalized, so a Save must fail
        // rather than report success. Discard and Restart do not keep the file.
        if stop_action == RecordingTerminalAction::Save {
            stop_error = Some("Timed out sending end-of-stream to the recording pipeline".into());
        }
    } else if stopping && !eos_received && stop_action == RecordingTerminalAction::Save {
        if let Err(err) = await_pipeline_eos(&bus, std::time::Duration::from_secs(10)).await {
            stop_error = Some(err);
        }
    }
    let teardown = set_state_null_bounded(&pipeline, TEARDOWN_TIMEOUT).await;
    if let Some(err) = stop_error {
        return Err(RecordError::GStreamerError(err));
    }
    teardown.map_err(|e| RecordError::GStreamerError(format!("Cleanup failed: {}", e)))?;

    if stopped_low_disk {
        crate::utils::notify::desktop_notification(
            &crate::i18n::t("Recording stopped"),
            &crate::i18n::t("Disk space is almost full"),
        );
    }

    if stop_action == RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(final_path);
    }

    Ok((final_path.to_path_buf(), stop_action, timeline))
}

async fn await_pipeline_eos(bus: &gst::Bus, timeout: std::time::Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("Timed out waiting for the recording to finish writing".into());
        }
        for msg in bus.iter_timed(gst::ClockTime::ZERO) {
            match msg.view() {
                gst::MessageView::Eos(..) => return Ok(()),
                gst::MessageView::Error(err) => return Err(err.error().to_string()),
                _ => {}
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

/// Run `work` on a detached OS thread and wait at most `timeout` for it.
/// Not `spawn_blocking`: dropping the tokio runtime waits for unfinished
/// blocking tasks, so a wedged GStreamer call there would still hang the
/// recorder at exit. A detached thread is simply abandoned.
async fn run_detached_bounded<T: Send + 'static>(
    name: &str,
    timeout: std::time::Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = tx.send(work());
        });
    if let Err(err) = spawned {
        eprintln!("[recording] failed to spawn {name}: {err}");
        return None;
    }
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(_)) => None,
        Err(_) => {
            eprintln!(
                "[recording] {name} did not return within {}s; abandoning it",
                timeout.as_secs()
            );
            None
        }
    }
}

/// Send EOS off the async task, bounded by `timeout`. Returns false when the
/// send times out; a timed-out send is left on its own thread.
async fn send_eos_bounded(pipeline: &gst::Pipeline, timeout: std::time::Duration) -> bool {
    let pipeline = pipeline.clone();
    run_detached_bounded("apexshot-x11-eos", timeout, move || {
        pipeline.send_event(gst::event::Eos::new())
    })
    .await
    .is_some()
}

/// Set the pipeline to NULL off the async task, bounded by `timeout`, so a
/// wedged pipeline cannot hang the recorder.
async fn set_state_null_bounded(
    pipeline: &gst::Pipeline,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let pipeline = pipeline.clone();
    match run_detached_bounded("apexshot-x11-teardown", timeout, move || {
        pipeline.set_state(gst::State::Null)
    })
    .await
    {
        Some(Ok(_)) => Ok(()),
        Some(Err(err)) => Err(err.to_string()),
        None => Err(format!(
            "pipeline did not shut down within {}s",
            timeout.as_secs()
        )),
    }
}

/// Build a GStreamer pipeline string for X11 capture (preserved from old code).
/// The muxer is named when audio is attached so the audio bin can request a pad.
///
/// `source_size` is what the capture feeds the encoder: the followed window's
/// pick-time size, the configured region, or the screen size for fullscreen.
/// With a `config.max_resolution` cap the pipeline scales to the
/// aspect-preserving fit via `fit_within_max_resolution` and pins exact even
/// dimensions (I420 cannot take odd sizes), e.g. 1920x1200 → 768x480 under the
/// 854x480 box, with `pixel-aspect-ratio=1/1` and never upscaling; an even
/// source already inside the box records untouched (no scale segment). When
/// the source size is unknown the historical caps range still applies the
/// ceiling. The raw caps pin `format=I420,colorimetry=bt709` so the encoder
/// input matches the Wayland backend's yuv420p + bt709 output (I420 alone
/// negotiates smpte170m), and a `h264parse` sits before the mp4mux because it
/// only accepts avc/avc3 while openh264enc (and byte-stream x264enc) emit
/// byte-stream. The scale segment must not sit adjacent to the framerate caps:
/// gst-parse rejects two consecutive caps filters.
///
/// A window recording reads the root window and crops it to the window's
/// current visible rectangle, then letterboxes the result into the fixed
/// pick-time (or capped) size. That crop-and-scale pair replaces the shared
/// scale stage so the output never carries a second, conflicting scale.
fn build_x11_gstreamer_pipeline(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    output_path: &Path,
    with_audio: bool,
    source_size: Option<(u32, u32)>,
) -> RecordResult<String> {
    let output_str = output_path.to_string_lossy();
    let video_source = get_x11_source(config)?;
    let video_raw_caps = format!(
        "video/x-raw,framerate={}/1,format=I420,colorimetry=bt709",
        config.fps
    );
    let (scale_segment, output_size) = match (config.max_resolution, source_size) {
        (max, Some((src_w, src_h))) => {
            let (mut target_w, mut target_h) = fit_within_max_resolution(src_w, src_h, max);
            target_w = (target_w & !1).max(2);
            target_h = (target_h & !1).max(2);
            if (target_w, target_h) == (src_w, src_h) {
                (String::new(), Some((src_w, src_h)))
            } else {
                (
                    format!(
                        " ! videoscale ! video/x-raw,width={target_w},height={target_h},pixel-aspect-ratio=1/1"
                    ),
                    Some((target_w, target_h)),
                )
            }
        }
        // Screen query failed: keep the historical never-upscale range so
        // the cap still applies; the output size stays unknown.
        (Some((max_w, max_h)), None) => (
            format!(
                " ! videoscale ! video/x-raw,width=[1,{max_w}],height=[1,{max_h}],pixel-aspect-ratio=1/1"
            ),
            None,
        ),
        (None, None) => (String::new(), None),
    };
    let (window_segment, scale_segment) = match config.x11_window {
        Some(window) => {
            let crop = crop_for_rect(
                (window.x, window.y, window.width, window.height),
                (window.screen_width, window.screen_height),
            );
            let (out_w, out_h) = output_size.unwrap_or((window.width, window.height));
            (
                format!(
                    " ! videocrop name=wcrop left={} top={} right={} bottom={} \
                     ! videoscale add-borders=true \
                     ! video/x-raw,width={out_w},height={out_h},pixel-aspect-ratio=1/1",
                    crop.0, crop.1, crop.2, crop.3
                ),
                String::new(),
            )
        }
        None => (String::new(), scale_segment),
    };
    let encoder_props = video_encoder_props(profile, config, output_size);
    let encoder_segment = if encoder_props.is_empty() {
        profile.encoder.to_string()
    } else {
        format!("{} {}", profile.encoder, encoder_props)
    };
    let parser_segment = if matches!(profile.encoder, "x264enc" | "openh264enc") {
        " ! h264parse"
    } else {
        ""
    };
    let muxer = if with_audio {
        format!("{} name=mux", profile.muxer)
    } else {
        profile.muxer.to_string()
    };
    let muxer = if profile.muxer == "mp4mux" {
        format!("{muxer} fragment-duration=2000")
    } else {
        muxer
    };

    Ok(format!(
        "{}{} ! videoconvert ! {} ! videorate{} ! queue ! {}{} ! {} ! filesink location=\"{}\"",
        video_source,
        window_segment,
        video_raw_caps,
        scale_segment,
        encoder_segment,
        parser_segment,
        muxer,
        output_str
    ))
}

pub(in crate::recording) fn get_x11_source(config: &RecordingConfig) -> RecordResult<String> {
    // The editor draws its own cursor when a pointer sidecar exists, so the
    // recorded frames must not carry the OS cursor.
    let show_pointer = if config.cursor && !config.pointer_track_enabled() {
        "true"
    } else {
        "false"
    };
    // A picked window is read from the root window; the pipeline crops it to
    // the window's current visible rectangle, so a resize cannot make
    // ximagesrc read past the window.
    if config.x11_window.is_some() {
        return Ok(format!(
            "ximagesrc name=vsrc show-pointer={} use-damage=false",
            show_pointer
        ));
    }
    let mut source = format!(
        "ximagesrc name=vsrc show-pointer={} use-damage=false",
        show_pointer
    );

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

/// The capture size before the optional cap: the followed window's pick-time
/// size, the configured region when the config pins one (mirroring
/// [`get_x11_source`]), otherwise the X screen size for fullscreen capture.
/// `None` only when the screen query fails.
pub(in crate::recording) fn x11_source_size(config: &RecordingConfig) -> Option<(u32, u32)> {
    if let Some(window) = config.x11_window {
        return Some((window.width, window.height));
    }
    if let (Some(_x), Some(_y), Some(w), Some(h)) =
        (config.x, config.y, config.width, config.height)
    {
        return Some((w, h));
    }
    x11_screen_size()
}

fn x11_screen_size() -> Option<(u32, u32)> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    Some((
        u32::from(screen.width_in_pixels),
        u32::from(screen.height_in_pixels),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::profile::PROFILES;
    use super::*;

    fn x11_config(max_resolution: Option<(u32, u32)>) -> RecordingConfig {
        RecordingConfig {
            output_path: PathBuf::from("/tmp/apexshot-test.mp4"),
            target: super::super::RecordingTarget::Screen,
            output: None,
            width: Some(1920),
            height: Some(1200),
            x: Some(0),
            y: Some(0),
            cursor: true,
            pointer_track: false,
            x11_window: None,
            hidpi: false,
            max_resolution,
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

    fn profile_by_encoder(encoder: &str) -> &'static EncoderProfile {
        PROFILES
            .iter()
            .find(|profile| profile.encoder == encoder)
            .expect("expected encoder profile to exist")
    }

    fn h264_profile() -> &'static EncoderProfile {
        profile_by_encoder("x264enc")
    }

    fn build(
        config: &RecordingConfig,
        profile: &'static EncoderProfile,
        source_size: Option<(u32, u32)>,
    ) -> String {
        build_x11_gstreamer_pipeline(config, profile, &config.output_path, false, source_size)
            .expect("expected pipeline string")
    }

    #[test]
    fn x11_pipeline_scales_down_to_the_configured_cap() {
        let config = x11_config(Some((854, 480)));

        let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

        // 1920x1200 fits the 854x480 box at 768x480 (height-limited, even);
        // the x264 CRF is compensated for the smaller output (diagonal ~906
        // → -5 → quantizer 15) exactly like the Wayland libx264 branch.
        assert_eq!(
            pipeline,
            "ximagesrc name=vsrc show-pointer=true use-damage=false startx=0 starty=0 endx=1919 endy=1199 \
             ! videoconvert ! video/x-raw,framerate=30/1,format=I420,colorimetry=bt709 ! videorate \
             ! videoscale ! video/x-raw,width=768,height=480,pixel-aspect-ratio=1/1 \
             ! queue ! x264enc speed-preset=veryfast pass=qual quantizer=15 key-int-max=60 \
             ! h264parse ! mp4mux fragment-duration=2000 ! filesink location=\"/tmp/apexshot-test.mp4\""
        );
    }

    #[test]
    fn x11_pipeline_omits_scaling_without_a_cap() {
        let config = x11_config(None);

        let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

        assert!(!pipeline.contains("videoscale"));
        assert!(!pipeline.contains("width=[1,"));
        // 1920x1200 is above the reduction cutoff, so the tier CRF stands.
        assert!(pipeline.contains("quantizer=20"));
    }

    #[test]
    fn x11_pipeline_scales_odd_sources_to_even_dimensions() {
        // I420 needs even plane sizes, so an odd capture region is stepped
        // down to even even without a cap (the Wayland path does the same
        // with its trunc(iw/2)*2 scale).
        let config = RecordingConfig {
            width: Some(801),
            height: Some(599),
            ..x11_config(None)
        };

        let pipeline = build(&config, h264_profile(), x11_source_size(&config));

        assert!(pipeline
            .contains("! videoscale ! video/x-raw,width=800,height=598,pixel-aspect-ratio=1/1"));
    }

    #[test]
    fn x11_pipeline_falls_back_to_the_range_caps_without_a_source_size() {
        // Screen query failed: the never-upscale range still applies the
        // cap, but the output size stays unknown so the CRF stands.
        let config = x11_config(Some((854, 480)));

        let pipeline = build(&config, h264_profile(), None);

        assert!(pipeline.contains(
            "! videoscale ! video/x-raw,width=[1,854],height=[1,480],pixel-aspect-ratio=1/1"
        ));
        assert!(pipeline.contains("quantizer=20"));
    }

    #[test]
    fn x11_pipeline_pins_i420_and_bt709_on_the_raw_caps() {
        // I420 alone negotiates smpte170m; both pins are needed for output
        // that matches the Wayland backend's yuv420p + bt709 tags.
        let config = x11_config(None);

        let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

        assert!(pipeline.contains("video/x-raw,framerate=30/1,format=I420,colorimetry=bt709"));
    }

    #[test]
    fn x11_pipeline_inserts_h264parse_before_mp4mux() {
        // mp4mux only accepts avc/avc3; openh264enc emits byte-stream and
        // x264enc can too, so both H.264 encoders get a parser. The WebM and
        // Ogg muxers need none.
        let config = x11_config(None);

        for encoder in ["x264enc", "openh264enc"] {
            let pipeline = build(&config, profile_by_encoder(encoder), Some((1920, 1200)));
            assert!(
                pipeline.contains("! h264parse ! mp4mux"),
                "{encoder} pipeline missing h264parse: {pipeline}"
            );
        }

        for encoder in ["vp9enc", "vp8enc", "theoraenc"] {
            let pipeline = build(&config, profile_by_encoder(encoder), Some((1920, 1200)));
            assert!(
                !pipeline.contains("h264parse"),
                "{encoder} pipeline should not parse H.264: {pipeline}"
            );
        }
    }

    #[test]
    fn x11_pipeline_injects_encoder_quality_props() {
        let config = x11_config(Some((854, 480)));

        let h264 = build(&config, h264_profile(), Some((1920, 1200)));
        assert!(
            h264.contains("x264enc speed-preset=veryfast pass=qual quantizer=15 key-int-max=60")
        );

        let vp9 = build(&config, profile_by_encoder("vp9enc"), Some((1920, 1200)));
        assert!(vp9.contains("vp9enc deadline=1000000 end-usage=cq cq-level=20"));
        assert!(vp9.contains("keyframe-max-dist=60"));
    }

    #[test]
    fn x11_pipeline_keeps_a_separator_between_framerate_caps_and_videorate() {
        // Regression: the string once concatenated them into
        // `framerate=30/1videorate`, which gst-parse folds into the caps value
        // and the pipeline then fails to link videoconvert to queue.
        for cap in [None, Some((1280, 720))] {
            let config = x11_config(cap);

            let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

            assert!(pipeline.contains("colorimetry=bt709 ! videorate"));
            assert!(!pipeline.contains("30/1videorate"));
        }
    }

    #[test]
    fn x11_pipeline_scale_follows_videorate_not_the_framerate_caps() {
        // gst-parse rejects two adjacent caps filters, so the scale caps must
        // sit after the videorate element rather than next to the framerate caps.
        let config = x11_config(Some((1280, 720)));

        let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

        let videorate = pipeline.find("videorate").expect("videorate in pipeline");
        let scale = pipeline
            .find("! videoscale !")
            .expect("videoscale segment in pipeline");
        assert!(videorate < scale);
        assert!(!pipeline.contains("1/1 ! video/x-raw,framerate"));
    }

    #[test]
    fn x11_pipeline_fragments_only_the_mp4_muxer() {
        let config = x11_config(None);

        for encoder in ["x264enc", "openh264enc"] {
            let pipeline = build(&config, profile_by_encoder(encoder), Some((1920, 1200)));
            assert!(
                pipeline.contains("mp4mux fragment-duration=2000"),
                "{encoder} MP4 pipeline missing its fragment cadence: {pipeline}"
            );
        }

        for encoder in ["vp9enc", "vp8enc", "theoraenc"] {
            let pipeline = build(&config, profile_by_encoder(encoder), Some((1920, 1200)));
            assert!(
                !pipeline.contains("fragment-duration"),
                "{encoder} pipeline must not carry MP4 fragment flags: {pipeline}"
            );
        }
    }

    #[test]
    fn x11_stop_waits_for_the_bus_eos_before_teardown() {
        let source = include_str!("x11.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        let resume = source
            .find("if paused {\n        let _ = pipeline.set_state(gst::State::Playing);\n    }")
            .expect("paused pipelines resume before awaiting EOS");
        let awaited = source.find("await_pipeline_eos").expect("bounded EOS wait");
        assert!(
            resume < awaited,
            "a paused pipeline must resume before the EOS wait",
        );
        assert!(
            source.contains("!eos_received && stop_action == RecordingTerminalAction::Save"),
            "only a Save stop that did not already consume the bus EOS waits again",
        );
    }

    #[test]
    fn x11_select_loop_never_sends_eos_inline() {
        let source = include_str!("x11.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        let loop_start = source
            .find("    loop {\n        tokio::select! {")
            .expect("select loop");
        let loop_end = loop_start
            + source[loop_start..]
                .find("\n    if paused {\n")
                .expect("loop exit before the resume");
        let select_loop = &source[loop_start..loop_end];
        assert!(
            !select_loop.contains("pipeline.send_event("),
            "the select loop must only record a stop; a synchronous send_event can block the async task",
        );
        assert!(
            source.contains("send_eos_bounded(&pipeline"),
            "the stop path sends EOS through the bounded helper",
        );

        let helper = source
            .find("async fn send_eos_bounded")
            .expect("bounded EOS helper");
        let helper_body = &source[helper..];
        let helper_body = &helper_body[..helper_body.find("\n}\n").expect("helper end")];
        assert!(helper_body.contains("run_detached_bounded("));

        // The bounded runner must not use spawn_blocking: the runtime's drop
        // waits for unfinished blocking tasks, so a wedged call would still
        // hang the recorder at exit.
        let runner = source
            .find("async fn run_detached_bounded")
            .expect("detached bounded runner");
        let runner_body = &source[runner..];
        let runner_body = &runner_body[..runner_body.find("\n}\n").expect("runner end")];
        assert!(runner_body.contains("std::thread::Builder::new()"));
        assert!(runner_body.contains("tokio::time::timeout"));
        assert!(!runner_body.contains("spawn_blocking"));
    }

    #[test]
    fn a_bus_eos_consumed_in_the_loop_is_not_waited_on_twice() {
        let source = include_str!("x11.rs");
        let source = source.split("#[cfg(test)]").next().unwrap();
        let eos_arm = source
            .find("MessageView::Eos(..) => {")
            .expect("bus EOS arm");
        assert!(
            source[eos_arm..].contains("eos_received = true"),
            "a consumed bus EOS must be recorded",
        );
        assert!(
            source.contains("tokio::time::sleep(std::time::Duration::from_millis(50)).await"),
            "the EOS wait must yield on the async tick rather than block",
        );
    }

    #[test]
    fn x11_pipeline_hides_the_os_cursor_when_pointer_tracking() {
        let config = RecordingConfig {
            pointer_track: true,
            ..x11_config(None)
        };

        let pipeline = build(&config, h264_profile(), Some((1920, 1200)));

        assert!(pipeline.contains("ximagesrc name=vsrc show-pointer=false use-damage=false"));
    }

    #[test]
    fn x11_source_size_uses_the_configured_region() {
        let config = x11_config(Some((854, 480)));
        assert_eq!(x11_source_size(&config), Some((1920, 1200)));

        let area = RecordingConfig {
            width: Some(640),
            height: Some(480),
            ..x11_config(None)
        };
        assert_eq!(x11_source_size(&area), Some((640, 480)));
    }

    fn window_source() -> crate::backend::x11_window_pick::X11WindowSource {
        crate::backend::x11_window_pick::X11WindowSource {
            frame: 928,
            client: Some(931),
            x: 120,
            y: 140,
            width: 260,
            height: 350,
            screen_width: 1920,
            screen_height: 1080,
        }
    }

    #[test]
    fn x11_window_pipeline_crops_the_screen_and_letterboxes_into_the_pick_size() {
        let config = RecordingConfig {
            x11_window: Some(window_source()),
            ..x11_config(None)
        };

        let pipeline = build(&config, h264_profile(), x11_source_size(&config));

        assert!(!pipeline.contains("xid="));
        assert!(pipeline.contains(
            "ximagesrc name=vsrc show-pointer=true use-damage=false \
             ! videocrop name=wcrop left=120 top=140 right=1540 bottom=590 \
             ! videoscale add-borders=true \
             ! video/x-raw,width=260,height=350,pixel-aspect-ratio=1/1 \
             ! videoconvert"
        ));
        // The letterbox scale replaces the shared cap scale.
        assert_eq!(pipeline.matches("videoscale").count(), 1);
        assert_eq!(x11_source_size(&config), Some((260, 350)));
    }

    #[test]
    fn x11_window_pipeline_letterboxes_into_the_capped_size() {
        let config = RecordingConfig {
            x11_window: Some(crate::backend::x11_window_pick::X11WindowSource {
                frame: 7,
                client: None,
                x: 0,
                y: 0,
                width: 1920,
                height: 1200,
                screen_width: 1920,
                screen_height: 1200,
            }),
            ..x11_config(Some((854, 480)))
        };

        let pipeline = build(&config, h264_profile(), x11_source_size(&config));

        assert!(pipeline.contains(
            "! videocrop name=wcrop left=0 top=0 right=0 bottom=0 \
             ! videoscale add-borders=true \
             ! video/x-raw,width=768,height=480,pixel-aspect-ratio=1/1"
        ));
        assert_eq!(pipeline.matches("videoscale").count(), 1);
        // The capped size drives the CRF tier like any other source.
        assert!(pipeline.contains("quantizer=15"));
    }

    #[test]
    fn x11_window_pipeline_hides_the_os_cursor_when_pointer_tracking() {
        let config = RecordingConfig {
            pointer_track: true,
            x11_window: Some(crate::backend::x11_window_pick::X11WindowSource {
                frame: 7,
                client: None,
                x: 0,
                y: 0,
                width: 640,
                height: 480,
                screen_width: 1920,
                screen_height: 1080,
            }),
            ..x11_config(None)
        };

        let pipeline = build(&config, h264_profile(), x11_source_size(&config));

        assert!(pipeline.contains("ximagesrc name=vsrc show-pointer=false use-damage=false"));
        assert!(pipeline.contains(
            "! videocrop name=wcrop left=0 top=0 right=1280 bottom=600 \
             ! videoscale add-borders=true \
             ! video/x-raw,width=640,height=480,pixel-aspect-ratio=1/1"
        ));
    }

    #[test]
    fn a_window_crop_update_only_reports_changes() {
        let screen = (1920, 1080);
        let last = (100, 120, 1720, 860);

        assert_eq!(crop_update(last, (100, 120, 100, 100), screen), None);
        assert_eq!(
            crop_update(last, (140, 120, 100, 100), screen),
            Some((140, 120, 1680, 860))
        );
    }
}
