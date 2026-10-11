use super::*;
#[cfg(test)]
use crate::capture_overlay::RecordingType;
use crate::{
    capture_overlay::RecordingRequest,
    config::{save_config, AppConfig},
    recording::editor::sidecar::{
        delete_recording_outputs, CaptureRegion, ClickSample, CursorKind, PointerSample,
        PointerSidecar, PressSample,
    },
};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

static HARVESTED_POINTER_TRACK: std::sync::Mutex<Option<crate::gnome_shell::PointerTrackResult>> =
    std::sync::Mutex::new(None);

fn preserve_first_pointer_track(
    slot: &mut Option<crate::gnome_shell::PointerTrackResult>,
    result: crate::gnome_shell::PointerTrackResult,
) -> bool {
    if slot.is_some() {
        return false;
    }
    *slot = Some(result);
    true
}

/// Stop GNOME pointer tracking as soon as capture ends so hotkeys work while
/// the file is still being muxed. `finish_save` writes the stashed samples.
pub(super) fn harvest_pointer_track_on_session_end() {
    if !crate::gnome_shell::is_shell_overlay_service_available() {
        return;
    }

    // Capture end and outer recording cleanup can both emit session_ended.
    // StopPointerTrack is destructive: the first call returns the recording,
    // while a second returns an empty t0=0 payload. Never let that duplicate
    // notification erase samples that are waiting for finish_save.
    if HARVESTED_POINTER_TRACK
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
    {
        return;
    }

    if let Ok(result) = crate::gnome_shell::stop_pointer_track() {
        if let Ok(mut guard) = HARVESTED_POINTER_TRACK.lock() {
            let _ = preserve_first_pointer_track(&mut guard, result);
        }
    }
}

fn take_harvested_pointer_track() -> Option<crate::gnome_shell::PointerTrackResult> {
    HARVESTED_POINTER_TRACK
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

struct PointerTrackSession {
    started: bool,
    region: CaptureRegion,
    window_cursor_metadata: bool,
}

fn pointer_capture_region(
    config: &super::RecordingConfig,
    fullscreen_monitor: Option<CaptureRegion>,
) -> CaptureRegion {
    let configured = CaptureRegion::from_capture(config.x, config.y, config.width, config.height);
    if configured.is_area() {
        return configured;
    }

    fullscreen_monitor.unwrap_or(configured)
}

fn window_clicks_from_shell_track(
    result: &crate::gnome_shell::PointerTrackResult,
    timeline: &super::timeline::RecordingTimeline,
) -> Vec<ClickSample> {
    const SHELL_SAMPLE_TOLERANCE_SECONDS: f64 = 0.05;

    if result.t0_monotonic_us <= 0 {
        return Vec::new();
    }
    result
        .clicks
        .iter()
        .filter_map(|(t, x, y, button)| {
            if !t.is_finite() {
                return None;
            }
            let at_us = result
                .t0_monotonic_us
                .saturating_add((*t * 1_000_000.0).round() as i64);
            timeline
                .window_cursor_positions_at(at_us)
                .into_iter()
                .filter_map(|(local_x, local_y, sampled_at_us)| {
                    let sampled_t =
                        sampled_at_us.saturating_sub(result.t0_monotonic_us) as f64 / 1_000_000.0;
                    let sample_index = result
                        .samples
                        .partition_point(|sample| sample.0.is_finite() && sample.0 <= sampled_t);
                    let before = sample_index
                        .checked_sub(1)
                        .filter(|&index| result.samples[index].0.is_finite());
                    let after = (sample_index < result.samples.len())
                        .then_some(sample_index)
                        .filter(|&index| result.samples[index].0.is_finite());
                    let matches_click = |index: usize| {
                        let (_, sample_x, sample_y, _) = &result.samples[index];
                        sample_x.abs_diff(*x) <= 2 && sample_y.abs_diff(*y) <= 2
                    };
                    let nearest_sample =
                        [before, after]
                            .into_iter()
                            .flatten()
                            .min_by(|&left, &right| {
                                (result.samples[left].0 - sampled_t)
                                    .abs()
                                    .total_cmp(&(result.samples[right].0 - sampled_t).abs())
                            });
                    let nearest_matches = nearest_sample.is_some_and(|index| {
                        (result.samples[index].0 - sampled_t).abs()
                            <= SHELL_SAMPLE_TOLERANCE_SECONDS
                            && matches_click(index)
                    });
                    let bracket_matches = before.zip(after).is_some_and(|(before, after)| {
                        matches_click(before) && matches_click(after)
                    });
                    if !nearest_matches && !bracket_matches {
                        return None;
                    }
                    Some((
                        sampled_at_us.abs_diff(at_us),
                        ClickSample {
                            t: *t,
                            x: local_x,
                            y: local_y,
                            button: *button,
                        },
                    ))
                })
                .min_by_key(|(distance, _)| *distance)
                .map(|(_, click)| click)
        })
        .collect()
}

fn write_window_cursor_sidecar(
    output_path: &Path,
    timeline: &super::timeline::RecordingTimeline,
    origin_us: i64,
    clicks: Vec<ClickSample>,
    presses: Vec<PressSample>,
) -> bool {
    let mut sidecar = PointerSidecar::new(
        origin_us,
        CaptureRegion::from_capture(None, None, None, None),
    );
    if !timeline.add_window_cursor_to_sidecar(&mut sidecar, origin_us) {
        PointerSidecar::delete_next_to_video(output_path);
        return false;
    }
    sidecar.clicks = clicks;
    sidecar.presses = presses;
    if !timeline.rebase(&mut sidecar) {
        eprintln!(
            "[recording] No retained media clock; deleting unsynchronized window cursor sidecar"
        );
        PointerSidecar::delete_next_to_video(output_path);
        return false;
    }
    sidecar.subtract_region();
    sidecar.compact_samples();
    match sidecar.write_next_to_video(output_path) {
        Ok(path) => {
            eprintln!("[recording] wrote window cursor sidecar {}", path.display());
            true
        }
        Err(err) => {
            eprintln!("[recording] failed to write window cursor sidecar: {err}");
            PointerSidecar::delete_next_to_video(output_path);
            false
        }
    }
}

fn window_presses_from_shell_track(
    result: &crate::gnome_shell::PointerTrackResult,
    clicks: &[ClickSample],
) -> Vec<PressSample> {
    result
        .presses
        .iter()
        .filter(|(down, _, button, _)| {
            down.is_finite()
                && clicks
                    .iter()
                    .any(|click| click.button == *button && (click.t - *down).abs() <= 0.05)
        })
        .map(|(down, up, button, dragged)| PressSample {
            down: *down,
            up: *up,
            button: *button,
            dragged: *dragged,
        })
        .collect()
}

fn notify_window_click_tracking_unavailable() {
    crate::utils::notify::desktop_notification(
        &crate::i18n::t("Pointer track unavailable"),
        &crate::i18n::t("The window cursor remains editable, but Auto Zoom clicks will not be recorded without the ApexShot GNOME extension."),
    );
}

fn shell_fullscreen_pointer_region(params: &RecordingControlsParams) -> Option<CaptureRegion> {
    if !params.is_fullscreen || params.capture_w <= 0 || params.capture_h <= 0 {
        return None;
    }
    let center_x = params.capture_x.saturating_add(params.capture_w / 2);
    let center_y = params.capture_y.saturating_add(params.capture_h / 2);
    crate::gnome_shell::get_monitor_geometry_at(center_x, center_y)
        .map(|(x, y, w, h)| CaptureRegion { x, y, w, h })
        .map_err(|err| {
            eprintln!(
                "[recording] GNOME monitor geometry unavailable; using legacy fullscreen pointer coordinates ({err})"
            );
            err
        })
        .ok()
}

impl PointerTrackSession {
    fn start(
        config: &super::RecordingConfig,
        region: CaptureRegion,
        window_cursor_metadata: bool,
    ) -> Self {
        let should_track = config.pointer_track_enabled()
            || (config.target == super::RecordingTarget::Window && window_cursor_metadata);
        if !should_track {
            if config.target == super::RecordingTarget::Window {
                let _ = take_harvested_pointer_track();
            }
            return Self {
                started: false,
                region,
                window_cursor_metadata,
            };
        }
        let _ = take_harvested_pointer_track();
        match crate::gnome_shell::start_pointer_track() {
            Ok(()) => {
                if window_cursor_metadata {
                    eprintln!("[recording] window pointer and click tracking started");
                } else {
                    eprintln!("[recording] pointer track started (OS cursor hidden)");
                }
                Self {
                    started: true,
                    region,
                    window_cursor_metadata,
                }
            }
            Err(err) => {
                if window_cursor_metadata {
                    eprintln!("[recording] StartPointerTrack failed ({err}); window cursor will remain editable without Auto Zoom click data");
                    notify_window_click_tracking_unavailable();
                } else {
                    eprintln!(
                        "[recording] StartPointerTrack failed ({err}); continuing without sidecar"
                    );
                    crate::utils::notify::desktop_notification(
                        &crate::i18n::t("Pointer track unavailable"),
                        &crate::i18n::t("Recording will continue with the OS cursor hidden. Enable the ApexShot GNOME extension for the editor."),
                    );
                }
                Self {
                    started: false,
                    region,
                    window_cursor_metadata,
                }
            }
        }
    }

    fn finish_save(
        mut self,
        output_path: &Path,
        timeline: Option<&super::timeline::RecordingTimeline>,
    ) -> bool {
        let was_started = self.started;
        self.started = false;
        let result = match take_harvested_pointer_track() {
            Some(result) => Ok(result),
            None if !was_started && !self.window_cursor_metadata => {
                PointerSidecar::delete_next_to_video(output_path);
                return false;
            }
            None if !was_started => {
                let Some(timeline) = timeline else {
                    PointerSidecar::delete_next_to_video(output_path);
                    return false;
                };
                let Some(origin_us) = timeline.origin_us() else {
                    PointerSidecar::delete_next_to_video(output_path);
                    return false;
                };
                return write_window_cursor_sidecar(
                    output_path,
                    timeline,
                    origin_us,
                    Vec::new(),
                    Vec::new(),
                );
            }
            None => crate::gnome_shell::stop_pointer_track(),
        };
        match result {
            Ok(result) => {
                if self.window_cursor_metadata {
                    let Some(timeline) = timeline else {
                        PointerSidecar::delete_next_to_video(output_path);
                        return false;
                    };
                    if result.samples.is_empty() || result.t0_monotonic_us <= 0 {
                        eprintln!("[recording] GNOME Shell returned no usable click clock; window cursor will remain editable without Auto Zoom click data");
                        notify_window_click_tracking_unavailable();
                    }
                    let origin_us = if result.t0_monotonic_us > 0 {
                        result.t0_monotonic_us
                    } else if let Some(origin_us) = timeline.origin_us() {
                        origin_us
                    } else {
                        PointerSidecar::delete_next_to_video(output_path);
                        return false;
                    };
                    let clicks = window_clicks_from_shell_track(&result, timeline);
                    let presses = window_presses_from_shell_track(&result, &clicks);
                    return write_window_cursor_sidecar(
                        output_path,
                        timeline,
                        origin_us,
                        clicks,
                        presses,
                    );
                }
                if result.samples.is_empty() {
                    eprintln!("[recording] StopPointerTrack returned no pointer samples");
                    crate::utils::notify::desktop_notification(
                        &crate::i18n::t("Pointer track missing"),
                        &crate::i18n::t("No cursor samples were captured. Zoom centers will use the frame center."),
                    );
                }
                let mut sidecar = PointerSidecar::new(result.t0_monotonic_us, self.region);
                sidecar.pointer = result
                    .samples
                    .into_iter()
                    .map(|(t, x, y, kind)| PointerSample {
                        t,
                        x: x as f64,
                        y: y as f64,
                        kind: CursorKind::parse(&kind),
                    })
                    .collect();
                sidecar.clicks = result
                    .clicks
                    .into_iter()
                    .map(|(t, x, y, button)| ClickSample {
                        t,
                        x: x as f64,
                        y: y as f64,
                        button,
                    })
                    .collect();
                sidecar.presses = result
                    .presses
                    .into_iter()
                    .map(|(down, up, button, dragged)| PressSample {
                        down,
                        up,
                        button,
                        dragged,
                    })
                    .collect();
                if let Some(timeline) = timeline {
                    let pointer_origin_us = sidecar.t0_monotonic_us;
                    if !timeline.rebase(&mut sidecar) {
                        eprintln!("[recording] No retained media clock; deleting unsynchronized pointer sidecar");
                        PointerSidecar::delete_next_to_video(output_path);
                        return false;
                    }
                    eprintln!(
                        "[recording] pointer origin monotonic_us={pointer_origin_us} media origin monotonic_us={}",
                        sidecar.t0_monotonic_us
                    );
                }
                sidecar.subtract_region();
                sidecar.compact_samples();
                match sidecar.write_next_to_video(output_path) {
                    Ok(path) => {
                        eprintln!("[recording] wrote pointer sidecar {}", path.display());
                        true
                    }
                    Err(err) => {
                        eprintln!("[recording] failed to write pointer sidecar: {err}");
                        PointerSidecar::delete_next_to_video(output_path);
                        false
                    }
                }
            }
            Err(err) => {
                eprintln!("[recording] StopPointerTrack failed ({err}); deleting partial sidecar");
                PointerSidecar::delete_next_to_video(output_path);
                false
            }
        }
    }

    fn finish_discard(mut self, output_path: &Path) {
        let was_started = self.started;
        self.started = false;
        if take_harvested_pointer_track().is_none() && was_started {
            if let Err(err) = crate::gnome_shell::stop_pointer_track() {
                eprintln!("[recording] StopPointerTrack on discard failed: {err}");
            }
        }
        delete_recording_outputs(output_path);
    }
}

impl Drop for PointerTrackSession {
    fn drop(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        if take_harvested_pointer_track().is_some() {
            return;
        }
        if let Err(err) = crate::gnome_shell::stop_pointer_track() {
            eprintln!("[recording] StopPointerTrack during cleanup failed: {err}");
        }
    }
}

fn maybe_prompt_gnome_extension() {
    if crate::gnome_shell::current_session_supports_gnome_shell_overlay()
        && !crate::gnome_shell::is_shell_overlay_service_available()
    {
        crate::utils::notify::desktop_notification(
            "Enable the ApexShot GNOME extension",
            "Pointer tracking and area dim need the ApexShot GNOME extension.",
        );
    }
}

fn should_open_recording_editor(open_editor: bool) -> bool {
    open_editor
}

fn validate_saved_recording(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::metadata(path).map_err(|err| {
        anyhow::anyhow!(
            "recording output was not created at {}: {err}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        anyhow::bail!("recording output is empty at {}", path.display());
    }
    // A non-empty file is not proof of a recording: a finalized container with
    // zero streams is only a few hundred bytes (the audit's sample was 261).
    // Structural check so "Recording saved" cannot fire on an unplayable file.
    match probe_recording_structure(path) {
        ProbeOutcome::Playable => Ok(()),
        ProbeOutcome::Unplayable(reason) => {
            anyhow::bail!(
                "recording output is not playable ({reason}): {}",
                path.display()
            )
        }
        ProbeOutcome::Unavailable => {
            // The probe already logged why it could not run.
            Ok(())
        }
    }
}

enum ProbeOutcome {
    Playable,
    Unplayable(String),
    /// ffprobe could not run at all — not a verdict on the file.
    Unavailable,
}

/// Ask ffprobe whether the container actually holds something: at least one
/// stream and a positive duration. Catches finalized-but-streamless outputs
/// that a byte-count check (or a clean ffmpeg exit) lets through.
fn probe_recording_structure(path: &Path) -> ProbeOutcome {
    let output = match std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=index",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!(
                "[recording] ffprobe could not run ({err}); skipping structural validation for {}",
                path.display()
            );
            return ProbeOutcome::Unavailable;
        }
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return ProbeOutcome::Unplayable(format!(
            "ffprobe rejected the container: {}",
            detail.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.lines().any(|line| line.starts_with("index=")) {
        return ProbeOutcome::Unplayable("container has no streams".into());
    }
    let duration_ok = stdout.lines().any(|line| {
        line.strip_prefix("duration=")
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|seconds| seconds > 0.0)
    });
    if !duration_ok {
        return ProbeOutcome::Unplayable("container reports no positive duration".into());
    }
    ProbeOutcome::Playable
}

#[derive(Debug)]
pub struct PreparedOverlayRecordingRequest {
    pub updated_app_config: AppConfig,
    pub output_path: PathBuf,
    pub recording_config: super::RecordingConfig,
    pub controls_params: Option<RecordingControlsParams>,
    pub use_shell_mask: bool,
    pub open_editor: bool,
}

enum PreparedShellRecording {
    Video(super::backend::BuiltPipeline),
}

fn should_use_shell_mask_for_request(
    request: &RecordingRequest,
    shell_mask_available: bool,
) -> bool {
    shell_mask_available
        && !request.window
        && request.dim_screen
        && !request.fullscreen
        && request.width > 0
        && request.height > 0
}

pub fn prepare_overlay_recording_request(
    mut app_config: AppConfig,
    request: &RecordingRequest,
    now: chrono::DateTime<chrono::Utc>,
) -> PreparedOverlayRecordingRequest {
    let shell_overlay_available =
        crate::gnome_shell::current_session_supports_gnome_shell_overlay();
    let use_shell_mask = should_use_shell_mask_for_request(request, shell_overlay_available);
    app_config.rec_controls = request.controls;
    app_config.rec_display_time = true; // always show recording time in top bar
    app_config.rec_hidpi = request.hidpi;
    app_config.rec_notifications = request.notifications;
    app_config.rec_cursor = true;
    app_config.rec_remember_selection = request.remember_selection;
    app_config.rec_dim_screen = request.dim_screen;
    app_config.rec_countdown = request.countdown;
    app_config.rec_mic = request.mic;
    app_config.rec_speaker = request.speaker;
    app_config.rec_video_format = 0;
    // `rec_video_max_res` is deliberately not taken from the request: neither
    // quick access offers a resolution picker any more, so the request only
    // echoes the value the overlay was launched with. Writing that echo back
    // reverted a resolution the user saved in Settings after the overlay
    // started.
    app_config.rec_video_fps = request.video_fps;
    app_config.rec_video_mono = request.record_mono;
    app_config.rec_noise_suppression = request.noise_suppression;
    app_config.rec_video_open_editor = request.open_editor;

    if request.remember_selection && !request.window {
        app_config.last_selection_x = Some(request.x);
        app_config.last_selection_y = Some(request.y);
        app_config.last_selection_w = Some(request.width);
        app_config.last_selection_h = Some(request.height);
    }

    let output_path = super::recording_output_path(&app_config, "mp4", now);
    super::ensure_recording_parent_dir(&output_path);

    // Settings is the only place the cap can be chosen, so it comes from the
    // config the caller loaded, never from the request (which only echoes the
    // overlay's launch-time value).
    let max_resolution = super::max_resolution_for_setting(app_config.rec_video_max_res);

    let fps = match request.video_fps {
        0 => 24,
        1 => 30,
        2 => 50,
        3 => 60,
        _ => 30,
    };

    let (capture_x, capture_y, capture_width, capture_height) =
        if request.fullscreen || request.window {
            (None, None, None, None)
        } else {
            (
                Some(request.x),
                Some(request.y),
                Some(request.width as u32),
                Some(request.height as u32),
            )
        };

    let recording_config = super::RecordingConfig {
        output_path: output_path.clone(),
        target: if request.window {
            super::RecordingTarget::Window
        } else if request.fullscreen {
            super::RecordingTarget::Screen
        } else {
            super::RecordingTarget::Area
        },
        width: capture_width,
        height: capture_height,
        x: capture_x,
        y: capture_y,
        cursor: true,
        pointer_track: !request.window && crate::gnome_shell::should_use_pointer_track(),
        hidpi: request.hidpi,
        max_resolution,
        fps,
        crf: super::crf_for_quality(app_config.rec_video_quality),
        mono_audio: request.record_mono,
        mic_enabled: request.mic,
        speaker_enabled: request.speaker,
        mic_source: None,
        speaker_source: None,
        noise_suppression: request.noise_suppression,
    };

    let controls_params = Some(RecordingControlsParams {
        capture_x: if request.window { 0 } else { request.x },
        capture_y: if request.window { 0 } else { request.y },
        capture_w: if request.window { 0 } else { request.width },
        capture_h: if request.window { 0 } else { request.height },
        is_fullscreen: request.fullscreen && !request.window,
        show_timer: true,
        use_shell_mask,
        dim_screen: request.dim_screen,
        countdown_enabled: request.countdown,
        countdown_seconds: 3, // Default to 3s for now, or fetch from config
        session_id: None,     // Will be set when controls are launched
    });

    PreparedOverlayRecordingRequest {
        updated_app_config: app_config,
        output_path,
        recording_config,
        controls_params,
        use_shell_mask,
        open_editor: request.open_editor,
    }
}

pub async fn run_recording_with_controls(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Fedora: video recording is not supported.
    super::refuse_fedora_recording()?;
    let _busy = super::control_session::RecordingBusyGuard::acquire()?;
    run_recording_with_controls_locked(config, params).await
}

async fn run_recording_with_controls_locked(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // On GNOME the shell extension dims the area outside the capture rect.
    if crate::gnome_shell::current_session_supports_gnome_shell_overlay() {
        return run_recording_with_shell_mask(config, params).await;
    }

    // Fallback for non-GNOME (Fedora KDE, Hyprland, Sway, Niri, River, etc.).
    // No floating control bar — shortcuts, tray, and desktop notifications only.
    eprintln!(
        "[recording] GNOME Shell not detected; using native recording path (shortcuts + notifications)."
    );
    run_recording_with_native_controls(config, params).await
}

/// Standalone countdown window for entry points without a controls deck
/// (e.g. CLI): shows the timer when Settings enables it. Returns false when
/// the user cancels the countdown.
pub async fn run_standalone_countdown() -> anyhow::Result<bool> {
    run_recording_countdown_subprocess(RecordingControlsParams {
        countdown_seconds: 3,
        ..Default::default()
    })
    .await
}

async fn run_recording_countdown_subprocess(
    params: RecordingControlsParams,
) -> anyhow::Result<bool> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
    let seconds = params.countdown_seconds;
    let params_json = serde_json::to_string(&params)?;

    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&exe)
            .arg("recording-countdown-internal")
            .arg(params_json)
            .arg(seconds.to_string())
            .status()
    })
    .await??;

    if status.success() {
        Ok(true)
    } else if status.code() == Some(2) {
        Ok(false)
    } else {
        Err(anyhow::anyhow!(
            "recording countdown exited with status {status}"
        ))
    }
}

async fn run_shell_recording_countdown(params: RecordingControlsParams) -> anyhow::Result<bool> {
    let geometry = crate::gnome_shell::RecordingMaskGeometry {
        x: params.capture_x,
        y: params.capture_y,
        width: params.capture_w,
        height: params.capture_h,
    };

    match crate::gnome_shell::show_recording_countdown(geometry, params.countdown_seconds) {
        Ok(countdown) if countdown.is_shown() => {
            tokio::time::sleep(std::time::Duration::from_secs(
                params.countdown_seconds.into(),
            ))
            .await;
            drop(countdown);
            Ok(true)
        }
        Ok(countdown) => {
            drop(countdown);
            eprintln!(
                "[recording] GNOME Shell did not show the countdown; using fallback countdown."
            );
            run_recording_countdown_subprocess(params).await
        }
        Err(err) => {
            eprintln!("[recording] Failed to show GNOME Shell countdown ({err}); using fallback countdown.");
            run_recording_countdown_subprocess(params).await
        }
    }
}

async fn prepare_shell_recording(
    config: super::RecordingConfig,
) -> super::RecordResult<Option<PreparedShellRecording>> {
    if super::wf_recorder::is_wlroots_session() && config.target != super::RecordingTarget::Window {
        return Ok(None);
    }

    super::backend::prepare_recording_backend(config)
        .await
        .map(PreparedShellRecording::Video)
        .map(Some)
}

async fn start_shell_recording(
    config: super::RecordingConfig,
    prepared: Option<PreparedShellRecording>,
    command_rx: mpsc::UnboundedReceiver<RecordingControlCommand>,
) -> super::RecordResult<super::RecordedSession> {
    match prepared {
        Some(PreparedShellRecording::Video(backend)) => {
            super::backend::start_recording_with_prepared_backend(backend, Some(command_rx)).await
        }
        None => super::start_recording_with_commands(config, Some(command_rx)).await,
    }
}

/// Non-GNOME recording path (Fedora KDE, wlroots, etc.).
///
/// No floating control bar — users control the session with configured
/// global shortcuts and the tray. Lifecycle feedback is via desktop
/// notifications.
pub async fn run_recording_with_native_controls(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    // Belt-and-suspenders: Fedora never records (entry points refuse first).
    if super::is_fedora_recording_unsupported() {
        super::refuse_fedora_recording()?;
    }

    // Prepare the capture backend *before* countdown so any portal / permission
    // UI appears first. Countdown then leads straight into recording without a
    // second chooser after "3-2-1".
    let prepared_backend = if super::wf_recorder::is_wlroots_session()
        && config.target != super::RecordingTarget::Window
    {
        None
    } else {
        match super::backend::prepare_recording_backend(config.clone()).await {
            Ok(prepared) => Some(prepared),
            Err(err) => {
                super::notify_recording_session_ended_best_effort();
                return Err(err.into());
            }
        }
    };

    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id).await?;

    // Optional pre-roll — never spawn the floating controls UI on native path.
    if params.countdown_enabled {
        match run_recording_countdown_subprocess(params.clone()).await {
            Ok(true) => {}
            Ok(false) => {
                super::notify_recording_session_ended_best_effort();
                return Ok((config.output_path.clone(), StopAction::Discard));
            }
            Err(err) => {
                super::notify_recording_session_ended_best_effort();
                return Err(err);
            }
        }
    }

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

    control_server.set_command_sender(control_command_tx);
    super::notify_daemon_event("recording_session_started");
    let outcome = if let Some(prepared_backend) = prepared_backend {
        super::backend::start_recording_with_prepared_backend(prepared_backend, Some(command_rx))
            .await
    } else {
        super::start_recording_with_commands(config.clone(), Some(command_rx)).await
    };
    control_server.clear_command_sender();
    dbus_forward.abort();

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            super::notify_recording_session_ended_best_effort();
            return Err(err.into());
        }
    };

    match outcome {
        super::RecordedSession {
            path,
            action: super::RecordingTerminalAction::Restart,
            ..
        } => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Restart)
            {
                super::notify_daemon_event(event);
            }
            delete_recording_outputs(&path);
            Box::pin(run_recording_with_native_controls(config, params)).await
        }
        super::RecordedSession {
            path,
            action: super::RecordingTerminalAction::Save,
            timeline,
        } => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Save)
            {
                super::notify_daemon_event(event);
            }
            if config.target == super::RecordingTarget::Window {
                if let Some(timeline) = timeline.as_ref() {
                    if let Some(origin_us) = timeline.origin_us() {
                        write_window_cursor_sidecar(
                            &path,
                            timeline,
                            origin_us,
                            Vec::new(),
                            Vec::new(),
                        );
                    } else {
                        PointerSidecar::delete_next_to_video(&path);
                    }
                } else {
                    PointerSidecar::delete_next_to_video(&path);
                }
            }
            Ok((path, StopAction::Save))
        }
        super::RecordedSession {
            path,
            action: super::RecordingTerminalAction::Discard,
            ..
        } => {
            if let Some(event) =
                super::daemon_event_for_terminal_action(super::RecordingTerminalAction::Discard)
            {
                super::notify_daemon_event(event);
            }
            Ok((path, StopAction::Discard))
        }
    }
}

/// GNOME recording path: identical to the native path except the shell
/// extension dims the area outside the capture rect while we record.
async fn run_recording_with_shell_mask(
    config: super::RecordingConfig,
    params: RecordingControlsParams,
) -> anyhow::Result<(PathBuf, StopAction)> {
    let _chrome = crate::gnome_shell::RecordingChromeGuard::clear_on_drop();
    // Same ordering as the native path: portal / backend first, then countdown.
    let mut prepared_backend = match prepare_shell_recording(config.clone()).await {
        Ok(prepared) => prepared,
        Err(err) => {
            super::notify_recording_session_ended_best_effort();
            return Err(err.into());
        }
    };

    let _shell_mask = if params.use_shell_mask {
        match crate::gnome_shell::show_recording_mask(crate::gnome_shell::RecordingMaskGeometry {
            x: params.capture_x,
            y: params.capture_y,
            width: params.capture_w,
            height: params.capture_h,
        }) {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!("[recording] Failed to show GNOME shell recording mask ({err}); continuing without shell mask.");
                None
            }
        }
    } else {
        None
    };

    if params.countdown_enabled {
        match run_shell_recording_countdown(params.clone()).await {
            Ok(true) => {}
            Ok(false) => {
                super::notify_recording_session_ended_best_effort();
                return Ok((config.output_path.clone(), StopAction::Discard));
            }
            Err(err) => {
                super::notify_recording_session_ended_best_effort();
                return Err(err);
            }
        }
    }

    let session_id = format!(
        "recording-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let control_server = RecordingControlServer::start(session_id).await?;

    super::notify_daemon_event("recording_session_started");
    let final_outcome = loop {
        let fullscreen_monitor = if config.pointer_track_enabled() && params.is_fullscreen {
            let monitor_params = params.clone();
            match tokio::task::spawn_blocking(move || {
                shell_fullscreen_pointer_region(&monitor_params)
            })
            .await
            {
                Ok(region) => region,
                Err(err) => {
                    eprintln!("[recording] GNOME monitor lookup failed: {err}");
                    None
                }
            }
        } else {
            None
        };
        let pointer_region = pointer_capture_region(&config, fullscreen_monitor);
        let window_cursor_metadata =
            prepared_backend
                .as_ref()
                .is_some_and(|prepared| match prepared {
                    PreparedShellRecording::Video(backend) => {
                        backend.window_cursor_metadata_enabled()
                    }
                });
        let pointer_track =
            PointerTrackSession::start(&config, pointer_region, window_cursor_metadata);
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        control_server.set_command_sender(command_tx);
        let outcome =
            start_shell_recording(config.clone(), prepared_backend.take(), command_rx).await;
        control_server.clear_command_sender();
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(err) => {
                pointer_track.finish_discard(&config.output_path);
                drop(control_server);
                super::notify_recording_session_ended_best_effort();
                return Err(err.into());
            }
        };

        match outcome {
            super::RecordedSession {
                path,
                action: action @ super::RecordingTerminalAction::Restart,
                ..
            } => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                pointer_track.finish_discard(&path);
                prepared_backend = prepare_shell_recording(config.clone()).await?;
                continue;
            }
            super::RecordedSession {
                path,
                action: action @ super::RecordingTerminalAction::Save,
                timeline,
            } => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                pointer_track.finish_save(&path, timeline.as_ref());
                break (path, StopAction::Save);
            }
            super::RecordedSession {
                path,
                action: action @ super::RecordingTerminalAction::Discard,
                ..
            } => {
                if let Some(event) = super::daemon_event_for_terminal_action(action) {
                    super::notify_daemon_event(event);
                }
                pointer_track.finish_discard(&path);
                break (path, StopAction::Discard);
            }
        }
    };

    drop(control_server);

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
    // Fedora: video recording is not supported.
    super::refuse_fedora_recording()?;
    maybe_prompt_gnome_extension();

    let prepared = prepare_overlay_recording_request(
        crate::config::load_config(),
        &request,
        chrono::Utc::now(),
    );

    if let Some(parent) = prepared.output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    save_config(&prepared.updated_app_config)?;

    // Own admission before changing desktop state. Keep it until DND and audio
    // exclusivity have both been restored so another take cannot overlap cleanup.
    let busy = super::control_session::RecordingBusyGuard::acquire()?;

    let dnd_guard = if request.notifications {
        crate::recording::dnd::DndGuard::enable()
    } else {
        None
    };

    eprintln!("Starting recording to {:?}...", prepared.output_path);

    crate::daemon::release_audio_monitors_for_recording();
    struct EndExclusive;
    impl Drop for EndExclusive {
        fn drop(&mut self) {
            crate::daemon::end_recording_audio_exclusive();
        }
    }
    let end_exclusive = EndExclusive;
    crate::gnome_shell::hide_recording_mask_best_effort();
    let chrome = crate::gnome_shell::RecordingChromeGuard::clear_on_drop();
    let open_editor = prepared.open_editor;
    let recording_config = prepared.recording_config.clone();
    let controls_params = prepared.controls_params.clone();
    let outcome = match std::thread::spawn(move || -> anyhow::Result<(PathBuf, StopAction)> {
        let runtime = tokio::runtime::Runtime::new()?;
        if let Some(params) = controls_params {
            runtime
                .block_on(run_recording_with_controls_locked(recording_config, params))
                .map_err(|err| {
                    if err
                        .downcast_ref::<super::RecordError>()
                        .is_some_and(|e| matches!(e, super::RecordError::Cancelled))
                    {
                        err
                    } else {
                        anyhow::anyhow!("failed to run recording controls: {err}")
                    }
                })
        } else {
            runtime
                .block_on(super::start_recording(recording_config))
                .map(|path| (path, StopAction::Save))
                .map_err(|err| {
                    // Preserve the type so the outcome handler below can tell
                    // a portal-dialog cancel apart from a real failure.
                    if matches!(err, super::RecordError::Cancelled) {
                        anyhow::Error::from(err)
                    } else {
                        anyhow::anyhow!("Recording failed: {err}")
                    }
                })
        }
    })
    .join()
    {
        Ok(outcome) => outcome,
        Err(_) => {
            crate::gnome_shell::hide_recording_mask_best_effort();
            super::notify_recording_session_ended_best_effort();
            return Err(anyhow::anyhow!("recording worker panicked"));
        }
    };

    // Restore desktop state before completion/error notifications and before
    // allowing another recording to acquire the process-wide admission guard.
    drop(chrome);
    drop(end_exclusive);
    drop(dnd_guard);
    drop(busy);

    match outcome {
        Ok((path, StopAction::Discard)) => {
            eprintln!("Recording discarded — deleting {:?}", path);
            delete_recording_outputs(&path);
            Ok(path)
        }
        Ok((path, StopAction::Save)) => {
            eprintln!("Recording saved to {:?}", path);
            let config = crate::config::load_config().sanitized();

            if config.rec_after_capture_copy_to_clipboard {
                if let Err(e) = crate::utils::clipboard::copy_uri_to_clipboard(&path) {
                    eprintln!("[recording] Failed to copy recording to clipboard: {e}");
                }
            }

            if !config.rec_after_capture_save {
                delete_recording_outputs(&path);
                eprintln!(
                    "[recording] Recording discarded because Save is disabled in after-capture settings"
                );
                crate::utils::notify::desktop_notification(
                    &crate::i18n::t("Recording not saved"),
                    &crate::i18n::t("Save is disabled in After capture settings"),
                );
                return Ok(path);
            }

            validate_saved_recording(&path)?;

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("recording");
            crate::utils::notify::desktop_notification(
                &crate::i18n::t("Recording saved"),
                file_name,
            );

            if should_open_recording_editor(open_editor) {
                spawn_recording_editor_subprocess(path.clone());
            }
            Ok(path)
        }
        Err(err) => {
            if err
                .downcast_ref::<super::RecordError>()
                .is_some_and(|e| matches!(e, super::RecordError::Cancelled))
            {
                eprintln!("[recording] Recording cancelled by user.");
                return Err(err);
            }
            crate::gnome_shell::hide_recording_mask_best_effort();
            super::notify_recording_session_ended_best_effort();
            crate::utils::notify::desktop_notification(
                &crate::i18n::t("Recording failed"),
                &err.to_string(),
            );
            Err(anyhow::anyhow!("Recording failed: {err}"))
        }
    }
}

/// Spawn the recording editor as a **subprocess** so it gets its own GTK
/// event loop and doesn't conflict with the recording worker thread.
fn spawn_recording_editor_subprocess(path: std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
    match std::process::Command::new(&exe)
        .arg("video-editor")
        .arg(&path)
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("[recording] Failed to spawn recording editor: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    fn pointer_result(
        t0_monotonic_us: i64,
        sample_x: i32,
    ) -> crate::gnome_shell::PointerTrackResult {
        crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us,
            samples: vec![(0.0, sample_x, 20, "default".into())],
            clicks: Vec::new(),
            presses: Vec::new(),
        }
    }

    #[test]
    fn window_clicks_require_a_matching_shell_press_and_fresh_visible_local_cursor() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_000_000, Some((10.0, 15.0)));
        timeline.record_window_cursor(1_016_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(1_032_000, None);
        timeline.record_window_cursor(2_000_000, Some((30.0, 40.0)));
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![
                (0.01, 100, 200, "default".into()),
                (0.03, 100, 200, "default".into()),
                (1.0, 400, 500, "default".into()),
            ],
            clicks: vec![
                (0.02, 100, 200, 1),
                (0.04, 100, 200, 1),
                (0.5, 300, 400, 1),
                (1.0, 400, 500, 1),
                (1.0, 700, 800, 1),
            ],
            presses: Vec::new(),
        };

        assert_eq!(
            window_clicks_from_shell_track(&result, &timeline),
            vec![
                ClickSample {
                    t: 0.02,
                    x: 12.0,
                    y: 18.0,
                    button: 1,
                },
                ClickSample {
                    t: 1.0,
                    x: 30.0,
                    y: 40.0,
                    button: 1,
                },
            ]
        );
    }

    #[test]
    fn stationary_window_click_maps_from_its_confirmed_shell_anchor() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_000_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(5_000_000, None);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![(0.0, 300, 400, "default".into())],
            clicks: vec![(3.0, 300, 400, 1)],
            presses: Vec::new(),
        };

        assert_eq!(
            window_clicks_from_shell_track(&result, &timeline),
            vec![ClickSample {
                t: 3.0,
                x: 12.0,
                y: 18.0,
                button: 1,
            }]
        );
    }

    #[test]
    fn stationary_click_accepts_a_matching_shell_sample_bracket() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_056_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(2_000_000, None);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![
                (0.0, 300, 400, "default".into()),
                (0.112, 300, 400, "default".into()),
            ],
            clicks: vec![(0.5, 300, 400, 1)],
            presses: Vec::new(),
        };

        assert_eq!(
            window_clicks_from_shell_track(&result, &timeline),
            vec![ClickSample {
                t: 0.5,
                x: 12.0,
                y: 18.0,
                button: 1,
            }]
        );
    }

    #[test]
    fn shell_bracket_with_mismatched_or_missing_endpoint_does_not_map_a_click() {
        for samples in [
            vec![
                (0.0, 300, 400, "default".into()),
                (0.112, 700, 800, "default".into()),
            ],
            vec![(0.112, 300, 400, "default".into())],
            vec![(0.0, 300, 400, "default".into())],
        ] {
            let mut timeline = super::super::timeline::RecordingTimeline::default();
            timeline.enable_window_cursor(100, 80);
            timeline.record_window_cursor(1_056_000, Some((12.0, 18.0)));
            timeline.record_window_cursor(2_000_000, None);
            let result = crate::gnome_shell::PointerTrackResult {
                t0_monotonic_us: 1_000_000,
                samples,
                clicks: vec![(0.5, 300, 400, 1)],
                presses: Vec::new(),
            };

            assert!(window_clicks_from_shell_track(&result, &timeline).is_empty());
        }
    }

    #[test]
    fn click_elsewhere_does_not_reuse_a_stale_window_cursor_position() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_000_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(4_000_000, None);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![
                (0.0, 300, 400, "default".into()),
                (3.0, 700, 800, "default".into()),
            ],
            clicks: vec![(3.0, 700, 800, 1)],
            presses: Vec::new(),
        };

        assert!(window_clicks_from_shell_track(&result, &timeline).is_empty());
    }

    #[test]
    fn click_outside_the_window_before_hidden_metadata_is_not_mapped() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_000_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(1_050_000, None);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![
                (0.0, 300, 400, "default".into()),
                (0.020, 700, 800, "default".into()),
            ],
            clicks: vec![(0.020, 700, 800, 1)],
            presses: Vec::new(),
        };

        assert!(window_clicks_from_shell_track(&result, &timeline).is_empty());
    }

    #[test]
    fn stationary_window_click_survives_sidecar_rebase_into_auto_zoom() {
        use crate::recording::editor::auto_zoom;

        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.frame_time_us(1_000_000);
        timeline.record_window_cursor(1_000_000, Some((12.0, 18.0)));
        timeline.record_window_cursor(4_500_000, None);
        timeline.finish(6_000_000);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![(0.0, 300, 400, "default".into())],
            clicks: vec![(3.0, 300, 400, 1)],
            presses: Vec::new(),
        };
        let mut sidecar = PointerSidecar::new(
            1_000_000,
            CaptureRegion::from_capture(None, None, None, None),
        );
        let clicks = window_clicks_from_shell_track(&result, &timeline);
        assert_eq!(clicks.len(), 1);
        assert!(timeline.add_window_cursor_to_sidecar(&mut sidecar, 1_000_000));
        sidecar.clicks = clicks;
        assert!(timeline.rebase(&mut sidecar));

        let zooms = auto_zoom::automatic_zooms(
            &auto_zoom::clicks_from_sidecar(&sidecar, 100.0, 80.0),
            &auto_zoom::pointer_from_sidecar(&sidecar, 100.0, 80.0),
            5.0,
        );
        assert_eq!(zooms.len(), 1);
        assert_eq!(zooms[0].center_time, 3.0);
        assert_eq!(zooms[0].center, (12.0, 18.0));
    }

    #[test]
    fn moving_click_uses_a_nearby_window_sample_after_the_click() {
        let mut timeline = super::super::timeline::RecordingTimeline::default();
        timeline.enable_window_cursor(100, 80);
        timeline.record_window_cursor(1_000_000, Some((10.0, 15.0)));
        timeline.record_window_cursor(1_016_000, Some((20.0, 25.0)));
        timeline.record_window_cursor(1_032_000, Some((30.0, 35.0)));
        timeline.record_window_cursor(2_000_000, None);
        let result = crate::gnome_shell::PointerTrackResult {
            t0_monotonic_us: 1_000_000,
            samples: vec![
                (0.016, 100, 200, "default".into()),
                (0.032, 300, 400, "default".into()),
            ],
            clicks: vec![(0.020, 300, 400, 1)],
            presses: Vec::new(),
        };

        assert_eq!(
            window_clicks_from_shell_track(&result, &timeline),
            vec![ClickSample {
                t: 0.020,
                x: 30.0,
                y: 35.0,
                button: 1,
            }]
        );
    }

    #[test]
    fn duplicate_session_end_keeps_the_first_pointer_track() {
        let mut slot = None;
        assert!(preserve_first_pointer_track(
            &mut slot,
            pointer_result(123, 10)
        ));
        assert!(!preserve_first_pointer_track(
            &mut slot,
            pointer_result(0, 99)
        ));

        let saved = slot.expect("first pointer track should remain stashed");
        assert_eq!(saved.t0_monotonic_us, 123);
        assert_eq!(saved.samples[0].1, 10);
    }

    #[test]
    fn recording_editor_launch_respects_the_open_editor_setting() {
        assert!(should_open_recording_editor(true));
        assert!(!should_open_recording_editor(false));
    }

    #[test]
    fn saved_recording_must_exist_and_be_non_empty() {
        let path = std::env::temp_dir().join(format!(
            "apexshot-saved-recording-validation-{}.mp4",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        assert!(validate_saved_recording(&path).is_err());
        std::fs::write(&path, []).unwrap();
        assert!(validate_saved_recording(&path).is_err());

        std::fs::remove_file(path).unwrap();
    }

    fn tool_installed(tool: &str) -> bool {
        std::process::Command::new(tool)
            .arg("-version")
            .output()
            .is_ok()
    }

    #[test]
    fn validate_saved_recording_probes_container_structure() {
        if !tool_installed("ffmpeg") || !tool_installed("ffprobe") {
            eprintln!("skipping: ffmpeg/ffprobe not available");
            return;
        }
        let path = std::env::temp_dir().join(format!(
            "apexshot-saved-recording-structure-{}.mp4",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        // Non-empty but not a container: ffprobe must reject it (this is the
        // byte-count-only hole the audit's 261-byte sample fell through).
        std::fs::write(&path, b"video").unwrap();
        assert!(validate_saved_recording(&path).is_err());

        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=0.2:size=64x64:rate=10",
                "-c:v",
                "mpeg4",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg should run");
        assert!(status.success(), "ffmpeg must produce the probe fixture");

        assert!(validate_saved_recording(&path).is_ok());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn selected_area_pointer_region_takes_precedence_over_monitor_geometry() {
        let config = super::RecordingConfig {
            x: Some(250),
            y: Some(-40),
            width: Some(640),
            height: Some(360),
            ..Default::default()
        };
        let monitor = CaptureRegion {
            x: 0,
            y: -200,
            w: 1920,
            h: 1080,
        };

        assert_eq!(
            pointer_capture_region(&config, Some(monitor)),
            CaptureRegion {
                x: 250,
                y: -40,
                w: 640,
                h: 360,
            }
        );
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
            noise_suppression: true,
            open_editor: true,
            fullscreen: false,
            window: false,
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig {
                video_export_location: "/tmp/apexshot-recordings".into(),
                // Settings holds 480p; the request below still asks for 720p
                // because the overlay echoes whatever it was launched with.
                rec_video_max_res: 5,
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
        // The stale request value must not overwrite what Settings saved.
        assert_eq!(prepared.updated_app_config.rec_video_max_res, 5);
        assert_eq!(prepared.updated_app_config.rec_video_fps, 3);
        assert_eq!(prepared.updated_app_config.rec_video_mono, true);
        assert_eq!(prepared.updated_app_config.rec_noise_suppression, true);
        assert_eq!(prepared.updated_app_config.rec_video_open_editor, true);
        assert_eq!(prepared.open_editor, true);
        assert_eq!(prepared.recording_config.output_path, prepared.output_path);
        assert_eq!(prepared.recording_config.width, Some(640));
        assert_eq!(prepared.recording_config.height, Some(480));
        assert_eq!(prepared.recording_config.x, Some(10));
        assert_eq!(prepared.recording_config.y, Some(20));
        assert_eq!(prepared.recording_config.cursor, true);
        assert_eq!(
            prepared.recording_config.pointer_track,
            crate::gnome_shell::should_use_pointer_track()
        );
        assert_eq!(prepared.recording_config.hidpi, true);
        assert_eq!(prepared.recording_config.max_resolution, Some((854, 480)));
        assert_eq!(prepared.recording_config.fps, 60);
        assert_eq!(prepared.recording_config.mono_audio, true);
        assert_eq!(prepared.recording_config.noise_suppression, true);
        assert_eq!(prepared.recording_config.mic_enabled, true);
        assert_eq!(prepared.recording_config.speaker_enabled, false);
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
                dim_screen: false,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, false);
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
    fn prepare_overlay_recording_request_maps_video_setting_variants() {
        // (video_format, saved video_max_res, video_fps, expected cap, fps).
        // Every option Settings offers must survive the trip, not just the
        // first three, and the cap must come from the saved value.
        let cases = [
            (0_u8, 0_u8, 0_u8, None, 24_u32),
            (1, 1, 1, Some((1920, 1080)), 30),
            (0, 2, 2, Some((1280, 720)), 50),
            (1, 0, 3, None, 60),
            (1, 3, 1, Some((2560, 1440)), 30),
            (1, 4, 1, Some((1600, 900)), 30),
            (1, 5, 1, Some((854, 480)), 30),
            (1, 6, 1, Some((3840, 2160)), 30),
            // An unknown index falls back to Original, never to a bogus cap.
            (1, 99, 1, None, 30),
        ];

        for (index, (video_format, video_max_res, video_fps, expected_max_res, expected_fps)) in
            cases.into_iter().enumerate()
        {
            let request = RecordingRequest {
                x: 5,
                y: 10,
                width: 800,
                height: 600,
                record_type: RecordingType::Video,
                video_format,
                // The overlay has no resolution picker; it always sends the
                // value it was launched with. This stale echo must be ignored.
                video_max_res: 0,
                video_fps,
                ..RecordingRequest::default()
            };

            let prepared = prepare_overlay_recording_request(
                AppConfig {
                    rec_video_max_res: video_max_res,
                    ..AppConfig::default()
                },
                &request,
                chrono::Utc
                    .with_ymd_and_hms(2026, 4, 2, 10, 0, index as u32)
                    .unwrap(),
            );

            assert_eq!(
                prepared.recording_config.max_resolution, expected_max_res,
                "saved resolution index {video_max_res} must keep its cap"
            );
            assert_eq!(prepared.recording_config.fps, expected_fps);
            assert_eq!(prepared.updated_app_config.rec_video_format, 0);
            assert_eq!(
                prepared.updated_app_config.rec_video_max_res, video_max_res,
                "the saved resolution must survive the request untouched"
            );
            assert_eq!(
                prepared
                    .output_path
                    .extension()
                    .and_then(|ext| ext.to_str()),
                Some("mp4")
            );
        }
    }

    #[test]
    fn prepare_overlay_recording_request_uses_full_monitor_bounds_for_fullscreen_capture() {
        let request = RecordingRequest {
            x: -1920,
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
            prepared.recording_config.target,
            super::super::RecordingTarget::Screen
        );
        assert_eq!(
            prepared.controls_params,
            Some(RecordingControlsParams {
                capture_x: -1920,
                capture_y: 32,
                capture_w: 1920,
                capture_h: 1048,
                is_fullscreen: true,
                show_timer: true,
                use_shell_mask: false,
                dim_screen: true,
                countdown_enabled: true,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        let monitor = CaptureRegion {
            x: -1920,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(
            pointer_capture_region(&prepared.recording_config, Some(monitor)),
            monitor
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
                dim_screen: true,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
        assert_eq!(prepared.use_shell_mask, shell_supported);
    }

    #[test]
    fn prepare_overlay_recording_request_skips_mask_for_gnome_wayland_fullscreen_recording() {
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
            fullscreen: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 3, 26, 1, 35, 0).unwrap(),
        );

        assert_eq!(prepared.use_shell_mask, false); // fullscreen never uses mask
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
                dim_screen: true,
                countdown_enabled: false,
                countdown_seconds: 3,
                session_id: None,
            })
        );
    }

    #[test]
    fn shell_mask_follows_gnome_wayland_support() {
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
            fullscreen: false,
            ..RecordingRequest::default()
        };

        assert!(should_use_shell_mask_for_request(&request, true));
        assert!(!should_use_shell_mask_for_request(&request, false));
    }

    #[test]
    fn window_overlay_request_uses_window_target_without_screen_geometry() {
        let request = RecordingRequest {
            x: 400,
            y: 250,
            width: 800,
            height: 600,
            remember_selection: true,
            window: true,
            ..RecordingRequest::default()
        };

        let prepared = prepare_overlay_recording_request(
            AppConfig::default(),
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 4, 2, 10, 0, 0).unwrap(),
        );

        assert_eq!(
            prepared.recording_config.target,
            super::super::RecordingTarget::Window
        );
        assert_eq!(prepared.recording_config.x, None);
        assert_eq!(prepared.recording_config.y, None);
        assert_eq!(prepared.recording_config.width, None);
        assert_eq!(prepared.recording_config.height, None);
        assert!(!prepared.recording_config.pointer_track);
        assert_eq!(prepared.updated_app_config.last_selection_x, None);
        assert!(!prepared.use_shell_mask);
        let controls_params = prepared.controls_params.unwrap();
        assert_eq!(
            (
                controls_params.capture_x,
                controls_params.capture_y,
                controls_params.capture_w,
                controls_params.capture_h,
                controls_params.is_fullscreen,
            ),
            (0, 0, 0, 0, false),
            "window geometry is unknown until the portal selects the target"
        );
    }

    #[test]
    fn window_overlay_keeps_saved_ultra_fps_and_original_resolution() {
        let request = RecordingRequest {
            window: true,
            video_fps: 3,
            video_max_res: 1,
            ..RecordingRequest::default()
        };
        let prepared = prepare_overlay_recording_request(
            AppConfig {
                rec_video_quality: 2,
                rec_video_max_res: 0,
                rec_video_fps: 3,
                ..AppConfig::default()
            },
            &request,
            chrono::Utc.with_ymd_and_hms(2026, 4, 2, 10, 0, 0).unwrap(),
        );

        assert_eq!(
            prepared.recording_config.target,
            super::super::RecordingTarget::Window
        );
        assert_eq!(prepared.recording_config.crf, 16);
        assert_eq!(prepared.recording_config.fps, 60);
        assert_eq!(prepared.recording_config.max_resolution, None);
        assert_eq!(prepared.updated_app_config.rec_video_quality, 2);
        assert_eq!(prepared.updated_app_config.rec_video_max_res, 0);
        assert_eq!(prepared.updated_app_config.rec_video_fps, 3);
    }

    #[test]
    fn hidden_shell_countdown_uses_the_visible_subprocess_fallback() {
        let source = include_str!("controls.rs");
        let countdown = source
            .split("async fn run_shell_recording_countdown")
            .nth(1)
            .unwrap()
            .split("async fn prepare_shell_recording")
            .next()
            .unwrap();
        assert!(countdown.contains("countdown.is_shown()"));
        assert!(countdown.contains("run_recording_countdown_subprocess(params).await"));

        let shell_path = source
            .split("async fn run_recording_with_shell_mask")
            .nth(1)
            .unwrap()
            .split("async fn ")
            .next()
            .unwrap();
        assert!(
            shell_path
                .find("prepare_shell_recording(config.clone()).await")
                .unwrap()
                < shell_path
                    .find("run_shell_recording_countdown(params.clone()).await")
                    .unwrap()
        );
    }

    #[test]
    fn window_target_disables_pointer_tracking_even_when_requested() {
        let config = super::super::RecordingConfig {
            target: super::super::RecordingTarget::Window,
            pointer_track: true,
            ..super::super::RecordingConfig::default()
        };

        assert!(!config.pointer_track_enabled());
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
}
