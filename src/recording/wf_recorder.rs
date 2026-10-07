use std::path::PathBuf;
use std::process::Stdio;
use tokio::sync::mpsc;

use super::*;

pub(super) fn is_wlroots_session() -> bool {
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
    if wayland_display && super::command_exists("labwc") {
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

pub(super) fn should_use_wf_recorder(_config: &super::RecordingConfig) -> bool {
    // Flatpak: never shell out to host wf-recorder; use portal/PipeWire path only.
    if crate::app_identity::portal_only() {
        return false;
    }
    is_wlroots_session()
}

pub(super) fn detect_vaapi_device() -> Option<String> {
    // Try the standard render node paths.
    for path in &["/dev/dri/renderD128", "/dev/dri/renderD129"] {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

pub(super) fn should_use_nvenc() -> bool {
    match std::env::var("APEXSHOT_HW_ENCODER").as_deref() {
        Ok("cpu" | "off" | "soft" | "software" | "vaapi") => false,
        Ok("nvenc") => crate::recording::editor::hardware_encode::recording_nvenc_works(),
        _ => crate::recording::editor::hardware_encode::recording_uses_nvenc(),
    }
}

pub(super) fn should_use_vaapi() -> bool {
    match std::env::var("APEXSHOT_HW_ENCODER").as_deref() {
        Ok("cpu" | "off" | "soft" | "software" | "nvenc") => false,
        Ok("vaapi") => crate::recording::editor::hardware_encode::recording_vaapi_works(),
        _ => {
            detect_vaapi_device().is_some()
                && crate::recording::editor::hardware_encode::recording_uses_vaapi()
        }
    }
}

pub(super) fn ffmpeg_vaapi_args(width: u32, height: u32, qp: u32) -> Vec<String> {
    // VAAPI recording: profile HIGH, tier-derived QP in CQP mode (a fixed QP
    // ignored the Ultra/High/Balanced setting entirely).
    let device = detect_vaapi_device().unwrap_or_else(|| "/dev/dri/renderD128".into());
    vec![
        "-vaapi_device".into(),
        device,
        "-vf".into(),
        // bt709 matrix for the RGB->NV12 conversion; swscale's default is 601,
        // which would not match the bt709 VUI tags on the output stream.
        format!("scale=out_color_matrix=bt709:out_range=tv,format=nv12,hwupload,scale_vaapi=w={width}:h={height}"),
        "-c:v".into(),
        "h264_vaapi".into(),
        "-rc_mode".into(),
        "CQP".into(),
        "-qp".into(),
        qp.to_string(),
        "-profile".into(),
        "high".into(),
    ]
}

fn video_capture_args(config: &super::RecordingConfig) -> Vec<String> {
    let mut args = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    }

    args.push("-r".into());
    args.push(config.fps.max(1).to_string());

    if config.max_resolution.is_some() {
        args.push("-F".into());
        args.push(super::backend::wayland_video_filter(config.max_resolution));
    }

    args
}

pub(super) async fn record_with_wf_recorder(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    if let Some(msg) = crate::app_identity::host_escape_blocked("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(msg));
    }
    if !super::command_exists("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(
            "wlroots recording requires wf-recorder. Install it with: sudo pacman -S wf-recorder"
                .into(),
        ));
    }

    let final_path = config.output_path.clone();
    let mut args = video_capture_args(&config);

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
                .unwrap_or_else(super::audio::get_pulse_speaker_monitor)
        } else {
            // wf-recorder accepts a single Pulse source. For mic-only use the
            // default mic. If both mic + speaker are requested, prefer the mic
            // here; the GStreamer backend can mix both, but wf-recorder cannot
            // portably mix two Pulse sources without an external filter graph.
            config
                .mic_source
                .clone()
                .unwrap_or_else(super::audio::get_pulse_default_source)
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

    super::notify_daemon_event("recording_session_started");
    let mut command_rx = command_rx;
    let mut stop_action = super::RecordingTerminalAction::Save;
    let mut paused = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RecordError::IoError)?;
                if !status.success() && stop_action == super::RecordingTerminalAction::Save {
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
                        super::notify_daemon_event("recording_session_paused");
                    }
                    RecordingControlCommand::Resume if paused => {
                        if let Some(pid) = child.id() {
                            let _ = std::process::Command::new("kill").args(["-CONT", &pid.to_string()]).status();
                        }
                        paused = false;
                        super::notify_daemon_event("recording_session_resumed");
                    }
                    RecordingControlCommand::Restart => {
                        stop_action = super::RecordingTerminalAction::Restart;
                        break;
                    }
                    RecordingControlCommand::StopSave => {
                        stop_action = super::RecordingTerminalAction::Save;
                        break;
                    }
                    RecordingControlCommand::StopDiscard => {
                        stop_action = super::RecordingTerminalAction::Discard;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if matches!(
        stop_action,
        super::RecordingTerminalAction::Save | super::RecordingTerminalAction::Discard
    ) {
        super::control_session::release_recording_busy();
        super::notify_daemon_event("recording_session_ended");
    }

    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;

    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&final_path);
    }
    if let Some(event) = super::daemon_event_for_terminal_action(stop_action) {
        super::notify_daemon_event(event);
    }
    Ok((final_path, stop_action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_capture_args_include_selected_fps_and_resolution() {
        let config = crate::recording::RecordingConfig {
            x: Some(10),
            y: Some(20),
            width: Some(2560),
            height: Some(1440),
            fps: 60,
            max_resolution: Some((1280, 720)),
            ..crate::recording::RecordingConfig::default()
        };

        let args = video_capture_args(&config);

        assert!(args
            .windows(2)
            .any(|pair| pair == ["-g", "10,20 2560x1440"]));
        assert!(args.windows(2).any(|pair| pair == ["-r", "60"]));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "-F" && pair[1].contains("min(iw,1280)") && pair[1].contains("min(ih,720)")
        }));
    }
}
