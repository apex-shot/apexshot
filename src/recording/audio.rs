use super::*;

pub(super) fn ensure_pipewire_pulse_running() {
    if !command_exists("systemctl") {
        return;
    }

    let active = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "pipewire-pulse.service"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if active {
        return;
    }

    eprintln!("[recording] pipewire-pulse is not active; attempting to start it for audio capture");
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", "pipewire-pulse.service"])
        .status();
}

pub(super) fn get_pulse_default_source() -> String {
    std::process::Command::new("pactl")
        .arg("get-default-source")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

pub(super) fn get_pulse_speaker_monitor() -> String {
    std::process::Command::new("pactl")
        .arg("get-default-sink")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| format!("{}.monitor", s.trim()))
        .filter(|s| s != ".monitor")
        .unwrap_or_else(|| "default.monitor".to_string())
}

/// A microphone input for GStreamer pulsesrc: either an explicit source name
/// or the server's default source (device property left unset).
#[derive(Debug, Clone)]
pub(super) enum PulseMicDevice {
    Default,
    Named(String),
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedAudioDevices {
    pub mic: Option<PulseMicDevice>,
    pub monitor: Option<String>,
}

/// Resolve the mic/monitor Pulse source names for the GStreamer audio path.
///
/// `None` when audio is disabled, or when the monitor source cannot be
/// resolved to a real `.monitor` name (pulsesrc has no "default monitor"
/// shorthand, unlike ffmpeg's pulse input) — callers then fall back to the
/// ffmpeg-pulse audio path.
pub(super) fn resolve_recording_audio_devices(
    config: &super::RecordingConfig,
) -> Option<ResolvedAudioDevices> {
    if !config.mic_enabled && !config.speaker_enabled {
        return None;
    }

    let mic = config
        .mic_enabled
        .then(|| match config.mic_source.as_deref() {
            Some(name) if name != "default" => PulseMicDevice::Named(name.to_string()),
            _ => PulseMicDevice::Default,
        });

    let monitor = if config.speaker_enabled {
        let name = config
            .speaker_source
            .clone()
            .unwrap_or_else(get_pulse_speaker_monitor);
        if name == "default.monitor" {
            return None;
        }
        Some(name)
    } else {
        None
    };

    if mic.is_none() && monitor.is_none() {
        return None;
    }
    Some(ResolvedAudioDevices { mic, monitor })
}

/// List all PulseAudio/PipeWire input sources (microphones).
pub fn list_audio_inputs() -> Vec<(String, String)> {
    // name, description
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    output
        .lines()
        .filter(|line| !line.contains(".monitor")) // exclude monitor sources
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let desc = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                // Filter out "auto_null" and other virtual sources
                if !name.contains("auto_null") && !desc.is_empty() {
                    Some((name, desc))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// List all PulseAudio/PipeWire monitor sources (speaker output capture).
pub fn list_audio_outputs() -> Vec<(String, String)> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    output
        .lines()
        .filter(|line| line.contains(".monitor"))
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let desc = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
                Some((name, desc))
            } else {
                None
            }
        })
        .collect()
}
