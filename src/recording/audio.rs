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

/// One microphone offered by the Quick Capture device menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputDevice {
    pub name: String,
    pub description: String,
    /// The server's current default source.
    pub default: bool,
}

/// One output offered by the Quick Capture device menus. The recorder opens
/// `monitor_source`, not the sink itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputDevice {
    pub name: String,
    pub description: String,
    pub monitor_source: String,
    /// The sink behind the server's current default output.
    pub default: bool,
}

/// Run one pactl query, giving up after 1.5 s so a wedged Pulse server never
/// blocks the toolbar. The child is left to exit on its own; pactl is local
/// and bounded, and killing it from the UI thread is worse than waiting out
/// the timeout.
fn run_pactl(args: &[&str]) -> Option<String> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::process::Command::new("pactl")
            .args(&args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok());
        let _ = sender.send(output);
    });
    receiver
        .recv_timeout(std::time::Duration::from_millis(1500))
        .ok()
        .flatten()
}

#[derive(Debug, Default)]
struct PactlBlock {
    name: String,
    description: String,
    monitor_source: String,
    monitor_of: String,
}

/// Split `pactl list sources` / `list sinks` long output into blocks. pactl
/// indents fields with one tab and properties with two, so only the one-tab
/// keys are read.
fn parse_pactl_blocks(text: &str) -> Vec<PactlBlock> {
    let mut blocks: Vec<PactlBlock> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Source #") || line.starts_with("Sink #") {
            blocks.push(PactlBlock::default());
            continue;
        }
        let Some(block) = blocks.last_mut() else {
            continue;
        };
        let Some(field) = line.strip_prefix('\t') else {
            continue;
        };
        if field.starts_with('\t') {
            continue;
        }
        let field = field.trim();
        if let Some(value) = field.strip_prefix("Name:") {
            block.name = value.trim().to_string();
        } else if let Some(value) = field.strip_prefix("Description:") {
            block.description = value.trim().to_string();
        } else if let Some(value) = field.strip_prefix("Monitor Source:") {
            block.monitor_source = value.trim().to_string();
        } else if let Some(value) = field.strip_prefix("Monitor of Sink:") {
            block.monitor_of = value.trim().to_string();
        }
    }
    blocks
}

fn device_description(description: &str, name: &str) -> String {
    if description.trim().is_empty() {
        name.to_string()
    } else {
        description.trim().to_string()
    }
}

fn json_description(value: &serde_json::Value, name: &str) -> String {
    let direct = value
        .get("description")
        .and_then(|description| description.as_str())
        .unwrap_or_default();
    if !direct.trim().is_empty() {
        return direct.trim().to_string();
    }
    let property = value
        .get("properties")
        .and_then(|properties| properties.get("device.description"))
        .and_then(|description| description.as_str())
        .unwrap_or_default();
    device_description(property, name)
}

fn parse_sources_json(text: &str) -> Vec<AudioInputDevice> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(text) else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.trim();
            if name.is_empty() || name.contains("auto_null") || name.ends_with(".monitor") {
                return None;
            }
            // A monitor source records an output, not a microphone.
            let monitor = value
                .get("monitor_source")
                .and_then(|monitor| monitor.as_str())
                .unwrap_or_default();
            if !monitor.is_empty() {
                return None;
            }
            Some(AudioInputDevice {
                name: name.to_string(),
                description: json_description(value, name),
                default: false,
            })
        })
        .collect()
}

fn parse_sources_text(text: &str) -> Vec<AudioInputDevice> {
    parse_pactl_blocks(text)
        .into_iter()
        .filter(|block| {
            !block.name.is_empty()
                && !block.name.contains("auto_null")
                // Real microphones report "n/a" here; monitors name their sink.
                && (block.monitor_of.is_empty() || block.monitor_of == "n/a")
                && !block.name.ends_with(".monitor")
        })
        .map(|block| AudioInputDevice {
            description: device_description(&block.description, &block.name),
            name: block.name,
            default: false,
        })
        .collect()
}

fn parse_sinks_json(text: &str) -> Vec<AudioOutputDevice> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(text) else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            let monitor = value
                .get("monitor_source")
                .and_then(|monitor| monitor.as_str())
                .map(str::trim)
                .filter(|monitor| !monitor.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{name}.monitor"));
            Some(AudioOutputDevice {
                name: name.to_string(),
                description: json_description(value, name),
                monitor_source: monitor,
                default: false,
            })
        })
        .collect()
}

fn parse_sinks_text(text: &str) -> Vec<AudioOutputDevice> {
    parse_pactl_blocks(text)
        .into_iter()
        .filter(|block| !block.name.is_empty())
        .map(|block| {
            let monitor = if block.monitor_source.is_empty() {
                format!("{}.monitor", block.name)
            } else {
                block.monitor_source
            };
            AudioOutputDevice {
                description: device_description(&block.description, &block.name),
                name: block.name,
                monitor_source: monitor,
                default: false,
            }
        })
        .collect()
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

/// A chosen Pulse source the server no longer lists falls back to the current
/// default, so a device unplugged since it was picked still records. `None`
/// means nothing needs to change: the source is present, or there is no
/// chosen source to fall back from.
fn fallback_source(chosen: &Option<String>, default: &str, sources: &[String]) -> Option<String> {
    let chosen = chosen.as_deref()?;
    if source_is_available(chosen, sources) {
        return None;
    }
    source_is_available(default, sources).then(|| default.to_string())
}

/// Resolve which requested mic/speaker inputs exist right now. A named
/// source that disappeared is replaced with the current default, and only a
/// missing default drops the input. Each dropped input is logged and reported
/// once so the user knows the recording continues without it instead of
/// failing when the backend cannot open it.
pub(super) fn resolve_available_audio_inputs(config: &mut super::RecordingConfig) -> AudioInputs {
    let requested = AudioInputs {
        mic: config.mic_enabled,
        speaker: config.speaker_enabled,
    };
    if !requested.any() {
        return requested;
    }

    let sources = pulse_source_names();
    let Some(sources) = sources else {
        // Pulse could not be queried; keep what was asked for.
        return requested;
    };

    if requested.mic {
        let default = get_pulse_default_source();
        if let Some(replacement) = fallback_source(&config.mic_source, &default, &sources) {
            eprintln!(
                "[recording] microphone source '{}' is unavailable; using '{replacement}'",
                config.mic_source.as_deref().unwrap_or_default()
            );
            config.mic_source = Some(replacement);
        }
    }
    if requested.speaker {
        let default = get_pulse_speaker_monitor();
        if let Some(replacement) = fallback_source(&config.speaker_source, &default, &sources) {
            eprintln!(
                "[recording] system audio source '{}' is unavailable; using '{replacement}'",
                config.speaker_source.as_deref().unwrap_or_default()
            );
            config.speaker_source = Some(replacement);
        }
    }

    let mic_device = config
        .mic_source
        .clone()
        .unwrap_or_else(get_pulse_default_source);
    let speaker_device = config
        .speaker_source
        .clone()
        .unwrap_or_else(get_pulse_speaker_monitor);
    let available = filter_audio_inputs(requested, &mic_device, &speaker_device, Some(&sources));

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

/// List all PulseAudio/PipeWire input sources (microphones), marking the
/// server's current default.
pub fn list_audio_inputs() -> Vec<AudioInputDevice> {
    let mut devices = run_pactl(&["-f", "json", "list", "sources"])
        .map(|text| parse_sources_json(&text))
        .unwrap_or_default();
    if devices.is_empty() {
        devices = run_pactl(&["list", "sources"])
            .map(|text| parse_sources_text(&text))
            .unwrap_or_default();
    }
    let default = get_pulse_default_source();
    for device in &mut devices {
        device.default = device.name == default;
    }
    devices
}

/// List all PulseAudio/PipeWire outputs, each with the monitor source that
/// records it, marking the server's current default.
pub fn list_audio_outputs() -> Vec<AudioOutputDevice> {
    let mut devices = run_pactl(&["-f", "json", "list", "sinks"])
        .map(|text| parse_sinks_json(&text))
        .unwrap_or_default();
    if devices.is_empty() {
        devices = run_pactl(&["list", "sinks"])
            .map(|text| parse_sinks_text(&text))
            .unwrap_or_default();
    }
    let default = get_pulse_speaker_monitor();
    for device in &mut devices {
        device.default = device.monitor_source == default;
    }
    devices
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

    #[test]
    fn pactl_json_lists_only_real_microphones_with_descriptions() {
        let json = r#"[
            {"name":"alsa_output.hdmi.monitor","description":"Monitor of HDMI",
             "monitor_source":"alsa_output.hdmi"},
            {"name":"alsa_input.usb-mic","description":"USB Microphone",
             "monitor_source":""},
            {"name":"auto_null.monitor","description":"Dummy Output",
             "monitor_source":"auto_null"}
        ]"#;
        let devices = parse_sources_json(json);
        assert_eq!(
            devices,
            vec![AudioInputDevice {
                name: "alsa_input.usb-mic".into(),
                description: "USB Microphone".into(),
                default: false,
            }]
        );
    }

    #[test]
    fn pactl_json_lists_outputs_with_their_monitor_source() {
        let json = r#"[
            {"name":"alsa_output.speaker","description":"Built-in Speaker",
             "monitor_source":"alsa_output.speaker.monitor"}
        ]"#;
        assert_eq!(
            parse_sinks_json(json),
            vec![AudioOutputDevice {
                name: "alsa_output.speaker".into(),
                description: "Built-in Speaker".into(),
                monitor_source: "alsa_output.speaker.monitor".into(),
                default: false,
            }]
        );

        // pactl omits monitor_source on some sinks; the name still resolves.
        let without_monitor = r#"[
            {"name":"alsa_output.speaker","description":"Built-in Speaker"}
        ]"#;
        assert_eq!(
            parse_sinks_json(without_monitor)[0].monitor_source,
            "alsa_output.speaker.monitor"
        );
    }

    #[test]
    fn pactl_text_lists_devices_when_json_is_unavailable() {
        let source_list = "Source #0\n\
        \tState: RUNNING\n\
        \tName: alsa_output.hdmi.monitor\n\
        \tDescription: Monitor of HDMI\n\
        \tMonitor of Sink: alsa_output.hdmi\n\
        Source #1\n\
        \tName: alsa_input.usb-mic\n\
        \tDescription: USB Microphone\n\
        \tMonitor of Sink: n/a\n";
        assert_eq!(
            parse_sources_text(source_list),
            vec![AudioInputDevice {
                name: "alsa_input.usb-mic".into(),
                description: "USB Microphone".into(),
                default: false,
            }]
        );

        let sink_list = "Sink #0\n\
        \tName: alsa_output.speaker\n\
        \tDescription: Built-in Speaker\n\
        \tMonitor Source: alsa_output.speaker.monitor\n";
        assert_eq!(
            parse_sinks_text(sink_list),
            vec![AudioOutputDevice {
                name: "alsa_output.speaker".into(),
                description: "Built-in Speaker".into(),
                monitor_source: "alsa_output.speaker.monitor".into(),
                default: false,
            }]
        );
    }

    #[test]
    fn a_disappeared_chosen_source_falls_back_to_the_default() {
        let available = sources(&["alsa_input.usb-mic", "alsa_output.speaker.monitor"]);
        assert_eq!(
            fallback_source(
                &Some("alsa_input.missing".into()),
                "alsa_input.usb-mic",
                &available
            ),
            Some("alsa_input.usb-mic".into())
        );
        // A source that is still present stays chosen.
        assert_eq!(
            fallback_source(
                &Some("alsa_input.usb-mic".into()),
                "alsa_input.other",
                &available
            ),
            None
        );
        // No unset choice has nothing to replace.
        assert_eq!(
            fallback_source(&None, "alsa_input.usb-mic", &available),
            None
        );
        // Neither the choice nor the default exists: the input is dropped.
        assert_eq!(
            fallback_source(
                &Some("alsa_input.missing".into()),
                "alsa_input.also-missing",
                &available
            ),
            None
        );
    }
}
