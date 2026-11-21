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

/// Requested recording audio inputs after checking that their Pulse sources
/// exist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct AudioInputs {
    pub mic: bool,
    pub speaker: bool,
}

impl AudioInputs {
    pub(super) fn any(self) -> bool {
        self.mic || self.speaker
    }
}

/// Names of every PulseAudio/PipeWire source, or `None` when Pulse cannot be
/// queried (no `pactl`, server down). Callers keep the requested inputs in
/// that case: an unverifiable source is better than silently dropping audio.
fn pulse_source_names() -> Option<Vec<String>> {
    // pipewire-pulse can still be publishing its graph right after we start
    // it, so an empty or failed answer is retried briefly before a machine
    // with real devices is mistaken for one without any.
    let mut last_success = None;
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        match query_pulse_source_names() {
            Some(names) if !names.is_empty() => return Some(names),
            Some(names) => last_success = Some(names),
            None => {}
        }
    }
    last_success
}

fn query_pulse_source_names() -> Option<Vec<String>> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(
        stdout
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1).map(str::to_string))
            .collect(),
    )
}

/// `true` when `device` is one of `sources`. The pulse input aliases
/// `default` / `default.monitor` have no entry of their own, so they resolve
/// to any real input / monitor respectively.
fn source_is_available(device: &str, sources: &[String]) -> bool {
    match device {
        "default" => sources.iter().any(|name| !name.ends_with(".monitor")),
        "default.monitor" => sources.iter().any(|name| name.ends_with(".monitor")),
        name => sources.iter().any(|source| source == name),
    }
}

/// Keep only the requested inputs that Pulse can actually open. A missing
/// source must cost its own track, never the recording. `sources` is `None`
/// when Pulse could not be queried, in which case everything is kept.
fn filter_audio_inputs(
    requested: AudioInputs,
    mic_device: &str,
    speaker_device: &str,
    sources: Option<&[String]>,
) -> AudioInputs {
    let Some(sources) = sources else {
        return requested;
    };
    AudioInputs {
        mic: requested.mic && source_is_available(mic_device, sources),
        speaker: requested.speaker && source_is_available(speaker_device, sources),
    }
}

/// Resolve which requested mic/speaker inputs exist right now. Each missing
/// source is logged and reported once so the user knows the recording
/// continues without it instead of failing when ffmpeg cannot open it.
pub(super) fn resolve_available_audio_inputs(config: &super::RecordingConfig) -> AudioInputs {
    let requested = AudioInputs {
        mic: config.mic_enabled,
        speaker: config.speaker_enabled,
    };
    if !requested.any() {
        return requested;
    }

    let mic_device = config
        .mic_source
        .clone()
        .unwrap_or_else(get_pulse_default_source);
    let speaker_device = config
        .speaker_source
        .clone()
        .unwrap_or_else(get_pulse_speaker_monitor);
    let sources = pulse_source_names();
    let available =
        filter_audio_inputs(requested, &mic_device, &speaker_device, sources.as_deref());

    if requested.mic && !available.mic {
        eprintln!(
            "[recording] microphone source '{mic_device}' is unavailable; continuing without it"
        );
        crate::utils::notify::desktop_notification(
            &crate::i18n::t("Microphone unavailable"),
            &crate::i18n::t("Recording continues without the microphone."),
        );
    }
    if requested.speaker && !available.speaker {
        eprintln!(
            "[recording] system audio source '{speaker_device}' is unavailable; continuing without it"
        );
        crate::utils::notify::desktop_notification(
            &crate::i18n::t("System audio unavailable"),
            &crate::i18n::t("Recording continues without system audio."),
        );
    }
    available
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
/// `inputs` is the already-checked set of sources that exist; unavailable
/// ones were dropped by `resolve_available_audio_inputs`. `None` when no
/// input remains, or when the monitor source cannot be resolved to a real
/// `.monitor` name (pulsesrc has no "default monitor" shorthand, unlike
/// ffmpeg's pulse input) — callers then fall back to the ffmpeg-pulse audio
/// path.
pub(super) fn resolve_recording_audio_devices(
    config: &super::RecordingConfig,
    inputs: AudioInputs,
) -> Option<ResolvedAudioDevices> {
    if !inputs.any() {
        return None;
    }

    let mic = inputs.mic.then(|| match config.mic_source.as_deref() {
        Some(name) if name != "default" => PulseMicDevice::Named(name.to_string()),
        _ => PulseMicDevice::Default,
    });

    let monitor = if inputs.speaker {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_missing_microphone_drops_only_the_mic_input() {
        let available = sources(&["alsa_output.pci-0000.stereo.monitor"]);
        let inputs = filter_audio_inputs(
            AudioInputs {
                mic: true,
                speaker: true,
            },
            "alsa_input.pci-0000.analog-stereo",
            "alsa_output.pci-0000.stereo.monitor",
            Some(&available),
        );
        assert_eq!(
            inputs,
            AudioInputs {
                mic: false,
                speaker: true
            }
        );
    }

    #[test]
    fn a_missing_monitor_drops_only_the_speaker_input() {
        let available = sources(&["alsa_input.pci-0000.analog-stereo"]);
        let inputs = filter_audio_inputs(
            AudioInputs {
                mic: true,
                speaker: true,
            },
            "alsa_input.pci-0000.analog-stereo",
            "alsa_output.pci-0000.stereo.monitor",
            Some(&available),
        );
        assert_eq!(
            inputs,
            AudioInputs {
                mic: true,
                speaker: false
            }
        );
    }

    #[test]
    fn no_sources_at_all_drops_every_requested_input() {
        let available: Vec<String> = Vec::new();
        let inputs = filter_audio_inputs(
            AudioInputs {
                mic: true,
                speaker: true,
            },
            "default",
            "default.monitor",
            Some(&available),
        );
        assert_eq!(
            inputs,
            AudioInputs {
                mic: false,
                speaker: false
            }
        );
    }

    #[test]
    fn default_aliases_resolve_to_any_matching_real_source() {
        let available = sources(&[
            "alsa_input.usb-mic.analog-stereo",
            "alsa_output.hdmi-stereo.monitor",
        ]);
        assert!(source_is_available("default", &available));
        assert!(source_is_available("default.monitor", &available));
        assert!(!source_is_available(
            "default.monitor",
            &sources(&["alsa_input.usb-mic.analog-stereo"])
        ));
        assert!(!source_is_available(
            "default",
            &sources(&["alsa_output.hdmi-stereo.monitor"])
        ));
    }

    #[test]
    fn an_unreadable_source_list_keeps_the_requested_inputs() {
        let requested = AudioInputs {
            mic: true,
            speaker: true,
        };
        assert_eq!(
            filter_audio_inputs(requested, "default", "default.monitor", None),
            requested
        );
    }
}
