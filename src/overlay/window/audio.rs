use std::sync::atomic::{AtomicU64, Ordering};

/// When the daemon is not running, the overlay starts its own GStreamer
/// pulsesrc level monitoring and stores the results here.
pub(super) static OVERLAY_MIC_LEVEL: AtomicU64 = AtomicU64::new(0);
pub(super) static OVERLAY_SPEAKER_LEVEL: AtomicU64 = AtomicU64::new(0);
pub(super) static LOCAL_MONITOR_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static LOCAL_MONITOR_STOP: std::sync::Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
    std::sync::Mutex::new(None);

pub(super) fn poll_daemon_audio_levels() -> Option<(f64, f64)> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        crate::daemon::DAEMON_BUS_NAME,
        crate::daemon::DAEMON_OBJECT_PATH,
        crate::daemon::DAEMON_INTERFACE,
    )
    .ok()?;
    let mic = proxy.call::<_, _, f64>("GetMicLevel", &()).ok()?;
    let speaker = proxy.call::<_, _, f64>("GetSpeakerLevel", &()).ok()?;
    Some((mic.clamp(0.0, 1.0), speaker.clamp(0.0, 1.0)))
}

/// Start local GStreamer pulsesrc meters for the overlay.
///
/// Used when the daemon is not running (standalone capture mode on
/// compositors like Hyprland). Only while the recording panel needs meters
/// so we do not force Bluetooth headsets into HSP/HFP mode (issue #41).
pub(super) fn start_local_audio_monitoring() {
    use std::sync::atomic::Ordering;
    if LOCAL_MONITOR_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Ok(mut guard) = LOCAL_MONITOR_STOP.lock() {
        *guard = Some(stop.clone());
    }
    let mic_target = crate::daemon::find_physical_input_device();
    // Two streams share one stop flag; clear RUNNING when both threads exit
    // via a simple join counter.
    let remaining = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(2));
    spawn_overlay_pw_stream(
        "mic",
        "apexshot-overlay-mic",
        mic_target.as_deref(),
        false,
        &OVERLAY_MIC_LEVEL,
        stop.clone(),
        remaining.clone(),
    );
    spawn_overlay_pw_stream(
        "speaker",
        "apexshot-overlay-speaker",
        None,
        true,
        &OVERLAY_SPEAKER_LEVEL,
        stop,
        remaining,
    );
}

pub(super) fn stop_local_audio_monitoring() {
    if let Ok(mut guard) = LOCAL_MONITOR_STOP.lock() {
        if let Some(stop) = guard.take() {
            eprintln!("[overlay] audio: releasing local meters");
            stop.store(true, Ordering::Release);
        }
    }
    OVERLAY_MIC_LEVEL.store(0.0f64.to_bits(), Ordering::Relaxed);
    OVERLAY_SPEAKER_LEVEL.store(0.0f64.to_bits(), Ordering::Relaxed);
}

/// Spawn a GStreamer pulsesrc meter for the overlay (same stack as recording).
pub(super) fn spawn_overlay_pw_stream(
    label: &'static str,
    _stream_name: &'static str,
    target: Option<&str>,
    capture_sink: bool,
    level: &'static AtomicU64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    remaining: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let target_owned = target.map(String::from);
    std::thread::spawn(move || {
        struct ClearRunning {
            remaining: std::sync::Arc<std::sync::atomic::AtomicUsize>,
            level: &'static AtomicU64,
            label: &'static str,
        }
        impl Drop for ClearRunning {
            fn drop(&mut self) {
                self.level.store(0.0f64.to_bits(), Ordering::Relaxed);
                if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                    LOCAL_MONITOR_RUNNING.store(false, Ordering::Release);
                }
                eprintln!("[overlay] audio ({}) monitoring stopped.", self.label);
            }
        }
        let _clear = ClearRunning {
            remaining,
            level,
            label,
        };

        if stop.load(Ordering::Acquire) {
            return;
        }

        crate::recording::run_audio_level_monitor(label, target_owned, capture_sink, level, stop);
    });
}

pub(super) fn set_mic_volume(vol: f64) {
    let pct = (vol.clamp(0.0, 1.0) * 100.0).round() as u32;
    std::thread::spawn(move || {
        let pactl_result = std::process::Command::new("pactl")
            .args([
                "set-source-volume",
                "@DEFAULT_SOURCE@",
                &format!("{}%", pct),
            ])
            .output();
        if !pactl_result
            .as_ref()
            .is_ok_and(|output| output.status.success())
        {
            let _ = std::process::Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SOURCE@", &format!("{}%", pct)])
                .output();
        }
    });
}

fn parse_pactl_volume(output: &str) -> Option<f64> {
    output
        .split_whitespace()
        .find_map(|token| token.strip_suffix('%')?.parse::<f64>().ok())
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
}

fn parse_wpctl_volume(output: &str) -> Option<f64> {
    output
        .split_whitespace()
        .nth(1)?
        .parse::<f64>()
        .ok()
        .map(|volume| volume.clamp(0.0, 1.0))
}

fn get_system_volume(command: &str, device: &str, wpctl_device: &str) -> Option<f64> {
    if let Ok(output) = std::process::Command::new("pactl")
        .args([command, device])
        .output()
    {
        if output.status.success() {
            if let Some(volume) = parse_pactl_volume(std::str::from_utf8(&output.stdout).ok()?) {
                return Some(volume);
            }
        }
    }

    let output = std::process::Command::new("wpctl")
        .args(["get-volume", wpctl_device])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    parse_wpctl_volume(std::str::from_utf8(&output.stdout).ok()?)
}

pub(super) fn get_mic_volume() -> Option<f64> {
    get_system_volume(
        "get-source-volume",
        "@DEFAULT_SOURCE@",
        "@DEFAULT_AUDIO_SOURCE@",
    )
}

pub(super) fn get_speaker_volume() -> Option<f64> {
    get_system_volume("get-sink-volume", "@DEFAULT_SINK@", "@DEFAULT_AUDIO_SINK@")
}

pub(super) fn set_speaker_volume(vol: f64) {
    let pct = (vol.clamp(0.0, 1.0) * 100.0).round() as u32;
    std::thread::spawn(move || {
        let pactl_result = std::process::Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", pct)])
            .output();
        if !pactl_result
            .as_ref()
            .is_ok_and(|output| output.status.success())
        {
            let _ = std::process::Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", pct)])
                .output();
        }
    });
}

/// Install overlay recording-panel meter orchestration:
/// - background worker: daemon D-Bus poll, else local PipeWire streams while
///   the recording panel is open (never holds the mic for plain area-select)
/// - 100ms UI-thread timer: copy levels into `SelectorState` and redraw on change
///
/// Behavior is unchanged from the prior inline setup path. Explicit cancellation
/// of the worker / GLib source on overlay close is a separate follow-up (10.22+).
pub(super) fn install_overlay_audio_meters(
    state: &std::sync::Arc<std::sync::Mutex<super::super::state::SelectorState>>,
    drawing_area: &gtk4::DrawingArea,
) {
    use gtk4::prelude::*;
    use std::sync::{Arc, Mutex};

    let audio_levels = Arc::new(Mutex::new((0.0_f64, 0.0_f64)));
    {
        let audio_levels = audio_levels.clone();
        let state_for_audio = state.clone();
        std::thread::spawn(move || {
            // Try daemon D-Bus first. If the daemon is not running (standalone
            // capture mode on Hyprland), fall back to local PipeWire monitoring.
            // Only open capture streams while the recording panel is open so a
            // plain area-select (or tray-only daemon) never holds the mic
            // (Bluetooth HSP/HFP / issue #41).
            let mut try_daemon = true;
            loop {
                let panel_open = state_for_audio
                    .lock()
                    .map(|st| st.recording.panel_open)
                    .unwrap_or(false);

                if !panel_open || crate::daemon::recording_audio_is_exclusive() {
                    stop_local_audio_monitoring();
                    if let Ok(mut guard) = audio_levels.lock() {
                        *guard = (0.0, 0.0);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }

                if try_daemon {
                    if let Some(levels) = poll_daemon_audio_levels() {
                        if let Ok(mut guard) = audio_levels.lock() {
                            *guard = levels;
                        }
                    } else {
                        try_daemon = false;
                        start_local_audio_monitoring();
                    }
                } else {
                    start_local_audio_monitoring();
                    let mic = f64::from_bits(OVERLAY_MIC_LEVEL.load(Ordering::Relaxed));
                    let speaker = f64::from_bits(OVERLAY_SPEAKER_LEVEL.load(Ordering::Relaxed));
                    if let Ok(mut guard) = audio_levels.lock() {
                        *guard = (mic.clamp(0.0, 1.0), speaker.clamp(0.0, 1.0));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }

    let state_audio_tick = state.clone();
    let drawing_area_weak_audio = drawing_area.downgrade();
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let (mic_level, speaker_level) = audio_levels
            .lock()
            .map(|guard| *guard)
            .unwrap_or((0.0, 0.0));
        if let Ok(mut st) = state_audio_tick.lock() {
            if !st.recording.panel_open {
                return gtk4::glib::ControlFlow::Continue;
            }
            let old_mic = st.recording.mic_level;
            let old_speaker = st.recording.speaker_level;
            st.recording.mic_level = if st.recording.mic_toggle {
                mic_level
            } else {
                0.0
            };
            st.recording.speaker_level = if st.recording.speaker_toggle {
                speaker_level
            } else {
                0.0
            };
            if (old_mic - st.recording.mic_level).abs() > 0.01
                || (old_speaker - st.recording.speaker_level).abs() > 0.01
            {
                if let Some(area) = drawing_area_weak_audio.upgrade() {
                    area.queue_draw();
                }
            }
        }
        gtk4::glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::{parse_pactl_volume, parse_wpctl_volume};

    #[test]
    fn parses_first_pactl_channel_volume() {
        let output =
            "Volume: front-left: 32768 / 50% / -18.06 dB, front-right: 32768 / 50% / -18.06 dB";
        assert_eq!(parse_pactl_volume(output), Some(0.5));
    }

    #[test]
    fn clamps_boosted_pactl_volume_to_ui_range() {
        assert_eq!(
            parse_pactl_volume("Volume: 98304 / 150% / 10.57 dB"),
            Some(1.0)
        );
        assert_eq!(parse_pactl_volume("not a volume"), None);
    }

    #[test]
    fn parses_wpctl_volume_with_optional_muted_marker() {
        assert_eq!(parse_wpctl_volume("Volume: 0.36 [MUTED]"), Some(0.36));
        assert_eq!(parse_wpctl_volume("Volume: 0.81"), Some(0.81));
        assert_eq!(parse_wpctl_volume("no volume"), None);
    }

    /// Owner contract: meter worker + UI timer live on audio, not setup.
    #[test]
    fn audio_owner_covers_meter_worker_and_ui_timer() {
        let source = include_str!("audio.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production audio source");
        assert!(
            production.contains("fn install_overlay_audio_meters"),
            "audio must own install_overlay_audio_meters"
        );
        assert!(
            production.contains("poll_daemon_audio_levels")
                && production.contains("start_local_audio_monitoring")
                && production.contains("stop_local_audio_monitoring"),
            "audio install must orchestrate daemon + local PW paths"
        );
        assert!(
            production.contains("from_millis(100)") && production.contains("timeout_add_local"),
            "audio must keep the 100ms UI meter timer"
        );
        assert!(
            production.contains("recording.panel_open") && production.contains("issue #41"),
            "audio must only open streams while recording panel is open (#41)"
        );
        assert!(
            production.contains("recording_audio_is_exclusive"),
            "meter polls must not reopen pulsesrc while a recording owns the mic"
        );
        assert!(
            production.contains("mic_toggle") && production.contains("speaker_toggle"),
            "UI tick must respect mic/speaker toggles when writing levels"
        );
    }
}
