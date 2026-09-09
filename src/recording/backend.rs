use super::*;
use std::path::PathBuf;
use tokio::sync::mpsc;

mod crop;
mod ffmpeg_process;
mod gif;
mod profile;
mod session;
mod source;
mod wayland;
mod x11;

#[cfg(test)]
use crop::{
    compute_wayland_crop, resolve_wayland_stream_position, wayland_area_crop_or_full, CropMargins,
};
use crop::{crop_rgba_frame, even_crop_output, scale_crop_to_frame};
pub(super) use crop::{fit_within_max_resolution, wayland_video_filter};
#[cfg(unix)]
pub(super) use ffmpeg_process::attach_audio_pipe_as_fd3;
#[cfg(not(unix))]
use ffmpeg_process::attach_audio_pipe_as_fd3;
use ffmpeg_process::{
    ffmpeg_error_detail, set_child_stdin_nonblocking, wait_for_ffmpeg_child,
    write_ffmpeg_frame_interruptible,
};
pub(super) use gif::{
    prepare_gif_wayland_recording, record_gif_rust_with_commands,
    record_prepared_gif_wayland_native, PreparedGifWaylandRecording,
};
#[cfg(test)]
use profile::{ffmpeg_available_encoders, video_encoder_props, PROFILES};
use profile::{normalize_recording_config_for_profile, select_encoder, EncoderProfile};
use session::RecordingAudioExclusiveGuard;
#[cfg(test)]
use source::discard_recording_restore_tokens_in;
pub(super) use source::get_wayland_source;
use source::WaylandSource;
pub(super) use wayland::record_wayland_with_ffmpeg_sync;
pub(super) use x11::get_x11_source;
use x11::record_x11_with_gstreamer;

#[allow(dead_code)]
pub(super) fn ffmpeg_encoder_available(name: &str) -> bool {
    profile::ffmpeg_encoder_available(name)
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
        let audio_muxer = built.profile.muxer;
        let config = built.config.clone();
        return tokio::task::spawn_blocking(move || {
            record_wayland_with_ffmpeg_sync(
                wayland_source,
                &final_path,
                &encoder_name,
                &encoder_props,
                audio_muxer,
                &config,
                command_rx,
            )
        })
        .await
        .map_err(|e| RecordError::GStreamerError(format!("Join error: {e}")))?;
    }

    record_x11_with_gstreamer(&built.config, built.profile, &built.final_path, command_rx).await
}

#[cfg(test)]
fn frames_due_for_catch_up(
    next_frame_at: Option<std::time::Instant>,
    now: std::time::Instant,
    frame_interval: std::time::Duration,
    max_frames: u64,
) -> u64 {
    let Some(mut deadline) = next_frame_at else {
        return 0;
    };
    if frame_interval.is_zero() || max_frames == 0 {
        return 0;
    }
    let mut due = 0u64;
    while deadline <= now && due < max_frames {
        due += 1;
        deadline += frame_interval;
    }
    due
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

/// GIF recording on X11 using GStreamer pipeline (preserved from old code).
#[allow(dead_code)]
pub(super) async fn record_gif_x11_gstreamer(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    gif::record_gif_x11_gstreamer(config, command_rx).await
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
            pointer_track: false,
            hidpi: false,
            max_resolution: None,
            fps: 30,
            mono_audio: false,
            mic_enabled: false,
            speaker_enabled: false,
            mic_source: None,
            speaker_source: None,
            noise_suppression: false,
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

        // veryfast + crf 22 + main profile
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
    fn scale_crop_maps_portal_size_onto_negotiated_frame() {
        let crop = CropMargins {
            left: 100,
            right: 100,
            top: 50,
            bottom: 50,
        };
        let scaled = scale_crop_to_frame(crop, 1920, 1080, 3840, 2160).expect("crop should scale");
        assert_eq!(scaled.left, 200);
        assert_eq!(scaled.right, 200);
        assert_eq!(scaled.top, 100);
        assert_eq!(scaled.bottom, 100);
        assert!(scale_crop_to_frame(crop, 200, 100, 200, 100).is_none());
    }

    #[test]
    fn ffmpeg_error_detail_keeps_the_useful_final_lines() {
        let stderr = "banner\ninput description\nMove this option before the file it belongs to.\n\
                      Error opening input files: Invalid argument\n";
        let detail = ffmpeg_error_detail(stderr);

        assert!(detail.contains("Move this option before the file it belongs to."));
        assert!(detail.contains("Error opening input files: Invalid argument"));
        assert!(!detail.contains("banner"));
    }

    #[test]
    fn ffmpeg_error_detail_has_a_fallback() {
        let detail = ffmpeg_error_detail("");
        assert!(detail.contains("selected encoder and audio devices"));
    }

    #[test]
    fn missing_stream_position_defaults_to_origin() {
        let pos = resolve_wayland_stream_position(None, (1920, 1080), (100, 100, 200, 200));
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

    #[test]
    fn compute_wayland_crop_tolerates_one_pixel_overlay_rounding() {
        let crop = compute_wayland_crop((0, 0), (1920, 1200), (150, 141, 1610, 1060))
            .expect("one-pixel edge mismatch should be clipped");
        assert_eq!(crop.left, 150);
        assert_eq!(crop.right, 160);
        assert_eq!(crop.top, 141);
        assert_eq!(crop.bottom, 0);
    }

    #[test]
    fn discard_recording_restore_tokens_removes_cached_files() {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-restore-token-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let screen = dir.join("wayland-record-screen.token");
        let area = dir.join("wayland-record-area.token");
        let other = dir.join("keep-me.txt");
        std::fs::write(&screen, "old-token").expect("write screen token");
        std::fs::write(&area, "old-token").expect("write area token");
        std::fs::write(&other, "keep").expect("write unrelated file");

        discard_recording_restore_tokens_in(&dir);

        assert!(!screen.exists());
        assert!(!area.exists());
        assert!(other.exists());
        let _ = std::fs::remove_file(&other);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn catch_up_writes_nothing_when_ahead_of_the_clock() {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_millis(33);
        assert_eq!(
            frames_due_for_catch_up(Some(now + interval), now, interval, 90),
            0
        );
        assert_eq!(frames_due_for_catch_up(None, now, interval, 90), 0);
    }

    #[test]
    fn catch_up_fills_missed_frame_deadlines_up_to_the_cap() {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_millis(100);
        let behind = now - std::time::Duration::from_millis(250);
        assert_eq!(frames_due_for_catch_up(Some(behind), now, interval, 90), 3);
        assert_eq!(frames_due_for_catch_up(Some(behind), now, interval, 2), 2);
    }

    #[test]
    fn catch_up_covers_encoder_lag_of_several_seconds() {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_millis(33);
        let behind = now - std::time::Duration::from_secs(10);
        let due = frames_due_for_catch_up(Some(behind), now, interval, 30 * 120);
        assert!(due >= 300, "expected ~10s of 30fps duplicates, got {due}");
        assert!(due <= 310);
    }
}
