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

pub(super) fn touch_mic_monitor() {
    MIC_LAST_POLL_MS.store(monitor_now_ms(), std::sync::atomic::Ordering::Relaxed);
    ensure_audio_idle_reaper();
    ensure_mic_monitor();
}

pub(super) fn touch_speaker_monitor() {
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
        "apexshot-mic-monitor",
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
        "apexshot-speaker-monitor",
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
            eprintln!("[daemon] PipeWire (mic): releasing capture (idle / not needed)");
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    MIC_LEVEL.store(0.0f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn stop_speaker_monitor() {
    if let Ok(mut guard) = SPEAKER_MONITOR_STOP.lock() {
        if let Some(stop) = guard.take() {
            eprintln!("[daemon] PipeWire (speaker): releasing capture (idle / not needed)");
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    SPEAKER_LEVEL.store(0.0f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn start_audio_level_stream(
    label: &'static str,
    stream_name: &'static str,
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
                eprintln!("[daemon] PipeWire ({}): monitoring stopped.", self.label);
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

        std::thread::sleep(std::time::Duration::from_millis(200));
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }

        use pipewire as pw;
        use pw::{properties::properties, spa};
        use spa::param::format::{MediaSubtype, MediaType};
        use spa::param::format_utils;

        struct UserData {
            format: spa::param::audio::AudioInfoRaw,
        }

        pw::init();

        let mainloop = match pw::main_loop::MainLoopRc::new(None) {
            Ok(ml) => ml,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create main loop: {e}");
                return;
            }
        };

        let context = match pw::context::ContextRc::new(&mainloop, None) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create context: {e}");
                return;
            }
        };

        let core = match context.connect_rc(None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to connect core: {e}");
                return;
            }
        };

        let data = UserData {
            format: Default::default(),
        };

        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Production",
        };
        if let Some(ref target_name) = target_owned {
            props.insert("target.object", target_name.as_str());
        }
        if capture_sink {
            props.insert("stream.capture.sink", "true");
        }

        let stream = match pw::stream::StreamBox::new(&core, stream_name, props) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to create stream: {e}");
                return;
            }
        };

        let _listener = stream
            .add_local_listener_with_user_data(data)
            .param_changed(move |_, user_data, id, param| {
                let Some(param) = param else { return };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let (media_type, media_subtype) = match format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }
                user_data.format.parse(param).ok();
                eprintln!(
                    "[daemon] PipeWire ({label}): capturing rate={} channels={}",
                    user_data.format.rate(),
                    user_data.format.channels(),
                );
            })
            .process(move |stream, _user_data| {
                let mut buf = match stream.dequeue_buffer() {
                    Some(b) => b,
                    None => return,
                };
                let datas = buf.datas_mut();
                if datas.is_empty() {
                    return;
                }

                let mut sum_sq: f64 = 0.0;
                let mut count: u64 = 0;
                for data in datas.iter_mut() {
                    let n_bytes = data.chunk().size() as usize;
                    if let Some(slice) = data.data() {
                        let ptr = slice.as_ptr() as *const f32;
                        if n_bytes >= std::mem::size_of::<f32>() {
                            let n_samples = n_bytes / std::mem::size_of::<f32>();
                            for j in 0..n_samples {
                                let s = unsafe { *ptr.add(j) };
                                sum_sq += (s * s) as f64;
                                count += 1;
                            }
                        }
                    }
                }

                // RMS gives natural, varied levels for both mic and speaker
                let rms = if count > 0 {
                    (sum_sq / count as f64).sqrt()
                } else {
                    0.0
                };
                let raw_level = (rms * 3.0).clamp(0.0, 1.0);

                // Noise gate for mic: ignore quiet audio to avoid picking up
                // ambient noise or speaker bleed
                let gated = if !capture_sink && raw_level < 0.15 {
                    0.0
                } else {
                    raw_level
                };

                level.store(gated.to_bits(), std::sync::atomic::Ordering::Relaxed);
            })
            .register();

        // Build audio format pod: F32LE, 44100Hz, mono
        let mut params: Vec<Vec<u8>> = Vec::new();
        {
            let mut audio_info = spa::param::audio::AudioInfoRaw::new();
            audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
            audio_info.set_rate(44100);
            audio_info.set_channels(1);

            let obj = spa::pod::Object {
                type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                id: spa::param::ParamType::EnumFormat.as_raw(),
                properties: audio_info.into(),
            };

            let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &spa::pod::Value::Object(obj),
            )
            .unwrap()
            .0
            .into_inner();

            if spa::pod::Pod::from_bytes(&values).is_some() {
                params.push(values);
            }
        }

        let mut param_refs: Vec<&spa::pod::Pod> = params
            .iter()
            .filter_map(|bytes| spa::pod::Pod::from_bytes(bytes))
            .collect();

        match stream.connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut param_refs,
        ) {
            Ok(_) => eprintln!("[daemon] PipeWire ({label}) monitoring started."),
            Err(e) => {
                eprintln!("[daemon] PipeWire ({label}): failed to connect stream: {e}");
                return;
            }
        }

        // Poll stop flag on the PipeWire thread (MainLoopRc is !Send).
        let stop_for_timer = stop.clone();
        let mainloop_for_timer = mainloop.clone();
        let stop_timer = mainloop.loop_().add_timer(move |_| {
            if stop_for_timer.load(std::sync::atomic::Ordering::Acquire) {
                mainloop_for_timer.quit();
            }
        });
        let _ = stop_timer.update_timer(
            Some(Duration::from_millis(250)),
            Some(Duration::from_millis(250)),
        );

        mainloop.run();
    });
}

/// Detect the first physical (non-monitor) audio input device via pactl.
/// Returns `None` if no suitable device is found, letting PipeWire fall back
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
        eprintln!("[daemon] PipeWire (mic): detected physical input device '{name}'");
        return Some(name.to_string());
    }
    eprintln!("[daemon] PipeWire (mic): no physical input device found; falling back to default");
    None
}
