use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::sync::{mpsc, Mutex};

use super::*;

const WF_RECORDER_STDERR_TAIL: usize = 8;

pub(crate) fn is_wlroots_session() -> bool {
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

/// Parse `wf-recorder -L` output. Each output is one line like
/// `1. Name: HEADLESS-2 Description: Some Monitor`.
fn parse_wf_recorder_outputs(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.split_once("Name: ")?.1.trim_start();
            let name = rest
                .split_once(" Description:")
                .map(|(name, _)| name)
                .unwrap_or(rest)
                .trim();
            let name = name.split_whitespace().next()?;
            (name != "Description:").then(|| name.to_string())
        })
        .collect()
}

fn listed_wf_recorder_outputs() -> Vec<String> {
    let Ok(output) = std::process::Command::new("wf-recorder")
        .arg("-L")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_wf_recorder_outputs(&String::from_utf8_lossy(&output.stdout))
}

fn focused_name_in_array(json: &str) -> Option<String> {
    let monitors: serde_json::Value = serde_json::from_str(json).ok()?;
    monitors
        .as_array()?
        .iter()
        .find(|monitor| monitor.get("focused").and_then(|focused| focused.as_bool()) == Some(true))
        .and_then(|monitor| monitor.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn parse_hyprland_focused_output(json: &str) -> Option<String> {
    focused_name_in_array(json)
}

fn parse_sway_focused_output(json: &str) -> Option<String> {
    focused_name_in_array(json)
}

fn parse_niri_focused_output(json: &str) -> Option<String> {
    let output: serde_json::Value = serde_json::from_str(json).ok()?;
    output
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn run_json_command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn focused_compositor_output() -> Option<String> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return run_json_command("hyprctl", &["monitors", "-j"])
            .and_then(|json| parse_hyprland_focused_output(&json));
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        return run_json_command("swaymsg", &["-t", "get_outputs", "-r"])
            .and_then(|json| parse_sway_focused_output(&json));
    }
    if std::env::var_os("NIRI_SOCKET").is_some() {
        return run_json_command("niri", &["msg", "-j", "focused-output"])
            .and_then(|json| parse_niri_focused_output(&json));
    }
    None
}

/// Pick a wf-recorder output from the listed outputs, a requested name, and
/// the compositor's focused output. An empty list means listing failed.
fn choose_output(
    listed: &[String],
    requested: Option<&str>,
    focused: Option<&str>,
) -> Option<String> {
    if let Some(requested) = requested.map(str::trim).filter(|name| !name.is_empty()) {
        if listed.is_empty() || listed.iter().any(|name| name == requested) {
            return Some(requested.to_string());
        }
    }
    if let Some(focused) = focused.map(str::trim).filter(|name| !name.is_empty()) {
        if listed.is_empty() || listed.iter().any(|name| name == focused) {
            return Some(focused.to_string());
        }
    }
    listed.first().cloned()
}

fn resolve_wf_recorder_output(requested: Option<&str>) -> Option<String> {
    let listed = listed_wf_recorder_outputs();
    let focused = focused_compositor_output();
    choose_output(&listed, requested, focused.as_deref())
}

fn video_capture_args(config: &super::RecordingConfig, output: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();

    if let (Some(x), Some(y), Some(width), Some(height)) =
        (config.x, config.y, config.width, config.height)
    {
        args.push("-g".into());
        args.push(format!("{},{} {}x{}", x, y, width, height));
    } else if config.target != super::RecordingTarget::Window {
        if let Some(output) = output {
            args.push("-o".into());
            args.push(output.to_string());
        }
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
    mut config: super::RecordingConfig,
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
    let has_geometry = config.x.is_some()
        && config.y.is_some()
        && config.width.is_some()
        && config.height.is_some();
    let output = if has_geometry || config.target == super::RecordingTarget::Window {
        None
    } else {
        resolve_wf_recorder_output(config.output.as_deref())
    };
    if let Some(output) = output.as_deref() {
        eprintln!("[recording] wf-recorder output: {output}");
    }
    let mut args = video_capture_args(&config, output.as_deref());

    // wf-recorder opens one Pulse source. Resolve it against the live server
    // first so an unplugged choice falls back to the default instead of
    // making wf-recorder exit.
    let audio_inputs = if config.mic_enabled || config.speaker_enabled {
        super::audio::resolve_available_audio_inputs(&mut config)
    } else {
        super::audio::AudioInputs::default()
    };

    // wf-recorder records the cursor by default on current wlroots setups.
    // Older packaged versions do not recognize `--show-cursor`, and passing it
    // can make recording startup noisy or fail. There is no portable positive
    // "show cursor" flag, so only omit cursor customization here.

    if audio_inputs.any() {
        if let Some(app) = config.speaker_app.as_ref() {
            // wf-recorder opens one Pulse source; a single app's streams need
            // the PipeWire graph, so this path records every app.
            eprintln!(
                "[recording] one-app audio is unavailable here; recording every app instead of '{}'",
                app.name
            );
        }
        let source = if audio_inputs.speaker && !audio_inputs.mic {
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
        // `-a` takes its device as an optional argument, so wf-recorder only
        // reads it attached (`--audio=<source>`); a separate argument is
        // ignored and the default source is recorded instead.
        if source.is_empty() {
            args.push("--audio".into());
        } else {
            args.push(format!("--audio={source}"));
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
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RecordError::IoError)?;

    // Echo wf-recorder's stderr and keep the tail so a failed start can
    // report the real reason instead of only the exit status.
    let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
    let mut stderr_reader = child.stderr.take().map(|stderr| {
        let stderr_tail = Arc::clone(&stderr_tail);
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                eprintln!("[wf-recorder] {line}");
                let mut tail = stderr_tail.lock().await;
                if tail.len() >= WF_RECORDER_STDERR_TAIL {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        })
    });

    super::notify_daemon_event("recording_session_started");
    let mut command_rx = command_rx;
    let mut stop_action = super::RecordingTerminalAction::Save;
    let mut paused = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RecordError::IoError)?;
                if !status.success() && stop_action == super::RecordingTerminalAction::Save {
                    if let Some(reader) = stderr_reader.take() {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            reader,
                        )
                        .await;
                    }
                    let tail = stderr_tail.lock().await;
                    let detail = tail
                        .iter()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .cloned();
                    drop(tail);
                    let message = match detail {
                        Some(line) => format!("wf-recorder exited with {status}: {line}"),
                        None => format!("wf-recorder exited with {status}"),
                    };
                    return Err(RecordError::RecorderFailed(message));
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

        let args = video_capture_args(&config, None);

        assert!(args
            .windows(2)
            .any(|pair| pair == ["-g", "10,20 2560x1440"]));
        assert!(args.windows(2).any(|pair| pair == ["-r", "60"]));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "-F" && pair[1].contains("min(iw,1280)") && pair[1].contains("min(ih,720)")
        }));
    }

    #[test]
    fn parses_wf_recorder_output_listing() {
        let stdout = concat!(
            "1. Name: WAYLAND-1 Description: \n",
            "2. Name: HEADLESS-2 Description: Some Monitor\n",
            "3. Name: DP-1 Description: Dell Inc. U2720Q\n",
        );
        assert_eq!(
            parse_wf_recorder_outputs(stdout),
            vec!["WAYLAND-1", "HEADLESS-2", "DP-1"]
        );
    }

    #[test]
    fn parses_hyprland_focused_monitor() {
        let json = r#"[{"name":"DP-1","focused":false},{"name":"DP-2","focused":true}]"#;
        assert_eq!(parse_hyprland_focused_output(json).as_deref(), Some("DP-2"));
    }

    #[test]
    fn parses_sway_focused_output() {
        let json = r#"[{"name":"HDMI-A-1","focused":true},{"name":"DP-1","focused":false}]"#;
        assert_eq!(parse_sway_focused_output(json).as_deref(), Some("HDMI-A-1"));
    }

    #[test]
    fn parses_niri_focused_output() {
        let json = r#"{"name":"eDP-1","make":"BOE"}"#;
        assert_eq!(parse_niri_focused_output(json).as_deref(), Some("eDP-1"));
    }

    #[test]
    fn chooses_the_requested_listed_output() {
        let listed = vec!["DP-1".to_string(), "DP-2".to_string()];
        assert_eq!(
            choose_output(&listed, Some("DP-2"), Some("DP-1")).as_deref(),
            Some("DP-2")
        );
    }

    #[test]
    fn unlisted_request_falls_back_to_the_focused_output() {
        let listed = vec!["DP-1".to_string(), "DP-2".to_string()];
        assert_eq!(
            choose_output(&listed, Some("HDMI-1"), Some("DP-1")).as_deref(),
            Some("DP-1")
        );
    }

    #[test]
    fn no_request_or_focus_uses_the_first_listed_output() {
        let listed = vec!["DP-1".to_string(), "DP-2".to_string()];
        assert_eq!(choose_output(&listed, None, None).as_deref(), Some("DP-1"));
    }

    #[test]
    fn failed_listing_keeps_the_requested_or_focused_output() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(
            choose_output(&empty, Some("DP-2"), None).as_deref(),
            Some("DP-2")
        );
        assert_eq!(
            choose_output(&empty, None, Some("DP-1")).as_deref(),
            Some("DP-1")
        );
        assert_eq!(choose_output(&empty, None, None), None);
    }

    #[test]
    fn video_capture_args_use_the_resolved_output_without_geometry() {
        let config = crate::recording::RecordingConfig::default();
        let args = video_capture_args(&config, Some("DP-1"));

        assert!(args.windows(2).any(|pair| pair == ["-o", "DP-1"]));
        assert!(!args.iter().any(|arg| arg == "-g"));
    }

    #[test]
    fn video_capture_args_prefer_geometry_over_the_output() {
        let config = crate::recording::RecordingConfig {
            x: Some(10),
            y: Some(20),
            width: Some(2560),
            height: Some(1440),
            ..crate::recording::RecordingConfig::default()
        };
        let args = video_capture_args(&config, Some("DP-1"));

        assert!(args
            .windows(2)
            .any(|pair| pair == ["-g", "10,20 2560x1440"]));
        assert!(!args.iter().any(|arg| arg == "-o"));
    }
}
