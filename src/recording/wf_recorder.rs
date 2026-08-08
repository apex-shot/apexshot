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

pub(super) fn should_use_wf_recorder(config: &super::RecordingConfig) -> bool {
    // Flatpak: never shell out to host wf-recorder; use portal/PipeWire path only.
    if crate::app_identity::portal_only() {
        return false;
    }
    is_wlroots_session() && config.output_path.extension().is_none_or(|e| e != "gif")
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

pub(super) fn should_use_vaapi() -> bool {
    if std::env::var_os("APEXSHOT_HW_ENCODER")
        .map(|v| v == "vaapi")
        .unwrap_or(false)
    {
        return detect_vaapi_device().is_some();
    }
    if let Ok(val) = std::env::var("APEXSHOT_HW_ENCODER") {
        return val == "vaapi" && detect_vaapi_device().is_some();
    }
    false
}

pub(super) fn ffmpeg_vaapi_args(width: u32, height: u32) -> Vec<String> {
    let device = detect_vaapi_device().unwrap_or_else(|| "/dev/dri/renderD128".into());
    vec![
        "-vaapi_device".into(),
        device,
        "-vf".into(),
        format!("format=nv12,hwupload,scale_vaapi=w={width}:h={height}"),
        "-c:v".into(),
        "h264_vaapi".into(),
        "-qp".into(),
        "24".into(),
        "-profile".into(),
        "main".into(),
    ]
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
    let mut args: Vec<String> = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    }

    args.push("-r".into());
    args.push(config.fps.max(1).to_string());

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

/// GIF recording on wlroots: record via wf-recorder to a temp MP4 file, then
/// convert to GIF with ffmpeg (palettegen + paletteuse).
pub(super) async fn record_gif_with_wf_recorder(
    config: super::RecordingConfig,
    command_rx: Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
) -> super::RecordResult<(PathBuf, super::RecordingTerminalAction)> {
    use std::process::{Command, Stdio};

    if let Some(msg) = crate::app_identity::host_escape_blocked("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(msg));
    }

    if !super::command_exists("wf-recorder") {
        return Err(RecordError::UnsupportedBackend(
            "wlroots GIF recording requires wf-recorder. Install it with: sudo pacman -S wf-recorder"
                .into(),
        ));
    }
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err(RecordError::NoEncoderFound);
    }

    let final_path = config.output_path.clone();

    // Build a temp .mp4 path in the same directory as the target GIF (or /tmp).
    let temp_dir = final_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("/tmp"));
    let temp_path = {
        let stem = final_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("apexshot-gif-temp");
        let mut p = temp_dir.join(format!("{}.temp.mp4", stem));
        // Avoid collisions if a previous temp file still exists.
        let mut counter = 1_u32;
        while p.exists() {
            p = temp_dir.join(format!("{}.temp-{}.mp4", stem, counter));
            counter += 1;
        }
        p
    };

    // ---- Phase 1: record video with wf-recorder ----
    let mut args: Vec<String> = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    }

    args.push("-r".into());
    args.push(config.fps.max(1).to_string());

    if config.mic_enabled || config.speaker_enabled {
        args.push("-a".into());
        let source = if config.speaker_enabled && !config.mic_enabled {
            config
                .speaker_source
                .clone()
                .unwrap_or_else(super::audio::get_pulse_speaker_monitor)
        } else {
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
    args.push(temp_path.to_string_lossy().to_string());

    println!("Recording GIF via wf-recorder (temp: {:?})", temp_path);
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
                    let _ = std::fs::remove_file(&temp_path);
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

    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;

    // ---- Handle stop actions ----
    if stop_action == super::RecordingTerminalAction::Discard {
        let _ = std::fs::remove_file(&temp_path);
        if let Some(event) = super::daemon_event_for_terminal_action(stop_action) {
            super::notify_daemon_event(event);
        }
        return Ok((final_path, stop_action));
    }

    if stop_action == super::RecordingTerminalAction::Restart {
        let _ = std::fs::remove_file(&temp_path);
        if let Some(event) = super::daemon_event_for_terminal_action(stop_action) {
            super::notify_daemon_event(event);
        }
        return Ok((final_path, stop_action));
    }

    // ---- Phase 2: convert MP4 to GIF with ffmpeg ----
    let max_colors = ((32.0 + 224.0 * config.gif_quality) as u32).clamp(32, 256);
    let dither = if config.gif_quality >= 0.5 {
        "floyd_steinberg"
    } else {
        "bayer:bayer_scale=5"
    };
    let stats_mode = if config.gif_optimize { "diff" } else { "full" };
    let scale_prefix = match config.gif_max_width {
        Some(w) => format!("scale={}:-2:flags=lanczos,", w),
        None => String::new(),
    };
    let vf_filter = format!(
        "fps={},{}format=rgb24,split[s0][s1];[s0]palettegen=max_colors={}:reserve_transparent=0:stats_mode={}[p];[s1][p]paletteuse=dither={}",
        config.fps, scale_prefix, max_colors, stats_mode, dither
    );

    println!("Converting to GIF with ffmpeg...");
    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-nostats")
        .arg("-i")
        .arg(&temp_path)
        .arg("-vf")
        .arg(&vf_filter)
        .arg(&final_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(RecordError::IoError)?;

    let _ = std::fs::remove_file(&temp_path);

    if !status.success() {
        let _ = std::fs::remove_file(&final_path);
        return Err(RecordError::GifError(format!(
            "FFmpeg GIF conversion failed with status: {status}"
        )));
    }

    if let Some(event) = super::daemon_event_for_terminal_action(stop_action) {
        super::notify_daemon_event(event);
    }

    println!("GIF saved to {:?}", final_path);
    Ok((final_path, stop_action))
}
