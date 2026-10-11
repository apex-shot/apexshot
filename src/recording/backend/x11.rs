use super::super::{
    gst_audio, notify_daemon_event, RecordError, RecordResult, RecordingConfig,
    RecordingControlCommand, RecordingTerminalAction,
};
use super::fit_within_max_resolution;
use super::profile::{video_encoder_props, EncoderProfile};
use super::session::RecordingAudioExclusiveGuard;
use gst::prelude::*;
use gstreamer as gst;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use x11rb::connection::Connection;

/// Best-effort audio for the X11 path: the shared GStreamer audio bin linked
/// into the video muxer. Audio is a new capability here — any setup failure
/// falls back to the historical video-only recording instead of failing.
fn build_x11_audio_bin(
    config: &RecordingConfig,
    profile: &EncoderProfile,
) -> Option<gst_audio::GstAudioBin> {
    use gst_audio::{
        audio_available, build_audio_bin, encoder_for_muxer, AudioTermination, GstAudioSetup,
    };

    let setup = GstAudioSetup::from_recording(config)?;
    if !audio_available(
        profile.muxer,
        AudioTermination::GhostPad,
        setup.noise_suppression,
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

/// X11 fallback recording using GStreamer ximagesrc.
/// Preserved from the previous implementation for backward compatibility,
/// now with optional audio from the shared GStreamer audio bin.
#[allow(unused_assignments)]
pub(super) async fn record_x11_with_gstreamer(
    config: &RecordingConfig,
    profile: &EncoderProfile,
    final_path: &Path,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> RecordResult<(PathBuf, RecordingTerminalAction)> {
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

    if paused {
        let _ = pipeline.set_state(gst::State::Playing);
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
/// The muxer is named when audio is attached so the audio bin can request a pad.
///
/// `source_size` is what `ximagesrc` will produce (the configured region, or
/// the screen size for fullscreen). With a `config.max_resolution` cap the
/// pipeline scales to the aspect-preserving fit via `fit_within_max_resolution`
/// and pins exact even dimensions (I420 cannot take odd sizes), e.g.
/// 1920x1200 → 768x480 under the 854x480 box, with `pixel-aspect-ratio=1/1`
/// and never upscaling; an even source already inside the box records
/// untouched (no scale segment). When the source size is unknown the
/// historical caps range still applies the ceiling. The raw caps pin
/// `format=I420,colorimetry=bt709` so the encoder input matches the Wayland
/// backend's yuv420p + bt709 output (I420 alone negotiates smpte170m), and a
/// `h264parse` sits before the mp4mux because it only accepts avc/avc3 while
/// openh264enc (and byte-stream x264enc) emit byte-stream. The scale segment
/// must not sit adjacent to the framerate caps: gst-parse rejects two
/// consecutive caps filters.
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

    Ok(format!(
        "{} ! videoconvert ! {} ! videorate{} ! queue ! {}{} ! {} ! filesink location=\"{}\"",
        video_source,
        video_raw_caps,
        scale_segment,
        encoder_segment,
        parser_segment,
        muxer,
        output_str
    ))
}

pub(in crate::recording) fn get_x11_source(config: &RecordingConfig) -> RecordResult<String> {
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

/// The dimensions `ximagesrc` will produce: the configured region when the
/// config pins one (mirroring [`get_x11_source`]), otherwise the X screen
/// size for fullscreen capture. `None` only when the screen query fails.
pub(in crate::recording) fn x11_source_size(config: &RecordingConfig) -> Option<(u32, u32)> {
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
            width: Some(1920),
            height: Some(1200),
            x: Some(0),
            y: Some(0),
            cursor: true,
            pointer_track: false,
            hidpi: false,
            max_resolution,
            fps: 30,
            crf: 20,
            mono_audio: false,
            mic_enabled: false,
            speaker_enabled: false,
            mic_source: None,
            speaker_source: None,
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
            "ximagesrc show-pointer=true use-damage=false startx=0 starty=0 endx=1919 endy=1199 \
             ! videoconvert ! video/x-raw,framerate=30/1,format=I420,colorimetry=bt709 ! videorate \
             ! videoscale ! video/x-raw,width=768,height=480,pixel-aspect-ratio=1/1 \
             ! queue ! x264enc speed-preset=veryfast pass=qual quantizer=15 key-int-max=60 \
             ! h264parse ! mp4mux ! filesink location=\"/tmp/apexshot-test.mp4\""
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
}
