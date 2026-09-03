use std::sync::Mutex;
use std::time::Duration;

/// Current mic level (f64 bits stored as u64), updated by mic monitoring thread.
pub(super) static MIC_LEVEL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0); // 0.0f64.to_bits()

/// Current system audio level (f64 bits stored as u64), updated by speaker monitoring thread.
pub(super) static SPEAKER_LEVEL: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// How long after the last level poll before mic/speaker capture streams stop.
pub(super) const AUDIO_MONITOR_IDLE: Duration = Duration::from_secs(3);

pub(super) static MIC_MONITOR_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) static SPEAKER_MONITOR_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) static MIC_LAST_POLL_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static SPEAKER_LAST_POLL_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

pub(super) static MIC_MONITOR_STOP: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
    Mutex::new(None);

pub(super) static SPEAKER_MONITOR_STOP: Mutex<
    Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
> = Mutex::new(None);

pub(super) static AUDIO_IDLE_REAPER: std::sync::Once = std::sync::Once::new();

/// Convert RMS audio amplitude to a visible meter value using a decibel range.
/// A linear gate hid normal built-in microphone speech below roughly -26 dBFS.
pub(crate) fn audio_meter_level(rms: f64, capture_sink: bool) -> f64 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    let floor_db = if capture_sink { -70.0 } else { -60.0 };
    let db = 20.0 * rms.log10();
    ((db - floor_db) / -floor_db).clamp(0.0, 1.0)
}

pub(super) fn monitor_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Returns true when `last_poll_ms` is older than `idle` relative to `now_ms`.
pub(super) fn audio_monitor_is_idle(last_poll_ms: u64, now_ms: u64, idle: Duration) -> bool {
    if last_poll_ms == 0 {
        return true;
    }
    now_ms.saturating_sub(last_poll_ms) >= idle.as_millis() as u64
}

pub(super) fn ensure_audio_idle_reaper() {
    AUDIO_IDLE_REAPER.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(500));
            let now = monitor_now_ms();
            if MIC_MONITOR_RUNNING.load(std::sync::atomic::Ordering::Acquire)
                && audio_monitor_is_idle(
                    MIC_LAST_POLL_MS.load(std::sync::atomic::Ordering::Relaxed),
                    now,
                    AUDIO_MONITOR_IDLE,
                )
            {
                stop_mic_monitor();
            }
            if SPEAKER_MONITOR_RUNNING.load(std::sync::atomic::Ordering::Acquire)
                && audio_monitor_is_idle(
                    SPEAKER_LAST_POLL_MS.load(std::sync::atomic::Ordering::Relaxed),
                    now,
                    AUDIO_MONITOR_IDLE,
                )
            {
                stop_speaker_monitor();
            }
        });
    });
}

/// Set while a recording owns the mic/monitor so overlay meter polls cannot
/// reopen a second pulsesrc on the same device (peaks then xrun into dropouts).
pub(super) static RECORDING_AUDIO_EXCLUSIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn recording_audio_is_exclusive() -> bool {
    RECORDING_AUDIO_EXCLUSIVE.load(std::sync::atomic::Ordering::Acquire)
}

pub fn begin_recording_audio_exclusive() {
    let already = RECORDING_AUDIO_EXCLUSIVE.swap(true, std::sync::atomic::Ordering::AcqRel);
    if already {
        return;
    }
    stop_monitors_and_wait(Duration::from_millis(1500));
}

pub fn end_recording_audio_exclusive() {
    RECORDING_AUDIO_EXCLUSIVE.store(false, std::sync::atomic::Ordering::Release);
}

pub(super) fn touch_mic_monitor() {
    if recording_audio_is_exclusive() {
        return;
    }
    MIC_LAST_POLL_MS.store(monitor_now_ms(), std::sync::atomic::Ordering::Relaxed);
    ensure_audio_idle_reaper();
    ensure_mic_monitor();
}

pub(super) fn touch_speaker_monitor() {
    if recording_audio_is_exclusive() {
        return;
    }
    SPEAKER_LAST_POLL_MS.store(monitor_now_ms(), std::sync::atomic::Ordering::Relaxed);
    ensure_audio_idle_reaper();
    ensure_speaker_monitor();
}

pub(super) fn ensure_mic_monitor() {
    use std::sync::atomic::Ordering;
    if MIC_MONITOR_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Ok(mut guard) = MIC_MONITOR_STOP.lock() {
        *guard = Some(stop.clone());
    }
    let mic_target = find_physical_input_device();
    start_audio_level_stream(
        "mic",
        mic_target.as_deref(),
        false,
        &MIC_LEVEL,
        stop,
        &MIC_MONITOR_RUNNING,
    );
}

pub(super) fn ensure_speaker_monitor() {
    use std::sync::atomic::Ordering;
    if SPEAKER_MONITOR_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Ok(mut guard) = SPEAKER_MONITOR_STOP.lock() {
        *guard = Some(stop.clone());
    }
    start_audio_level_stream(
        "speaker",
        None,
        true,
        &SPEAKER_LEVEL,
        stop,
        &SPEAKER_MONITOR_RUNNING,
    );
}

pub(super) fn stop_mic_monitor() {
    if let Ok(mut guard) = MIC_MONITOR_STOP.lock() {
        if let Some(stop) = guard.take() {
            eprintln!("[daemon] audio (mic): releasing capture (idle / not needed)");
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    MIC_LEVEL.store(0.0f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn stop_speaker_monitor() {
    if let Ok(mut guard) = SPEAKER_MONITOR_STOP.lock() {
        if let Some(stop) = guard.take() {
            eprintln!("[daemon] audio (speaker): releasing capture (idle / not needed)");
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    SPEAKER_LEVEL.store(0.0f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Stop overlay meter streams and wait until their threads exit so recording
/// can open the same pulsesrc device.
pub(super) fn stop_monitors_and_wait(timeout: Duration) {
    stop_mic_monitor();
    stop_speaker_monitor();
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let mic = MIC_MONITOR_RUNNING.load(std::sync::atomic::Ordering::Acquire);
        let speaker = SPEAKER_MONITOR_RUNNING.load(std::sync::atomic::Ordering::Acquire);
        if !mic && !speaker {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!(
        "[daemon] audio meters still running after {:?}; continuing",
        timeout
    );
}

pub(super) fn start_audio_level_stream(
    label: &'static str,
    target: Option<&str>,
    capture_sink: bool,
    level: &'static std::sync::atomic::AtomicU64,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    running: &'static std::sync::atomic::AtomicBool,
) {
    let target_owned = target.map(String::from);
    std::thread::spawn(move || {
        struct ClearRunning {
            running: &'static std::sync::atomic::AtomicBool,
            level: &'static std::sync::atomic::AtomicU64,
            label: &'static str,
        }
        impl Drop for ClearRunning {
            fn drop(&mut self) {
                self.level
                    .store(0.0f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
                self.running
                    .store(false, std::sync::atomic::Ordering::Release);
                eprintln!("[daemon] audio ({}): monitoring stopped.", self.label);
            }
        }
        let _clear = ClearRunning {
            running,
            level,
            label,
        };

        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        crate::recording::run_audio_level_monitor(label, target_owned, capture_sink, level, stop);
    });
}

/// Detect the first physical (non-monitor) audio input device via pactl.
/// Returns `None` if no suitable device is found, letting Pulse fall back
/// to the default input.
pub(crate) fn find_physical_input_device() -> Option<String> {
    let output = std::process::Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;

    for line in stdout.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 2 {
            continue;
        }
        let name = fields[1].trim();
        // Monitor sources end with `.monitor` — skip them to avoid picking up
        // system audio loopback (which the speaker stream already captures).
        if name.ends_with(".monitor") {
            continue;
        }
        eprintln!("[daemon] audio (mic): detected physical input device '{name}'");
        return Some(name.to_string());
    }
    eprintln!("[daemon] audio (mic): no physical input device found; falling back to default");
    None
}
